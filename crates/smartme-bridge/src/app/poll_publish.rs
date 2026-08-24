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
use crate::domain::{MeterId, Quality, UtcMillis};

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

/// Why a judged reading never reached the wire (story 4.11, FR22, AR7).
///
/// # A CLOSED set, and why that is the property rather than the tidiness
///
/// FR22 says a reading lost to a broker outage must read as *loss*, never as
/// silence. That is only true if EVERY path on which a reading can fail to reach
/// the wire lands in this enum — a seventh path that quietly went nowhere would
/// be the exact failure the requirement exists to prevent, and it would look like
/// a healthy fleet.
///
/// **THE SET IS CLOSED OVER THE HAND-OVER, NOT OVER THE JOURNEY, and the 2026-08-18
/// review found three places where the difference bites.** They are recorded as
/// issues rather than described as covered:
///
/// - **[#85]** — `try_publish` answers `Ok` on entering `rumqttc`'s request
///   channel, not on leaving the socket. What is still queued when the connection
///   drops is discarded with the event loop, counted by nothing.
/// - **[#87]** — whatever sits in the 64-slot inbox when the session ends is
///   dropped with the receiver at shutdown. No arm fires; it is a seventh path.
/// - **[#88]** — a DBIRTH refused by the transport leaves the device in
///   `declared`, so every later reading is `Emitted` here while the host discards
///   it as undeclared. The one path where a reading provably fails to reach the
///   SCADA is the one [`Self::UndeclaredDevice`] structurally cannot see.
///
/// With that stated, the six are exhaustive over the hand-over paths that exist:
///
/// | Variant | Where it fires |
/// |---|---|
/// | [`Self::OutboxFull`] | `step_once` — the driver is not draining the channel (it is reconnecting) |
/// | [`Self::MqttTaskGone`] | `step_once` — the receiver is dropped |
/// | [`Self::TransportQueueFull`] | `mqtt_driver` — `AsyncClient::try_publish` refused |
/// | [`Self::BeforeBirth`] | `mqtt_driver` — [`Published::DroppedBeforeBirth`] |
/// | [`Self::UndeclaredDevice`] | `mqtt_driver` — [`Published::DroppedUndeclaredDevice`] |
/// | [`Self::Unpublishable`] | `mqtt_driver` — the reading could not be encoded or addressed |
///
/// [`Published::DroppedBeforeBirth`]: crate::adapters::sparkplug_publisher::Published::DroppedBeforeBirth
/// [`Published::DroppedUndeclaredDevice`]: crate::adapters::sparkplug_publisher::Published::DroppedUndeclaredDevice
/// [#85]: https://github.com/guycorbaz/smartme_mqtt/issues/85
/// [#87]: https://github.com/guycorbaz/smartme_mqtt/issues/87
/// [#88]: https://github.com/guycorbaz/smartme_mqtt/issues/88
///
/// **Certificates are not readings and are not counted.** A DBIRTH or DDEATH that
/// the outbound queue refuses has its own `error!` at the `DeviceCommand` arm and
/// belongs to story 3.5's contract; folding it in here would make a count named
/// after readings answer a different question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// The channel to the driver was full: the driver is alive but not draining
    /// it, which is what a reconnect backoff looks like from this side.
    OutboxFull,
    /// The driver task is gone. The reading is lost and so is every later one.
    MqttTaskGone,
    /// The transport's own outbound queue refused the message.
    TransportQueueFull,
    /// The node has not published its BIRTH, so a DATA now would carry sequence 0
    /// and read as a BIRTH on the wire.
    BeforeBirth,
    /// The device was never declared in a BIRTH, so a consumer would discard it.
    UndeclaredDevice,
    /// The reading could not be encoded or addressed at all.
    Unpublishable,
}

impl DropReason {
    /// Whose fault a lost reading is (AR19, story 6.3).
    ///
    /// **This is where `Culprit::Bridge` comes from.** No [`Cause`] can produce it —
    /// the oracle judges readings, and a reading is never the bridge's fault — so a
    /// culprit derived from causes alone could never accuse this process. Five of
    /// these six can.
    ///
    /// [`Cause`]: crate::core::oracle::Cause
    pub const fn culprit(self) -> crate::core::oracle::Culprit {
        use crate::core::oracle::Culprit;
        match self {
            // THE BRIDGE LOST IT. Its own inbox was full, its own transport task
            // was gone, it had not birthed yet, it had not declared the device, or
            // it could not build a topic. Every one of these is repaired here.
            Self::OutboxFull
            | Self::MqttTaskGone
            | Self::BeforeBirth
            | Self::UndeclaredDevice
            | Self::Unpublishable => Culprit::Bridge,
            // THE BROKER IS NOT KEEPING UP, or is gone. `rumqttc`'s request channel
            // fills because the far end is not draining it — that is the world,
            // and telling an operator to look at this process would send them to
            // the wrong machine.
            Self::TransportQueueFull => Culprit::World,
        }
    }

    /// Every reason, in the order the counters are indexed.
    ///
    /// The array is the single source of both the count and the index, so a
    /// seventh variant cannot be added without the counter widening with it.
    pub const ALL: [DropReason; 6] = [
        DropReason::OutboxFull,
        DropReason::MqttTaskGone,
        DropReason::TransportQueueFull,
        DropReason::BeforeBirth,
        DropReason::UndeclaredDevice,
        DropReason::Unpublishable,
    ];

    /// How many reasons there are — the width of every per-meter counter.
    pub const COUNT: usize = Self::ALL.len();

    /// This reason's cell in a counter array.
    ///
    /// Spelled as an exhaustive `match` rather than `ALL.iter().position(…)`:
    /// the compiler then refuses a new variant that nobody indexed, which a
    /// runtime search would answer with `None` at the call site instead.
    /// `the_index_and_the_list_agree` pins the two together.
    pub fn index(self) -> usize {
        match self {
            DropReason::OutboxFull => 0,
            DropReason::MqttTaskGone => 1,
            DropReason::TransportQueueFull => 2,
            DropReason::BeforeBirth => 3,
            DropReason::UndeclaredDevice => 4,
            DropReason::Unpublishable => 5,
        }
    }

    /// The slug an operator reads — in the log line and on `/healthz`.
    ///
    /// Kebab-case, matching `Cause::as_str`'s vocabulary, so the two families of
    /// reason read as one language on a screen that shows both.
    pub fn as_str(self) -> &'static str {
        match self {
            DropReason::OutboxFull => "outbox-full",
            DropReason::MqttTaskGone => "mqtt-task-gone",
            DropReason::TransportQueueFull => "transport-queue-full",
            DropReason::BeforeBirth => "before-birth",
            DropReason::UndeclaredDevice => "undeclared-device",
            DropReason::Unpublishable => "unpublishable",
        }
    }
}

/// One meter, as the fleet stood at one instant.
///
/// What one publication knows about itself (AR19, stories 6.3 and 6.4).
///
/// **A type rather than five parameters**, and clippy asked for it before the
/// design did: `record_at` reached eight arguments and the lint refused. It was
/// right — these five values are one event seen from five angles, and passing them
/// separately let a caller supply the instant without the threshold it was judged
/// against, which is exactly the pairing AC1 exists to keep together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Publication {
    /// When the bridge published.
    pub at: UtcMillis,
    /// The staleness threshold the verdict was reached against.
    pub threshold_ms: i64,
    /// The source's own acquisition time, which is what tells a new reading from
    /// the same one republished.
    pub value_date: Option<UtcMillis>,
    /// The published power, as a number. `None` is "nothing read", never zero.
    pub power_kw: Option<f64>,
    /// The published energy index. See [`Self::power_kw`].
    pub energy_kwh: Option<f64>,
}

/// AR6's `MeterState`, which the architecture has named since Epic 0 and which
/// **did not exist until 2026-08-08** — story 3.1 ticked the box for it. What
/// shipped instead was three independent atomics per meter, read one at a time.
/// **`Eq` was dropped on 2026-08-19 (story 6.4), and the reason is the type rather
/// than the compiler.** This state now carries a measured value as `f64`, and a
/// measurement has no total equality: `NaN != NaN` is not a defect of Rust but the
/// arithmetic saying that "unknown equals unknown" is not a question with an answer.
/// `PartialEq` remains, so every existing comparison still works; nothing keyed a
/// map or a set on this state, which is what made the change free.
#[derive(Debug, Clone, PartialEq)]
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
    /// When this meter last published, and **against which staleness threshold**
    /// (AR19, story 6.3 AC1).
    ///
    /// The threshold travels with the instant because a freshness judgement read
    /// against a different threshold than the one used is a different judgement —
    /// and the threshold is configurable, so a screen that assumed the current one
    /// would misread every verdict reached before the last change.
    pub last_published_at: Option<UtcMillis>,
    /// The threshold in force when the verdict above was reached, in milliseconds.
    pub staleness_threshold_ms: Option<i64>,
    /// When what this meter publishes last **changed** — distinct from when it was
    /// last published (AR19, story 6.3 AC2).
    ///
    /// ADR 0027 requires a verdict every cycle, so most publications repeat the
    /// previous reading. Without this field a frozen meter and a quiet one look
    /// identical: both show a recent publication. **Change is measured on the
    /// source's own `ValueDate`**, not on the bridge's clock — a source
    /// re-answering with the same acquisition time has produced nothing new,
    /// whatever the bridge does with it.
    pub last_changed_at: Option<UtcMillis>,
    /// The acquisition time of the reading behind the last publication, kept so
    /// the field above can tell a new reading from a repeated one.
    pub source_value_date: Option<UtcMillis>,
    /// The last published reading, as **numbers** (story 6.4 AC1).
    ///
    /// # Why two floats and not the `Measurement`
    ///
    /// `MeterId` and `Serial` are `String` newtypes, so keeping the measurement
    /// whole would allocate twice per meter per tick — under the `send_modify`
    /// lock every poll task waits on, which story 6.3 AC4 forbids. Two
    /// `Option<f64>` allocate nothing.
    ///
    /// **The unit is not here on purpose.** `Kw` and `Kwh` are domain types; kW and
    /// kWh are constants of this bridge, and a per-meter copy would duplicate what
    /// the type already holds — a second place for them to disagree.
    ///
    /// `None` means "nothing published yet", never "zero": FR16's rule that a
    /// missing field is never a substituted value reaches the screen through this.
    pub last_power_kw: Option<f64>,
    /// The energy index of the same reading. See [`Self::last_power_kw`].
    pub last_energy_kwh: Option<f64>,
    /// Whose fault the last fault was (AR19, story 6.3 AC3) — `None` when nothing
    /// is wrong.
    ///
    /// **Last writer wins, deliberately.** A publication records the culprit of its
    /// cause; a lost reading records the culprit of its drop reason. The operator
    /// is shown the most recent fault rather than the worst one, because "what is
    /// happening now" is the question a 3 a.m. screen answers — and a worst-of
    /// rule would keep an hour-old credential rejection on screen while the broker
    /// is refusing everything.
    pub culprit: Option<crate::core::oracle::Culprit>,
    /// How many judged readings never reached the wire, per [`DropReason`]
    /// (story 4.11, FR22, AR7).
    ///
    /// # Why a fixed array, and why that IS the bounded-memory argument
    ///
    /// AR7 forbids a buffer, and the criterion this discharges is *"the drop path
    /// allocates nothing that survives it"*. A fixed `[u64; COUNT]` per meter
    /// makes that true by construction rather than by discipline: the cardinality
    /// is `served meters × DropReason::COUNT`, both closed sets fixed at start-up,
    /// so a million drops cost the same bytes as one. A map keyed by reason — or
    /// worse, by reason and timestamp — would have been a buffer wearing a
    /// counter's name.
    ///
    /// **Cumulative for the process lifetime, and saturating.** Not per session:
    /// an outage that spans three reconnects is one outage to the operator
    /// reading the number, and a counter reset by the reconnect would report the
    /// smallest figure exactly when the fault was largest. Saturating because a
    /// count that wraps to zero is a surface that lies, which is the one thing
    /// this bridge is built not to do.
    pub dropped: [u64; DropReason::COUNT],
    /// The operator switched this meter OFF ([#90]).
    ///
    /// # Why the counters above are not cleared instead
    ///
    /// [`Self::dropped`] is cumulative for the process lifetime, and its own
    /// documentation says why: a counter reset by a reconnect reports the
    /// smallest figure exactly when the fault was largest. Clearing it on a
    /// disable would apply that same erasure to a fact that did happen — the
    /// readings WERE lost — so what is added is the sentence that explains the
    /// number, not a rule that deletes it.
    ///
    /// **This marks the DELIBERATE gesture and nothing else.** A meter whose
    /// configuration row was removed keeps polling until the restart the cost
    /// table demanded, and its `undeclared-device` count keeps rising; that
    /// counter is the restart debt staying visible, and marking it retired would
    /// hide exactly what the poll loop's own comment says must stay loud.
    ///
    /// It also answers a question no field could answer before: a retired meter
    /// and one that has never completed a tick both carry `None` everywhere.
    pub retired: bool,
    /// How many of [`Self::dropped`] were **republications** rather than fresh
    /// readings ([#92]).
    ///
    /// # Two questions that shared one number
    ///
    /// ADR 0027 requires a verdict every cycle, so a meter whose source has gone
    /// quiet republishes its last known value with a degraded verdict, once per
    /// period. Under a source failure and a broker outage at the same time the
    /// SAME value is refused again and again, and each refusal moved the counter
    /// — so the total said N where the historian was missing exactly one distinct
    /// measurement.
    ///
    /// Both readings are true and they answer different questions: *how many
    /// messages did the bridge fail to hand over*, which is the total the manual
    /// defines and which must not shrink, and *how many distinct measurements is
    /// the historian missing*, which is the total minus this.
    ///
    /// **Not a seventh `DropReason`**, deliberately: that enum answers *why* the
    /// bridge could not hand a message over, and the reason here is unchanged —
    /// a full queue. What differs is *what* was lost, and that is a second axis.
    pub republications_lost: u64,
}

