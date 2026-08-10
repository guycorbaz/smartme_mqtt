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
use crate::core::source::{Source, SourceError, Tick};
use crate::core::state_machine::{Policy, State};
use crate::domain::MeterId;

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
    pub fn record(&self, meter: &MeterId, state: State) {
        self.0.send_modify(|fleet| {
            if let Some(entry) = fleet.meters.iter_mut().find(|m| &m.meter == meter) {
                entry.verdict = Some(state);
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
) -> State {
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

    let tick: Tick = match tokio::time::timeout(config.fetch_timeout, source.fetch(meter)).await {
        Ok(result) => result,
        // The deadline elapsed: the cloud is silent. That is a verdict input,
        // not an error to swallow.
        Err(_elapsed) => Err(SourceError::Timeout),
    };

    let (next, published) = policy.step(previous, &tick, clock.wall());

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
    let to_publish = match &tick {
        Ok(reading) => {
            *last = Some(reading.value.clone());
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
    next
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
) {
    let heartbeat = pulse.of(&meter).unwrap_or_else(|| {
        panic!("no heartbeat for {meter}; the collection is built from the served meters")
    });
    let mut state = State::initial();
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
        state = step_once(&ctx, &mut source, state, &mut last).await;
        // The verdict reaches anything outside that can report on this meter.
        pulse.record(&meter, state);
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
                power: Kw(0.018),
                energy: Kwh(4_843.822),
                value_date: UtcMillis(BASE),
                quality,
            },
            http_date: Some(UtcMillis(BASE + age_ms)),
        }
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
        let good = step_once(&ctx, &mut source, State::initial(), &mut last).await;
        assert_eq!(
            good,
            State::Fresh,
            "the premise: the meter must first be proven fresh"
        );

        let after = step_once(&ctx, &mut source, good, &mut last).await;
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
        let state = step_once(&ctx, &mut source, State::initial(), &mut None).await;
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
        let _ = step_once(&ctx, &mut source, State::initial(), &mut None).await;
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
        let after_timeout = step_once(&ctx, &mut source, State::initial(), &mut last).await;
        assert_eq!(after_timeout, State::Stale);

        let after_good = step_once(&ctx, &mut source, after_timeout, &mut last).await;
        assert_eq!(after_good, State::Fresh);
        drop(tx);
        let u = rx.recv().await.expect("the good reading was forwarded");
        assert_eq!(u.published(), Quality::Good);
    }
}
