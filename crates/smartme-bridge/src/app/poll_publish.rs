//! The poll+publish task (Story 1.11).
//!
//! One meter, one loop: heartbeat, fetch, JUDGE, forward. The judging is the
//! pure [`Policy::step`] — no truth is decided in this `async fn`, it only
//! carries data to and from the function that decides.
//!
//! The state machine lives ENTIRELY here and never crosses into the mqtt task,
//! which knows only connection birth and death.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::core::channel::MeterUpdate;
use crate::core::clock::{Clock, MonotonicMs};
use crate::core::oracle::{Cause, Judgement, Measured, Verdict, Verdicts, energy_is_monotonic};
use crate::core::source::{Source, SourceError, Tick};
use crate::core::state_machine::{Policy, State};
use crate::domain::{MeterId, Quality};

/// How the loop is paced and how long a single fetch may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollConfig {
    /// Delay between the start of one iteration and the next.
    pub interval: Duration,
    /// Per-fetch deadline. Beyond it the fetch is abandoned and the tick counts
    /// as [`SourceError::Timeout`] — the cloud going silent must not wedge the
    /// loop, because a wedged loop stops publishing STALE and starts lying by
    /// omission.
    pub fetch_timeout: Duration,
}

/// One meter, as the fleet stood at one instant.
///
/// AR6's `MeterState`, which the architecture has named since Epic 0 and which
/// **did not exist until 2026-08-08** — story 3.1 ticked the box for it. What
/// shipped instead was three independent atomics per meter, read one at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeterState {
    /// Which meter this is.
    pub meter: MeterId,
    /// The monotonic instant at the top of its last loop iteration, or `None`
    /// if it has never run.
    ///
    /// Written before the network call, so a fetch that hangs forever leaves it
    /// visibly old — that is what makes a wedge detectable from outside (the
    /// health check, Story 1.13/Epic 7). Written AFTER the call it would look
    /// healthy exactly when it is not.
    pub last_tick: Option<MonotonicMs>,
    /// The period the loop was pacing at when it last ticked.
    ///
    /// # Why the period is recorded and not merely read
    ///
    /// AR12's allowance is `N × poll_interval`, and `/healthz` used to take that
    /// interval from the live configuration. A hot period change moves the
    /// configuration IMMEDIATELY and the loop only notices at its next tick,
    /// because it is parked in `ticker.tick().await` for the OLD period. So an
    /// operator dropping the period from 300 s to 5 s — a change the screen
    /// truthfully calls *"in force now"* — made `/healthz` report `wedged: true`
    /// for up to five minutes about a loop behaving exactly as designed. Epic 7
    /// wires that to a container restart, so the reward for a supported
    /// reconfiguration would be a killed Sparkplug session.
    ///
    /// The observed cadence is the only honest denominator.
    pub period_ms: i64,
    /// The oracle's verdict, or `None` before the first tick completes.
    ///
    /// **`None` is not `Stale`.** A task that has not finished its first tick has
    /// reached no verdict, and mapping that onto `Stale` would report a fault
    /// about a bridge that is merely starting.
    pub verdict: Option<State>,
    /// What was actually PUBLISHED for this meter, and why (Story 2.3 AC6).
    ///
    /// # Why the state above is not enough, and what it cost
    ///
    /// [`State`] is the freshness machine's answer — `Fresh`, `Stale`, `Failed`.
    /// It knows nothing of the oracles, so a meter whose energy counter went
    /// backwards published `Bad` with null values to the SCADA host while every
    /// operator surface, reading this field, called it `Fresh`.
    ///
    /// That is [#62] exactly: on 2026-08-10 a meter froze for ten hours, was
    /// published `Bad_Stale` throughout, and `/healthz` and `/` reported the
    /// fleet healthy the whole time. The bridge's own screens were the last place
    /// the fault could be seen. Carrying the composed verdict here — the same
    /// value that reached the wire, computed once — is what makes the two agree
    /// by construction rather than by two pieces of code happening to concur.
    ///
    /// `None` before the first completed tick, for the reason above.
    pub published: Option<Verdict>,
}

/// The whole fleet at one instant (AR6).
///
/// # `generation`, and why a counter earns its place here
///
/// It is the invariant that makes "one instant" testable. Every modification
/// touches exactly one meter's fields **and** this counter, inside a single
/// `send_modify`, so a reader holding a snapshot holds a state that existed:
/// `generation` equals the number of writes that produced it.
///
/// Without it, a snapshot test is nearly vacuous — over a quiet fleet every
/// implementation looks coherent, including the per-meter atomics this replaces.
/// With it, a reader that sampled meters one at a time observes a total that no
/// single instant ever had.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FleetState {
    /// One entry per served meter, in the order the tasks were spawned.
    pub meters: Vec<MeterState>,
    /// Incremented by every write, in the same modification as the write.
    pub generation: u64,
}

impl FleetState {
    /// One meter's state, if it is served.
    pub fn of(&self, meter: &MeterId) -> Option<&MeterState> {
        self.meters.iter().find(|m| &m.meter == meter)
    }

    /// Every meter whose oracle has reached a verdict, and which one.
    ///
    /// A meter that has not completed a tick is absent rather than guessed at.
    pub fn verdicts(&self) -> impl Iterator<Item = (&MeterId, State)> {
        self.meters
            .iter()
            .filter_map(|m| m.verdict.map(|v| (&m.meter, v)))
    }

    /// The meters whose PUBLISHED verdict is not good, and why (Story 2.3 AC6).
    ///
    /// Distinct from [`Self::failed`], and the distinction is the point: `failed`
    /// answers *"which meters put nothing on the wire"*, this answers *"which
    /// meters put something the host must not trust"*. A meter can be the second
    /// without being the first — a backwards energy counter publishes `Bad` and
    /// keeps polling — and until Story 2.3 no surface reported that state at all
    /// ([#62]).
    ///
    /// A meter that has not completed a tick is absent rather than guessed at.
    ///
    /// **A meter in [`State::Failed`] is NOT here, and the exclusion is the point**
    /// (added 2026-08-12 by the review of story 2.3's fix). This method shipped
    /// claiming to be "distinct from `failed`" and filtered on the published
    /// quality alone, while `record` writes the state and the verdict together on
    /// every tick — so a refused credential put one meter in BOTH lists, and `/`
    /// printed *"One meter is not being read: cellar … a restart is needed to
    /// clear"* directly above *"One meter is being read … Nothing here is cleared
    /// by a restart"*. The distinction was written in three places and implemented
    /// in none of them.
    ///
    /// Excluded here rather than at each surface because there were already two —
    /// `/` and `/healthz` — and a rule applied at the caller is a rule the third
    /// caller will not know about.
    pub fn degraded(&self) -> Vec<(&MeterId, Verdict)> {
        self.meters
            .iter()
            .filter(|m| m.verdict != Some(State::Failed))
            .filter_map(|m| m.published.map(|v| (&m.meter, v)))
            .filter(|(_, v)| v.quality() != Quality::Good)
            .collect()
    }

    /// The meters whose source has failed fatally — a rejected credential, a
    /// configuration the cloud will not accept. **Absorbing**: only a restart
    /// clears one, so a page that does not name them leaves the operator with a
    /// bridge that looks healthy and publishes nothing.
    pub fn failed(&self) -> Vec<&MeterId> {
        self.verdicts()
            .filter(|(_, s)| *s == State::Failed)
            .map(|(m, _)| m)
            .collect()
    }
}

/// The fleet's live state, written by the poll tasks and read as a snapshot
/// (Story 3.1, AR6 since Story 3.3).
///
/// # Why this is a collection and not one value
///
/// The runtime served a single meter until 2026-08-06, so a single heartbeat
/// *was* the poll loop's. With one task per meter, a shared tick would be
/// touched by whichever task ran most recently — so three healthy siblings would
/// keep it fresh while the fourth had not read its meter for an hour, and
/// `/healthz` would report a bridge that was, for that meter, completely wedged.
///
/// That is the shape of lie this project exists to prevent, and its twin was
/// found on 2026-08-06: a `Failed` source reported as publishing.
///
/// # Why a `watch` and not N atomics, since 2026-08-08
///
/// The atomics were per-meter, which is the half that mattered for the fleet,
/// and they were not a snapshot: a reader walking four meters observed four
/// different instants. `watch::send_modify` takes `&mut` on the shared value
/// under the channel's own lock, so N writers serialise their own field updates
/// while [`Self::snapshot`] hands a reader the whole fleet as it stood.
///
/// **`ArcSwap` was rejected**: rebuilding the vector needs a read-modify-write,
/// which is a race between tasks — the defect being repaired, reintroduced in
/// the repair.
///
/// # The verdict is the WORST of them, and that is deliberate
///
/// Epic 7 wires `/healthz` to a container restart, which kills the session for
/// every meter. Restarting all four because one task wedged is the right trade
/// anyway: unlike a rejected credential ([ADR 0027]), a wedged poll task is
/// exactly what a restart fixes. This is the one place where the fleet makes the
/// healthcheck stricter rather than more forgiving.
///
/// # The name stays `Heartbeats`
///
/// It has carried the verdicts as well as the ticks since Story 3.2, so it was
/// already inexact. Renaming it in the same change that alters its semantics
/// would put a cosmetic diff on top of a behavioural one, and this repository
/// reviews by reading diffs.
///
/// [ADR 0027]: ../../../docs/adr/0027-a-failed-source-is-a-fault-the-screen-must-name.md
#[derive(Clone)]
pub struct Heartbeats(Arc<tokio::sync::watch::Sender<FleetState>>);

impl Default for Heartbeats {
    fn default() -> Self {
        Self::for_meters([])
    }
}

impl Heartbeats {
    /// One entry per meter, in the order the meters were spawned.
    pub fn for_meters(meters: impl IntoIterator<Item = MeterId>) -> Self {
        let meters = meters
            .into_iter()
            .map(|meter| MeterState {
                meter,
                last_tick: None,
                period_ms: 0,
                verdict: None,
                published: None,
            })
            .collect();
        Self(Arc::new(tokio::sync::watch::Sender::new(FleetState {
            meters,
            generation: 0,
        })))
    }

    /// The fleet as it stands, at one instant.
    ///
    /// Cloned out rather than handed as a borrow: `watch`'s read guard blocks
    /// writers for as long as it is held, and a caller that kept one across an
    /// `await` — a template render, say — would stall every poll task.
    pub fn snapshot(&self) -> FleetState {
        self.0.borrow().clone()
    }

    /// The handle one poll task writes through.
    pub fn of(&self, meter: &MeterId) -> Option<MeterPulse> {
        let index = self
            .0
            .borrow()
            .meters
            .iter()
            .position(|m| &m.meter == meter)?;
        Some(MeterPulse {
            fleet: Arc::clone(&self.0),
            index,
        })
    }

    /// Records the oracle's verdict for one meter (Story 3.2 AC5, [ADR 0027] §1).
    ///
    /// **The screen had no way to see this**, so `/` said the bridge was *"polling
    /// the meters and publishing what it reads"* about a source in `Failed` that
    /// had put nothing on the wire since start-up. The page was describing which
    /// branch of `main.rs` ran, presented to an operator as an observation — the
    /// same defect `/healthz` was cured of on 2026-08-04 (`publishing` →
    /// `intends_to_publish`).
    ///
    /// [ADR 0027]: ../../../docs/adr/0027-a-failed-source-is-a-fault-the-screen-must-name.md
    pub fn record(&self, meter: &MeterId, state: State, published: Verdict) {
        self.0.send_modify(|fleet| {
            if let Some(entry) = fleet.meters.iter_mut().find(|m| &m.meter == meter) {
                entry.verdict = Some(state);
                entry.published = Some(published);
                fleet.generation += 1;
            }
        });
    }

    /// The meters the runtime is **actually serving** — one entry per spawned
    /// poll task, because that is how this collection is built
    /// (`supervisor::run_with_control`).
    ///
    /// Exposed for [`crate::app::reconfigure::classify`], which until 2026-08-08
    /// guessed at this from the stored configuration: *"the first enabled meter,
    /// or the first one"*. That guess was the truth while the runtime served one
    /// meter and became wrong the day story 3.1 served them all — disabling the
    /// second of four was then classified as needing a restart, and its DDEATH
    /// never sent, so a host kept showing a buried meter's last value as current.
    ///
    /// A configuration cannot answer this question: it says what is *desired*,
    /// and a meter enabled after start-up is desired without being polled. Only
    /// the set of running tasks knows, and this is it.
    pub fn meters(&self) -> Vec<MeterId> {
        self.0
            .borrow()
            .meters
            .iter()
            .map(|m| m.meter.clone())
            .collect()
    }