/// One meter's losses under one reason, as the operator surfaces read them.
///
/// # Why a struct where a tuple did
///
/// It grew a fourth member ([#90]) and `lost[0].3` says nothing. The members are
/// also not of one kind any more: three describe the loss, and `retired`
/// describes the METER — a reader that misses that distinction reports a
/// disabled meter's history as a live fault, which is the confusion this exists
/// to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lost<'a> {
    /// The meter the readings belonged to.
    pub meter: &'a MeterId,
    /// Why they never reached the wire.
    pub reason: DropReason,
    /// How many, cumulative for the process lifetime.
    pub count: u64,
    /// The operator has since switched this meter off, so the count is history
    /// and cannot rise. **Not set for a meter whose configuration row was
    /// removed** — see [`FleetMeter::retired`].
    pub retired: bool,
    /// How many of this METER's losses were republications rather than distinct
    /// readings ([#92]). Per meter and not per reason: a republication is refused
    /// for whatever reason the transport gives, and the reason is not what this
    /// counts.
    pub republications: u64,
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
// `Eq` follows `MeterState`'s, dropped with it and for the same reason.
#[derive(Debug, Clone, Default, PartialEq)]
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
            .filter(|m| !matches!(m.verdict, Some(State::Failed(_))))
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
            .filter(|(_, s)| matches!(s, State::Failed(_)))
            .map(|(m, _)| m)
            .collect()
    }

    /// Every reading this fleet lost, by meter and reason — zero cells omitted
    /// (story 4.11 AC4).
    ///
    /// The omission is here rather than at each surface for the reason
    /// [`Self::degraded`]'s exclusion is: there are already two operator surfaces
    /// and a rule applied at the caller is a rule the third caller will not know
    /// about. A fleet that has lost nothing renders as an empty list, which is
    /// the honest shape — not six zeros per meter, which is noise an operator
    /// learns to scroll past.
    pub fn dropped(&self) -> Vec<Lost<'_>> {
        self.meters
            .iter()
            .flat_map(|m| {
                DropReason::ALL.into_iter().map(move |reason| Lost {
                    meter: &m.meter,
                    reason,
                    count: m.dropped[reason.index()],
                    retired: m.retired,
                    republications: m.republications_lost,
                })
            })
            .filter(|lost| lost.count > 0)
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
                last_published_at: None,
                staleness_threshold_ms: None,
                last_changed_at: None,
                source_value_date: None,
                last_power_kw: None,
                last_energy_kwh: None,
                culprit: None,
                dropped: [0; DropReason::COUNT],
                retired: false,
                republications_lost: 0,
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
        self.record_at(meter, state, published, None);
    }

    /// As [`record`](Self::record), carrying what AR19 asks the state to know:
    /// **when** it published, **against which threshold**, and **what the source's
    /// own acquisition time was** so a repeat can be told from a change.
    ///
    /// # Nothing formatted is built here
    ///
    /// `send_modify` holds a write lock every poll task waits on, so this writes
    /// data and never text (story 6.3 AC4). The repair gesture an operator reads is
    /// derived at render time from [`Culprit`](crate::core::oracle::Culprit) and the
    /// cause; storing the sentence would put a `String` allocation under that lock,
    /// once per meter per tick, for the benefit of a page nobody may open.
    pub fn record_at(
        &self,
        meter: &MeterId,
        state: State,
        published: Verdict,
        publication: Option<Publication>,
    ) {
        self.0.send_modify(|fleet| {
            if let Some(entry) = fleet.meters.iter_mut().find(|m| &m.meter == meter) {
                entry.verdict = Some(state);
                entry.published = Some(published);
                if let Some(p) = publication {
                    entry.last_published_at = Some(p.at);
                    // CHANGE IS THE SOURCE'S, NOT OURS. A reading carrying the same
                    // `ValueDate` as the last one is the same reading republished —
                    // which ADR 0027 requires and which must not read as movement.
                    if p.value_date.is_some() && p.value_date != entry.source_value_date {
                        entry.last_changed_at = Some(p.at);
                    }
                    if p.value_date.is_some() {
                        entry.source_value_date = p.value_date;
                    }
                    entry.staleness_threshold_ms = Some(p.threshold_ms);
                    entry.last_power_kw = p.power_kw;
                    entry.last_energy_kwh = p.energy_kwh;
                }
                entry.culprit = published.cause().map(crate::core::oracle::Cause::culprit);
                // A meter that publishes is not retired, whatever it was a
                // moment ago. Cleared HERE rather than where the operator
                // re-enables it, for the reason `retire` is called from the poll
                // loop and not from `apply`: the state follows what the bridge
                // OBSERVES, and a re-enable that never produces a reading has
                // not un-retired anything an operator should be shown.
                entry.retired = false;
                fleet.generation += 1;
            }
        });
    }

    /// Clears one meter's recorded opinion — the operator disabled it, and the
    /// alarm it carried is retired with it (story 3.5 AC2, [#65] item 3).
    ///
    /// **Retiring is clearing the OPINION, not the meter**: `last_tick` stays,
    /// so the wedge detector still sees a live loop, and a cleared cell reads
    /// exactly like a meter that has not completed a tick — absent from
    /// `failed()` and `degraded()` by the rule those two already apply
    /// ("absent rather than guessed at"), so no surface needs a new filter.
    /// The asymmetry with the account's refusal is deliberate: disable is the
    /// operator saying "stop" and quietens their screens; a device the account
    /// refuses keeps its alarm for as long as the latch holds (story 3.5 AC3).
    pub fn retire(&self, meter: &MeterId) {
        self.0.send_modify(|fleet| {
            if let Some(entry) = fleet.meters.iter_mut().find(|m| &m.meter == meter) {
                entry.verdict = None;
                entry.published = None;
                // The enriched opinion goes with the verdict it belongs to, for the
                // reason the doc above gives: retiring clears the OPINION. What
                // stays is what a wedge detector reads — `last_tick` and the period.
                entry.culprit = None;
                entry.last_published_at = None;
                entry.staleness_threshold_ms = None;
                entry.last_changed_at = None;
                entry.source_value_date = None;
                entry.last_power_kw = None;
                entry.last_energy_kwh = None;
                // The opinion is gone; the COUNT of what was lost is not, and
                // this is what tells an operator why a number that cannot rise
                // any more is still on their screen ([#90]).
                entry.retired = true;
                fleet.generation += 1;
            }
        });
    }

    /// Counts one lost reading against a meter named at run time (story 4.11 AC1).
    ///
    /// The driver's counterpart to [`MeterPulse::dropped`]. It holds no per-meter
    /// handle — one task serves the whole fleet, and which meter a lost reading
    /// belonged to is only known when the reading is in hand — so this one looks
    /// the entry up. A meter that is not served is silently absent rather than a
    /// panic: `retire` and `record` already take that position, and a reading for
    /// an unserved meter is a reconfiguration race, not a bug worth killing the
    /// transport for.
    /// Counts one lost reading that was a REPUBLICATION of a value already
    /// published ([#92]).
    ///
    /// Moves the ordinary counter too: the message really was not handed over,
    /// and the manual defines the total as messages the bridge could not hand
    /// over. This one only says how many of them were copies.
    pub fn dropped_republication(&self, meter: &MeterId, reason: DropReason) {
        self.dropped(meter, reason);
        self.0.send_modify(|fleet| {
            if let Some(entry) = fleet.meters.iter_mut().find(|m| &m.meter == meter) {
                entry.republications_lost = entry.republications_lost.saturating_add(1);
                fleet.generation += 1;
            }
        });
    }

    pub fn dropped(&self, meter: &MeterId, reason: DropReason) {
        self.0.send_modify(|fleet| {
            if let Some(entry) = fleet.meters.iter_mut().find(|m| &m.meter == meter) {
                let cell = &mut entry.dropped[reason.index()];
                *cell = cell.saturating_add(1);
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

    /// Counts one reading this meter lost before the wire (story 4.11 AC1).
    ///
    /// `generation` advances with it, in the SAME modification, for the reason
    /// [`FleetState::generation`] gives: a write that skips it hands a reader a
    /// state no instant ever had, and the snapshot property AR6 rests on becomes
    /// untestable.
    pub fn dropped(&self, reason: DropReason) {
        self.fleet.send_modify(|fleet| {
            let entry = &mut fleet.meters[self.index];
            // **THE ONLY PATH BY WHICH `Culprit::Bridge` REACHES A SCREEN** (story
            // 6.3): no `Cause` yields it, because the oracle judges readings and a
            // reading is never this process's fault. A reading LOST is.
            entry.culprit = Some(reason.culprit());
            let cell = &mut entry.dropped[reason.index()];
            *cell = cell.saturating_add(1);
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

/// What one meter's loop carries from one tick to the next.
///
/// # Why this is a struct and not four parameters (story 2.7 AC4)
///
/// It was three, threaded individually through a `step_once` that already had six
/// parameters, and every call site was a place to pass `last` where
/// `energy_reference` belonged — two `Option`s of different types kept them apart
/// by luck rather than by design. The fourth memory is what made it worth paying:
/// `last_http_date` has the same type as nothing else here, and the oracle that
/// needs it is the one whose whole subject is a value that stopped changing.
///
/// **Every field is a memory the ORACLES read, never bookkeeping.** Anything the
/// loop needs for itself stays in the loop.
#[derive(Debug, Default)]
pub struct MeterMemory {
    /// The last reading this meter ADOPTED — refused readings do not enter, which
    /// is story 2.3 AC4 and the defect it closed.
    pub last: Option<crate::domain::Measurement>,
    /// The energy-monotonicity yardstick (story 2.2, persisted since 2.3 AC5).
    pub energy_reference: Option<crate::domain::Kwh>,
    /// When the source told us not to come back before. Story 2.6: the ONE
    /// source-side wait this bridge honours, because it is the one the poll
    /// interval cannot know about.
    pub rate_limited_until: Option<MonotonicMs>,
    /// The over-age cause this meter last published, so a RE-SERVED measurement
    /// keeps it instead of falling back ([#79], ADR 0048).
    ///
    /// A meter that measures less often than the bridge polls re-serves the same
    /// measurement on the intermediate ticks. Its `value_date` has not advanced,
    /// and that is not evidence that it stopped — it is the absence of evidence
    /// either way, so the discrimination keeps the answer it already reached.
    /// Without it the cause flapped with the polling phase: at ADR 0004's measured
    /// cadences against the default 30 s period, roughly every other tick called a
    /// producing meter stopped.
    ///
    /// **In memory only.** Like `last_http_date` it is not persisted, so the first
    /// tick after a restart has no previous answer and falls back to what a single
    /// reading honestly deserves. [#80] owns that class of question.
    pub over_age_cause: Option<crate::core::oracle::Cause>,
    /// The `Date` header of the last SUCCESSFUL fetch, for the stalled-feed oracle
    /// (story 2.7 AC1).
    ///
    /// **This one does NOT follow the adoption rule, and the difference is the
    /// point.** `last` and `energy_reference` refuse a reading the oracles refused
    /// (story 2.3 AC4) because they are yardsticks for a VALUE, and a value we
    /// distrusted must not become the reference. This is a yardstick for the
    /// RESPONSE: the question it answers is *"is the cloud still rebuilding its
    /// answer?"*, which has nothing to do with whether we trusted the numbers
    /// inside. Refusing to record a header because the reading was stale would make
    /// a stale meter look like a frozen cloud on the following tick.
    pub last_http_date: Option<crate::domain::UtcMillis>,
    /// The `value_date` of the last successful fetch, for the over-age guard's
    /// wrong-clock/old-data discrimination (story 2.7 AC2,
    /// [`Policy::step_remembering`]).
    ///
    /// Same family as `last_http_date` and the same non-adoption rationale: it
    /// answers *"is the meter still producing new measurements?"*, which is a
    /// fact about the feed rather than about whether we trusted the numbers. One
    /// guard at the recording site: story 1.7 pins an unparseable `ValueDate` to
    /// the epoch, and a sentinel is not a measurement time — recording it would
    /// make the next real reading look like production resuming. Anything below
    /// the plausibility floor therefore never enters this memory.
    pub last_value_date: Option<crate::domain::UtcMillis>,
}

/// Runs one iteration: heartbeat, fetch (bounded), judge, forward.
///
/// Split out of the loop so a test can drive exactly one step without a timer.
/// Returns the state to carry into the next iteration.
pub async fn step_once<S: Source + Send>(
    ctx: &Context<'_>,
    source: &mut S,
    previous: State,
    memory: &mut MeterMemory,
) -> (State, Verdict) {
    let MeterMemory {
        last,
        energy_reference,
        rate_limited_until,
        last_http_date,
        last_value_date,
        over_age_cause,
    } = memory;
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
    // AND THE OTHER CASE WHERE ASKING IS POINTLESS ([#81]). `Failed` is absorbing
    // (ADR 0009): no answer to this fetch can change an absorbing verdict, so the
    // call is a request made for nothing — every period, for ever, until a
    // restart. For the credential latch that is precisely the hammering story
    // 2.6's own doc warns against: *"retrying with a credential the other end
    // refused is how a bridge hammers an API"*, performed by the poll loop rather
    // than by a caller.
    //
    // **The cycle still publishes**, which is why this is shaped exactly like the
    // rate-limit wait above rather than as an early return: ADR 0027 requires a
    // verdict every cycle for these meters — they are not gone, the asking is
    // broken — and the refusal is now available to synthesise one, which is what
    // [#75] made possible by putting it inside `State::Failed`.
    //
    // `DeviceNotInAccount` is NOT here: story 3.5 already stops that one at the
    // loop, where the DDEATH ends the publication too (ADR 0034). Reaching it
    // here would mean the certificate never went out.
    //
    // Nothing arms a wait from this path: the tick is `Fatal`, and only a
    // `RateLimited` carrying a delay sets `rate_limited_until`.
    //
    // **AND ONLY WHEN THERE IS SOMETHING TO REPUBLISH**, which is the condition
    // the first draft of this change omitted and an integration test caught. A
    // meter latched on its very FIRST tick has no last measurement, and the
    // republish path correctly gives it nothing — *"its DBIRTH already declared it
    // valueless and non-good, which is the truth"*. Skipping the fetch there takes
    // away the only way it could ever acquire one, and the meter falls silent for
    // good: this defect traded for a worse one.
    //
    // The case [#81] is about is the other one, and it is the common one: a
    // credential that EXPIRES under a running bridge, on a meter that has been
    // publishing. That meter has a last measurement, so the cycle keeps its
    // per-cycle `Bad` verdict and the API stops being asked.
    let latched = match previous {
        State::Failed(refusal)
            if refusal != crate::core::source::Refusal::DeviceNotInAccount && last.is_some() =>
        {
            Some(refusal)
        }
        _ => None,
    };
    let tick: Tick = if let Some(refusal) = latched {
        Err(SourceError::Fatal {
            refusal,
            reason: "the meter is latched; no request was made, because no answer \
                     could change an absorbing verdict"
                .to_string(),
        })
    } else if waiting {
        Err(SourceError::RateLimited { retry_after: None })
    } else {
        match tokio::time::timeout(config.fetch_timeout, source.fetch(meter)).await {
            Ok(result) => result,
            // The deadline elapsed: the cloud is silent. That is a verdict input,
            // not an error to swallow.
            Err(_elapsed) => Err(SourceError::Timeout),
        }
    };

    // THE REASON REACHES AN OPERATOR HERE, AND NOWHERE ELSE DID (story 2.6 AC5).
    //
    // Every `SourceError` carries a `reason` written to tell someone what to
    // repair — ADR 0029's *"correct the serial or the device id in the
    // configuration, then restart"*, `UnknownDevice`'s two origins, serde's field
    // name. **None of it was rendered anywhere.** Verified on 2026-08-13 by
    // deleting both `impl Display` and `impl Error` for `SourceError`: the library
    // compiled with zero errors. The `Cause` token reached the wire and the screen;
    // the sentence that says what to DO reached nobody.
    //
    // One line per failing cycle, which is the cadence this codebase already uses
    // for a fault it cannot fix by itself (the `DroppedUndeclaredDevice` warn). A
    // latched meter therefore repeats — deliberately: ADR 0027's rule is that every
    // cycle publishes a verdict rather than falling silent, and a log that goes
    // quiet while the fault persists is the same lie in another medium.
    // AND THE HONOURED WAIT SAYS SO INSTEAD OF LYING, which the 2026-08-13 review
    // caught in this very block. While the wait is running, line ~407 mints a
    // synthetic `RateLimited { retry_after: None }`, whose `Display` reads *"source
    // rate-limited, no delay given"* — the exact opposite of what happened, since
    // `rate_limited_until` is armed ONLY from a `Some(delay)`. At the minimum period
    // a `Retry-After: 300` would have produced sixty lines denying that a delay was
    // ever given. The lie predates this warn; nothing rendered it until now.
    if let Err(error) = &tick {
        match *rate_limited_until {
            Some(until) if waiting => tracing::warn!(
                meter = %meter,
                remaining_s = (until.0 - now_mono.0).max(0) / 1_000,
                "not asking this meter: the source asked us to wait, and the wait is \
                 being honoured"
            ),
            _ => tracing::warn!(meter = %meter, %error, "this meter could not be read"),
        }
    }

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

    // Judged WITH the previous reading's `value_date` (story 2.7 AC2): the
    // over-age guard is the one row of the table that can tell a wrong clock
    // from old data, and only when it knows whether the meter produced since.
    let (freshness_state, freshness) = policy.step_remembering(
        previous,
        &tick,
        clock.wall(),
        *last_value_date,
        *over_age_cause,
    );

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
    // THE FEED ITSELF, judged from the relation between two responses (story 2.7
    // AC1). Separate from `freshness` on purpose: a replayed response is internally
    // consistent, so nothing `Policy::step` can see distinguishes it from a working
    // cloud. `feed_is_advancing` is the only judgement here whose input is not a
    // fact about this reading.
    let feed = match &tick {
        Ok(reading) => match reading.http_date {
            Some(http_date) => crate::core::oracle::feed_is_advancing(*last_http_date, http_date),
            // No `Date` header at all: `judge_reading` already publishes
            // `NoFreshnessProof` for it, and an oracle about a header that is not
            // there would answer a question nobody asked.
            None => Verdict::good(),
        },
        // A fetch that did not complete carries no response to compare.
        Err(_) => Verdict::good(),
    };
    // RECORDED ON EVERY SUCCESSFUL FETCH, whatever the verdict — see
    // `MeterMemory::last_http_date` for why this one does not follow the adoption
    // rule the value memories obey.
    if let Ok(reading) = &tick
        && let Some(http_date) = reading.http_date
    {
        *last_http_date = Some(http_date);
    }
    // AND THE METER'S OWN CLOCK, for the wrong-clock/old-data discrimination —
    // recorded AFTER judging, so this tick was judged against the previous one.
    // The floor guard keeps story 1.7's epoch sentinel out: an unparseable
    // `ValueDate` is pinned to 0, and a sentinel entering this memory would make
    // the next real reading look like production resuming.
    if let Ok(reading) = &tick
        && reading.value_date() >= crate::core::state_machine::PLAUSIBILITY_FLOOR
    {
        *last_value_date = Some(reading.value_date());
    }

    // AND THE OVER-AGE ANSWER IS REMEMBERED, so a re-served measurement can keep
    // it ([#79], ADR 0048). Only the two over-age causes are: this memory answers
    // *"which of the two faults did we decide this reading has"*, and a verdict
    // that is not about age has no opinion on that question — clearing it on one
    // would make the next re-serve fall back, which is the flap this repairs.
    if let Some(cause @ (Cause::TimestampsDisagree | Cause::ReadingTooOld)) = freshness.cause() {
        *over_age_cause = Some(cause);
    }

    let judgements = [
        // Freshness and identity judge the whole response: a reading that is too
        // old is too old in both its numbers.
        Judgement::about_reading(freshness),
        // And whether that response is a new one at all.
        Judgement::about_reading(feed),
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
    //
    // ADR 0048 TURNED THAT INTO A QUESTION WITH A SHAPE. `State::Failed` now names
    // the refusal that latched it, and a composed metric-scoped cause carries no
    // refusal — so the branch this comment describes can no longer be written
    // without deciding WHICH refusal such a cause means. It was unreachable today
    // (the paragraph above proves it, and deleting it was measured to change
    // nothing), so the line goes and [#71] inherits a decision that now has a
    // compiler-shaped reason to be taken rather than a prose one.
    let next = freshness_state;

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
    // A REFUSED FEED ADOPTS NOTHING, and this is where [#69] closes (story 2.7
    // AC5). A replayed response proves nothing new — its numbers are copies of
    // an earlier answer — yet the SOURCE marks it `Good`, which is exactly the
    // disagreement story 2.3 AC3's adoption rule has been waiting to observe.
    // Two leaks without this gate, both reachable:
    //
    //  - the YARDSTICK: a replayed OLDER response carries a lower index, the
    //    monotonicity oracle duly says `counter-went-backwards`, and the
    //    meter-replacement exemption below would adopt it — the reference
    //    rewound by a replay, so the next genuine reading is judged against an
    //    index from the past and a real reset can pass as `Good`. FR15 defeated
    //    by the exemption that exists to serve it.
    //  - the BUFFER: a replay is `Stale`, not `Bad`, so `last` would adopt it
    //    and the next silent cloud republishes an OLD reading in place of the
    //    newest accepted one.
    //
    // The old guard (`reading.value.quality != Quality::Bad`) admits both,
    // because it asks the SOURCE whether the reading is trustworthy — the very
    // question the oracle layer exists to answer instead.
    let feed_refused = feed.quality() != Quality::Good;
    // THE EXEMPTION NEEDS THE FEED TO VOUCH, NOT MERELY TO NOT OBJECT — the
    // review of this story's own gate found the difference reachable. A response
    // with NO `Date` header leaves the feed oracle silent (`None => good()`
    // above: no header, no question), so `!feed_refused` is true of it — and a
    // replayed older response with its header stripped walked through the
    // meter-replacement exemption and rewound the reference by the door the gate
    // had just closed for headered replays. The exemption re-baselines FR15's
    // yardstick, which is the single most trusting thing this function does; it
    // therefore requires the positive evidence (a `Date` the oracle saw and did
    // not refuse), while ordinary adoption keeps requiring only the absence of
    // refusal — a merely headerless reading should not lose its republication.
    //
    // Two limits of the voucher, named by the review of the repair rather than
    // discovered later: a genuine meter replacement behind a permanently
    // Date-stripping path stays `Bad(counter-went-backwards)` — surviving
    // restarts, since the reference is persisted — until one headered response
    // arrives (such a feed is already loudly `Stale(no-freshness-proof)` on
    // every tick, so the state is visible, and the accepted trade is that FR15's
    // yardstick never re-baselines on evidence the feed oracle could not see).
    // And on the first tick after a restart the vouch is only "carries a Date"
    // — `last_http_date` is not persisted, so the oracle has nothing to compare
    // and a replayed older answer WITH its header can still rewind the restored
    // reference in that one window ([#80]; pre-existing, not opened by this
    // repair).
    let feed_vouches = !feed_refused && matches!(&tick, Ok(reading) if reading.http_date.is_some());
    let energy_verdict = published.for_metric(Measured::Energy);
    let reference_adoptable = match energy_verdict.quality() {
        Quality::Bad => feed_vouches && energy_verdict.cause() == Some(Cause::CounterWentBackwards),
        _ => !feed_refused,
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
    // AND THE BUFFER NEVER GOES BACK IN TIME — UNLESS THE CANDIDATE IS PROVEN.
    // From the same review finding, then corrected by the review of the repair
    // itself. `last` is by definition the NEWEST accepted measurement, so a
    // candidate whose `value_date` precedes the held one contradicts the
    // definition — the feed gate catches the headered replay, and this clause
    // catches the header-stripped one the oracle cannot see.
    //
    // The first version applied the clause to EVERY candidate, and the repair's
    // own review ran the case that breaks it: a meter whose clock was FAST left
    // a future `value_date` in the buffer, and once the clock was corrected,
    // every genuine reading — published `Good` on the wire — was refused
    // adoption until real time caught up with the mis-dated one. The clause was
    // trusting the exact meter-supplied timestamp the `timestamps-disagree`
    // oracle had just distrusted. A composed-`Good` reading is proven current
    // by the cloud's own clock (age within the allowance, feed advancing), so
    // it always becomes `last`; the no-going-back rule binds only candidates
    // whose freshness could NOT be proven, which is exactly the population a
    // header-stripped replay hides in.
    let last_adoptable = !feed_refused
        && match &tick {
            Ok(reading) => {
                let no_refused_value = Measured::ALL.iter().all(|metric| {
                    let refused = published.for_metric(*metric).quality() == Quality::Bad;
                    let carries_a_number = match metric {
                        Measured::Power => reading.value.power.is_some(),
                        Measured::Energy => reading.value.energy.is_some(),
                    };
                    !(refused && carries_a_number)
                });
                let proven_current = published.meter().quality() == Quality::Good;
                no_refused_value
                    && (proven_current
                        || last
                            .as_ref()
                            .is_none_or(|held| reading.value.value_date >= held.value_date))
            }
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
            // TRY, NEVER AWAIT (story 4.11 AC3). This was `send(update).await`
            // until 2026-08-18, and a blocking send here is not merely a lost
            // reading — it is a lost LOOP. The driver drains this channel inside
            // its `select!`, but NOT while it sleeps between reconnect attempts
            // (up to `RECONNECT_CEILING`, 30 s). Under a long enough outage the
            // 64 slots fill, every poll task parks inside `send`, `last_tick`
            // stops advancing, and `/healthz` answers `wedged: true` — which
            // Epic 7 wires to a CONTAINER RESTART. A broker being down would
            // therefore restart the bridge and kill the Sparkplug session for
            // every meter, on account of a fault entirely outside this process.
            //
            // The reading is dropped instead, counted, and traced with its
            // SOURCE timestamp — never buffered, never re-timestamped (AR7).
            if let Err(error) = outbox.try_send(update) {
                let (reason, update) = match error {
                    mpsc::error::TrySendError::Full(update) => (DropReason::OutboxFull, update),
                    mpsc::error::TrySendError::Closed(update) => (DropReason::MqttTaskGone, update),
                };
                heartbeat.dropped(reason);
                tracing::warn!(
                    meter = %meter,
                    reason = reason.as_str(),
                    // The reading's OWN timestamp, not `now`: what was lost is
                    // identified by when it was true, which is the only thing that
                    // lets an operator line the gap up against the source.
                    value_date = update.measurement.value_date.0,
                    "the judged reading never reached the transport; dropped, never buffered"
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

/// The identity one poll task serves: the meter, and the serial its Sparkplug
/// device was BIRTHED under — captured at spawn, which is what makes it the
/// IN-FORCE serial (story 3.5's review found the certificate taking the serial
/// from the stored row instead, and `Control::apply` stores a serial edit that
/// `reconfigure` classified ProcessRestart — not in force until the restart —
/// so a Death could name a device the wire never birthed while the born one
/// was left alive for ever). Bundled per the 2.7 rule: these two travel
/// together — they are ADR 0029's pair, seen from the wire's side.
#[derive(Debug, Clone)]
pub struct PolledMeter {
    /// The logical meter.
    pub meter: MeterId,
    /// The serial the DBIRTH used. Every certificate this task ever sends
    /// names THIS serial, whatever a not-yet-in-force edit desires.
    pub serial: crate::domain::Serial,
}

/// The task: loops until the outbox closes.
///
/// Eight parameters, the eighth being story 3.5's device channel. The lint is
/// right that this is a lot; a bundling struct is deferred deliberately — every
/// parameter is a distinct wiring concern the supervisor threads once, and a
/// struct would add ceremony at four call sites without removing a decision.
/// The 2.7 precedent (bundle at the FOURTH member) applies to values that
/// travel together; these do not. A ninth parameter is the revisit trigger.
#[allow(clippy::too_many_arguments)]
pub async fn run<S: Source + Send>(
    polled: PolledMeter,
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
    // The device-topology channel (story 3.5): the one thing this loop ever
    // sends on it is the DDEATH that ends a device the account refuses
    // (ADR 0034). Births and disable-deaths stay `reconfigure`'s.
    devices: mpsc::Sender<crate::app::mqtt_driver::DeviceCommand>,
) {
    let PolledMeter { meter, serial } = polled;
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
    // Everything this meter carries between ticks, in one place since story 2.7:
    // the energy yardstick loaded above, the wait a rate limit may arm (story 2.6,
    // monotonic so a wall-clock correction cannot shorten or extend it), and the
    // last measurement adopted — carried so a failed tick publishes a verdict about
    // it rather than saying nothing (story 3.2).
    let mut memory = MeterMemory {
        energy_reference: load_energy_reference(&reference_dir, &meter),
        ..MeterMemory::default()
    };
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
    // Story 3.5's two loop facts. `was_enabled` starts true because tasks are
    // spawned for the enabled set; `certified_gone` records that this device's
    // ending — the one DDEATH ADR 0034 prescribes when the account refuses the
    // id — has been sent, after which the certificate IS the publication and
    // the loop stops asking a question whose answer cannot change the verdict.
    let mut was_enabled = true;
    let mut certified_gone = false;
    let mut gone_pending = false;
    loop {
        ticker.tick().await;
        if outbox.is_closed() {
            tracing::info!(meter = %meter, "outbox closed; poll task stopping");
            return;
        }
        let current = config.load();
        // THE PERIOD RE-ARMS BEFORE ANY SKIP — the review of this story found
        // it below the two idle `continue`s, where a hot interval change left
        // the ticker pacing the OLD period while every touch recorded the NEW
        // one. `loop_age` divides age by the recorded period, so one meter
        // idling on purpose read as WEDGED for most of every window — the
        // false 503 `LastLoopTick::touch`'s own doc records as a past defect,
        // reintroduced on the idle paths, and `loop_age` takes the WORST
        // meter, so one disabled meter poisoned the whole bridge's health.
        // An idle loop still re-paces; the touches below then record the
        // cadence the ticker actually runs at.
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
        // THE ENABLED FLAG IS READ EVERY TICK ([#65] items 1 and 2). The loop
        // read only the period and the policy until story 3.5, so disabling a
        // meter kept calling the smart-me API every period for ever and filled
        // the log with one warn per discarded reading — nobody chose either;
        // they fell out. A disabled meter's task stays bound (the deliberate
        // design: re-enabling is a DBIRTH, not a restart) and keeps its
        // heartbeat (idling on purpose is not wedging), but it asks nothing,
        // publishes nothing, and warns about nothing.
        let row = current.meters.iter().find(|m| m.meter == meter);
        // A MISSING row reads as enabled, and the direction is deliberate: the
        // operator's "stop" is `enabled: false` on a row that exists, never a
        // row's absence. The review of this story corrected the first version
        // of this comment, which called a rowless served meter "unreachable":
        // `Control::apply` stores `new.meters` wholesale, so REMOVING a served
        // row and saving produces exactly this state — classified
        // ProcessRestart, its DDEATH sent by `classify_meters`, and the task
        // keeps polling until the restart the classifier demanded (the
        // pre-existing zombie, unchanged here). Reading absence as "stop"
        // would HIDE that restart debt behind a quiet meter; the loud
        // dropped-undeclared warns are the debt staying visible.
        let enabled = row.is_none_or(|m| m.enabled);
        if !enabled {
            heartbeat.touch(clock.monotonic(), current.poll.interval.as_millis() as i64);
            if was_enabled {
                // Said ONCE, and the fault goes with the meter ([#65] item 3):
                // the operator disabling a broken meter is the obvious gesture,
                // and it must quieten the alarm it is aimed at.
                tracing::info!(
                    meter = %meter,
                    "meter disabled by the operator; polling idles and its \
                     fault, if any, is retired with it"
                );
                pulse.retire(&meter);
                // A re-enable starts from Stale-until-proven, and a gone
                // episode ends with the disable: if the device is still not in
                // the account, the next enabled fetch re-latches and
                // re-certifies within one tick — loudly, which is the point.
                state = State::initial();
                certified_gone = false;
                gone_pending = false;
            }
            was_enabled = false;
            continue;
        }
        if !was_enabled {
            tracing::info!(
                meter = %meter,
                "meter re-enabled; judged afresh — Stale until proven, as at \
                 any start"
            );
            was_enabled = true;
        }
        // AFTER THE CERTIFICATE, SILENCE — and the silence is the honesty
        // (ADR 0034). The account said it has no such device; the DDEATH said
        // so on the wire; re-asking every period would hammer the API with a
        // question whose answer cannot change an absorbing latch, and
        // republishing `Bad` would tell a host "misbehaving" about a device
        // that is GONE. The alarm stays: the cell keeps its `Failed`, so
        // `failed_sources` and `/` name the meter until a restart or a
        // configuration change — the certificate retires the WIRE's device,
        // never the operator's alarm.
        if certified_gone {
            heartbeat.touch(clock.monotonic(), current.poll.interval.as_millis() as i64);
            continue;
        }
        // THE CERTIFICATE GOES OUT ONE TICK AFTER THE LATCH VERDICT, and the
        // review of this story is why: verdict and certificate travel on
        // sibling channels into the driver's `select!`, so sending them in the
        // same tick left their wire order to a coin toss — a Death winning
        // meant the final `device-not-in-account` verdict was dropped as
        // undeclared and never reached the host. One period of separation
        // NARROWS that race to a driver stalled for a full poll period (a
        // reconnect backoff spanning >= PERIOD_MIN can still leave both queued
        // together, and the `select!` is unbiased) — said rather than claimed
        // away, per the review of the repair; closing it fully needs an
        // ordered path the driver does not have. Certified only on a
        // SUCCESSFUL send (the second review finding here): a failed send
        // retries next tick rather than entering a silence nothing ever
        // ended.
        if gone_pending {
            heartbeat.touch(clock.monotonic(), current.poll.interval.as_millis() as i64);
            if devices
                .send(crate::app::mqtt_driver::DeviceCommand::Death(
                    serial.clone(),
                ))
                .await
                .is_ok()
            {
                tracing::warn!(
                    meter = %meter,
                    "the account has no such device; its DDEATH ends it on the \
                     wire, and the fault stays named until a restart or a \
                     configuration change"
                );
                gone_pending = false;
                certified_gone = true;
            } else {
                tracing::warn!(
                    meter = %meter,
                    "mqtt task is gone; the device certificate could not be \
                     queued and will be retried next tick"
                );
            }
            continue;
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
        let before = memory.energy_reference;
        (state, published) = step_once(&ctx, &mut source, state, &mut memory).await;
        // Persisted only when it MOVED, so a quiet meter does not rewrite the
        // same number every period — an fsync per meter per cycle for a value
        // that did not change.
        if memory.energy_reference != before
            && let Some(energy) = memory.energy_reference
        {
            store_energy_reference(&reference_dir, &meter, energy);
        }
        // The verdict reaches anything outside that can report on this meter —
        // BOTH halves of it since Story 2.3, so a screen cannot call a meter
        // healthy while the broker is being told otherwise ([#62]).
        // AR19's enriched state, written where the verdict already was (story
        // 6.3). Everything here is data the tick already holds: the wall instant,
        // the threshold the verdict was reached against, and the source's own
        // acquisition time — which is what lets `last_changed_at` tell a NEW
        // reading from the same one republished, as ADR 0027 requires every cycle
        // to do.
        pulse.record_at(
            &meter,
            state,
            published,
            Some(Publication {
                at: clock.wall(),
                threshold_ms: current.policy.max_age_ms(),
                value_date: memory.last.as_ref().map(|m| m.value_date),
                power_kw: memory.last.as_ref().and_then(|m| m.power.map(|p| p.0)),
                energy_kwh: memory.last.as_ref().and_then(|m| m.energy.map(|e| e.0)),
            }),
        );

        // THE ENDING ARMS HERE (story 3.5 AC3, ADR 0034): the account
        // pronounced this device absent. Only THIS latch: a credential or
        // base-URL latch is evidence about the ASKING, not about the device,
        // and a certificate there would declare dead a device nobody has
        // evidence about. The certificate itself goes out next tick — see the
        // `gone_pending` arm above for why the separation is the ordering.
        if !certified_gone && !gone_pending && published.cause() == Some(Cause::DeviceNotInAccount)
        {
            gone_pending = true;
        }
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
        // From contract v12 the cause is a metric of its own (ADR 0044): a
        // property is written by a BIRTH and never updated by a DDATA, measured
        // against a live host, so a cause carried as one stands frozen for ever.
        let cause = |name: &str| {
            metrics
                .iter()
                .find(|m| m.name == name)
                .map(|m| match &m.value {
                    sparkplug_b::model::MetricValue::String(v) => v.clone(),
                    other => panic!("a cause is a string, got {other:?}"),
                })
                .unwrap_or_else(|| panic!("{name} is published"))
        };
        assert_eq!(cause("Cause/Power"), "unit-not-recognised");
        assert!(
            power.properties.is_empty(),
            "and the metric itself carries no property at all now"
        );
        assert!(
            matches!(energy.value, sparkplug_b::model::MetricValue::Double(v) if v == 4_843.822),
            "the sound index reaches the consumer at full value. Got {:?}",
            energy.value
        );
        assert_eq!(
            cause("Cause/Energy"),
            "no-cause",
            "and names no cause — explicitly, since contract v11, and on its own \
             tag since v12 — least of all its neighbour's"
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
    /// **An honoured wait says so, instead of denying that a delay was given.**
    ///
    /// Found by the 2026-08-13 review of story 2.6's own repair. While the wait
    /// runs, the loop synthesises `RateLimited { retry_after: None }`, whose
    /// `Display` is *"source rate-limited, no delay given"* — and the wait exists
    /// precisely BECAUSE a delay was given, since `rate_limited_until` is armed
    /// only from a `Some`. At the minimum period a `Retry-After: 300` would have
    /// printed sixty lines denying it.
    ///
    /// The lie is older than the log line; nothing rendered a `SourceError` until
    /// story 2.6 AC5, so it had never been visible. Which is the argument for
    /// AC5 in one sentence.
    ///
    /// FALSIFIED — mutation RUN, message copied: collapsing the two arms back into
    /// the generic `warn!` goes RED with the pre-fix line quoted in full —
    /// *"…: \"… WARN … this meter could not be read meter=garage error=source
    /// rate-limited, no delay given\""*.
    #[tokio::test]
    async fn an_honoured_wait_does_not_deny_that_a_delay_was_given() {
        // THROUGH THE SHARED HARNESS ([#94]): a capture built by hand here is one
        // a thread with no subscriber can switch off for the whole process.
        let (sink, _capture_guard) = crate::test_capture::capture_guard(tracing::Level::TRACE);
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let mut source = FakeSource::new()
            .then(Ok(reading(Quality::Good, 950)))
            .then(Err(SourceError::RateLimited {
                retry_after: Some(Duration::from_secs(60)),
            }))
            .then(Ok(later(reading(Quality::Good, 950), 1)));
        let (tx, _rx) = mpsc::channel(8);
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
        let mut mem = MeterMemory::default();

        let (state, _) = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
        // The 429 itself: a real refusal, logged as one.
        let (_, _) = step_once(&ctx, &mut source, state, &mut mem).await;
        let after_refusal = sink.text();
        // And now the tick that is merely WAITING, which is the subject.
        let (_, _) = step_once(&ctx, &mut source, State::Stale, &mut mem).await;
        drop(_capture_guard);
        let while_waiting = sink.text()[after_refusal.len()..].to_string();

        assert!(
            !while_waiting.contains("no delay given"),
            "the wait is running because the server named a delay; denying it sends \
             an operator looking for a rate limit nobody described: {while_waiting:?}"
        );
        assert!(
            while_waiting.contains("the wait is being honoured"),
            "and an honoured wait is not the same event as a fresh refusal: \
             {while_waiting:?}\n\
             \n\
             **IF THAT SEGMENT IS EMPTY, THIS IS [#94] AND HERE IS WHAT DECIDES IT.** \
             The waiting branch is unconditional — `rate_limited_until` is Some and the \
             FakeClock never advances, so the third tick MUST log. Three readings, \
             printed rather than left to the next investigator:\n\
             - the wait as the memory holds it: {:?}\n\
             - the whole capture, not the segment: {:?}\n\
             - the segment boundary: {} bytes before the third tick\n\
             \n\
             A wait of None means the SECOND tick did not arm it, and the fault is \
             upstream of the capture. A wait armed, with the line absent from the whole \
             capture, is the capture losing a line — which is what [#94] describes and \
             what two eliminated hypotheses (the fetch deadline; the callsite interest \
             cache) do NOT explain.",
            mem.rate_limited_until,
            sink.text(),
            after_refusal.len()
        );
        assert!(
            while_waiting.contains("remaining_s="),
            "an operator waiting must be told how long, or the line says only that \
             something is wrong: {while_waiting:?}"
        );
    }

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
        let mut mem = MeterMemory::default();

        // A good reading first: `last` must hold something, or a silent cycle has
        // nothing to republish and ADR 0027's certificate path is a different test.
        let (state, _) = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
        let (_, first) = step_once(&ctx, &mut source, state, &mut mem).await;
        assert_eq!(first.cause(), Some(Cause::SourceRateLimited));
        assert!(
            mem.rate_limited_until.is_some(),
            "the wait is armed by the server's own delay"
        );

        // The clock has NOT advanced past the deadline, so no fetch may happen.
        let fetches_before = source.calls.len();
        let (_, second) = step_once(&ctx, &mut source, State::Stale, &mut mem).await;
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

    /// **[#76] — the wait ENDS.** The test above pins that the wait is honoured
    /// and never advances the clock past the deadline, so it never asks whether
    /// the wait is RELEASED — and the 2026-08-13 review ran the mutation that
    /// matters (`rate_limited_until.is_some()`, a wait that never expires) and
    /// the suite stayed green. Under that mutation a single 429 with any
    /// `Retry-After` takes the meter off the wire until the process restarts,
    /// publishing `source-rate-limited` for ever.
    ///
    /// FALSIFIED 2026-08-15, the review's own mutation RUN before this note:
    /// `rate_limited_until.is_some()` goes RED here — *"the deadline passed and
    /// the source was not asked again: the wait never ends, and one rate limit
    /// costs the meter its process lifetime"*.
    #[tokio::test]
    async fn the_honoured_wait_ends_when_the_deadline_passes() {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let mut source = FakeSource::new()
            .then(Err(SourceError::RateLimited {
                retry_after: Some(Duration::from_secs(60)),
            }))
            .then(Ok(later(reading(Quality::Good, 950), 1)));
        let (tx, _rx) = mpsc::channel(8);
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
        let mut mem = MeterMemory::default();

        let (state, first) = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
        assert_eq!(first.cause(), Some(Cause::SourceRateLimited), "the premise");

        // 59 s into a 60 s wait is still the wait — the boundary matters,
        // because a release that fires early is the interval ignoring the server.
        clock.advance_ms(59_000);
        let fetches = source.calls.len();
        let (state, second) = step_once(&ctx, &mut source, state, &mut mem).await;
        assert_eq!(
            source.calls.len(),
            fetches,
            "one second before the instant the source named is before it"
        );
        assert_eq!(second.cause(), Some(Cause::SourceRateLimited));

        // And past it, the wait is OVER. A wait that never ends turns one 429
        // into a meter that is off the wire until somebody restarts the process.
        clock.advance_ms(2_000);
        let (_, third) = step_once(&ctx, &mut source, state, &mut mem).await;
        assert!(
            source.calls.len() > fetches,
            "the deadline passed and the source was not asked again: the wait \
             never ends, and one rate limit costs the meter its process lifetime"
        );
        assert_eq!(
            third.quality(),
            Quality::Good,
            "one 429 with a delay costs exactly the delay, then the meter is back"
        );
        assert!(
            mem.rate_limited_until.is_none(),
            "a served wait is disarmed, not left to be compared against for ever"
        );
    }

    /// A reading with a chosen energy index, for the monotonicity tests.
    fn reading_with_energy(quality: Quality, age_ms: i64, energy: f64) -> Reading {
        let mut r = reading(quality, age_ms);
        r.value.energy = Some(Kwh(energy));
        r
    }

    /// The same answer one poll period later — both timestamps advance, the AGE
    /// between them does not.
    ///
    /// **Story 2.7 added this, and what it fixed is a blind spot in the fixtures
    /// themselves.** Every reading here pinned `value_date` to `BASE` and
    /// `http_date` to `BASE + age`, so a sequence of ticks handed back a
    /// byte-identical response — which is precisely the frozen cloud the
    /// stalled-feed oracle exists to refuse. The tests were modelling the fault
    /// while asserting health, and nothing could see it until an oracle looked at
    /// two responses instead of one.
    fn later(mut r: Reading, ticks: i64) -> Reading {
        let shift = ticks * config().interval.as_millis() as i64;
        r.value.value_date = UtcMillis(r.value.value_date.0 + shift);
        r.http_date = r.http_date.map(|d| UtcMillis(d.0 + shift));
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
            .then(Ok(later(reading_with_energy(Quality::Good, 950, 12.0), 1)))
            .then(Ok(later(reading_with_energy(Quality::Good, 950, 12.5), 2)));
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };

        let mut mem = MeterMemory::default();

        // THE PREMISE: a rising counter is Good, or the Bad below proves nothing.
        let (s1, _) = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
        assert_eq!(s1, State::Fresh);

        // The drop. The STATE is kept and asserted since 2026-08-11 (deferred
        // review patch): `let _ =` discarded exactly the value that would have
        // shown the wire and the operator surfaces disagreeing, which is what let
        // that divergence live until the review found it by reading.
        let (s2, _) = step_once(&ctx, &mut source, s1, &mut mem).await;
        assert_eq!(
            s2,
            State::Fresh,
            "a backwards counter is a VALUE fault, not an identity one: the meter              stays in the freshness machine's `Fresh` and keeps polling. What must              NOT happen is this state reaching an operator surface unaccompanied —              see the published verdict asserted below, and `MeterState::published`"
        );

        // And a reading consistent with the NEW index.
        let (s3, _) = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
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

        let mut mem = MeterMemory::default();
        let (s1, _) = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
        let _ = step_once(&ctx, &mut source, s1, &mut mem).await;
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

        let mut mem = MeterMemory::default();
        let (good, _) = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
        assert_eq!(
            good,
            State::Fresh,
            "the premise: the meter must first be proven fresh"
        );

        let (after, _) = step_once(&ctx, &mut source, good, &mut mem).await;
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
    /// **Story 2.7 AC1, through the pipeline rather than against the function.**
    ///
    /// The unit test in `core::oracle` proves the comparison compares. This proves
    /// a REPLAYED response reaches the outbox marked as one — the distinction
    /// stories 2.1 and 2.3 were both caught by, where an assertion on the
    /// in-process value passed while nothing reached the wire.
    ///
    /// **Reading-scoped, so BOTH metrics carry it**, and that is not ADR 0031 being
    /// violated: a response that was not regenerated says nothing about either
    /// number in particular, because neither number is new.
    #[tokio::test]
    async fn a_replayed_response_is_refused_on_both_metrics() {
        let identical = reading(Quality::Good, 950);
        let (state, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(identical.clone()))
                .then(Ok(identical)),
        )
        .await;

        assert_eq!(sent.len(), 2, "every tick publishes a verdict (ADR 0027)");
        assert_eq!(
            sent[0].verdict().cause(),
            None,
            "the premise: the first answer is good, because it has no predecessor"
        );
        assert_eq!(
            sent[1].verdict().cause(),
            Some(Cause::FeedNotAdvancing),
            "the second answer is the first answer, and a frozen cloud must not \
             look like a working one"
        );
        for metric in [Measured::Power, Measured::Energy] {
            assert_eq!(
                sent[1].published_for(metric),
                Quality::Stale,
                "neither number is new, so neither may be published as current"
            );
        }
        // AND THE STATE STAYS `Fresh`, WHICH IS CORRECT AND WORTH EXPLAINING,
        // because the first draft of this test asserted `Stale` and was wrong.
        //
        // `State` is the freshness state machine's verdict on the timestamps INSIDE
        // one reading, and a replay's are impeccable — that is the whole difficulty.
        // What an operator reads is the COMPOSED verdict: `FleetState::degraded`
        // filters on `published.quality()`, so this meter is reported degraded with
        // its cause, which is the [#62] lesson already applied. The two answers
        // differ on purpose and only one of them is a surface.
        //
        // What would be a defect is a LATCH — a frozen feed is not a refusal, and
        // requiring a restart to clear a cloud that thawed by itself would be the
        // fault ADR 0029 accepted for identity and nobody accepted for this.
        assert!(
            !matches!(state, State::Failed(_)),
            "a frozen feed must not need a restart"
        );
    }

    /// The other half, and the reason AC1 chose `http_date` over `value_date` at
    /// drafting: **a meter that genuinely stops reporting is not a replay.** Its
    /// `value_date` freezes while the cloud's `Date` keeps advancing — ordinary
    /// staleness, and calling it a frozen feed would send an operator to smart-me
    /// when the fault is at the meter.
    #[tokio::test]
    async fn a_silent_meter_behind_a_live_cloud_is_not_called_a_replay() {
        let first = reading(Quality::Good, 950);
        // The cloud rebuilt its answer — `http_date` moved — but the meter did not
        // report again, so `value_date` did not.
        let mut second = later(first.clone(), 1);
        second.value.value_date = first.value.value_date;

        let (_, sent) = drive_sequence(FakeSource::new().then(Ok(first)).then(Ok(second))).await;

        assert_ne!(
            sent[1].verdict().cause(),
            Some(Cause::FeedNotAdvancing),
            "the FEED advanced; it is the METER that went quiet, and the two send \
             an operator to different places"
        );
    }

    /// **Story 2.7 AC5 — a replayed response rewinds nothing, and [#69] closes
    /// on this test.**
    ///
    /// The disagreement story 2.3 AC3 waited for since FR14's withdrawal: a
    /// reading the SOURCE marks `Good` (asserted on the fixture below) that the
    /// composed verdict refuses (`feed-not-advancing`). The OLD adoption guard —
    /// `reading.value.quality != Quality::Bad` — asks the source, so it adopts
    /// the replay into both memories; the rule that replaced it must not. Until
    /// this story the set of causes producing that disagreement was empty, which
    /// is why swapping the rules turned nothing red.
    ///
    /// Both leaks are asserted where a consumer would meet them:
    ///  - tick 3 (a GENUINE reading below the true reference) must be `Bad`
    ///    `counter-went-backwards`. Under the old guard the replayed 850 000
    ///    became the reference via the meter-replacement exemption, so this
    ///    genuine backwards reading passed as `Good` — FR15 defeated by a replay.
    ///  - tick 5 (a silent cloud) must republish the newest ACCEPTED reading —
    ///    same values, its own `value_date` — not the replayed old one `last`
    ///    would have adopted. The two replays differ on purpose: the
    ///    EQUAL-index replay is the one only the feed oracle refuses (`Stale`,
    ///    so the pre-gate buffer rule adopted it), and the LOWER-index replay
    ///    is the one the meter-replacement exemption adopted as a reference.
    ///    Each gate has its own witness, so each mutation fails its own
    ///    assertion.
    #[tokio::test]
    async fn a_replayed_response_rewinds_neither_memory() {
        // The newest accepted state of the meter: index 900 000, measured at BASE.
        let genuine = reading_with_energy(Quality::Good, 950, 900_000.0);
        // A replay of a MINUTE-OLD answer with the same index: nothing about it
        // is wrong except that it is not new, so only the feed oracle refuses it
        // — and the source's own opinion of it is `Good`, which is the whole
        // disagreement.
        let mut replay_equal = reading_with_energy(Quality::Good, 950, 900_000.0);
        replay_equal.value.value_date = UtcMillis(BASE - 60_000);
        replay_equal.http_date = Some(UtcMillis(BASE - 59_050));
        assert_eq!(
            replay_equal.value.quality,
            Quality::Good,
            "the premise: the SOURCE calls the replay good; only the feed \
             oracle disagrees"
        );
        // A replay of an OLDER answer with a LOWER index: the monotonicity
        // oracle also refuses this one (`counter-went-backwards`), and the
        // meter-replacement exemption would adopt it as the new reference.
        let mut replay_older = reading_with_energy(Quality::Good, 950, 850_000.0);
        replay_older.value.value_date = UtcMillis(BASE - 120_000);
        replay_older.http_date = Some(UtcMillis(BASE - 119_050));
        // A genuine LATER reading whose index is below the true reference but
        // above the replayed one: the only input that can tell the two
        // reference rules apart.
        let backwards = later(reading_with_energy(Quality::Good, 950, 860_000.0), 1);

        let (_, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(genuine))
                .then(Ok(replay_equal))
                .then(Ok(replay_older))
                .then(Ok(backwards))
                .then(Err(SourceError::Timeout)),
        )
        .await;
        assert_eq!(sent.len(), 5, "every tick publishes a verdict (ADR 0027)");
        assert_eq!(
            sent[0].published(),
            Quality::Good,
            "the premise reached the wire"
        );

        // Both replays are refused on the wire; the equal one only by the feed.
        assert_eq!(
            sent[1].published_for(Measured::Energy),
            Quality::Stale,
            "a replayed number is not a current one, even when it is the same \
             number"
        );
        assert_eq!(
            sent[1].verdicts.for_metric(Measured::Energy).cause(),
            Some(Cause::FeedNotAdvancing)
        );

        // THE YARDSTICK HALF: the genuine backwards reading is judged against
        // the reference the replays tried to rewind.
        assert_eq!(
            sent[3].published_for(Measured::Energy),
            Quality::Bad,
            "the reference must still be 900 000: under the source-quality guard \
             the replayed 850 000 became the reference and this genuine \
             backwards reading passed as Good — the negative delta FR15 exists \
             to prevent, delivered by a replay"
        );
        assert_eq!(
            sent[3].verdicts.for_metric(Measured::Energy).cause(),
            Some(Cause::CounterWentBackwards)
        );

        // THE BUFFER HALF: the silent cloud republishes the newest ACCEPTED
        // reading — tick 1's, with tick 1's own `value_date`. Adopting the
        // equal-index replay leaves the same numbers behind a value_date from a
        // minute earlier: the wire would say the reading was true at a moment
        // the bridge never proved.
        assert_eq!(
            sent[4].measurement.energy,
            Some(Kwh(900_000.0)),
            "the republished index is the newest accepted one"
        );
        assert_eq!(
            sent[4].measurement.value_date,
            UtcMillis(BASE),
            "`last` must not have adopted the replay: its value_date is the \
             replayed answer's, and republishing it re-dates the reading to a \
             moment the bridge never accepted"
        );
    }

    /// **The review's finding on AC5's own gate: a replay with its `Date` header
    /// STRIPPED walked through it.** The feed oracle answers `good()` when there
    /// is no header (no header, no question — right for judging), so
    /// `!feed_refused` was true of a header-less replay and the
    /// meter-replacement exemption rewound the reference through the door the
    /// gate had just closed for headered ones. Two rules close it: the exemption
    /// now needs the feed to VOUCH (a `Date` seen and not refused), and `last`
    /// refuses any candidate older than what it holds, whatever produced it.
    ///
    /// FALSIFIED 2026-08-15, both mutations RUN before this note: reverting the
    /// exemption to `!feed_refused` goes RED on the tick-4 assertion
    /// (*"left: Good, right: Bad"* — the rewind, through the headerless door);
    /// removing the no-going-back clause goes RED on the tick-5 `value_date`
    /// (*"left: UtcMillis(…640000)"* — the republish re-dated a minute back).
    #[tokio::test]
    async fn a_replay_with_its_date_header_stripped_rewinds_neither_memory() {
        let genuine = reading_with_energy(Quality::Good, 950, 900_000.0);
        // The header-stripped EQUAL replay: only its age is unprovable
        // (`no-freshness-proof`), so nothing refuses its values — the buffer
        // clause is the only thing between it and `last`.
        let mut stripped_equal = reading_with_energy(Quality::Good, 950, 900_000.0);
        stripped_equal.value.value_date = UtcMillis(BASE - 60_000);
        stripped_equal.http_date = None;
        // The header-stripped OLDER replay: its lower index earns
        // `counter-went-backwards`, and the exemption would adopt it as a
        // meter replacement unless the feed vouches — which, headerless, it
        // cannot.
        let mut stripped_older = reading_with_energy(Quality::Good, 950, 850_000.0);
        stripped_older.value.value_date = UtcMillis(BASE - 120_000);
        stripped_older.http_date = None;
        let backwards = later(reading_with_energy(Quality::Good, 950, 860_000.0), 1);

        let (_, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(genuine))
                .then(Ok(stripped_equal))
                .then(Ok(stripped_older))
                .then(Ok(backwards))
                .then(Err(SourceError::Timeout)),
        )
        .await;
        assert_eq!(sent.len(), 5, "every tick publishes a verdict (ADR 0027)");
        assert_eq!(
            sent[1].verdict().cause(),
            Some(Cause::NoFreshnessProof),
            "the premise: a headerless replay is refused for its missing proof \
             only — the feed oracle has no header to compare, so nothing here \
             says 'replay'"
        );

        // THE YARDSTICK: still 900 000, or the genuine backwards reading passes.
        assert_eq!(
            sent[3].published_for(Measured::Energy),
            Quality::Bad,
            "the reference must not have been rewound through the headerless \
             door: the exemption re-baselines FR15's yardstick and may only act \
             on a response the feed oracle actually saw advance"
        );
        assert_eq!(
            sent[3].verdicts.for_metric(Measured::Energy).cause(),
            Some(Cause::CounterWentBackwards)
        );

        // THE BUFFER: the republish carries tick 1, not the minute-old replay.
        assert_eq!(sent[4].measurement.energy, Some(Kwh(900_000.0)));
        assert_eq!(
            sent[4].measurement.value_date,
            UtcMillis(BASE),
            "`last` holds the NEWEST accepted measurement by definition; \
             adopting one that is older contradicts it whatever stripped the \
             header"
        );
    }

    /// **The review of the repair found the repair wrong, and this is its probe
    /// made permanent.** The first no-going-back clause bound EVERY candidate,
    /// so a meter whose clock had been FAST — one adopted reading with a future
    /// `value_date` — starved the buffer of every genuine reading after the
    /// clock was corrected, until real time caught up with the mis-dated stamp.
    /// A reading published `Good` on the wire was barred from `last` while a
    /// `timestamps-disagree` one was served in its place. The clause was
    /// trusting the exact meter-supplied timestamp the oracle had just
    /// distrusted; a composed-`Good` candidate is proven current by the cloud's
    /// own clock and now always adopts.
    ///
    /// FALSIFIED 2026-08-15, mutation RUN before this note: removing the
    /// `proven_current` bypass goes RED here — *"left: Some(Kwh(900000.0))"*,
    /// the mis-dated reading republished in place of the corrected Good one —
    /// which is byte-for-byte the review probe's failure, now standing guard.
    #[tokio::test]
    async fn a_corrected_clock_does_not_starve_the_buffer() {
        // The meter's clock is an hour FAST: negative age, timestamps-disagree,
        // adopted into `last` (Stale holds its values) with a future value_date.
        let mut fast = reading_with_energy(Quality::Good, 950, 900_000.0);
        fast.value.value_date = UtcMillis(BASE + 3_600_000);
        fast.http_date = Some(UtcMillis(BASE + 950));
        // The operator fixes the clock: a genuine, Fresh, Good reading whose
        // value_date is honest again — and therefore EARLIER than the held one.
        let mut corrected = reading_with_energy(Quality::Good, 950, 900_010.0);
        corrected.value.value_date = UtcMillis(BASE + 30_000);
        corrected.http_date = Some(UtcMillis(BASE + 30_950));

        let (_, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(fast))
                .then(Ok(corrected))
                .then(Err(SourceError::Timeout)),
        )
        .await;
        assert_eq!(sent.len(), 3, "every tick publishes a verdict (ADR 0027)");
        assert_eq!(
            sent[0].verdict().cause(),
            Some(Cause::TimestampsDisagree),
            "the premise: the fast clock is caught, and its reading is held"
        );
        assert_eq!(
            sent[1].published(),
            Quality::Good,
            "the premise: the corrected reading is proven current on the wire"
        );
        assert_eq!(
            sent[2].measurement.energy,
            Some(Kwh(900_010.0)),
            "a reading the bridge published as Good must be what a silent cloud \
             republishes; barring it while serving the mis-dated one inverts \
             the buffer's own rule that `last` holds publishable measurements"
        );
        assert_eq!(
            sent[2].measurement.value_date,
            UtcMillis(BASE + 30_000),
            "and the republish carries the corrected stamp, not the future one"
        );
    }

    /// **The feed gate's own witness on the buffer, restored.** The
    /// no-going-back clause silently covered the equal-index replay in
    /// `a_replayed_response_rewinds_neither_memory` (its value_date is older),
    /// so deleting the feed gate from `last_adoptable` left the whole suite
    /// green — a witness lost to a repair, found by reviewing the repair. The
    /// case where the gate alone is load-bearing: a response whose `Date` went
    /// BACKWARDS (a replayed window, an out-of-order delivery) while its
    /// `value_date` is NEWER than the held one — the no-going-back clause
    /// passes it, its values are held (Stale, not Bad), and only the feed
    /// refusal keeps it out of `last`.
    ///
    /// FALSIFIED 2026-08-15, mutation RUN before this note: deleting
    /// `!feed_refused` from `last_adoptable` goes RED here — the out-of-order
    /// response's energy republished by the silent cloud — while the rest of
    /// the suite stays green, which is exactly the witness this test restores.
    #[tokio::test]
    async fn an_out_of_order_response_is_kept_out_of_the_buffer_by_the_feed_gate_alone() {
        let genuine = reading_with_energy(Quality::Good, 950, 900_000.0);
        // Date stepped BACK relative to the PREVIOUS response (feed refused)
        // while staying consistent with its own value_date (age 700 ms, so the
        // freshness oracle has no objection); value_date stepped FORWARD (the
        // no-going-back clause has none either); values intact (nothing Bad).
        let mut out_of_order = reading_with_energy(Quality::Good, 950, 900_020.0);
        out_of_order.value.value_date = UtcMillis(BASE + 200);
        out_of_order.http_date = Some(UtcMillis(BASE + 900));

        let (_, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(genuine))
                .then(Ok(out_of_order))
                .then(Err(SourceError::Timeout)),
        )
        .await;
        assert_eq!(sent.len(), 3, "every tick publishes a verdict (ADR 0027)");
        assert_eq!(
            sent[1].verdict().cause(),
            Some(Cause::FeedNotAdvancing),
            "the premise: only the feed oracle objects to this response"
        );
        assert_eq!(
            sent[2].measurement.energy,
            Some(Kwh(900_000.0)),
            "a response the feed refused adopts nothing (story 2.7 AC5), and \
             with the no-going-back clause blind to it — its value_date is \
             newer — the feed gate is the only thing keeping it out of `last`"
        );
    }

    /// **Story 2.7 AC2, through the pipeline rather than against the function.**
    ///
    /// The unit tests in `core::state_machine` prove the discrimination
    /// discriminates. This proves the memory is actually THREADED — that
    /// `step_once` hands `Policy::step_remembering` the previous reading's
    /// `value_date` and records this one's after judging. Epic 2's recurring
    /// review finding is a property tested one layer above where it lives; this
    /// is the layer where a forgotten wire would hide.
    #[tokio::test]
    async fn a_meter_with_a_wrong_clock_is_told_apart_from_a_stopped_one() {
        // THE WRONG CLOCK: the meter keeps producing (`value_date` advances every
        // tick) but its clock runs 150 s behind the cloud's, so every age is far
        // beyond the 90 s allowance and perfectly stable.
        let behind = reading(Quality::Good, 150_000);
        let (_, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(behind.clone()))
                .then(Ok(later(behind, 1))),
        )
        .await;
        assert_eq!(sent.len(), 2, "every tick publishes a verdict (ADR 0027)");
        assert_eq!(
            sent[0].verdict().cause(),
            Some(Cause::ReadingTooOld),
            "first contact: no previous reading, so nobody can tell yet — the \
             pre-2.7 answer is the honest one"
        );
        assert_eq!(
            sent[1].verdict().cause(),
            Some(Cause::TimestampsDisagree),
            "the meter produced a NEW measurement and the age did not shrink: \
             a wrong clock or a late-ingesting cloud, and `reading-too-old` \
             would send the operator to a meter that never stopped"
        );
        assert_eq!(
            sent[1].published(),
            Quality::Stale,
            "only the cause moves; a reading the clocks disagree about is still \
             unproven"
        );

        // THE STOPPED METER: `value_date` freezes while the cloud's `Date`
        // advances past the allowance. The data is genuinely old and must keep
        // saying so.
        let first = reading(Quality::Good, 950);
        let frozen_meter = reading(Quality::Good, 150_000); // same value_date, later Date
        let (_, sent) =
            drive_sequence(FakeSource::new().then(Ok(first)).then(Ok(frozen_meter))).await;
        assert_eq!(
            sent[1].verdict().cause(),
            Some(Cause::ReadingTooOld),
            "no new measurement since the previous tick: this is a meter that \
             stopped, and calling it a clock fault would send the operator away \
             from it"
        );
    }

    #[tokio::test]
    async fn a_backwards_energy_index_does_not_withhold_the_power_reading() {
        let (_, sent) = drive_sequence(
            FakeSource::new()
                .then(Ok(reading_with_energy(Quality::Good, 950, 4_843.822)))
                .then(Ok(later(reading_with_energy(Quality::Good, 950, 12.0), 1))),
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
            let mut mem = MeterMemory {
                energy_reference: load_energy_reference(&dir, &meter),
                ..MeterMemory::default()
            };
            let (_, _) = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
            let energy = mem
                .energy_reference
                .expect("a good reading is adopted as the reference");
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
        let mut mem = MeterMemory {
            energy_reference: restored,
            ..MeterMemory::default()
        };
        let _ = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
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
        let mut mem = MeterMemory::default();
        let mut state = State::initial();
        for _ in 0..source.remaining() {
            (state, _) = step_once(&ctx, &mut source, state, &mut mem).await;
        }
        drop(tx);
        let mut got = Vec::new();
        while let Some(u) = rx.recv().await {
            got.push(u);
        }
        (state, got)
    }

    /// [#81] — a latched meter stops being asked, and goes on publishing.
    ///
    /// # The two halves, and why one without the other would be wrong
    ///
    /// `Failed` is absorbing (ADR 0009), so no answer can change the verdict: the
    /// call is a request made for nothing, every period, for ever. For the
    /// credential latch it is the hammering story 2.6's own doc warns against —
    /// *"retrying with a credential the other end refused is how a bridge hammers
    /// an API"* — performed by the poll loop.
    ///
    /// But ADR 0027 requires a verdict EVERY cycle for these meters: they are not
    /// gone, the asking is broken, and silence on a Sparkplug wire reads as
    /// "nothing has changed". So the tick is synthesised from the refusal the
    /// state carries — which [#75] made possible three commits ago by putting it
    /// inside `State::Failed`. Stopping the fetch without publishing would trade
    /// this defect for a worse one.
    ///
    /// **Falsification, 2026-08-24:**
    ///
    /// 1. the latched arm deleted — the state [#81] reported: RED, the source is
    ///    asked once more;
    /// 2. `DeviceNotInAccount` allowed into the arm: RED on the control below.
    ///    That control was ADDED because the mutation survived without it — the
    ///    first draft asserted only that a credential latch is not asked, and an
    ///    arm taking every refusal passed it.
    ///
    /// 3. `last.is_some()` dropped from the condition: RED — on
    ///    `a_disabled_meters_alarm_retires_with_it_and_a_re_enable_judges_afresh`,
    ///    an INTEGRATION test, not on this one.
    ///
    /// **That third mutation is how this condition was found, not a guard written
    /// for it.** The first draft skipped the fetch for every latched meter, all
    /// 304 unit tests stayed green, and the integration test went red with `the
    /// first tick publishes: Elapsed`. A meter latched on its first tick has no
    /// last measurement, so skipping the fetch took away the only way it could
    /// acquire one and it fell silent for good.
    ///
    /// A fourth was written into this list before being run — an early `return` in
    /// place of the synthesised tick — and is not claimed: it is not expressible
    /// without restructuring the function, since what follows the tick is the
    /// whole of the judging. The publication half is covered by the assertion on
    /// `published` rather than by a mutation.
    #[tokio::test]
    async fn a_latched_meter_is_not_asked_again_and_still_publishes() {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let beats = Heartbeats::for_meters([MeterId::new("garage")]);
        let heartbeat = beats.of(&MeterId::new("garage")).expect("present");
        let (tx, mut rx) = mpsc::channel(8);
        let meter = MeterId::new("garage");
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };

        // A source that would answer perfectly if it were asked.
        let mut source = FakeSource::new().then(Ok(reading(Quality::Good, 950)));
        // AND A METER THAT HAS ANSWERED BEFORE. Without this the test proves
        // nothing about publication: a meter that has NEVER answered has no last
        // measurement and correctly gets nothing — its DBIRTH already declared it
        // valueless. The first draft omitted it and read the resulting silence as
        // a defect in the change, which it was not.
        let mut mem = MeterMemory {
            last: Some(reading(Quality::Good, 950).value),
            ..MeterMemory::default()
        };
        let (state, _) = step_once(
            &ctx,
            &mut source,
            State::Failed(crate::core::source::Refusal::Credential),
            &mut mem,
        )
        .await;

        assert!(
            source.calls.is_empty(),
            "a latched meter must not be asked: the answer cannot change an \
             absorbing verdict, and asking anyway is the hammering story 2.6 warns \
             about — one call per period per latched meter, until a restart"
        );
        assert_eq!(
            source.remaining(),
            1,
            "and the scripted answer is untouched, which is what says the fetch \
             was skipped rather than consumed"
        );

        drop(tx);
        let mut published = Vec::new();
        while let Some(u) = rx.recv().await {
            published.push(u);
        }
        assert_eq!(
            published.len(),
            1,
            "AND THE CYCLE STILL PUBLISHES: ADR 0027 requires a verdict every cycle \
             for a meter that is latched but not gone. Stopping the fetch AND the \
             publication would trade this defect for silence, which a Sparkplug \
             consumer reads as 'nothing has changed'"
        );
        assert_eq!(
            published[0].verdict().cause(),
            Some(Cause::CredentialRejected),
            "and the verdict names the refusal that latched it, not a generic one"
        );
        assert_eq!(
            state,
            State::Failed(crate::core::source::Refusal::Credential),
            "the latch is unchanged"
        );

        // THE CONTROL: the GONE latch is still asked here. Story 3.5 stops that
        // one at the loop, where the DDEATH ends the publication too (ADR 0034),
        // and swallowing it in `step_once` would hide the path that emits the
        // certificate. Without this assertion the arm could take every refusal
        // and the test above would not notice.
        let (tx2, _rx2) = mpsc::channel(8);
        let ctx2 = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx2,
        };
        let mut gone_source = FakeSource::new().then(Ok(reading(Quality::Good, 950)));
        let mut gone_mem = MeterMemory::default();
        let _ = step_once(
            &ctx2,
            &mut gone_source,
            State::Failed(crate::core::source::Refusal::DeviceNotInAccount),
            &mut gone_mem,
        )
        .await;
        assert_eq!(
            gone_source.calls.len(),
            1,
            "the gone latch is the loop's business, not this function's: it is \
             stopped one level up, and absorbing it here would mean the DDEATH \
             never went out"
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
        let mut mem = MeterMemory::default();
        let (state, _) = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
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

    /// Captures what a trace macro actually wrote, so a log line can be an
    /// assertion rather than a hope.
    ///
    /// **Story 2.6 AC5's second half, and the half that was missing entirely.**
    ///
    /// Every `SourceError` carries a sentence written to tell an operator what to
    /// repair. Until 2026-08-13 not one of them was rendered anywhere: deleting
    /// both `impl Display` and `impl Error` for `SourceError` left the library
    /// compiling with zero errors. Carrying serde's field name up from the client
    /// would have been pointless on its own — it would have arrived nowhere.
    ///
    /// The needles are the FIELD NAME and the meter, never a bare digit: this
    /// subscriber writes a full RFC-3339 timestamp on every line, and the story
    /// 4.6 review found a one-character needle satisfied by the clock.
    ///
    /// FALSIFIED — two mutations, both RUN, both messages copied:
    /// - the `warn!` deleted: RED, *"no line reported the failure to the operator;
    ///   the log was: … INFO … no reading this tick and none ever …"* — and that
    ///   remaining `INFO` is the whole reason `unreadable_line` exists;
    /// - `meter = %meter` dropped from the `warn!`: RED, *"and which meter it was,
    ///   ON THIS LINE …"*, over the line
    ///   `WARN … this meter could not be read error=transient source error:
    ///   response decode failed: missing field \`ActivePower\` at line 2 column 76`.
    #[tokio::test]
    async fn a_payload_the_bridge_could_not_read_names_its_field_to_the_operator() {
        // THROUGH THE SHARED HARNESS ([#94]): a capture built by hand here is one
        // a thread with no subscriber can switch off for the whole process.
        let (sink, _capture_guard) = crate::test_capture::capture_guard(tracing::Level::TRACE);
        let (state, sent) = drive(
            FakeSource::new().then(Err(SourceError::Transient {
                reason: "response decode failed: missing field `ActivePower` at line 2 column 76"
                    .to_string(),
            })),
        )
        .await;
        drop(_capture_guard);

        assert_eq!(state, State::Stale, "a payload anomaly is retried");
        assert!(sent.is_empty(), "there is no reading to carry");

        let line = unreadable_line(&sink.text());
        assert!(
            !line.contains("source fetch timed out"),
            "NOT THIS PROPERTY: the 2 s fetch deadline elapsed before the fake source \
             was polled, so the tick became a Timeout and never carried the decode \
             failure. Observed once on 2026-08-13 during a full workspace run on a \
             loaded machine, and not reproduced in 17 later runs. If this fires, the \
             machine is busy — re-run before believing anything: {line:?}"
        );
        assert!(
            line.contains("ActivePower"),
            "the operator must learn WHICH field the API changed: {line:?}"
        );
        assert!(
            line.contains("garage"),
            "and which meter it was, ON THIS LINE — at four meters a reason without \
             a subject is a reason about nobody: {line:?}"
        );
    }

    /// The warn this story added, isolated from everything else the tick logs.
    ///
    /// **It is isolated because it has to be.** The loop already emits an `INFO`
    /// carrying `meter=garage` and the cause token, so `log.contains("garage")` over
    /// the whole capture passes whether or not this story's line exists at all —
    /// found by running the mutation rather than by reading, and it is the story 4.6
    /// needle problem in a new place.
    fn unreadable_line(log: &str) -> String {
        log.lines()
            .find(|l| l.contains("this meter could not be read"))
            .unwrap_or_else(|| {
                // **IF THIS FIRES, READ [#94] BEFORE READING THE CODE.** The line
                // is emitted on the same tick as the `INFO` below it, so a log
                // holding one and not the other is a capture that lost a line —
                // observed three times, twice on 2026-08-19 during full-workspace
                // runs, where it refused a push.
                //
                // **AND DO NOT READ `SourceUnreachable` IN THAT LOG AS A TIMEOUT.**
                // A first diagnosis on 2026-08-19 did, and was wrong: the state
                // machine maps BOTH `Timeout` and `Transient` to that cause
                // (`state_machine.rs:225`), and this test scripts a `Transient`.
                // The cause line is the NOMINAL path; what is missing is the warn.
                panic!("no line reported the failure to the operator; the log was: {log:?}")
            })
            .to_string()
    }

    /// The same surface for the refusal an operator can actually act on: ADR 0029's
    /// identity message names the repair, and it reached nobody either.
    #[tokio::test]
    async fn a_refusal_reaches_the_operator_with_the_repair_it_names() {
        // THROUGH THE SHARED HARNESS ([#94]): a capture built by hand here is one
        // a thread with no subscriber can switch off for the whole process.
        let (sink, _capture_guard) = crate::test_capture::capture_guard(tracing::Level::TRACE);
        // The refusal matches what `map_error` actually produces for this
        // reason since story 3.5's split — the review caught the fixture
        // staging a pairing (`Configuration` + a 404's message) that no code
        // path can emit any more, the fixture-models-the-impossible class.
        let (state, _) = drive(FakeSource::new().then(Err(SourceError::Fatal {
            refusal: crate::core::source::Refusal::DeviceNotInAccount,
            reason: "smart-me does not know device 9202685".to_string(),
        })))
        .await;
        drop(_capture_guard);

        assert!(matches!(state, State::Failed(_)));
        let line = unreadable_line(&sink.text());
        assert!(
            line.contains("9202685"),
            "a refusal that names no subject sends the operator nowhere: {line:?}"
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
        assert!(matches!(state, State::Failed(_)));
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
        let mut mem = MeterMemory::default();
        let _ = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
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
                PolledMeter {
                    meter: meter.clone(),
                    serial: Serial::new("9202685"),
                },
                source,
                Arc::clone(&clock) as Arc<dyn Clock + Send + Sync>,
                Arc::clone(&handle),
                beats.clone(),
                tx,
                dir.clone(),
                mpsc::channel(4).0,
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
            PolledMeter {
                meter: meter.clone(),
                serial: Serial::new("9202685"),
            },
            source,
            Arc::clone(&clock) as Arc<dyn Clock + Send + Sync>,
            Arc::clone(&handle),
            beats,
            tx,
            dir.clone(),
            mpsc::channel(4).0,
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
                PolledMeter {
                    meter: meter.clone(),
                    serial: Serial::new("30000001"),
                },
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
                mpsc::channel(4).0,
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
        let mut mem = MeterMemory::default();
        let (after_timeout, _) = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
        assert_eq!(after_timeout, State::Stale);

        let (after_good, _) = step_once(&ctx, &mut source, after_timeout, &mut mem).await;
        assert_eq!(after_good, State::Fresh);
        drop(tx);
        let u = rx.recv().await.expect("the good reading was forwarded");
        assert_eq!(u.published(), Quality::Good);
    }

    // ================================================================
    // Story 4.11 — the traced drop, exhaustively (FR22, AR7)
    // ================================================================

    /// **AC5 — the index and the list are ONE list.**
    ///
    /// `DropReason::index` is written as an exhaustive `match` and
    /// `DropReason::ALL` as an array, and nothing but this test stops the two
    /// drifting. They must not: `ALL` is what `FleetState::dropped` walks and
    /// `index` is where the counter was written, so a disagreement reports one
    /// reason's losses under another's name — a surface lying quietly, which is
    /// worse than one that says nothing.
    ///
    /// FALSIFIED 2026-08-18 — mutation RUN, output copied: swapping the
    /// `BeforeBirth` and `UndeclaredDevice` arms of `index` goes red with
    /// `assertion `left == right` failed: DropReason::ALL[3] is BeforeBirth but
    /// it indexes cell 4 […] left: 4, right: 3`.
    #[test]
    fn the_index_and_the_list_agree() {
        for (cell, reason) in DropReason::ALL.into_iter().enumerate() {
            assert_eq!(
                reason.index(),
                cell,
                "DropReason::ALL[{cell}] is {reason:?} but it indexes cell {}; \
                 the list and the index must not drift, or one reason's losses \
                 are counted under another's name",
                reason.index()
            );
        }
        assert_eq!(DropReason::COUNT, DropReason::ALL.len());

        // And the slugs are distinct, because they are the operator's whole
        // vocabulary: two reasons sharing a slug is two faults reading as one.
        let mut slugs: Vec<_> = DropReason::ALL.iter().map(|r| r.as_str()).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(
            slugs.len(),
            DropReason::COUNT,
            "every reason needs its own slug: {slugs:?}"
        );
    }

    /// **Story 6.3 AC2 — a republished reading is not a changed one.**
    ///
    /// ADR 0027 requires a verdict every cycle, so most publications repeat the
    /// previous value. Without two distinct fields a frozen meter and a quiet one
    /// look identical: both show a recent publication. **Change is measured on the
    /// SOURCE's acquisition time**, not on the bridge's clock — a source
    /// re-answering with the same `ValueDate` has produced nothing new, whatever
    /// the bridge does with it.
    ///
    /// FALSIFIED 2026-08-19 — mutation RUN, output copied: dropping the
    /// `value_date != entry.source_value_date` guard, so every publication counts
    /// as a change, goes red with `a REPUBLISHED reading must not move
    /// last_changed_at … left: Some(UtcMillis(3000)), right: Some(UtcMillis(1000))`.
    #[test]
    fn a_republished_reading_moves_the_publication_instant_and_not_the_change() {
        let meter = MeterId::new("garage");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let first = UtcMillis(1_000);
        let second = UtcMillis(3_000);
        let acquired = UtcMillis(1_784_984_792_050);

        beats.record_at(
            &meter,
            State::Fresh,
            Verdict::good(),
            Some(Publication {
                at: first,
                threshold_ms: 90_000,
                value_date: Some(acquired),
                power_kw: Some(0.018),
                energy_kwh: Some(4_843.0),
            }),
        );
        // The SAME reading, republished a cycle later — ADR 0027's normal case.
        beats.record_at(
            &meter,
            State::Stale,
            Verdict::stale(Cause::NotRevalidated),
            Some(Publication {
                at: second,
                threshold_ms: 90_000,
                value_date: Some(acquired),
                power_kw: Some(0.018),
                energy_kwh: Some(4_843.0),
            }),
        );

        let fleet = beats.snapshot();
        let state = &fleet.meters[0];
        assert_eq!(
            state.last_published_at,
            Some(second),
            "the publication instant follows every cycle, because every cycle publishes"
        );
        assert_eq!(
            state.last_changed_at,
            Some(first),
            "a REPUBLISHED reading must not move last_changed_at: the source answered \
             with the same acquisition time, so nothing new was measured. This is the \
             field that tells a frozen meter from a quiet one"
        );
        assert_eq!(
            state.staleness_threshold_ms,
            Some(90_000),
            "and the verdict carries the threshold it was reached against, or a screen \
             reading it against today's threshold reads a different judgement"
        );
    }

    /// **Story 6.3 AC3 — a lost reading is the one thing that accuses the bridge.**
    ///
    /// No `Cause` yields `Culprit::Bridge` — `oracle::culprit_tests` pins that as a
    /// property. This is the other half: the path by which the accusation an
    /// operator most needs to see actually reaches a screen.
    ///
    /// FALSIFIED 2026-08-19 — mutation RUN: removing the `entry.culprit` write from
    /// `dropped` goes red with `a reading the BRIDGE lost must say so … left: None`.
    #[test]
    fn a_reading_the_bridge_lost_names_the_bridge() {
        let meter = MeterId::new("garage");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let pulse = beats.of(&meter).expect("served");

        // A publication first, so the culprit cannot be `Bridge` by never having
        // been written — the shape this repository keeps finding in its own tests.
        beats.record_at(
            &meter,
            State::Fresh,
            Verdict::good(),
            Some(Publication {
                at: UtcMillis(1_000),
                threshold_ms: 90_000,
                value_date: Some(UtcMillis(1_784_984_792_050)),
                power_kw: Some(0.018),
                energy_kwh: Some(4_843.0),
            }),
        );
        assert_eq!(
            beats.snapshot().meters[0].culprit,
            None,
            "nothing is wrong yet, and `None` is how that is said"
        );

        pulse.dropped(DropReason::Unpublishable);

        assert_eq!(
            beats.snapshot().meters[0].culprit,
            Some(crate::core::oracle::Culprit::Bridge),
            "a reading the BRIDGE lost must say so: `Unpublishable` is this process \
             failing to build a topic, not the world failing to answer"
        );
        assert_eq!(
            DropReason::TransportQueueFull.culprit(),
            crate::core::oracle::Culprit::World,
            "but a full transport queue is the BROKER not draining — sending an \
             operator to this process would send them to the wrong machine"
        );
    }

    /// **AC1 + AC3 — a full outbox costs the READING, never the LOOP.**
    ///
    /// This is the story's sharpest property and it is not about bookkeeping.
    /// Until 2026-08-18 the hand-over was `outbox.send(update).await`, and the
    /// driver does NOT drain that channel while it sleeps between reconnect
    /// attempts. So a long enough broker outage parked every poll task inside
    /// `send`, `last_tick` stopped advancing, and `/healthz` answered
    /// `wedged: true` — which Epic 7 wires to a container restart. The bridge
    /// would have been restarted, killing the Sparkplug session for every meter,
    /// because a broker somewhere else was down.
    ///
    /// The timeout is the assertion. A blocking send does not fail this test by
    /// returning something wrong; it fails by never returning at all.
    ///
    /// FALSIFIED 2026-08-18 — mutation RUN, output copied: restoring
    /// `outbox.send(update).await` goes red with `a full outbox must not park the
    /// poll loop: the driver does not drain this channel while it reconnects, and
    /// a parked loop reads as `wedged` to a health check that restarts the
    /// container: Elapsed(())`.
    #[tokio::test]
    async fn a_full_outbox_costs_the_reading_and_not_the_loop() {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let mut source = FakeSource::new().then(Ok(reading(Quality::Good, 950)));
        // ONE slot, filled before the tick and never drained — `_rx` is held so
        // the channel is FULL rather than CLOSED, which is the other reason and
        // a different arm.
        let (tx, _rx) = mpsc::channel(1);
        let meter = MeterId::new("garage");
        tx.try_send(MeterUpdate::uniform(
            meter.clone(),
            reading(Quality::Good, 950).value,
            Verdict::good(),
        ))
        .expect("the one slot is free before the tick");

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
        let mut mem = MeterMemory::default();

        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            step_once(&ctx, &mut source, State::initial(), &mut mem),
        )
        .await
        .expect(
            "a full outbox must not park the poll loop: the driver does not drain \
             this channel while it reconnects, and a parked loop reads as `wedged` \
             to a health check that restarts the container",
        );

        // And the loss is on the record, per meter and per reason.
        let fleet = beats.snapshot();
        let lost = fleet.dropped();
        assert_eq!(lost.len(), 1, "one reading was lost, once: {lost:?}");
        assert_eq!(lost[0].meter, &meter);
        assert_eq!(lost[0].reason, DropReason::OutboxFull);
        assert_eq!(lost[0].count, 1);
    }

    /// **AC1 — a dead transport task is its own reason, not a full queue.**
    ///
    /// `MqttTaskGone` had NO test until the 2026-08-18 review, while Task 3's
    /// subtask was ticked. The two arms of `TrySendError` are the only place the
    /// distinction is drawn, and they are one word apart: filing a closed channel
    /// under `outbox-full` would tell an operator to look at the broker when the
    /// bridge's own transport task is dead — the one reason in the six that a
    /// container restart WOULD clear.
    ///
    /// FALSIFIED 2026-08-18 — mutation RUN, output copied: mapping
    /// `TrySendError::Closed` to `DropReason::OutboxFull` goes red with `a closed
    /// channel is a DEAD TRANSPORT TASK … left: OutboxFull, right: MqttTaskGone`.
    #[tokio::test]
    async fn a_closed_outbox_is_a_dead_transport_task_and_says_so() {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let mut source = FakeSource::new().then(Ok(reading(Quality::Good, 950)));
        let (tx, rx) = mpsc::channel(8);
        // The receiver is DROPPED: room in the channel, nobody at the other end.
        // That is `Closed`, and it must not read as `Full`.
        drop(rx);
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
        let mut mem = MeterMemory::default();

        let _ = step_once(&ctx, &mut source, State::initial(), &mut mem).await;

        let fleet = beats.snapshot();
        let lost = fleet.dropped();
        assert_eq!(lost.len(), 1, "one reading, one reason: {lost:?}");
        assert_eq!(
            lost[0].reason,
            DropReason::MqttTaskGone,
            "a closed channel is a DEAD TRANSPORT TASK, not a busy one — and it is \
             the one reason of the six that a container restart would clear, so \
             filing it as `outbox-full` sends the operator to the broker"
        );
    }

    /// **AC1 — the WARN names the meter, the reason and WHEN THE READING WAS TRUE.**
    ///
    /// `value_date`, not `now`. A gap an operator has to line up against the
    /// source is identified by the instant the reading described, and the
    /// publication instant is the one number that cannot do it.
    ///
    /// FALSIFIED 2026-08-18 — mutation RUN, output copied: replacing
    /// `value_date = update.measurement.value_date.0` with
    /// `value_date = clock.wall().0` goes red with `the line must carry the
    /// READING's own timestamp […] value_date=1784984793000`, the publication
    /// instant standing where `1784984700000` belongs.
    #[tokio::test]
    async fn a_lost_reading_is_traced_with_its_own_timestamp() {
        // THROUGH THE SHARED HARNESS ([#94]): a capture built by hand here is one
        // a thread with no subscriber can switch off for the whole process.
        let (sink, _capture_guard) = crate::test_capture::capture_guard(tracing::Level::TRACE);
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let mut source = FakeSource::new().then(Ok(reading(Quality::Good, 950)));
        let (tx, _rx) = mpsc::channel(1);
        let meter = MeterId::new("garage");
        tx.try_send(MeterUpdate::uniform(
            meter.clone(),
            reading(Quality::Good, 950).value,
            Verdict::good(),
        ))
        .expect("the one slot is free before the tick");
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
        let mut mem = MeterMemory::default();

        let _ = step_once(&ctx, &mut source, State::initial(), &mut mem).await;
        drop(_capture_guard);
        let logged = sink.text();

        assert!(
            logged.contains("reason=\"outbox-full\""),
            "the line must name WHICH of the six ways the reading was lost: {logged:?}"
        );
        assert!(
            logged.contains(&format!("value_date={BASE}")),
            "the line must carry the READING's own timestamp, not the instant it \
             was dropped — the publication instant cannot be lined up against the \
             source: {logged:?}"
        );
        assert!(
            logged.contains("meter=garage"),
            "an operator with four meters must be told which one lost a reading: \
             {logged:?}"
        );
    }

    /// **AC1 — a drop is a DELIVERY fact and must not touch the VERDICT.**
    ///
    /// The oracle layer judges the reading before it reaches the outbox. If a
    /// full channel could change what the reading was judged to be, the wire
    /// would depend on the transport — and the next tick's republished value,
    /// which is read from `MeterMemory::last`, would carry a quality that
    /// describes a queue rather than a measurement.
    ///
    /// The two runs are identical in every input, so the assertion is exact
    /// equality rather than a property.
    ///
    /// FALSIFIED 2026-08-18 — mutation RUN, output copied: letting the drop path
    /// choose its own outcome (`return (State::Stale, published.meter());` after
    /// the WARN) goes red with `a drop is a delivery fact; letting it move the
    /// verdict makes the wire depend on the transport […] left: (Stale, Verdict {
    /// quality: Good, cause: None }), right: (Fresh, Verdict { quality: Good,
    /// cause: None })`.
    #[tokio::test]
    async fn a_drop_does_not_change_what_the_reading_was_judged_to_be() {
        let meter = MeterId::new("garage");
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let beats = Heartbeats::for_meters([meter.clone()]);
        let heartbeat = beats.of(&meter).expect("served");

        // Delivered: a channel with room.
        let (open, mut rx) = mpsc::channel(8);
        let mut source = FakeSource::new().then(Ok(reading(Quality::Good, 950)));
        let mut mem = MeterMemory::default();
        let delivered = step_once(
            &Context {
                meter: &meter,
                clock: &clock,
                policy: policy(),
                config: config(),
                heartbeat: &heartbeat,
                outbox: &open,
            },
            &mut source,
            State::initial(),
            &mut mem,
        )
        .await;
        assert!(rx.try_recv().is_ok(), "the premise: this one WAS delivered");

        // Dropped: the same reading, the same instant, a channel with none.
        let (full, _rx) = mpsc::channel(1);
        full.try_send(MeterUpdate::uniform(
            meter.clone(),
            reading(Quality::Good, 950).value,
            Verdict::good(),
        ))
        .expect("the one slot is free before the tick");
        let mut source = FakeSource::new().then(Ok(reading(Quality::Good, 950)));
        let mut mem = MeterMemory::default();
        let dropped = step_once(
            &Context {
                meter: &meter,
                clock: &clock,
                policy: policy(),
                config: config(),
                heartbeat: &heartbeat,
                outbox: &full,
            },
            &mut source,
            State::initial(),
            &mut mem,
        )
        .await;

        assert_eq!(
            dropped, delivered,
            "a drop is a delivery fact; letting it move the verdict makes the wire \
             depend on the transport, and the republished value would then carry a \
             quality describing a queue rather than a measurement"
        );
    }

    /// **AC2 — a thousand losses touch one cell and advance the generation.**
    ///
    /// # What this test does NOT prove, and why it is named accordingly
    ///
    /// It was called `a_thousand_losses_cost_what_one_costs` and asserted
    /// `after.dropped.len() == before.dropped.len()`, which is `6 == 6` for every
    /// value of every program — `dropped` is `[u64; DropReason::COUNT]` and
    /// `.len()` on a fixed-size array is a compile-time constant. **No possible
    /// implementation could turn it red**, and it was scored as the discharge of
    /// AC2. Found by the 2026-08-18 review; it is the hollow-assertion class this
    /// repository has now met four times.
    ///
    /// **The cardinality argument is carried by the TYPE, not by a test.** The
    /// `const` block below is the real pin: change `dropped` to a `HashMap` or a
    /// `Vec` — the shapes that would make the drop path allocate — and the crate
    /// stops compiling. That is a stronger guarantee than any assertion, and it is
    /// why nothing here tries to measure it at run time.
    ///
    /// What this test pins is what a test can pin: one loss touches ONE cell,
    /// leaves the other five alone, and advances `generation` exactly once.
    ///
    /// FALSIFIED 2026-08-18 — mutation RUN, output copied: dropping
    /// `fleet.generation += 1` from `MeterPulse::dropped` goes red with `every
    /// write advances the generation … left: 0, right: 1000`. A second mutation,
    /// indexing `[0]` instead of `[reason.index()]`, goes red on the neighbouring
    /// cell with `the losses must land in the cell of the reason they were filed
    /// under … left: 0, right: 1000`.
    #[test]
    fn a_thousand_losses_touch_one_cell_and_advance_the_generation() {
        // THE CARDINALITY PIN, and it is a compile-time one. A `dropped` that
        // stopped being a fixed-size array — the only way the drop path could
        // begin allocating — would not compile past this line.
        const _: () = {
            let _: fn(&MeterState) -> &[u64; DropReason::COUNT] = |m| &m.dropped;
        };

        let meter = MeterId::new("cellar");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let pulse = beats.of(&meter).expect("served");
        let before = beats.snapshot();

        for _ in 0..1000 {
            pulse.dropped(DropReason::MqttTaskGone);
        }
        let after = beats.snapshot();

        assert_eq!(
            after.generation - before.generation,
            1000,
            "every write advances the generation, in the same modification, or the \
             snapshot invariant AR6 rests on is untestable"
        );
        assert_eq!(
            after.meters[0].dropped[DropReason::MqttTaskGone.index()],
            1000,
            "the losses must land in the cell of the reason they were filed under"
        );
        for reason in DropReason::ALL {
            if reason == DropReason::MqttTaskGone {
                continue;
            }
            assert_eq!(
                after.meters[0].dropped[reason.index()],
                0,
                "a loss must touch its OWN cell and no other, or one reason's \
                 losses are reported under another's name ({reason:?})"
            );
        }

        let lost = after.dropped();
        assert_eq!(lost.len(), 1, "one reason, one row: {lost:?}");
        assert_eq!(
            (lost[0].reason, lost[0].count),
            (DropReason::MqttTaskGone, 1000)
        );
    }

    /// **AC2 — the count saturates rather than wrapping.**
    ///
    /// A counter that returns to zero reports the smallest figure at the moment
    /// the fault is largest. Saturating is the only honest end of the range.
    ///
    /// # The positive control is half the test
    ///
    /// It started at `u64::MAX` and incremented once, so a `Heartbeats::dropped`
    /// that silently did NOTHING left the cell at `MAX` and the test was green —
    /// it could not tell saturation from a no-op, which is the neighbouring
    /// failure mode. Found by the 2026-08-18 review. Starting at `MAX - 1` and
    /// incrementing twice fixes it: the first increment proves the path runs, the
    /// second proves it saturates.
    ///
    /// FALSIFIED 2026-08-18 — two mutations RUN, output copied. `*cell += 1` in
    /// place of `saturating_add` goes red in debug with `attempt to add with
    /// overflow`. Making `Heartbeats::dropped` a no-op (an early `return`) goes
    /// red on the FIRST assertion with `the increment path must actually run …
    /// left: 18446744073709551614, right: 18446744073709551615`.
    #[test]
    fn the_count_saturates_and_never_returns_to_zero() {
        let meter = MeterId::new("cellar");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let cell = DropReason::OutboxFull.index();
        beats.0.send_modify(|fleet| {
            fleet.meters[0].dropped[cell] = u64::MAX - 1;
        });

        // POSITIVE CONTROL: the path runs at all.
        beats.dropped(&meter, DropReason::OutboxFull);
        assert_eq!(
            beats.snapshot().meters[0].dropped[cell],
            u64::MAX,
            "the increment path must actually run, or the saturation assertion \
             below cannot tell saturation from a counter that never moved"
        );

        // AND NOW the property.
        beats.dropped(&meter, DropReason::OutboxFull);
        assert_eq!(
            beats.snapshot().meters[0].dropped[cell],
            u64::MAX,
            "a count that wraps to 0 reports the smallest figure exactly when the \
             fault is largest"
        );
    }

    /// **AC4 — a fleet that has lost nothing renders as nothing.**
    ///
    /// Six zero rows per meter is noise an operator learns to scroll past, and
    /// the rule belongs here rather than at each surface — there are already two,
    /// and a third would not know about a rule applied at the caller.
    ///
    /// FALSIFIED 2026-08-18 — mutation RUN, output copied: removing the
    /// `.filter(|(_, _, count)| *count > 0)` from `FleetState::dropped` goes red
    /// at `poll_publish.rs:4111` with `a clean fleet reports an empty list, not
    /// six zeros per meter`.
    #[test]
    fn a_clean_fleet_reports_no_losses_at_all() {
        let beats = Heartbeats::for_meters([MeterId::new("a"), MeterId::new("b")]);
        assert!(
            beats.snapshot().dropped().is_empty(),
            "a clean fleet reports an empty list, not six zeros per meter"
        );

        // And one loss on one meter surfaces exactly one row.
        beats.dropped(&MeterId::new("b"), DropReason::BeforeBirth);
        let fleet = beats.snapshot();
        let lost = fleet.dropped();
        assert_eq!(lost.len(), 1, "{lost:?}");
        assert_eq!(lost[0].meter, &MeterId::new("b"));
        assert_eq!(lost[0].reason, DropReason::BeforeBirth);
    }
}