    /// How many meters are being watched.
    pub fn len(&self) -> usize {
        self.0.borrow().meters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One meter's write handle into the fleet state.
///
/// Holds an index rather than a reference: the vector's length never changes
/// after `for_meters`, because the served set is fixed at start-up and a
/// configuration change that would alter it costs a process restart
/// (`app::reconfigure`).
#[derive(Clone)]
pub struct MeterPulse {
    fleet: Arc<tokio::sync::watch::Sender<FleetState>>,
    index: usize,
}

impl MeterPulse {
    /// Records that an iteration has just started, **and the period it is
    /// actually pacing at** — see [`MeterState::period_ms`] for why the pacing
    /// is recorded rather than read from the configuration.
    pub fn touch(&self, now: MonotonicMs, period_ms: i64) {
        self.fleet.send_modify(|fleet| {
            let entry = &mut fleet.meters[self.index];
            entry.last_tick = Some(now);
            entry.period_ms = period_ms;
            fleet.generation += 1;
        });
    }

    /// The last recorded instant, or `None` if this meter's loop has never run.
    pub fn last(&self) -> Option<MonotonicMs> {
        self.fleet.borrow().meters[self.index].last_tick
    }

    /// The period this meter's loop was pacing at when it last ticked.
    pub fn period_ms(&self) -> i64 {
        self.fleet.borrow().meters[self.index].period_ms
    }
}

/// Everything one iteration needs that does not change between iterations.
pub struct Context<'a> {
    /// The meter being polled.
    pub meter: &'a MeterId,
    /// The injected clock.
    pub clock: &'a (dyn Clock + Send + Sync),
    /// The staleness policy.
    pub policy: Policy,
    /// Loop pacing and the fetch deadline.
    pub config: PollConfig,
    /// The liveness heartbeat.
    pub heartbeat: &'a MeterPulse,
    /// Where judged readings go.
    pub outbox: &'a mpsc::Sender<MeterUpdate>,
}

/// Runs one iteration: heartbeat, fetch (bounded), judge, forward.
///
/// Split out of the loop so a test can drive exactly one step without a timer.
/// Returns the state to carry into the next iteration.
pub async fn step_once<S: Source + Send>(
    ctx: &Context<'_>,
    source: &mut S,
    previous: State,
    last: &mut Option<crate::domain::Measurement>,
    energy_reference: &mut Option<crate::domain::Kwh>,
    // When the source told us not to come back before. Story 2.6: the ONE
    // source-side wait this bridge honours, because it is the one the poll
    // interval cannot know about.
    rate_limited_until: &mut Option<MonotonicMs>,
) -> (State, Verdict) {
    let Context {
        meter,
        clock,
        policy,
        config,
        heartbeat,
        outbox,
    } = ctx;
    let (policy, config) = (*policy, *config);
    // Heartbeat FIRST: before anything that can block.
    heartbeat.touch(clock.monotonic(), config.interval.as_millis() as i64);

    // THE ONE WAIT THIS BRIDGE HONOURS (story 2.6). If the source asked us not to
    // come back before an instant, no fetch is attempted — **but the cycle still
    // publishes a verdict**, as ADR 0027 requires. Skipping the publication would
    // make a rate limit look like silence, which is the failure this project
    // exists to prevent.
    let now_mono = clock.monotonic();
    let waiting = matches!(*rate_limited_until, Some(until) if now_mono < until);
    let tick: Tick = if waiting {
        Err(SourceError::RateLimited { retry_after: None })
    } else {
        match tokio::time::timeout(config.fetch_timeout, source.fetch(meter)).await {
            Ok(result) => result,
            // The deadline elapsed: the cloud is silent. That is a verdict input,
            // not an error to swallow.
            Err(_elapsed) => Err(SourceError::Timeout),
        }
    };

    // A FRESH RATE LIMIT ARMS THE WAIT, and only when the server named a delay.
    //
    // **AC3 asked for a doubling fallback when `Retry-After` is absent, and it is
    // deliberately not built** — because AC4 of the same story argues that the
    // poll interval already spaces retries, bounded by ADR 0020 and never off. The
    // two criteria contradict each other, and the contradiction is mine: a
    // fallback timer below the interval would do nothing, and above it would be
    // the second competing timer AC4 exists to refuse. Recorded in the story
    // rather than resolved by silently picking one.
    if let Err(SourceError::RateLimited {
        retry_after: Some(delay),
    }) = &tick
    {
        *rate_limited_until = Some(MonotonicMs(now_mono.0 + delay.as_millis() as i64));
    } else if !waiting {
        *rate_limited_until = None;
    }

    let (freshness_state, freshness) = policy.step(previous, &tick, clock.wall());

    // The monotonicity oracle (Story 2.2). It judges a RELATION between two
    // readings, so it has nothing to say when the fetch failed — there is no new
    // index to compare, and the freshness verdict already covers the silence.
    //
    // Scoped to ENERGY (Story 2.3). It looks at the energy index and at nothing
    // else, so it has no business degrading the power value beside it: before
    // 2.3 a backwards counter published `Power = null` labelled
    // `counter-went-backwards`, which withheld a perfectly current number and
    // then blamed it for a fault in its neighbour.
    // WHAT THE SOURCE COULD NOT READ, PER FIELD (story 2.5). The adapter no longer
    // sets one `Quality::Bad` for the whole reading when a single field fails; it
    // says which field and why, and those judgements compose like every other
    // oracle's. This is the first producer of a `Bad` on a metric while the
    // READING-level quality stays `Good`, which is what makes the adoption rule of
    // story 2.3 AC3 observable at last ([#69]).
    let source_faults = match &tick {
        Ok(reading) => reading.faults,
        // A fetch that did not complete has no fields to fault.
        Err(_) => crate::core::source::SourceFaults::NONE,
    };
    let judgements = [
        // Freshness and identity judge the whole response: a reading that is too
        // old is too old in both its numbers.
        Judgement::about_reading(freshness),
        Judgement::about(
            Measured::Power,
            match source_faults.of(Measured::Power) {
                Some(cause) => Verdict::bad(cause),
                None => Verdict::good(),
            },
        ),
        Judgement::about(
            Measured::Energy,
            match source_faults.of(Measured::Energy) {
                Some(cause) => Verdict::bad(cause),
                None => Verdict::good(),
            },
        ),
        Judgement::about(
            Measured::Energy,
            match &tick {
                // NO INDEX, NOTHING TO ORDER. A reading whose energy field the
                // source could not give us is already refused by its own
                // judgement above; asking an ordering oracle about an absent
                // number would answer a question nobody posed.
                //
                // **The guard this replaces was a workaround for the collapse
                // story 2.5 removed.** It read `reading.value.quality ==
                // Quality::Bad` and existed so that `BAD_CARRIER = 0.0` — the
                // substituted non-value a failed conversion used to produce —
                // was never handed to an ordering oracle, which duly answered
                // `counter-went-backwards` about a number nobody claimed was a
                // measurement. There is no such number any more: absence is
                // `None`, and `None` is not comparable.
                Ok(reading) => match reading.value.energy {
                    Some(energy) => energy_is_monotonic(*energy_reference, energy),
                    None => Verdict::good(),
                },
                // No new index to compare, and the freshness verdict already
                // covers the silence.
                Err(_) => Verdict::good(),
            },
        ),
    ];

    // ONE composition, per metric and for the meter (Stories 2.1 and 2.3). Not
    // `if stale { ... } else if backwards { ... }`: the point of the layer is
    // that a reading which is both too old AND backwards publishes the worse of
    // the two, whatever order the oracles were consulted in.
    let published = Verdicts::from_judgements(&judgements);

    // A LATCHING VERDICT PUTS THE METER IN `Failed`, whatever produced it.
    //
    // **This is a net, not a replacement, and saying otherwise was wrong.** The
    // 2026-08-11 review of this story proved the branch is a no-op today:
    // `latches()` is true only for `Cause::SourceRefused`, which `Policy::step`
    // produces at exactly the two sites that already return `State::Failed`
    // (`prev == Failed`, and `Err(Fatal)`). So the condition holds if and only if
    // `freshness_state` is already `Failed`, and deleting these four lines cannot
    // change any answer. Story 2.3's AC2 claimed the composed verdict now DECIDES
    // the latch and that the rule lives in `oracle.rs` "and nowhere else"; both
    // were false, and ADR 0032 asserted them as fact. Corrected there.
    //
    // What the branch is actually for is the case Epic 2 is about to create: a
    // METRIC-scoped judgement carrying a latching cause. `compose_for_meter`
    // folds it into the meter verdict, and without this line the meter would keep
    // publishing while a cause meaning *this is not the meter you asked for* went
    // unlatched. Story 2.4's oracles are metric-scoped by design.
    //
    // Unifying the two for real means taking `State::Failed` out of
    // `Policy::step` and deriving it here from `prev` plus the composed verdict.
    // That is a change to the table AC10 requires be preserved verbatim, so it
    // does not belong in this story — it belongs in the one that first needs it.
    let next = if published.meter().latches() {
        State::Failed
    } else {
        freshness_state
    };

    // WHAT MAY BE REMEMBERED, and it is one rule for both memories (Story 2.3
    // AC3/AC4). Before it, `energy_reference` advanced on `reading.value.quality
    // != Bad` — the SOURCE's opinion — and `last` advanced unconditionally on
    // every successful fetch. Both were wrong, in ways that reached the wire:
    //
    //  - every freshness-level refusal leaves `value.quality == Good`, so a
    //    replayed response rewound the reference and the genuine reset that
    //    followed was published `Good`. FR15 defeated by the oracle's own
    //    bookkeeping.
    //  - `last` held whatever the last fetch returned, including the substituted
    //    `BAD_CARRIER = 0.0` of a failed unit conversion — which the next timeout
    //    then republished as a real `Double` marked `Stale`. The number story 2.2
    //    exists to withhold, on the wire one tick later.
    //
    // TWO MEMORIES, TWO RULES — they were one flag until the 2026-08-11 review of
    // this story, and the exemption that is right for one is wrong for the other.
    //
    // The YARDSTICK may adopt a reading refused for going backwards: story 2.2
    // AC3, a replaced meter legitimately reads lower for ever after, so its new
    // index must become the reference or every later reading is judged against an
    // index that no longer exists.
    //
    // The REPUBLICATION BUFFER may not. `last` is what a later tick hands to a
    // consumer when the cloud goes quiet, and by then the verdict that refused it
    // is gone. With one flag, a meter replaced at 12.0 published `Energy = null`
    // (correct), put that reading in `last` (the exemption), and the next timeout
    // republished `Energy = 12.0` as a genuine `Double` marked `Stale`, cause
    // `source-unreachable` — the number withheld one tick earlier, handed over
    // under a transport fault. Reachable on this story's own AC5 path: restart,
    // reference restored at 900_000, first reading 12.0, then any timeout.
    //
    // AC4 already said so — *"`last` holds only measurements whose composed
    // verdict was publishable"* — and a reading published with a null value is
    // not publishable. The exemption's justification is an argument about the
    // yardstick and does not transfer.
    // **BOTH MEMORIES ARE PER-METRIC SINCE 2026-08-12**, and reading the METER
    // verdict here was this story's own review finding. ADR 0031 had reached the
    // adapter and the wire and stopped at the bookkeeping.
    //
    // THE YARDSTICK FOLLOWS THE ENERGY METRIC. It judges energy and nothing else,
    // so a fault in the POWER field has no business freezing it. It did: an
    // unreadable power unit made the meter verdict `Bad`, a perfectly readable
    // index never became the reference, and a genuine counter reset afterwards was
    // judged against a frozen one and published `Good` — FR15 defeated by the
    // oracle's own bookkeeping, which is exactly the failure story 2.3's review
    // named for the replay case.
    let energy_verdict = published.for_metric(Measured::Energy);
    let reference_adoptable = match energy_verdict.quality() {
        Quality::Bad => energy_verdict.cause() == Some(Cause::CounterWentBackwards),
        _ => true,
    };
    // THE REPUBLICATION BUFFER guards against one thing: a number the bridge
    // REFUSED being handed over later, when the verdict that refused it is gone.
    // So what disqualifies a reading is a metric that was refused **while holding
    // a value** — a refused metric whose value is `None` carries nothing to leak,
    // and since story 2.5 that is the ordinary shape of an unreadable field.
    //
    // Reading the meter verdict instead cost freshness for nothing: a reading with
    // one unreadable field was kept out of `last` entirely, so the next silent
    // cloud republished an OLDER reading than the most recent one whose other
    // metric was sound.
    let last_adoptable = match &tick {
        Ok(reading) => Measured::ALL.iter().all(|metric| {
            let refused = published.for_metric(*metric).quality() == Quality::Bad;
            let carries_a_number = match metric {
                Measured::Power => reading.value.power.is_some(),
                Measured::Energy => reading.value.energy.is_some(),
            };
            !(refused && carries_a_number)
        }),
        Err(_) => false,
    };

    if let Ok(reading) = &tick
        && reference_adoptable
    {
        // Only a reading that HAS an index can become the yardstick. Absence is
        // not a lower reading, and must not be mistaken for one.
        if let Some(energy) = reading.value.energy {
            *energy_reference = Some(energy);
        }
    }

    // EVERY TICK PUBLISHES A VERDICT, or there is nothing to publish one about
    // (Story 3.2, [ADR 0027] §3).
    //
    // Until 2026-08-07 this sent an update only when the fetch SUCCEEDED, and
    // traced the verdict otherwise — with a comment promising that "the mqtt task
    // republishes the last known value with this quality", which no code did. So
    // a meter that had published `Good` and then went silent left the host
    // displaying that value, at that quality, indefinitely: silence on a Sparkplug
    // wire is indistinguishable from "nothing has changed".
    //
    // The republish carries the LAST KNOWN measurement, untouched, with the new
    // verdict — never a synthesised value, and never `now` as its timestamp.
    // `publish()` stamps `value_date`, so the wire says when the reading was TRUE,
    // and the quality says it is no longer proven. The same reasoning as the
    // rebirth re-declaration path, which had it right first.
    //
    // A meter that has NEVER answered has no last measurement, and gets nothing:
    // its DBIRTH already declared it valueless and non-good, which is the truth.
    // Reaching for something to send here would be the defect this fixes, wearing
    // the other face.
    //
    // [ADR 0027]: ../../../docs/adr/0027-a-failed-source-is-a-fault-the-screen-must-name.md
    //
    // AND `last` OBEYS THE SAME RULE (Story 2.3 AC4). The reading is still
    // published now — refusing it does not mean withholding it, it means
    // publishing it marked, which is what `Bad` and its nulls already do. What
    // changes is that it does not become the thing republished LATER, when the
    // verdict that refused it is no longer attached.
    let to_publish = match &tick {
        Ok(reading) => {
            if last_adoptable {
                *last = Some(reading.value.clone());
            }
            Some(reading.value.clone())
        }
        Err(_) => last.clone(),
    };

    match to_publish {
        Some(measurement) => {
            let update = MeterUpdate::new((*meter).clone(), measurement, published);
            if outbox.send(update).await.is_err() {
                tracing::warn!(
                    meter = %meter,
                    "mqtt task is gone; dropping the judged reading"
                );
            }
        }
        None => {
            tracing::info!(
                meter = %meter, ?next, ?published,
                "no reading this tick and none ever, so there is no value to \
                 re-publish; the device birth already declared it valueless"
            );
        }
    }
    (next, published.meter())
}

/// The monotonicity reference as it is written to disk (Story 2.3 AC5).
///
/// # One file per meter, and the reason is concurrency rather than tidiness
///
/// **The first version of this comment said the reason was isolating a corrupt
/// file, and that argument is weak**: one malformed shared file would cost every
/// meter a single unjudged reading, which is a small harm. Corrected 2026-08-11.
///
/// The real reason is that **one task runs per meter** (`supervisor::run` spawns
/// them side by side) and each persists its own reference on its own cycle. A
/// single file holding a `meter → index` map would need a read-modify-write, so
/// two meters storing at the same moment would silently drop one of the two
/// updates — and the loser would come back from a restart with a stale reference,
/// which is worse than no reference at all: it judges against a number that was
/// true two sessions ago.
///
/// Avoiding that needs a shared mutex or an owning task. Both are more machinery
/// than N files, for a value that is one `f64` per meter. The per-meter file is
/// the cheap way to have no shared mutable state at all — the same reasoning that
/// keeps `energy_reference` a task-local in the first place.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedReference {
    /// Which meter this reference belongs to.
    ///
    /// **Written and checked since 2026-08-11.** The comment on
    /// [`reference_path_for`] claimed this field existed and that the id was
    /// verified on load; it did not, and nothing was. The review of this story
    /// found the claim before the collision found a deployment — a documentation
    /// that promises a guard is worse than one that admits its absence, because
    /// the next reader stops looking.
    ///
    /// It is a second lock, not the first: the file name is now collision-free by
    /// construction (see [`reference_path_for`]). This one catches what a path
    /// cannot — a file copied, renamed or restored from a backup under the wrong
    /// meter's name, which is an ordinary thing to do while repairing a
    /// deployment over a file share.
    meter: String,
    /// The last accepted energy index, in kWh.
    energy_kwh: f64,
}

/// Where one meter's reference lives, under the state directory.
pub fn reference_path_for(dir: &std::path::Path, meter: &MeterId) -> std::path::PathBuf {
    // The meter id is operator-chosen, so it is not trusted as a path component.
    //
    // **PERCENT-ENCODED since 2026-08-11, and the first version was wrong.** It
    // mapped every character that is not alphanumeric, `-` or `_` to `_`, which
    // is LOSSY: `gar age`, `gar.age` and `gar_age` all became
    // `energy-reference-gar_age.toml`. `config.rs` rejects only EXACT duplicate
    // meter ids and applies no charset rule, so all three are configurable at
    // once. Two poll tasks would then write one file on their own cycles, and
    // after a restart each meter would be judged against the other's index — a
    // silent break of the per-meter isolation stories 3.1-3.3 established, in
    // the one place where being wrong means missing a counter reset.
    //
    // Percent-encoding is reversible, so distinct ids give distinct names; it is
    // stable across compiler and library versions, which a hash of the id would
    // not be (`DefaultHasher` is explicitly not guaranteed stable, and a name
    // that moves on a toolchain bump loses the reference it was protecting); and
    // it keeps the file readable in a directory listing, which matters on a
    // deployment reachable only over a file share.
    let mut encoded = String::with_capacity(meter.to_string().len());
    for byte in meter.to_string().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => {
                encoded.push(byte as char);
            }
            // Everything else, including `.`, `/`, `%` itself and any non-ASCII
            // byte, becomes `%XX`. Encoding `%` is what makes it reversible: an
            // id containing a literal `%2E` must not collide with one containing
            // `.`.
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    dir.join(format!("energy-reference-{encoded}.toml"))
}

/// Reads a meter's monotonicity reference, or `None` if there is not a usable one.
///
/// # A failed load is ABSENT, not fatal — decided at drafting (Story 2.3 AC5)
///
/// The tempting alternative is to refuse to start: an unreadable reference means
/// the next reading cannot be judged, and this repository's instinct is to stop
/// rather than to publish something it cannot vouch for.
///
/// It is the wrong call here, and the reasoning is worth keeping. A corrupt
/// state file would take a WORKING FLEET off the wire — every meter silent,
/// including the three whose references are fine — to prevent one meter's first
/// reading going unjudged. The bridge would then need a human before it published
/// anything at all, which is a worse failure than the one being prevented, and it
/// is precisely the shape ADR 0026 refused for the configuration.
///
/// What is NOT acceptable is silence about it, so the `warn` names the meter and
/// the error. The meter starts exactly as a brand-new one does — unjudged for one
/// reading, then judged for ever after.
fn load_energy_reference(dir: &std::path::Path, meter: &MeterId) -> Option<crate::domain::Kwh> {
    let path = reference_path_for(dir, meter);
    if !path.exists() {
        // The ordinary first run. Not a fault, and not worth a warning.
        return None;
    }
    match crate::persist::load::<PersistedReference>(&path) {
        Ok(persisted) if persisted.meter != meter.to_string() => {
            // The path is collision-free, so reaching this means the file was
            // moved rather than mis-derived: copied while repairing a
            // deployment, restored from a backup, or renamed by hand. Refusing
            // it costs one unjudged reading; accepting it judges a live counter
            // against a different meter's index, which is the failure this
            // check exists for.
            tracing::warn!(
                meter = %meter,
                stored = %persisted.meter,
                path = %path.display(),
                "this reference file belongs to another meter; ignoring it. This \
                 meter's first reading will go unjudged, as if it had never been \
                 read"
            );
            None
        }
        Ok(persisted) if persisted.energy_kwh.is_finite() => {
            tracing::info!(
                meter = %meter,
                reference = persisted.energy_kwh,
                "restored the energy-monotonicity reference across the restart"
            );
            Some(crate::domain::Kwh(persisted.energy_kwh))
        }
        Ok(persisted) => {
            // A non-finite reference would disable the oracle for this meter
            // without saying so — `x < NaN` is false for every x.
            tracing::warn!(
                meter = %meter,
                reference = persisted.energy_kwh,
                "the stored energy reference is not a finite number; this meter's \
                 first reading will go unjudged, as if it had never been read"
            );
            None
        }
        Err(error) => {
            tracing::warn!(
                meter = %meter, %error, path = %path.display(),
                "no readable energy-monotonicity reference; this meter's first \
                 reading will go unjudged, as if it had never been read. The \
                 bridge keeps publishing: a corrupt file for one meter must not \
                 take the fleet off the wire"
            );
            None
        }
    }
}

/// Writes a meter's reference, best-effort.
///
/// **Failure is logged and swallowed, deliberately.** This runs on the publish
/// path: propagating a full disk here would stop a meter that is otherwise
/// reading and publishing perfectly, to protect a check that only matters across
/// a restart. The cost of the swallow is bounded and stated — the reference
/// reverts to whatever was last written, and at worst to absent.
fn store_energy_reference(dir: &std::path::Path, meter: &MeterId, energy: crate::domain::Kwh) {
    let path = reference_path_for(dir, meter);
    if let Err(error) = crate::persist::persist_atomic(
        &path,
        &PersistedReference {
            meter: meter.to_string(),
            energy_kwh: energy.0,
        },
    ) {
        tracing::warn!(
            meter = %meter, %error, path = %path.display(),
            "could not persist the energy-monotonicity reference; a restart will \
             leave this meter's first reading unjudged"
        );
    }
}

/// The task: loops until the outbox closes.
pub async fn run<S: Source + Send>(
    meter: MeterId,
    mut source: S,
    clock: Arc<dyn Clock + Send + Sync>,
    config: crate::app::supervisor::ConfigHandle,
    // The whole collection, not this meter's tick: the task also records its
    // oracle verdict here, so anything that reports on the bridge reads the same
    // cell the task writes rather than a second opinion.
    pulse: Heartbeats,
    outbox: mpsc::Sender<MeterUpdate>,
    // Where this meter's monotonicity reference is kept across restarts (Story
    // 2.3 AC5). The directory, not the file: the file name is derived from the
    // meter id so that a corrupt one costs exactly the meter it belongs to.
    reference_dir: std::path::PathBuf,
) {
    let heartbeat = pulse.of(&meter).unwrap_or_else(|| {
        panic!("no heartbeat for {meter}; the collection is built from the served meters")
    });
    let mut state = State::initial();
    // The monotonicity reference (Story 2.2), deliberately beside `last` rather
    // than inside it: `last` is what we would republish, this is what we judge
    // against.
    //
    // RESTORED FROM DISK since Story 2.3 (AC5). Until then it started at `None`
    // on every boot, and `None` means "no accepted reading yet", which the oracle
    // correctly treats as unjudgeable — so the FIRST reading after a restart was
    // never compared to anything and silently became the new baseline.
    //
    // That is the one window where a counter is most likely to have moved: a
    // maintenance visit in which somebody touched both the bridge and the meter.
    // Restarts are routine here — any `Cost::ProcessRestart` configuration change
    // performs one, and Epic 7 will wire `/healthz` to an automatic one — so the
    // gap was not rare, it was scheduled.
    let mut energy_reference = load_energy_reference(&reference_dir, &meter);
    // Story 2.6: a wait the SOURCE asked for, per meter, monotonic so a wall
    // clock correction cannot shorten or extend it.
    let mut rate_limited_until: Option<MonotonicMs> = None;
    // The last measurement this meter produced, carried so a failed tick can
    // publish a verdict about it rather than say nothing (Story 3.2).
    let mut last: Option<crate::domain::Measurement> = None;
    // The period is READ FROM THE HANDLE, not captured (Story 5.2 AC4).
    //
    // `tokio::time::interval` fixes its period at construction, so a hot change
    // means noticing and rebuilding — there is no setter that would not also
    // reset the schedule. Rebuilding only when the value actually differs is
    // what keeps an unrelated reconfiguration from silently restarting the
    // cadence.
    let mut period = config.load().poll.interval;
    let mut ticker = tokio::time::interval(period);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if outbox.is_closed() {
            tracing::info!(meter = %meter, "outbox closed; poll task stopping");
            return;
        }
        let current = config.load();
        if current.poll.interval != period {
            tracing::info!(
                from_secs = period.as_secs(),
                to_secs = current.poll.interval.as_secs(),
                "publish period changed; the next tick uses the new one"
            );
            period = current.poll.interval;
            ticker = tokio::time::interval(period);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // `interval` fires immediately on its first tick, and that first tick
            // is this rebuild's — so it is consumed here rather than letting a
            // period change slip an extra unscheduled poll into the loop.
            ticker.tick().await;
        }
        let ctx = Context {
            meter: &meter,
            clock: clock.as_ref(),
            policy: current.policy,
            config: current.poll,
            heartbeat: &heartbeat,
            outbox: &outbox,
        };
        let published;
        let before = energy_reference;
        (state, published) = step_once(
            &ctx,
            &mut source,
            state,
            &mut last,
            &mut energy_reference,
            &mut rate_limited_until,
        )
        .await;
        // Persisted only when it MOVED, so a quiet meter does not rewrite the
        // same number every period — an fsync per meter per cycle for a value
        // that did not change.
        if energy_reference != before
            && let Some(energy) = energy_reference
        {
            store_energy_reference(&reference_dir, &meter, energy);
        }
        // The verdict reaches anything outside that can report on this meter —
        // BOTH halves of it since Story 2.3, so a screen cannot call a meter
        // healthy while the broker is being told otherwise ([#62]).
        pulse.record(&meter, state, published);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::clock::FakeClock;
    use crate::core::source::{FakeSource, Reading};
    use crate::domain::{Kw, Kwh, Measurement, Quality, Serial, UtcMillis};

    const SANE_NOW: i64 = 1_784_984_793_000;
    const BASE: i64 = 1_784_984_700_000;

    /// A scratch directory belonging to THIS test binary and this purpose.
    ///
    /// **One helper rather than five literals, since 2026-08-12.** The story 2.3
    /// review fixed a fixed `/tmp` path in `a_hanging_meter_does_not_cost_the_others_their_cadence`
    /// and wrote the reason beside it — *"two concurrent `cargo test` runs cannot
    /// couple through the filesystem"* — while leaving four others in this file
    /// untouched, two of them added by that same commit. Reviewing the fix caught
    /// the class the fix had named and not applied.
    ///
    /// It matters most for `a_reference_file_belongs_to_exactly_one_meter`, which
    /// stores, renames and reloads a reference under one meter's name: a second
    /// run interleaving with the rename does not fail cleanly, it fails as a
    /// wrong verdict about which meter owns a file. `std::process::id` is the
    /// right grain — the tasks within one run share the directory on purpose.
    fn scratch_dir(purpose: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("smartme_{purpose}_{}", std::process::id()))
    }

    fn config() -> PollConfig {
        PollConfig {
            interval: Duration::from_secs(5),
            fetch_timeout: Duration::from_secs(2),
        }
    }

    fn policy() -> Policy {
        Policy::DEFAULT
    }

    fn reading(quality: Quality, age_ms: i64) -> Reading {
        Reading {
            value: Measurement {
                meter: MeterId::new("garage"),
                serial: Serial::new("30000001"),
                power: Some(Kw(0.018)),
                energy: Some(Kwh(4_843.822)),
                value_date: UtcMillis(BASE),
                quality,
            },
            http_date: Some(UtcMillis(BASE + age_ms)),
            faults: crate::core::source::SourceFaults::NONE,
        }
    }

    /// A reading whose POWER field the source could not read, its energy index
    /// perfectly sound. The shape story 2.5 exists for.
    fn reading_with_unreadable_power(age_ms: i64) -> Reading {
        let mut r = reading(Quality::Good, age_ms);
        r.value.power = None;
        r.faults = crate::core::source::SourceFaults {
            reading: None,
            power: Some(Cause::UnitNotRecognised),
            energy: None,
        };
        r
    }

    /// **Story 2.5 AC1, FROM THE SOURCE TO THE PUBLISHED METRICS.**
    ///
    /// A field the source could not read degrades that field and leaves its
    /// neighbour alone — asserted where a consumer would see it, not on the
    /// in-process verdict. **The story names this trap and story 2.3's review
    /// found it for real one layer up**: every test reaching `metrics_for` handed
    /// it a `Verdicts::uniform`, where the old and new code agree on every output,
    /// so reverting the whole of ADR 0031 left the suite green.
    ///
    /// FALSIFIED 2026-08-12, three mutations, each red on its own assertion:
    /// scoping the adapter's fault to the READING instead of the metric
    /// (`Judgement::about_reading`) — the `Energy` value assertion goes red, the
    /// index nulled for a fault in its neighbour, which is the pre-2.5 wire;
    /// restoring `Quality::Bad` for a single failed field in `map_device` — same;
    /// making `metrics_for` read `verdicts.meter()` — same.
    #[tokio::test]
    async fn an_unreadable_field_is_refused_alone_all_the_way_to_the_wire() {
        let (_, sent) =
            drive_sequence(FakeSource::new().then(Ok(reading_with_unreadable_power(950)))).await;
        assert_eq!(sent.len(), 1);

        // THE CORE composed it per metric.
        assert_eq!(sent[0].published_for(Measured::Power), Quality::Bad);
        assert_eq!(
            sent[0].verdicts.for_metric(Measured::Power).cause(),
            Some(Cause::UnitNotRecognised),
            "the operator is sent to smart-me's unit contract, not to a meter"
        );
        assert_eq!(
            sent[0].published_for(Measured::Energy),
            Quality::Good,
            "the energy index was read and converted perfectly; degrading it for \
             a fault in its neighbour is exactly what ADR 0031 removed downstream \
             and story 2.5 removes here"
        );

        // AND THE WIRE SAYS THE SAME THING. This is the half story 2.3's review
        // found missing.
        let metrics = crate::adapters::sparkplug_publisher::metrics_for_test(
            &sent[0].measurement,
            sent[0].verdicts,
        );
        let power = metrics
            .iter()
            .find(|m| m.name == "Power")
            .expect("power is published");
        let energy = metrics
            .iter()
            .find(|m| m.name == "Energy")
            .expect("energy is published");
        assert!(
            matches!(power.value, sparkplug_b::model::MetricValue::Null(_)),
            "a refused field withholds its number — and there is no substituted \
             one left to publish. Got {:?}",
            power.value
        );
        assert_eq!(
            power.properties,
            vec![("Cause".to_string(), "unit-not-recognised".to_string())]
        );
        assert!(
            matches!(energy.value, sparkplug_b::model::MetricValue::Double(v) if v == 4_843.822),
            "the sound index reaches the consumer at full value. Got {:?}",
            energy.value
        );
        assert!(
            energy.properties.is_empty(),
            "and carries no cause — least of all its neighbour's"
        );
    }

    /// **The yardstick follows the ENERGY metric, not the meter** — the review of
    /// story 2.5 found it following the worst of both, on 2026-08-12.
    ///
    /// An unreadable power unit made the composed METER verdict `Bad`, so a
    /// perfectly readable energy index never became the reference. A genuine
    /// counter reset afterwards was then judged against a frozen index and could
    /// publish `Good`: **FR15 defeated by the oracle's own bookkeeping**, which is
    /// the failure story 2.3's review named for the replay case and this story
    /// re-introduced through a different door — in the very change whose subject
    /// is that metrics are independent.
    ///
    /// FALSIFIED 2026-08-12, run before the fix and again after: reading the
    /// meter verdict instead of the energy metric's leaves the third assertion
    /// with `cause: None`, the reset silently blessed.
    #[tokio::test]
    async fn a_readable_index_becomes_the_yardstick_even_when_its_neighbour_is_not() {
        let mut broken_power = reading_with_unreadable_power(950);
        broken_power.value.energy = Some(Kwh(1_000.0));
        let mut after_reset = reading(Quality::Good, 950);
        after_reset.value.energy = Some(Kwh(12.0));

        let (_, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(broken_power))
                .then(Ok(after_reset)),
        )
        .await;
        assert_eq!(sent.len(), 2, "every tick publishes a verdict (ADR 0027)");

        // The premise: the first reading's POWER is refused and its ENERGY is not.
        assert_eq!(sent[0].published_for(Measured::Power), Quality::Bad);
        assert_eq!(sent[0].published_for(Measured::Energy), Quality::Good);

        // And the index it carried is the one the next reading is judged against.
        assert_eq!(
            sent[1].verdicts.for_metric(Measured::Energy).cause(),
            Some(Cause::CounterWentBackwards),
            "1000 kWh was read and converted perfectly; a fault in the POWER field \
             must not stop it becoming the yardstick, or the drop to 12 is \
             published as a valid measurement"
        );
    }

    /// **Story 2.6 AC3 — a rate limit is honoured WITHOUT the cycle going silent.**
    ///
    /// The source asks for 60 s; the next tick must not fetch, and must still
    /// publish. Skipping the publication would make a rate limit look like
    /// silence, which is the failure this project exists to prevent (ADR 0027).
    ///
    /// FALSIFIED 2026-08-12, run before this note was written:
    ///  - deleting the `waiting` guard lets the second tick call the source, and
    ///    the fetch-count assertion goes red with `2` where `1` was owed;
    ///  - returning early instead of synthesising a tick makes `sent.len()` 1,
    ///    and the assertion names ADR 0027.
    #[tokio::test]
    async fn a_rate_limit_is_waited_out_without_the_cycle_going_silent() {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let mut source = FakeSource::new()
            .then(Ok(reading(Quality::Good, 950)))
            .then(Err(SourceError::RateLimited {
                retry_after: Some(Duration::from_secs(60)),
            }))
            .then(Ok(reading(Quality::Good, 950)));
        let (tx, mut rx) = mpsc::channel(8);
        let meter = MeterId::new("garage");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let heartbeat = beats.of(&meter).expect("served");
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };
        let (mut last, mut energy, mut until) = (None, None, None);

        // A good reading first: `last` must hold something, or a silent cycle has
        // nothing to republish and ADR 0027's certificate path is a different test.
        let (state, _) = step_once(
            &ctx,
            &mut source,
            State::initial(),
            &mut last,
            &mut energy,
            &mut None,
        )
        .await;
        let (_, first) =
            step_once(&ctx, &mut source, state, &mut last, &mut energy, &mut until).await;
        assert_eq!(first.cause(), Some(Cause::SourceRateLimited));
        assert!(
            until.is_some(),
            "the wait is armed by the server's own delay"
        );

        // The clock has NOT advanced past the deadline, so no fetch may happen.
        let fetches_before = source.calls.len();
        let (_, second) = step_once(
            &ctx,
            &mut source,
            State::Stale,
            &mut last,
            &mut energy,
            &mut until,
        )
        .await;
        assert_eq!(
            source.calls.len(),
            fetches_before,
            "no fetch may be attempted before the instant the source named"
        );
        assert_eq!(
            second.cause(),
            Some(Cause::SourceRateLimited),
            "and the cycle still publishes a verdict — ADR 0027 forbids silence, \
             so a rate limit must look like a rate limit rather than like nothing"
        );

        drop(tx);
        let mut published = 0;
        while rx.recv().await.is_some() {
            published += 1;
        }
        assert_eq!(published, 3, "three cycles, three verdicts (ADR 0027)");
    }

    /// A reading with a chosen energy index, for the monotonicity tests.
    fn reading_with_energy(quality: Quality, age_ms: i64, energy: f64) -> Reading {
        let mut r = reading(quality, age_ms);
        r.value.energy = Some(Kwh(energy));
        r
    }

    /// **Story 2.2 AC1 and AC3** — a counter that goes backwards is published
    /// `Bad`, and the meter is not stuck there.
    ///
    /// Driven through `step_once` rather than against the oracle directly: what
    /// matters is that a READING judged in the pipeline reaches the outbox with
    /// that verdict, not that a comparison compares. The repository has twice been
    /// caught by a test that asserted an implementation against itself.
    ///
    /// The recovery half (AC3) is the one that would be skipped, and it is the one
    /// that protects a replaced meter: it legitimately reads lower for ever after,
    /// so keeping the old reference would mark every later reading `Bad` against
    /// an index that no longer exists.
    ///
    /// FALSIFIED 2026-08-10, three mutations, each red on its own assertion:
    /// removing the comparison (`_ => Verdict::good()` for every arm) leaves the
    /// backwards reading `Good`; flipping it to `>` marks the RISING reading bad
    /// and the falling one good; and refusing to adopt the new index — keeping the
    /// reference at the pre-drop value — leaves the third reading `Bad`, which is
    /// exactly the stuck meter AC3 forbids.
    #[tokio::test]
    async fn a_counter_that_goes_backwards_is_bad_once_and_then_recovers() {
        let meter = MeterId::new("garage");
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let beats = Heartbeats::for_meters([MeterId::new("garage")]);
        let heartbeat = beats.of(&MeterId::new("garage")).expect("present");
        let (tx, mut rx) = mpsc::channel(8);
        let mut source = FakeSource::new()
            .then(Ok(reading_with_energy(Quality::Good, 950, 4_843.822)))
            .then(Ok(reading_with_energy(Quality::Good, 950, 12.0)))
            .then(Ok(reading_with_energy(Quality::Good, 950, 12.5)));
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };

        let mut last = None;
        let mut energy = None;

        // THE PREMISE: a rising counter is Good, or the Bad below proves nothing.
        let (s1, _) = step_once(
            &ctx,
            &mut source,
            State::initial(),
            &mut last,
            &mut energy,
            &mut None,
        )
        .await;
        assert_eq!(s1, State::Fresh);

        // The drop. The STATE is kept and asserted since 2026-08-11 (deferred
        // review patch): `let _ =` discarded exactly the value that would have
        // shown the wire and the operator surfaces disagreeing, which is what let
        // that divergence live until the review found it by reading.
        let (s2, _) = step_once(&ctx, &mut source, s1, &mut last, &mut energy, &mut None).await;
        assert_eq!(
            s2,
            State::Fresh,
            "a backwards counter is a VALUE fault, not an identity one: the meter              stays in the freshness machine's `Fresh` and keeps polling. What must              NOT happen is this state reaching an operator surface unaccompanied —              see the published verdict asserted below, and `MeterState::published`"
        );

        // And a reading consistent with the NEW index.
        let (s3, _) = step_once(
            &ctx,
            &mut source,
            State::initial(),
            &mut last,
            &mut energy,
            &mut None,
        )
        .await;
        assert_eq!(s3, State::Fresh);
        drop(tx);

        let mut got = Vec::new();
        while let Some(u) = rx.recv().await {
            got.push(u);
        }
        assert_eq!(got.len(), 3, "every tick publishes a verdict (ADR 0027)");

        assert_eq!(got[0].published(), Quality::Good, "the premise");
        assert_eq!(got[0].verdict().cause(), None);

        assert_eq!(
            got[1].published(),
            Quality::Bad,
            "a counter that went backwards must not be published as a valid \
             measurement: a consumer differencing these two indices would get a \
             negative delta and no reason to distrust it"
        );
        assert_eq!(
            got[1].verdict().cause(),
            Some(crate::core::oracle::Cause::CounterWentBackwards)
        );

        assert_eq!(
            got[2].published(),
            Quality::Good,
            "AC3: the new index became the reference, so the meter recovers. A \
             replaced meter reads lower for ever after, and staying Bad would take \
             a working meter off the wire until somebody restarted the container"
        );
    }

    /// **Story 2.2, Task 3** — a reading that is BOTH too old and backwards
    /// publishes the worse of the two.
    ///
    /// This is Story 2.1's composition rule meeting its first real second oracle.
    /// Publishing `Stale` here — the freshness verdict, because freshness is
    /// consulted first — would be the rule broken by its own first user, and no
    /// other test in the tree would notice: both verdicts are non-good, so any
    /// "is it degraded?" assertion passes either way.
    #[tokio::test]
    async fn a_reading_that_is_both_stale_and_backwards_publishes_the_worse() {
        let meter = MeterId::new("garage");
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let beats = Heartbeats::for_meters([MeterId::new("garage")]);
        let heartbeat = beats.of(&MeterId::new("garage")).expect("present");
        let (tx, mut rx) = mpsc::channel(8);
        let mut source = FakeSource::new()
            .then(Ok(reading_with_energy(Quality::Good, 950, 4_843.822)))
            // Older than the allowance AND a lower index.
            .then(Ok(reading_with_energy(Quality::Good, 600_000, 12.0)));
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };

        let mut last = None;
        let mut energy = None;
        let (s1, _) = step_once(
            &ctx,
            &mut source,
            State::initial(),
            &mut last,
            &mut energy,
            &mut None,
        )
        .await;
        let _ = step_once(&ctx, &mut source, s1, &mut last, &mut energy, &mut None).await;
        drop(tx);

        let mut got = Vec::new();
        while let Some(u) = rx.recv().await {
            got.push(u);
        }
        // ADDED 2026-08-11 (deferred review patch). Without it this test dies on
        // an index panic if the second update stops being emitted — the exact
        // regression ADR 0027 exists to prevent — instead of failing on the
        // assertion that names the property. Its sibling above always had it.
        assert_eq!(
            got.len(),
            2,
            "every tick publishes a verdict (ADR 0027); a missing update must              fail HERE, naming the rule, rather than as an index panic below"
        );
        assert_eq!(
            got[1].published(),
            Quality::Bad,
            "worst wins: Stale from freshness, Bad from monotonicity"
        );
        assert_eq!(
            got[1].verdict().cause(),
            Some(crate::core::oracle::Cause::CounterWentBackwards),
            "and the cause travels with the quality it belongs to, not with \
             whichever oracle was consulted first"
        );
    }

    /// **Story 3.2 AC1 and AC2** — a meter that answered and then stopped must be
    /// published stale, not withheld.
    ///
    /// The lie this closes: its last DDATA said `Good`, then nothing followed, and
    /// silence on a Sparkplug wire is indistinguishable from *"nothing has
    /// changed"* — so the host went on showing that value at that quality
    /// indefinitely. Until 2026-08-07 the task sent an update only when the fetch
    /// SUCCEEDED, under a comment promising a republish no code performed.
    ///
    /// **The premise is checked, not assumed.** The first tick must actually reach
    /// the wire as `Good`, or the second assertion would be about a stream that
    /// never flowed — the shape that made three of story 3.1's attempts worthless.
    ///
    /// AC2 is asserted as the QUALITY and the VALUE, not as "a message appeared":
    /// a republish that emitted `Good`, or a synthesised zero, would satisfy a
    /// count.
    ///
    /// FALSIFIED 2026-08-07 by restoring the `if let Ok(reading)` guard, so a
    /// failed tick publishes nothing. Copied from the run:
    ///
    /// ```text
    /// test app::poll_publish::tests::a_meter_that_goes_silent_is_republished_stale ... FAILED
    ///
    /// thread '…a_meter_that_goes_silent_is_republished_stale' (57) panicked at
    /// crates/smartme-bridge/src/app/poll_publish.rs:413:9:
    /// assertion `left == right` failed: a meter that stops answering must be published
    /// stale, not withheld: the host otherwise keeps showing its last Good value for ever
    ///   left: 1
    ///  right: 2
    /// ```
    ///
    /// `left: 1` is the premise reading alone — so the mutation removed exactly the
    /// republish and nothing else.
    #[tokio::test]
    async fn a_meter_that_goes_silent_is_republished_stale() {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let beats = Heartbeats::for_meters([MeterId::new("garage")]);
        let heartbeat = beats.of(&MeterId::new("garage")).expect("present");
        let (tx, mut rx) = mpsc::channel(8);
        let meter = MeterId::new("garage");
        let mut source = FakeSource::new()
            .then(Ok(reading(Quality::Good, 950)))
            .then(Err(SourceError::Timeout));
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };

        let mut last = None;
        let (good, _) = step_once(
            &ctx,
            &mut source,
            State::initial(),
            &mut last,
            &mut None,
            &mut None,
        )
        .await;
        assert_eq!(
            good,
            State::Fresh,
            "the premise: the meter must first be proven fresh"
        );

        let (after, _) = step_once(&ctx, &mut source, good, &mut last, &mut None, &mut None).await;
        assert_eq!(after, State::Stale);
        drop(tx);

        let mut got = Vec::new();
        while let Some(u) = rx.recv().await {
            got.push(u);
        }
        assert_eq!(
            got.len(),
            2,
            "a meter that stops answering must be published stale, not withheld: the \
             host otherwise keeps showing its last Good value for ever"
        );
        assert_eq!(
            got[0].published(),
            Quality::Good,
            "the premise reached the wire"
        );
        assert_eq!(
            got[1].published(),
            Quality::Stale,
            "the republish carries the ORACLE's new verdict; re-asserting Good would \
             be the same lie with more messages"
        );
        assert_eq!(
            got[1].measurement, got[0].measurement,
            "the value and its ValueDate are the last KNOWN ones, untouched — not a \
             synthesised zero, and not stamped `now`, which would turn an outage \
             into a fresh-looking reading"
        );
    }

    /// **Story 3.2 AC4** — a meter that has never answered is given nothing.
    ///
    /// Guy's fourth meter is unplugged, permanently. Its DBIRTH already declares it
    /// valueless and non-good, which is true; the risk this test guards is a
    /// republish path that reaches for *something* to send.
    ///
    /// FALSIFIED 2026-08-07 by making the `None` arm forward a default
    /// `Measurement`. Copied from the run:
    ///
    /// ```text
    /// test app::poll_publish::tests::a_meter_that_never_answered_is_given_no_value ... FAILED
    ///
    /// thread '…a_meter_that_never_answered_is_given_no_value' (57) panicked at
    /// crates/smartme-bridge/src/app/poll_publish.rs:470:9:
    /// assertion `left == right` failed: a meter with no reading has no value to
    /// re-publish; inventing one is the defect this story fixes wearing its other face
    ///   left: 1
    ///  right: 0
    /// ```
    #[tokio::test]
    async fn a_meter_that_never_answered_is_given_no_value() {
        let (state, got) = drive(FakeSource::new().then(Err(SourceError::Timeout))).await;
        assert_eq!(state, State::Stale);
        assert_eq!(
            got.len(),
            0,
            "a meter with no reading has no value to re-publish; inventing one is \
             the defect this story fixes wearing its other face"
        );
    }

    /// **Story 2.3 AC1** — a fault in one metric does not withhold the other.
    ///
    /// The whole subject of the story, asserted where it is observable: on the
    /// update that reaches the outbox. Before 2.3 one verdict belonged to the
    /// READING, so a backwards energy index published `Power = null` stamped
    /// `counter-went-backwards` — a number the bridge had no complaint about,
    /// withheld and then blamed for its neighbour's fault.
    ///
    /// FALSIFIED 2026-08-11: scoping the monotonicity judgement to the reading
    /// (`Judgement::about_reading` instead of `about(Measured::Energy, …)`) —
    /// which is exactly the pre-2.3 behaviour — turns the two `Power` assertions
    /// red, on quality and on cause.
    #[tokio::test]
    async fn a_backwards_energy_index_does_not_withhold_the_power_reading() {
        let (_, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(reading_with_energy(Quality::Good, 950, 4_843.822)))
                .then(Ok(reading_with_energy(Quality::Good, 950, 12.0))),
        )
        .await;
        assert_eq!(sent.len(), 2, "every tick publishes a verdict (ADR 0027)");

        let refused = &sent[1];

        // The energy index is refused, and says why.
        assert_eq!(refused.published_for(Measured::Energy), Quality::Bad);
        assert_eq!(
            refused.verdicts.for_metric(Measured::Energy).cause(),
            Some(Cause::CounterWentBackwards)
        );

        // The power value was never judged by that oracle, and is untouched.
        assert_eq!(
            refused.published_for(Measured::Power),
            Quality::Good,
            "the monotonicity oracle looked at the energy index and at nothing \
             else; withholding a current power value because of it publishes a \
             fault where there is none"
        );
        assert_eq!(
            refused.verdicts.for_metric(Measured::Power).cause(),
            None,
            "and a good metric carries no cause — least of all its neighbour's"
        );

        // The METER, though, is not healthy. This is the distinction the story
        // exists to make: per-metric on the wire, worst-of for the meter.
        assert_eq!(
            refused.published(),
            Quality::Bad,
            "an operator surface must not call this meter healthy just because \
             one of its two numbers survived"
        );
    }

    /// **Story 2.3 AC4** — a reading the bridge refused never becomes the value
    /// republished later.
    ///
    /// The live defect the 2026-08-11 review found in story 2.2, and the reason
    /// this story is not only a refactor. `last` was adopted on EVERY successful
    /// fetch, including one the oracle refused — so the substituted
    /// `BAD_CARRIER = 0.0` of a failed unit conversion sat in `last`, and the
    /// next timeout republished it as a genuine `Double` marked `Stale`. A
    /// consumer differencing `4843.822 → 0.0` gets −4843.8 under a flag that says
    /// the network hiccuped: the exact harm FR15 exists to prevent, produced by
    /// the code that prevents it.
    ///
    /// FALSIFIED 2026-08-11: restoring the unconditional `*last = Some(...)` on
    /// `Ok` makes the last assertion red with `0.0` — the defect, reproduced.
    #[tokio::test]
    async fn a_refused_reading_is_never_what_gets_republished() {
        let (_, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(reading_with_energy(Quality::Good, 950, 4_843.822)))
                // The unit was unreadable: the source substitutes BAD_CARRIER and
                // marks the value Bad (`smartme_source.rs`). This IS published,
                // marked and with null values — refusing a reading does not mean
                // hiding it.
                .then(Ok(reading_with_energy(Quality::Bad, 950, 0.0)))
                // And then the cloud goes quiet, which is when `last` speaks.
                .then(Err(SourceError::Timeout)),
        )
        .await;
        assert_eq!(sent.len(), 3);

        assert_eq!(sent[0].published(), Quality::Good, "the premise");
        assert_eq!(sent[1].published(), Quality::Bad, "the refusal");

        assert_eq!(
            sent[2].published(),
            Quality::Stale,
            "a silent cloud republishes the last known reading, marked (ADR 0027)"
        );
        assert_eq!(
            sent[2].measurement.energy,
            Some(Kwh(4_843.822)),
            "the republished value must be the last reading the bridge ACCEPTED. \
             Republishing the refused one hands over 0.0 as a real Double under a \
             `Stale` flag, and a consumer differencing it gets a delta of \
             −4843.8 with nothing to warn it"
        );
    }

    /// **Story 2.3 AC5** — the monotonicity reference survives a restart, and a
    /// meter that was reset while the bridge was down is caught.
    ///
    /// The window this closes is not a rare one. `energy_reference` started at
    /// `None` on every boot, and `None` means *"no accepted reading yet"* — which
    /// the oracle correctly treats as unjudgeable, so the first reading after a
    /// restart silently became the new baseline whatever it said. A maintenance
    /// visit in which somebody touches both the bridge and the meter is exactly
    /// when a counter is most likely to have moved, and Epic 7 will make restarts
    /// automatic.
    ///
    /// The restart is simulated the only honest way: a fresh `energy_reference`
    /// binding, loaded from disk exactly as `run` loads it. Nothing is carried in
    /// memory across the two halves.
    ///
    /// FALSIFIED 2026-08-11 by skipping the `store_energy_reference` call: the
    /// second half then loads `None`, judges nothing, and publishes the reset
    /// index as `Good` — the defect, reproduced with the persistence removed.
    #[tokio::test]
    async fn the_monotonicity_reference_survives_a_restart() {
        let dir = scratch_dir("reference_restart");
        let _ = std::fs::create_dir_all(&dir);
        let meter = MeterId::new("garage");
        let _ = std::fs::remove_file(reference_path_for(&dir, &meter));

        // BEFORE THE RESTART: one accepted reading at 900_000 kWh.
        {
            let clock = FakeClock::new(UtcMillis(SANE_NOW));
            let beats = Heartbeats::for_meters([meter.clone()]);
            let heartbeat = beats.of(&meter).expect("present");
            let (tx, _rx) = mpsc::channel(8);
            let ctx = Context {
                meter: &meter,
                clock: &clock,
                policy: policy(),
                config: config(),
                heartbeat: &heartbeat,
                outbox: &tx,
            };
            let mut source =
                FakeSource::new().then(Ok(reading_with_energy(Quality::Good, 950, 900_000.0)));
            let mut last = None;
            let mut reference = load_energy_reference(&dir, &meter);
            let (_, _) = step_once(
                &ctx,
                &mut source,
                State::initial(),
                &mut last,
                &mut reference,
                &mut None,
            )
            .await;
            let energy = reference.expect("a good reading is adopted as the reference");
            store_energy_reference(&dir, &meter, energy);
        }

        // THE RESTART. Everything in memory is gone; only the file remains.
        let restored = load_energy_reference(&dir, &meter);
        assert_eq!(
            restored,
            Some(crate::domain::Kwh(900_000.0)),
            "the reference must come back from disk, or the first reading after \
             every restart goes unjudged"
        );

        // AFTER THE RESTART: the meter was replaced while we were down.
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let beats = Heartbeats::for_meters([meter.clone()]);
        let heartbeat = beats.of(&meter).expect("present");
        let (tx, mut rx) = mpsc::channel(8);
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };
        let mut source = FakeSource::new().then(Ok(reading_with_energy(Quality::Good, 950, 12.0)));
        let mut last = None;
        let mut reference = restored;
        let _ = step_once(
            &ctx,
            &mut source,
            State::initial(),
            &mut last,
            &mut reference,
            &mut None,
        )
        .await;
        drop(tx);

        let update = rx.recv().await.expect("the reading was published");
        assert_eq!(
            update.published_for(Measured::Energy),
            Quality::Bad,
            "a meter that was reset while the bridge was down must be caught. \
             Without the restored reference this reads Good and 12.0 kWh becomes \
             the new baseline, with a consumer differencing 900000 -> 12"
        );
        assert_eq!(
            update.verdicts.for_metric(Measured::Energy).cause(),
            Some(Cause::CounterWentBackwards)
        );

        let _ = std::fs::remove_file(reference_path_for(&dir, &meter));
    }

    /// A reference file that cannot be read costs its own meter and nothing else
    /// (Story 2.3 AC5, the decision taken at drafting).
    ///
    /// Refusing to start was the tempting alternative and is the wrong call: a
    /// corrupt file would take a WORKING FLEET off the wire — every meter silent,
    /// including those whose references are fine — to prevent one meter's first
    /// reading going unjudged. The bridge would then need a human before it
    /// published anything, which is a worse failure than the one prevented, and
    /// the shape ADR 0026 already refused for the configuration.
    ///
    /// FALSIFIED 2026-08-11 (added by this story's review, which found this the
    /// one new test carrying no falsification note): returning
    /// `Some(Kwh(persisted.energy_kwh))` from the `Err` arm instead of `None`
    /// makes the corrupt-file assertion red; dropping the `is_finite` guard makes
    /// the `nan` assertion red. Both matter — a NaN reference is worse than none,
    /// since `x < NaN` is false for every x and the oracle goes quiet.
    #[test]
    fn an_unreadable_reference_is_absent_rather_than_fatal() {
        let dir = scratch_dir("reference_corrupt");
        let _ = std::fs::create_dir_all(&dir);
        let meter = MeterId::new("garage");

        std::fs::write(reference_path_for(&dir, &meter), b"this is not toml {{{").expect("written");
        assert_eq!(
            load_energy_reference(&dir, &meter),
            None,
            "a corrupt reference reads as absent — the meter starts unjudged for \
             one reading, exactly as a brand-new meter does"
        );

        // A non-finite reference is refused too: `x < NaN` is false for every x,
        // so keeping it would disable the oracle for this meter with no signal.
        std::fs::write(reference_path_for(&dir, &meter), b"energy_kwh = nan\n").expect("written");
        assert_eq!(load_energy_reference(&dir, &meter), None);

        let _ = std::fs::remove_file(reference_path_for(&dir, &meter));
    }

    /// **Story 2.2 AC6's third mutation, playable at last** — the reference does
    /// not advance on a refused reading.
    ///
    /// AC6 named three mutations and only two were played: the third, *"letting
    /// the reference advance on a refused reading"*, was quietly replaced by a
    /// different one, and the 2026-08-11 review found the guard it aimed at
    /// covered by no test at all. Deleting it left everything green.
    ///
    /// It could not be played meaningfully before story 2.3, because the guard as
    /// written keyed on the SOURCE's quality — and the only refusal that reached
    /// it was one the source had already marked `Bad`, so the two rules agreed on
    /// every input. With adoption following the COMPOSED verdict, the sequence
    /// below separates them.
    ///
    /// The sequence: an accepted index at 4843.822; a reading whose unit could
    /// not be converted, carrying the substituted `0.0`; then a real reading at
    /// 4800. If the refused reading had become the reference, 4800 sits ABOVE it
    /// and publishes `Good` — a counter that dropped 43 kWh, blessed. Judged
    /// against the reference the bridge actually accepted, it is caught.
    ///
    /// FALSIFIED 2026-08-11 by making `adoptable` always `true`: the third
    /// reading comes back `Good` with no cause, which is the defect stated.
    #[tokio::test]
    async fn the_reference_does_not_advance_on_a_refused_reading() {
        let (_, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(reading_with_energy(Quality::Good, 950, 4_843.822)))
                .then(Ok(reading_with_energy(Quality::Bad, 950, 0.0)))
                .then(Ok(reading_with_energy(Quality::Good, 950, 4_800.0))),
        )
        .await;
        assert_eq!(sent.len(), 3, "every tick publishes a verdict (ADR 0027)");

        assert_eq!(sent[0].published(), Quality::Good, "the premise");

        // The refused reading names the fault the SOURCE found, not one an
        // ordering oracle invented about a value nobody claimed was a
        // measurement.
        assert_eq!(
            sent[1].verdicts.for_metric(Measured::Energy).cause(),
            Some(Cause::ValueUnusable),
            "a unit that could not be converted is `value-unusable`; reporting \
             `counter-went-backwards` would send an operator to the meter when \
             the fault is in the API contract"
        );

        // And the reference is still 4843.822, so the drop to 4800 is caught.
        assert_eq!(
            sent[2].published_for(Measured::Energy),
            Quality::Bad,
            "the reference must still be the last ACCEPTED index. Had the refused \
             reading's 0.0 become the reference, 4800 would sit above it and a \
             counter that lost 43 kWh would publish as a valid measurement"
        );
        assert_eq!(
            sent[2].verdicts.for_metric(Measured::Energy).cause(),
            Some(Cause::CounterWentBackwards)
        );
    }

    /// **The defect the 2026-08-11 review of this story found** — a value
    /// withheld for going backwards must not be republished a tick later.
    ///
    /// One flag governed both memories, and the `CounterWentBackwards` exemption
    /// — right for the yardstick — let the refused reading into `last` too. The
    /// sequence below is the one two review layers reconstructed independently,
    /// and it is reachable on this story's own AC5 path: a meter replaced while
    /// the bridge was down, then any timeout.
    ///
    /// What the wire did: `Energy = null, Bad, counter-went-backwards`, then one
    /// tick later `Energy = 12.0` as a genuine `Double` marked `Stale`, cause
    /// `source-unreachable`. The number the bridge had just refused to hand over,
    /// handed over under a transport fault, with nothing left saying why it had
    /// been refused.
    ///
    /// FALSIFIED 2026-08-11 by restoring the single flag (`last_adoptable` =
    /// `reference_adoptable`): the third publication carries `12.0` and the
    /// assertion names it.
    #[tokio::test]
    async fn a_value_withheld_for_going_backwards_is_not_republished_a_tick_later() {
        let (_, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(reading_with_energy(Quality::Good, 950, 4_843.822)))
                // The meter was replaced: the index drops, and the bridge refuses
                // to hand the number over.
                .then(Ok(reading_with_energy(Quality::Good, 950, 12.0)))
                // Then the cloud goes quiet, which is when `last` speaks.
                .then(Err(SourceError::Timeout)),
        )
        .await;
        assert_eq!(sent.len(), 3, "every tick publishes a verdict (ADR 0027)");

        assert_eq!(
            sent[1].published_for(Measured::Energy),
            Quality::Bad,
            "the premise: the drop is refused"
        );

        assert_eq!(
            sent[2].published(),
            Quality::Stale,
            "a silent cloud republishes the last known reading, marked"
        );
        assert_eq!(
            sent[2].measurement.energy,
            Some(Kwh(4_843.822)),
            "the republished value must be the last reading the bridge ACCEPTED. \
             Republishing the refused index hands over the very number withheld \
             one tick earlier, under a cause about the network — and the verdict \
             that refused it is gone by then"
        );
    }

    /// **Two meters whose ids differ only in punctuation do not share a file, and
    /// a file that belongs to another meter is refused** (Story 2.3 AC5).
    ///
    /// Both halves were found by this story's review. The first version mapped
    /// every character outside `[A-Za-z0-9_-]` to `_`, which is LOSSY: `gar age`,
    /// `gar.age` and `gar_age` all landed on one path. `config.rs` rejects only
    /// exact duplicate ids and applies no charset rule, so all three are
    /// configurable together — and two poll tasks would then write one file, each
    /// meter judged after a restart against whichever wrote last. That is the
    /// per-meter isolation of stories 3.1-3.3 broken through the filesystem, in
    /// the one place where being wrong means missing a counter reset.
    ///
    /// The second half is the guard the comment CLAIMED existed and did not: the
    /// meter id written inside and checked on load. With collision-free paths it
    /// is no longer the first lock, but it catches what a path cannot — a file
    /// copied, renamed or restored under the wrong meter's name, which is an
    /// ordinary thing to do while repairing a deployment over a file share.
    ///
    /// FALSIFIED 2026-08-11, both halves: restoring the lossy `_` mapping makes
    /// the distinctness assertion red (all three paths equal); dropping the
    /// `persisted.meter != meter` arm makes the last assertion red, the reference
    /// coming back as `Some(4843.822)` for a meter that never wrote it.
    #[test]
    fn a_reference_file_belongs_to_exactly_one_meter() {
        let dir = scratch_dir("reference_identity");
        let _ = std::fs::create_dir_all(&dir);

        // DISTINCT PATHS. These three ids all collapsed to one file before.
        let spaced = MeterId::new("gar age");
        let dotted = MeterId::new("gar.age");
        let scored = MeterId::new("gar_age");
        let paths = [
            reference_path_for(&dir, &spaced),
            reference_path_for(&dir, &dotted),
            reference_path_for(&dir, &scored),
        ];
        let unique: std::collections::BTreeSet<_> = paths.iter().collect();
        assert_eq!(
            unique.len(),
            3,
            "ids differing only in punctuation must not share a reference file: \
             one meter's index would judge another's readings after a restart. \
             Got {paths:?}"
        );

        // AND THE FILE CARRIES ITS OWNER. Written for one meter, refused for
        // another — the case a collision-free path cannot catch.
        for path in &paths {
            let _ = std::fs::remove_file(path);
        }
        store_energy_reference(&dir, &spaced, crate::domain::Kwh(4_843.822));
        assert_eq!(
            load_energy_reference(&dir, &spaced),
            Some(crate::domain::Kwh(4_843.822)),
            "its own meter reads it back"
        );

        // Simulate the hand-repair: the file is moved under another meter's name.
        std::fs::rename(
            reference_path_for(&dir, &spaced),
            reference_path_for(&dir, &dotted),
        )
        .expect("renamed");
        assert_eq!(
            load_energy_reference(&dir, &dotted),
            None,
            "a reference belonging to another meter must be refused: accepting it \
             judges a live counter against an index that was never its own"
        );

        for path in &paths {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Drives several ticks through one `step_once` chain, carrying `last` and
    /// the monotonicity reference across them — which is what makes a SEQUENCE
    /// testable rather than a single judgement.
    async fn drive_sequence(source: FakeSource) -> (State, Vec<MeterUpdate>) {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let beats = Heartbeats::for_meters([MeterId::new("garage")]);
        let heartbeat = beats.of(&MeterId::new("garage")).expect("present");
        let (tx, mut rx) = mpsc::channel(8);
        let mut source = source;
        let meter = MeterId::new("garage");
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };
        let mut last = None;
        let mut energy = None;
        let mut state = State::initial();
        for _ in 0..source.remaining() {
            (state, _) =
                step_once(&ctx, &mut source, state, &mut last, &mut energy, &mut None).await;
        }
        drop(tx);
        let mut got = Vec::new();
        while let Some(u) = rx.recv().await {
            got.push(u);
        }
        (state, got)
    }

    async fn drive(source: FakeSource) -> (State, Vec<MeterUpdate>) {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let beats = Heartbeats::for_meters([MeterId::new("garage")]);
        let heartbeat = beats.of(&MeterId::new("garage")).expect("present");
        let (tx, mut rx) = mpsc::channel(8);
        let mut source = source;
        let meter = MeterId::new("garage");
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };
        let (state, _) = step_once(
            &ctx,
            &mut source,
            State::initial(),
            &mut None,
            &mut None,
            &mut None,
        )
        .await;
        drop(tx);
        let mut got = Vec::new();
        while let Some(u) = rx.recv().await {
            got.push(u);
        }
        (state, got)
    }

    #[tokio::test]
    async fn a_fresh_reading_is_forwarded_as_good() {
        let (state, sent) = drive(FakeSource::new().then(Ok(reading(Quality::Good, 950)))).await;
        assert_eq!(state, State::Fresh);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].published(), Quality::Good);
        assert_eq!(sent[0].meter, MeterId::new("garage"));
    }

    #[tokio::test]
    async fn an_out_of_bounds_age_is_forwarded_as_stale() {
        // The fetch succeeded; the timestamps say the data is old.
        let (state, sent) =
            drive(FakeSource::new().then(Ok(reading(Quality::Good, 600_000)))).await;
        assert_eq!(state, State::Stale);
        assert_eq!(sent[0].published(), Quality::Stale);
        assert_eq!(
            sent[0].measurement.quality,
            Quality::Good,
            "the source's own view is preserved alongside the verdict"
        );
    }

    #[tokio::test]
    async fn a_transient_error_yields_stale_and_nothing_to_forward() {
        let (state, sent) = drive(FakeSource::new().then(Err(SourceError::Transient {
            reason: "503".to_string(),
        })))
        .await;
        assert_eq!(state, State::Stale);
        assert!(sent.is_empty(), "there is no reading to carry");
    }

    #[tokio::test]
    async fn a_fatal_error_latches_failed() {
        let (state, _) = drive(FakeSource::new().then(Err(SourceError::Fatal {
            refusal: crate::core::source::Refusal::Credential,
            reason: "auth rejected".to_string(),
        })))
        .await;
        assert_eq!(state, State::Failed);
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_cloud_times_out_into_stale_instead_of_wedging() {
        // The localization twin of chaos_stale_on_cloud_timeout: the source
        // never answers, the REAL timeout path fires under paused time.
        let (state, sent) = drive(FakeSource::new().then_hang()).await;
        assert_eq!(state, State::Stale, "a silent cloud is STALE, not a hang");
        assert!(sent.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn the_heartbeat_is_written_before_the_network_call() {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        // Advance BEFORE the step so "the instant at the top of the loop" and
        // "the instant after the fetch" are different numbers: without this the
        // assertion below would hold whichever side of the fetch the touch sat
        // on, and would prove nothing.
        clock.advance_ms(7_000);
        let beats = Heartbeats::for_meters([MeterId::new("garage")]);
        let heartbeat = beats.of(&MeterId::new("garage")).expect("present");
        assert_eq!(heartbeat.last(), None, "never run yet");
        let (tx, _rx) = mpsc::channel(8);
        // A source that never answers: if the heartbeat were written after the
        // fetch, it would still be None when the timeout fires.
        let mut source = FakeSource::new().then_hang();
        let meter = MeterId::new("garage");
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };
        let _ = step_once(
            &ctx,
            &mut source,
            State::initial(),
            &mut None,
            &mut None,
            &mut None,
        )
        .await;
        assert_eq!(
            heartbeat.last(),
            Some(MonotonicMs(7_000)),
            "a hung fetch still leaves a heartbeat — that is what makes a wedge visible"
        );
    }

    /// **Story 3.1 AC4** — a meter that never answers must not cost the others
    /// their cadence (FR12, and NFR2's bound measured per meter).
    ///
    /// One of Guy's four meters is physically unplugged, so this is the steady
    /// state of the deployment rather than an unlucky case.
    ///
    /// **The assertion is a COUNT per meter, not an absence and not a shape.**
    /// "The others were polled on time" holds vacuously over a run that polled
    /// nobody — this repository has shipped absence assertions that held over an
    /// empty stream, and the fix is to name a number only the property produces.
    /// The hanging meter's own count is asserted too, at 3: it must keep being
    /// *tried* rather than dropped, or a silent meter would silently stop being
    /// a meter.
    ///
    /// **What this proves and what it does not.** It proves the tasks are
    /// independent when spawned independently: the fetch timeout here (2 s) times
    /// four exceeds the 5 s period, so a single task walking the four would fall
    /// behind by construction. It does NOT prove `supervisor` spawns one per
    /// meter — that is production wiring this test never touches, and it is
    /// covered instead by the heartbeat count, which is one per spawned task.
    ///
    /// FALSIFIED 2026-08-07 by replacing the four spawns with one task that
    /// awaits each meter's `step_once` in turn — the design this AC exists to
    /// forbid. Copied from the run:
    ///
    /// ```text
    /// test app::poll_publish::tests::a_hanging_meter_does_not_cost_the_others_their_cadence ... FAILED
    ///
    /// thread '…a_hanging_meter_does_not_cost_the_others_their_cadence' (355) panicked at
    /// crates/smartme-bridge/src/app/poll_publish.rs:614:9:
    /// assertion `left == right` failed: a meter that never answers must not cost another
    /// meter its cadence: three periods must produce three readings each
    ///   left: [2, 2, 2]
    ///  right: [3, 3, 3]
    ///
    /// test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 168 filtered out
    /// ```
    ///
    /// **Re-run 2026-08-07** after story 3.2 shortened the window to exactly three
    /// ticks: a failed tick now republishes, so the fourth tick at 15 s made the
    /// count 4 and the old window stopped measuring what it named.
    ///
    /// Two rounds were needed, and the first is the instructive one: the mutation
    /// compiled and the test stayed GREEN, because the pacing made a serialised
    /// walk fit inside the period anyway. A mutation that changes nothing
    /// observable is not a falsification.
    /// The pacing this AC needs: a **5 s period and a 10 s fetch deadline**, which
    /// is the real default pairing at the minimum period (ADR 0020's `PERIOD_MIN`
    /// with the shipped `fetch_timeout`).
    ///
    /// The first draft used a 2 s deadline on the reasoning that "four serialised
    /// timeouts, 8 s, overrun a 5 s period". That arithmetic was wrong — only ONE
    /// meter hangs, so a serialised cycle cost 2 s and fitted inside the period.
    /// **The falsification is what caught it**: the serialised mutation compiled
    /// and the test stayed green, proving nothing. With the deadline above the
    /// period, one hang is enough to put a serialised walk behind.
    fn bridge_config() -> crate::app::supervisor::BridgeConfig {
        crate::app::supervisor::BridgeConfig {
            api_base: "https://api.smart-me.com".to_string(),
            credentials: smart_me_client::Credentials::Basic {
                user: "u".to_string(),
                password: "p".to_string(),
            },
            http_timeout: Duration::from_secs(10),
            meters: Vec::new(),
            group_id: "G".to_string(),
            node_id: "N".to_string(),
            broker_host: "b".to_string(),
            broker_port: 1883,
            bd_seq_path: std::path::PathBuf::from("/data/bdseq.toml"),
            poll: PollConfig {
                interval: Duration::from_secs(5),
                fetch_timeout: Duration::from_secs(10),
            },
            policy: policy(),
            log_dir: None,
            log_keep: None,
            ui_port: None,
        }
    }

    /// **Story 3.3 AC3 — the fleet is read at one instant** (AR6).
    ///
    /// # The invariant, and why a counter earns its place
    ///
    /// Every write touches one meter's fields and `generation` inside the same
    /// `send_modify`. Each task here writes `last_tick = its own number of
    /// touches`, so for any state that ever existed:
    ///
    /// ```text
    /// generation == sum of every meter's last_tick
    /// ```
    ///
    /// A reader that samples meters one at a time can satisfy neither side
    /// honestly: it observes meter A before a write and meter B after it, and the
    /// total belongs to no single instant.
    ///
    /// # The vacuity this is built against
    ///
    /// A snapshot test over a QUIET fleet passes against any implementation,
    /// including the per-meter atomics this replaces. The writers therefore run
    /// concurrently with the reader, on a multi-threaded runtime, for enough
    /// iterations that a torn read is overwhelmingly likely rather than merely
    /// possible.
    ///
    /// FALSIFIED 2026-08-08 by rebuilding the snapshot field by field — one
    /// `borrow()` per meter plus one for the generation, which is exactly what
    /// `Heartbeats::iter()` did before this story:
    ///
    /// ```text
    /// test app::poll_publish::tests::the_fleet_is_read_at_one_instant ... FAILED
    ///
    /// thread '…the_fleet_is_read_at_one_instant' (357) panicked at
    /// crates/smartme-bridge/src/app/poll_publish.rs:1111:9:
    /// a snapshot must belong to ONE instant: generation 57 against meters summing
    /// to 56 — the reader saw some meters before a write and others after
    /// ```
    ///
    /// It tears on the 57th write out of 8000, which is the measure of how little
    /// concurrency it takes: the old read was not unlikely to be torn, it was
    /// torn almost immediately whenever anything was writing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_fleet_is_read_at_one_instant() {
        const WRITES: i64 = 2_000;
        let all: Vec<MeterId> = ["garage", "cellar", "attic", "unplugged"]
            .into_iter()
            .map(MeterId::new)
            .collect();
        let beats = Heartbeats::for_meters(all.clone());

        let writers: Vec<_> = all
            .iter()
            .map(|meter| {
                let pulse = beats.of(meter).expect("served");
                tokio::spawn(async move {
                    for n in 1..=WRITES {
                        // `last_tick` carries this task's own write count, which
                        // is what makes the sum checkable.
                        pulse.touch(MonotonicMs(n), 5_000);
                        tokio::task::yield_now().await;
                    }
                })
            })
            .collect();

        let reader = {
            let beats = beats.clone();
            tokio::spawn(async move {
                let mut torn = None;
                for _ in 0..20_000 {
                    let fleet = beats.snapshot();
                    let sum: i64 = fleet
                        .meters
                        .iter()
                        .filter_map(|m| m.last_tick)
                        .map(|t| t.0)
                        .sum();
                    if sum != fleet.generation as i64 {
                        torn = Some((fleet.generation, sum));
                        break;
                    }
                    tokio::task::yield_now().await;
                }
                torn
            })
        };

        for w in writers {
            w.await.expect("writer");
        }
        let torn = reader.await.expect("reader");

        // THE PREMISE: the writers really did write, or "no torn read" is a claim
        // about a fleet nobody touched.
        let end = beats.snapshot();
        assert_eq!(
            end.generation,
            (WRITES * all.len() as i64) as u64,
            "every write must be counted, or the invariant below is checked over a \
             fleet that was never written to"
        );
        assert!(
            torn.is_none(),
            "a snapshot must belong to ONE instant: generation {} against meters \
             summing to {} — the reader saw some meters before a write and others \
             after",
            torn.map(|t| t.0).unwrap_or(0),
            torn.map(|t| t.1).unwrap_or(0)
        );
    }

    /// **Story 2.3 AC5, THROUGH `run`** — the production load and store, which
    /// nothing exercised.
    ///
    /// **Added 2026-08-11 by this story's review.** `the_monotonicity_reference_survives_a_restart`
    /// calls `load_energy_reference` and `store_energy_reference` directly, so it
    /// proves the two helpers agree with each other and nothing about the bridge
    /// using them. Deleting either call site in `run` — the `let mut
    /// energy_reference = load_…` at the top, or the `if energy_reference !=
    /// before` block after each tick — left every test green, including the one
    /// whose recorded falsification was *"deleting the persist call"*: the call
    /// being deleted was the test's own.
    ///
    /// This drives `run` itself, with a real config handle and a real outbox, and
    /// then reads the file off disk. It also covers the equality guard: a second
    /// tick at the SAME index must not rewrite the file, which is what keeps a
    /// quiet fleet from fsyncing once per meter per period for a number that did
    /// not move.
    ///
    /// FALSIFIED 2026-08-11: removing the `store_energy_reference` call from
    /// `run` makes the "written by run" assertion red; removing the
    /// `load_energy_reference` call makes the restored-value assertion red with
    /// `None`.
    #[tokio::test(start_paused = true)]
    async fn run_persists_and_restores_the_reference_without_help_from_a_test() {
        use crate::app::supervisor::ConfigHandle;

        let dir = scratch_dir("run_persistence");
        let _ = std::fs::create_dir_all(&dir);
        let meter = MeterId::new("garage");
        let _ = std::fs::remove_file(reference_path_for(&dir, &meter));

        let clock = Arc::new(FakeClock::new(UtcMillis(SANE_NOW)));
        let handle: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(bridge_config()));

        // FIRST PROCESS: two ticks at the same index, then the outbox closes.
        {
            let (tx, mut rx) = mpsc::channel(8);
            let beats = Heartbeats::for_meters([meter.clone()]);
            let source = FakeSource::new()
                .then(Ok(reading_with_energy(Quality::Good, 950, 4_843.822)))
                .then(Ok(reading_with_energy(Quality::Good, 950, 4_843.822)));
            let task = tokio::spawn(run(
                meter.clone(),
                source,
                Arc::clone(&clock) as Arc<dyn Clock + Send + Sync>,
                Arc::clone(&handle),
                beats.clone(),
                tx,
                dir.clone(),
            ));
            // Two ticks: the interval fires immediately, then once more.
            tokio::time::sleep(Duration::from_secs(6)).await;
            drop(rx.recv().await);
            task.abort();
        }

        let written: PersistedReference = crate::persist::load(&reference_path_for(&dir, &meter))
            .expect(
                "`run` must persist the reference itself — a test calling the helper \
                 proves only that the helper works",
            );
        assert_eq!(written.energy_kwh, 4_843.822);
        assert_eq!(written.meter, "garage");

        // SECOND PROCESS: nothing in memory survives, and the meter comes back
        // reading lower. `run` must restore the reference and catch it.
        let (tx, mut rx) = mpsc::channel(8);
        let beats = Heartbeats::for_meters([meter.clone()]);
        let source = FakeSource::new().then(Ok(reading_with_energy(Quality::Good, 950, 12.0)));
        let task = tokio::spawn(run(
            meter.clone(),
            source,
            Arc::clone(&clock) as Arc<dyn Clock + Send + Sync>,
            Arc::clone(&handle),
            beats,
            tx,
            dir.clone(),
        ));
        let update = rx.recv().await.expect("the first tick published");
        task.abort();

        assert_eq!(
            update.published_for(Measured::Energy),
            Quality::Bad,
            "`run` must LOAD the reference at startup. Without it the first \
             reading after every restart is unjudged, and a meter replaced during \
             the maintenance window that caused the restart goes unnoticed"
        );
        assert_eq!(
            update.verdicts.for_metric(Measured::Energy).cause(),
            Some(Cause::CounterWentBackwards)
        );

        let _ = std::fs::remove_file(reference_path_for(&dir, &meter));
    }

    #[tokio::test(start_paused = true)]
    async fn a_hanging_meter_does_not_cost_the_others_their_cadence() {
        use crate::app::supervisor::ConfigHandle;

        let clock = Arc::new(FakeClock::new(UtcMillis(SANE_NOW)));
        let (tx, mut rx) = mpsc::channel(64);
        let healthy = [
            MeterId::new("garage"),
            MeterId::new("cellar"),
            MeterId::new("attic"),
        ];
        let silent = MeterId::new("unplugged");
        let all: Vec<MeterId> = healthy.iter().cloned().chain([silent.clone()]).collect();
        let beats = Heartbeats::for_meters(all.clone());
        let handle: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(bridge_config()));

        let mut tasks = Vec::new();
        for meter in &all {
            // The unplugged one hangs on every fetch, so its task spends
            // `fetch_timeout` inside each tick. Four of those serialised would be
            // 8 s against a 5 s period.
            // The reading must carry ITS OWN meter id. The shared fixture hard-codes
            // one, and with it every task's update arrived labelled "garage" — the
            // first draft of this test read 9/0/0 and would have read 3/3/3 for a
            // runtime that polled one meter three times as fast.
            let mine = |q| {
                let mut r = reading(q, 950);
                r.value.meter = meter.clone();
                r
            };
            let source = if *meter == silent {
                FakeSource::new().then_hang().then_hang().then_hang()
            } else {
                FakeSource::new()
                    .then(Ok(mine(Quality::Good)))
                    .then(Ok(mine(Quality::Good)))
                    .then(Ok(mine(Quality::Good)))
            };
            tasks.push(tokio::spawn(run(
                meter.clone(),
                source,
                Arc::clone(&clock) as Arc<dyn Clock + Send + Sync>,
                Arc::clone(&handle),
                beats.clone(),
                tx.clone(),
                // A directory unique to THIS test binary, so two concurrent
                // `cargo test` runs cannot couple through the filesystem — which
                // is the very thing this test asserts does not happen, and which
                // a fixed path under /tmp reintroduced. See [`scratch_dir`],
                // which is where that reasoning now lives for every test here.
                scratch_dir("fleet_refs"),
            )));
        }
        drop(tx);

        // EXACTLY three ticks: `interval` fires immediately, so 0 s, 5 s and 10 s,
        // and the window stops before the fourth at 15 s. It used to run to 15.5 s
        // and expect three — which held only while a failed tick published
        // nothing. Since story 3.2 the fourth tick exhausts the scripted source and
        // REPUBLISHES the last value with a stale verdict, so the count became 4.
        // Virtual time, so this costs nothing and cannot flake on load.
        tokio::time::sleep(Duration::from_millis(12_000)).await;
        for task in &tasks {
            task.abort();
        }

        let mut polled: std::collections::HashMap<MeterId, usize> = Default::default();
        while let Ok(update) = rx.try_recv() {
            *polled.entry(update.measurement.meter.clone()).or_default() += 1;
        }
        let counts: Vec<usize> = healthy
            .iter()
            .map(|m| polled.get(m).copied().unwrap_or(0))
            .collect();
        assert_eq!(
            counts,
            vec![3, 3, 3],
            "a meter that never answers must not cost another meter its cadence: \
             three periods must produce three readings each"
        );
        // The silent one forwards nothing — it has no reading to carry — but its
        // heartbeat proves it was TRIED rather than dropped.
        assert!(
            beats.of(&silent).expect("present").last().is_some(),
            "the unplugged meter must keep being polled; a meter that stops being \
             tried stops being a meter, and its silence would then be the runtime's \
             rather than the cloud's"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn recovery_needs_one_proven_reading() {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let beats = Heartbeats::for_meters([MeterId::new("garage")]);
        let heartbeat = beats.of(&MeterId::new("garage")).expect("present");
        let (tx, mut rx) = mpsc::channel(8);
        let mut source = FakeSource::new()
            .then(Err(SourceError::Timeout))
            .then(Ok(reading(Quality::Good, 950)));
        let meter = MeterId::new("garage");

        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };
        let mut last = None;
        let (after_timeout, _) = step_once(
            &ctx,
            &mut source,
            State::initial(),
            &mut last,
            &mut None,
            &mut None,
        )
        .await;
        assert_eq!(after_timeout, State::Stale);

        let (after_good, _) = step_once(
            &ctx,
            &mut source,
            after_timeout,
            &mut last,
            &mut None,
            &mut None,
        )
        .await;
        assert_eq!(after_good, State::Fresh);
        drop(tx);
        let u = rx.recv().await.expect("the good reading was forwarded");
        assert_eq!(u.published(), Quality::Good);
    }
}
