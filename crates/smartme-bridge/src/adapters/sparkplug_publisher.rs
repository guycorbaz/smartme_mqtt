//! `SparkplugPublisher` — the ONLY place a `Measurement` becomes a Sparkplug
//! metric (Story 1.9), enforced by `tests/arch_purity.rs`.
//!
//! Three rules govern everything here:
//!
//! 1. **The contract is versioned, on the wire.** Topic grammar and metric names
//!    are an external contract with the SCADA host: changing either orphans the
//!    tag history already recorded on the other side. So any change bumps
//!    [`CONTRACT_VERSION`], which is PUBLISHED in the node BIRTH — a consumer can
//!    see the contract change rather than having to notice it.
//! 2. **The first message is honest.** A cold-start device BIRTH declares the tag
//!    set with NO value and quality `Stale`: before the first successful fetch
//!    the bridge knows nothing, and a birth carrying `0.0` marked `Good` would be
//!    a lie the consumer could not detect.
//! 3. **Nothing is emitted until everything is valid.** Every topic is built and
//!    validated BEFORE the first message reaches the sink, so a bad identifier
//!    can never leave a node born with a half-declared tag set.

use std::collections::HashMap;

use sparkplug_b::{
    BdSeq, DataType, EdgeNode, LiveSession, MessageType, Metric, MetricValue, NodeSession, Quality,
    TopicError, encode,
};

use crate::core::channel::MeterUpdate;
use crate::core::oracle::{Cause, Measured, Verdict, Verdicts};
use crate::domain::{Measurement, Serial, UtcMillis};

/// Version of the topic/metric contract with the SCADA host. Bump on ANY change
/// to the topic grammar, to a metric name or unit, or to the meaning of a
/// published quality code.
///
/// # Additive and breaking changes both bump, and the number does not say which
///
/// The rule above admits no exception, and that is deliberate: a consumer must
/// be able to see that the contract moved. But the two kinds of move are not
/// the same kind of problem, and the version alone cannot tell them apart, so
/// each entry below says which it was.
///
/// - An **additive** change adds a tag. Nothing a consumer already holds becomes
///   wrong; a browse tree gains a node and history keeps its meaning. Version 3
///   is one of these.
/// - A **breaking** change alters what something already published MEANS.
///   Version 2 is one of these: a quality code a consumer had recorded as good
///   was, from that release on, not good. Historised values from either side of
///   the boundary cannot be compared without knowing which side they are on.
///
/// This distinction is written down because it had to be re-derived at Story
/// 4.7. The Tier-3 runbook's run table is indexed by this constant
/// (`docs/ignition-contract-runbook.md`), so two runs sharing a version number
/// attest to the same tag set — which is the property the bump protects, and it
/// holds for additive changes too.
///
/// - **3** — *additive*. The NBIRTH now declares
///   [`METRIC_NODE_CONTROL_REBIRTH`] — boolean, `false`. A consumer sees a new
///   tag in its browse tree, and — the point of the exercise — a host gains a
///   declared endpoint it can use to request a rebirth (Story 4.7, FR19).
///   Nothing was removed or renamed.
/// - **2** — *breaking*. Quality codes changed from the specification's
///   `0`/`192`/`500` to Ignition's `QualityCode` encoding. A Tier-3 run showed
///   Ignition reading `500` as `Good(500)` and `0` as `Good_Unspecified`, so a
///   v1 consumer was told an unusable value was trustworthy. A deliberate
///   deviation from the specification — see [`ignition_quality_code`] and ADR
///   0012.
/// - **1** — initial contract, with the specification's quality codes.
///
/// # ADR 0044 bumps it to 12, and it REPLACES v11 rather than extending it
///
/// **The cause travels as a metric — `Cause/Power` and `Cause/Energy` — and the
/// `Cause` PROPERTY is gone.** v11 declared that property in every BIRTH so a
/// host would materialise it, which it did; but on 2026-08-22 the evening Tier-3
/// session measured the other half, with the wire read at the same instant as the
/// screen: **a metric property is written by a BIRTH and by nothing else.** A
/// DDATA carrying a new value for it is ignored, while the same message's quality
/// update is applied.
///
/// So v11 did not merely fail to help — it published a falsehood that never
/// expires: `no-reading-yet` beside a healthy meter, for as long as the session
/// lives. Under v10 the operator saw nothing, which is uninformative; under v11
/// they saw something untrue.
///
/// **Breaking, on both criteria at once**: two names appear in the tag set (v4's
/// criterion) and one disappears from it. A consumer that learned to read the
/// property under v11 finds nothing there under v12, which is exactly the silent
/// breakage this constant exists to make visible.
///
/// # ADR 0043 bumped it to 11, and it is SUPERSEDED — kept because the shape of
/// # the mistake is the lesson
///
/// **Every metric now carries the `Cause` property, including a good one**, with
/// the explicit value `no-cause` ([`CAUSE_NONE`]); and the cold-start BIRTH
/// declares `no-reading-yet`, a cause that did not exist under v10.
///
/// The reason is a host behaviour rather than a change of mind: Ignition
/// materialises a metric property **only when a BIRTH declares it** ([#107],
/// measured twice against a live host). A property first appearing in DDATA is
/// ignored, so contract v4's whole operator-facing purpose — see WHY a value is
/// not trustworthy — never crossed. Declaring it at BIRTH forces publishing it
/// on every DDATA too, because a host holds the last value of a property it was
/// sent: omitting it on recovery would leave a stale cause beside a good value.
///
/// **Breaking by the rule below, on both counts.** The tag set changes — a
/// consumer browsing a `Good` metric now sees a property that did not exist
/// there under v10 — and the cause vocabulary grows by one, which is what
/// bumped v5. Two runs sharing a version number attest to the same tag set, and
/// that promise would be false across this boundary.
///
/// # ADR 0042 does NOT bump this either, and the reasoning follows Story 5.2's
///
/// From 2026-08-22 a bridge with no persisted state numbers its first session
/// **0** instead of 1 ([#100]) — the *start at zero* half of
/// `tck-id-topics-nbirth-bdseq-increment`, which this repository had not
/// honoured. It looks like a wire change, and it is one.
///
/// It does not move this number, by the rule below. No metric name, unit,
/// datatype or quality code changes, and `bdSeq` is present in NBIRTH and NDEATH
/// exactly as before: **the tag set is untouched**, which is the property this
/// constant exists to protect. What changes is the VALUE one metric carries, in
/// one session, on a node that has never connected — and [#100] records what
/// that costs a consumer, which is nothing: a DEATH is paired to a BIRTH by
/// matching `bdSeq` values, and that works from any starting number.
///
/// An existing deployment sees no change at all: its state file is present and
/// readable, so it takes the path it always took.
///
/// # Story 5.2 does NOT bump this, and the reasoning is recorded because the
/// # question will be asked again
///
/// The bridge began emitting **DDEATH** on 2026-08-04 (AC4: disabling a meter
/// buries its device). A consumer therefore now receives a message type it had
/// never received from this node before, which looks like it should move the
/// number.
///
/// It does not, on the rule stated above: the topic grammar is unchanged
/// (DDEATH has always been part of the Sparkplug namespace), no metric name or
/// unit moved, and no quality code changed meaning. More to the point, the
/// property this constant exists to protect is the one the Tier-3 runbook
/// indexes by — *two runs sharing a version number attest to the same tag set* —
/// and **the tag set is untouched**. What changed is the message repertoire, not
/// the contract about tags.
///
/// If that ever stops being the property being protected, this decision should
/// be re-weighed rather than assumed to still hold.
///
/// # Story 2.1 DOES bump it, by that same rule
///
/// Every non-good metric now carries a `Cause` property naming which oracle
/// refused (Story 2.1). Unlike the DDEATH above, this **is** a change to the tag
/// set: a consumer browsing a degraded metric sees a property that did not exist
/// under v3, and the Tier-3 runbook's promise — *two runs sharing a version
/// number attest to the same tag set* — would be false across the boundary.
///
/// What the number now stands for is pinned by `tests/contract_golden.rs`, which
/// fails if any of it moves without this constant moving too (AR16).
///
/// # Story 2.3 bumps it to 6, and it is BREAKING
///
/// Until this story one verdict belonged to the READING and was stamped on both
/// metrics, so an oracle that judged only the energy index published `Power` as
/// null, labelled with the energy's cause. Verdicts are now per metric: the same
/// physical situation that used to yield `Power = null` now yields a real value
/// with quality `Good` and no cause at all.
///
/// **Breaking rather than additive**, by the criterion stated above: nothing is
/// renamed and no tag is added, but a consumer that recorded `Power = null`
/// whenever the energy index was refused will record a genuine value for the
/// identical situation from this version on. Historised values from either side
/// of the boundary cannot be compared without knowing which side they are on,
/// which is exactly what makes version 2 breaking too.
///
/// The change is a correction — the nulls were a fault reported on a metric the
/// bridge had no complaint about — but a correction that alters what a stored
/// point MEANS is still breaking. Calling it additive because the new behaviour
/// is better is how a consumer gets surprised.
///
/// # Story 2.2 bumps it to 5, and the guard is what said so
///
/// The energy-monotonicity oracle added `counter-went-backwards` to the cause
/// vocabulary, which is part of what a consumer reads off a degraded metric.
/// `contract_golden` failed on the change before anything else did — *"the cause
/// vocabulary changed size (11 live, 10 in the v4 golden) without
/// CONTRACT_VERSION moving"* — which is the first time that test caught a real
/// change rather than a mutation written to try it.
pub const CONTRACT_VERSION: i64 = 12;

/// The quality code this bridge publishes for `quality`.
///
/// **A deliberate deviation from Sparkplug B**, taken with eyes open.
///
/// The specification admits exactly three quality codes — `0`, `192`, `500`
/// (`tck-id-payloads-propertyset-quality-value-value`). Ignition does not
/// classify the property by that enumeration. It reads the raw integer as its
/// own `QualityCode`, in which the *level* lives in the top bits: `500` comes
/// back as `Good(500)` and `0` is `Good_Unspecified`. Measured on Ignition
/// 8.3.7 — see `quality_code_probe`, and ADR 0012.
///
/// So against this consumer the conformant codes are worse than useless: two of
/// the three report an unusable value as trustworthy, which is the one failure
/// this project exists to prevent. Between conforming and not lying, we do not
/// lie — and we say so here rather than bending the generic crate, which stays
/// specification-correct for everyone else.
///
/// `Stale` deliberately reuses `Bad_Stale`, the code Ignition itself raises
/// when a node's DEATH marks its tags stale: transport-level and app-level
/// staleness then present identically, which is what the two-mechanism design
/// promises — one visible outcome, whichever mechanism noticed.
pub const fn ignition_quality_code(quality: Quality) -> u32 {
    /// Ignition's `Bad` quality level, carried in the top bits of the code.
    const BAD_LEVEL: u32 = 0x8000_0000;
    match quality {
        // 192 is `Good` in both encodings — the only code they agree on.
        Quality::Good => 192,
        // `Bad_Stale`, subcode 516 → -2147483132 read as a signed 32-bit int.
        Quality::Stale => BAD_LEVEL | 516,
        // `Bad`, subcode 512 → -2147483136.
        Quality::Bad => BAD_LEVEL | 512,
    }
}

/// Metric under which [`CONTRACT_VERSION`] is published in the node BIRTH.
pub const METRIC_CONTRACT_VERSION: &str = "Contract/Version";
/// The command endpoint every conformant NBIRTH must declare.
///
/// The name is fixed by the specification and is not ours to choose: a host
/// addresses it by this exact string. Five MUST clauses in three chapters
/// require it — `tck-id-topics-nbirth-rebirth-metric`,
/// `tck-id-payloads-nbirth-rebirth-req`, and
/// `tck-id-operational-behavior-data-commands-rebirth-name`, `-datatype`,
/// `-value`.
pub const METRIC_NODE_CONTROL_REBIRTH: &str = "Node Control/Rebirth";
/// Metric name for instantaneous power.
pub const METRIC_POWER: &str = "Power";
/// Engineering unit published with [`METRIC_POWER`].
pub const UNIT_POWER: &str = "kW";
/// Metric name for the cumulative energy counter.
pub const METRIC_ENERGY: &str = "Energy";

/// The property key under which a non-good verdict names WHY (Story 2.1).
///
/// **Not `Quality`, and the norm is what decides that.**
/// `tck-id-payloads-propertyset-quality-value-value`
/// (`Sparkplug_6_Payloads.adoc:634-636`) restricts the `Quality` property to the
/// values `0`, `192` or `500`. This bridge already deviates there on purpose
/// (ADR 0012: the conformant codes display as *good* on Ignition, which is the
/// exact lie the project exists to prevent), and encoding a cause as a fourth
/// value would deepen a deviation accepted only because the alternative was a
/// silent lie. A separate key costs nothing: a `PropertySet` constrains only that
/// keys and values are equal in number.
///
/// Present ONLY on a non-good metric. A cause beside a good value is noise a
/// consumer would learn to ignore, and then miss the day it meant something.
/// The metrics carrying the cause, one per measurement, from contract v12.
///
/// **A `/` makes a FOLDER in Ignition** — established by `Contract/Version` and
/// `Node Control/Rebirth` — so these two become a `Cause` folder holding two
/// string tags. `Power/Cause` would have made `Power` a folder, and `Power` is
/// already a tag: the tree cannot hold both.
///
/// **They replace the `Cause` PROPERTY, which could not work and was not merely
/// inelegant.** Measured on 2026-08-22, on a virgin group, with the wire read at
/// the same instant as the screen: a metric property is written by a BIRTH and by
/// nothing else. A DDATA carrying a new value for a declared property is ignored
/// — while the same message's quality update is applied. So the property stood
/// frozen at its birth value, which under v11 meant `no-reading-yet` beside a
/// healthy meter, for ever. A metric's value is precisely what a DDATA exists to
/// change (ADR 0044, superseding ADR 0043).
pub const METRIC_CAUSE_POWER: &str = "Cause/Power";
/// See [`METRIC_CAUSE_POWER`].
pub const METRIC_CAUSE_ENERGY: &str = "Cause/Energy";

/// The value [`METRIC_PROPERTY_CAUSE`] carries when a metric is `Good` — the
/// explicit spelling of *no cause*.
///
/// **It exists because a property that stops being published does not stop being
/// displayed.** Ignition materialises a metric property only when a BIRTH
/// declares it ([#107], measured on 2026-08-21 and again on a virgin group on
/// 2026-08-22), so from contract v11 the property is declared at BIRTH — and
/// once a host holds a tag property, it holds the LAST value it was sent.
/// Omitting the property on the tick a metric recovers would leave
/// `reading-too-old` standing beside a `Good` value, indefinitely. That is worse
/// than the silence it replaces: silence is uninformative, a stale cause is
/// false.
///
/// It is deliberately NOT a [`Cause`](crate::core::oracle::Cause). *"A good
/// metric carries no cause"* still holds in the domain — `Verdict::cause()`
/// remains an `Option` and the vocabulary names only reasons. What pays for the
/// host's behaviour is the wire, at this one boundary, which is where a
/// host-shaped concession belongs.
pub const CAUSE_NONE: &str = "no-cause";
/// Engineering unit published with [`METRIC_ENERGY`].
pub const UNIT_ENERGY: &str = "kWh";

/// One message ready for the transport: where it goes and what it carries.
///
/// The publisher produces these; it never touches a socket. That is the egress
/// seam — a test collects them, the mqtt task publishes them.
#[derive(Debug, Clone, PartialEq)]
pub struct Outbound {
    /// Fully-qualified Sparkplug topic.
    pub topic: String,
    /// Encoded protobuf payload.
    pub payload: Vec<u8>,
    /// What kind of message this is, so the transport can apply the right
    /// delivery semantics without re-parsing the topic.
    pub message: MessageType,
}

/// The injectable egress seam.
pub trait Sink {
    /// Accepts one outbound message.
    fn emit(&mut self, message: Outbound);
}

/// Collects messages instead of publishing them — the test double behind the
/// sink seam.
#[derive(Debug, Default)]
pub struct RecordingSink {
    /// Everything emitted, in order.
    pub emitted: Vec<Outbound>,
}

impl Sink for RecordingSink {
    fn emit(&mut self, message: Outbound) {
        self.emitted.push(message);
    }
}

/// What happened to a reading handed to [`SparkplugPublisher::publish`].
///
/// A drop is REPORTED, never silent: the architecture requires a per-device
/// traced drop, and a caller cannot trace what it cannot observe.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum Published {
    /// The reading was encoded and handed to the sink.
    Emitted,
    /// Dropped: the node has not published its BIRTH yet. A DATA before the
    /// BIRTH carries sequence 0 and reads as a BIRTH on the wire.
    DroppedBeforeBirth,
    /// Dropped: this device was never declared in a BIRTH, so a consumer would
    /// discard the message anyway (a serial that changed under us).
    DroppedUndeclaredDevice {
        /// The serial that was not declared.
        serial: Serial,
    },
}

/// Where the session is in its lifecycle. The `sparkplug-b` type-state makes a
/// DATA-before-BIRTH unrepresentable; this enum is how a long-lived struct holds
/// that type-state.
enum Session {
    /// Connected but not yet born.
    Pending(NodeSession),
    /// Born: DATA may flow.
    Live(LiveSession),
    /// Transient state held only while a birth is being built. Reaching it from
    /// the outside is impossible; it exists so the lifecycle move needs no
    /// placeholder that could be mistaken for a real session.
    Moving,
}

/// Maps judged readings onto the Sparkplug wire for one edge node.
pub struct SparkplugPublisher {
    node: EdgeNode,
    session: Session,
    /// Devices declared by the last BIRTH, with the last reading published for
    /// each — a rebirth re-declares what is actually known instead of resetting
    /// every tag to "nothing".
    declared: HashMap<Serial, Option<MeterUpdate>>,
}

impl SparkplugPublisher {
    /// Opens the session that follows `previous_bd_seq` (restored from storage),
    /// or this node's FIRST session when storage holds nothing.
    ///
    /// `None` is not "start from zero and advance": it is the absence of a
    /// previous session, and it produces `bdSeq = 0` — the *start at zero* half
    /// of `tck-id-topics-nbirth-bdseq-increment` ([#100], ADR 0042).
    pub fn new(node: EdgeNode, previous_bd_seq: Option<BdSeq>) -> Self {
        Self {
            node,
            session: Session::Pending(NodeSession::start(previous_bd_seq)),
            declared: HashMap::new(),
        }
    }

    /// The session number to persist before connecting.
    pub fn bd_seq(&self) -> BdSeq {
        match &self.session {
            Session::Pending(s) => s.bd_seq(),
            Session::Live(s) => s.bd_seq(),
            Session::Moving => unreachable!("a birth is never observable mid-move"),
        }
    }

    /// Begins a NEW session, advancing the session number.
    ///
    /// Every reconnect must do this: reusing a number lets the broker deliver
    /// the previous session's will against the live one, marking the node dead
    /// while it is publishing.
    pub fn new_session(&mut self) {
        let previous = self.bd_seq();
        // Always `Some`: a session that is being replaced IS the previous one.
        // Only a node with no persisted state has none.
        self.session = Session::Pending(NodeSession::start(Some(previous)));
    }

    /// The node DEATH to register as the connection's last will, built BEFORE
    /// connecting (Story 1.12 owns that ordering).
    pub fn will(&self, now: UtcMillis) -> Outbound {
        let payload = match &self.session {
            Session::Pending(s) => s.will(millis(now)),
            Session::Live(s) => s.death(millis(now)),
            Session::Moving => unreachable!("a birth is never observable mid-move"),
        };
        Outbound {
            // A node topic with a node message type cannot fail; the error is
            // unrepresentable here and collapsing it keeps the will infallible,
            // which is what the boot order needs.
            topic: node_topic(&self.node, MessageType::NDeath),
            payload: encode(&payload),
            message: MessageType::NDeath,
        }
    }

    /// Emits the BIRTH: the node BIRTH (declaring the contract version) plus one
    /// device BIRTH per meter.
    ///
    /// A device with no reading yet is declared with NO value and quality
    /// `Stale` — the "never a fresh-looking lie" guarantee at its sharpest. A
    /// device that already has a reading (a rebirth after a reconnect) is
    /// re-declared with that reading and ITS quality, so a transport blip does
    /// not blank a tag the bridge can still account for.
    ///
    /// Every topic is validated before anything is emitted: on error, nothing
    /// has been sent and the session is unchanged.
    pub fn birth(
        &mut self,
        now: UtcMillis,
        meters: &[Serial],
        sink: &mut impl Sink,
    ) -> Result<(), TopicError> {
        // Validate EVERYTHING first — a half-emitted birth would put an
        // incomplete tag set on the irreversible side of the contract.
        let device_topics = meters
            .iter()
            .map(|serial| {
                self.node
                    .device_topic(MessageType::DBirth, serial.as_str())
                    .map(|topic| (serial.clone(), topic))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let timestamp = millis(now);
        let mut live = match std::mem::replace(&mut self.session, Session::Moving) {
            Session::Pending(pending) => {
                let (live, payload) = pending.birth(timestamp, node_metrics(timestamp));
                sink.emit(Outbound {
                    topic: node_topic(&self.node, MessageType::NBirth),
                    payload: encode(&payload),
                    message: MessageType::NBirth,
                });
                live
            }
            Session::Live(mut live) => {
                let payload = live.rebirth(timestamp, node_metrics(timestamp));
                sink.emit(Outbound {
                    topic: node_topic(&self.node, MessageType::NBirth),
                    payload: encode(&payload),
                    message: MessageType::NBirth,
                });
                live
            }
            Session::Moving => unreachable!("a birth is never observable mid-move"),
        };

        let mut declared = HashMap::with_capacity(device_topics.len());
        for (serial, topic) in device_topics {
            let known = self.declared.get(&serial).cloned().flatten();
            let metrics = match &known {
                // A re-declared reading has NOT been re-judged against now, so
                // it is never re-asserted as Good: it is true history, published
                // stale, stamped with its own ValueDate. Claiming Good here
                // would turn a 45-minute broker outage into a fresh-looking lie
                // the moment the link came back.
                Some(update) => metrics_for(&update.measurement, update.verdicts.map(degrade)),
                None => cold_start_metrics(timestamp),
            };
            // The payload timestamp follows the data: a re-declared reading is
            // stamped with when it was TRUE, not with now.
            //
            // ROUTED THROUGH THE TABLE since story 4.12, so the table is a
            // MECHANISM and not a second statement of the truth. Flip
            // `DeviceBirthRedeclaring`'s row and this line changes what it emits —
            // which is what makes `the_timestamp_table_says_which_clock_each_message_speaks`
            // able to fail.
            let payload_ts = match &known {
                Some(update) => match timestamp_source_for(Emission::DeviceBirthRedeclaring) {
                    TimestampSource::ReadingValueDate => millis(update.measurement.value_date),
                    TimestampSource::PublicationInstant => timestamp,
                },
                // A device with no reading has NO acquisition time to carry, so
                // this line emits the publication instant whichever way the table
                // answers. It was written as a `match` over
                // `Emission::DeviceBirthColdStart` by story 4.12 and both arms
                // returned `timestamp` — a branch that cannot change what is
                // emitted, reading as a mechanism it was not. **That row is pinned
                // by `the_timestamp_table_says_which_clock_each_message_speaks`,
                // not by this call site** (2026-08-19 review).
                None => timestamp,
            };
            let payload = live.device_birth(payload_ts, metrics);
            sink.emit(Outbound {
                topic,
                payload: encode(&payload),
                message: MessageType::DBirth,
            });
            declared.insert(serial, known);
        }

        self.declared = declared;
        self.session = Session::Live(live);
        Ok(())
    }

    /// Announces ONE device on a session that is already alive, without
    /// re-birthing the node (Story 5.2 AC4).
    ///
    /// # This is conformant, and the norm says so in as many words
    ///
    /// > *"A Device can publish a DBIRTH as long as an NBIRTH has been sent
    /// > previously and the MQTT session is active."*
    /// > — `Sparkplug_5_Operational_Behavior.adoc:409`
    ///
    /// with `tck-id-message-flow-device-birth-publish-nbirth-wait` requiring only
    /// that *"the NBIRTH message MUST have been sent within the current MQTT
    /// session prior to a DBIRTH being published"*. That precondition is exactly
    /// what [`Session::Live`] means here, so enabling a meter costs one DBIRTH —
    /// no NDEATH, no new `bdSeq`, no interruption to any other device.
    ///
    /// **Do not generalise this to metrics.** Adding a *metric* to an existing
    /// device does need a full rebirth (`Sparkplug_5:695` — *"metrics can even be
    /// added dynamically at runtime and with a new NBIRTH and DBIRTH sequence"*).
    /// A device is a different granularity, and conflating the two is how a host
    /// ends up with a tag it was never told about.
    ///
    /// Returns `false` when the session is not live: there is nothing to add a
    /// device to, and the next connect will birth the current set anyway.
    pub fn device_birth(
        &mut self,
        now: UtcMillis,
        serial: &Serial,
        sink: &mut impl Sink,
    ) -> Result<bool, TopicError> {
        // Validate the topic BEFORE touching the session, as `birth` does.
        let topic = self
            .node
            .device_topic(MessageType::DBirth, serial.as_str())?;
        let Session::Live(live) = &mut self.session else {
            return Ok(false);
        };
        let timestamp = millis(now);
        // A device declared for the first time has no reading yet, so it births
        // cold — the same metrics a connect-time birth would give it. Anything
        // else would assert a value nobody measured.
        let payload = live.device_birth(timestamp, cold_start_metrics(timestamp));
        sink.emit(Outbound {
            topic,
            payload: encode(&payload),
            message: MessageType::DBirth,
        });
        self.declared.insert(serial.clone(), None);
        Ok(true)
    }

    /// Buries ONE device while the node stays alive (Story 5.2 AC4).
    ///
    /// # Why disabling a meter owes a DDEATH rather than silence
    ///
    /// > *"If at any time the Sparkplug Device cannot provide real time
    /// > information, the Sparkplug Specification requires that an DDEATH be
    /// > published. This will inform the Primary Host Application that all metric
    /// > information associated with that Sparkplug Device be set to a STALE data
    /// > quality."* — `Sparkplug_5_Operational_Behavior.adoc:470`
    ///
    /// A meter an operator has just disabled is precisely a device that can no
    /// longer provide real-time information. Stopping quietly would leave its
    /// last value on the host's screen, current-looking and wrong, for as long as
    /// the session lasts — which is the failure this whole project exists to
    /// prevent.
    ///
    /// After the DDEATH the device is undeclared, so
    /// [`Self::publish`] will drop any DDATA that still arrives for it rather
    /// than sending data for a device the host has been told is offline.
    pub fn device_death(
        &mut self,
        now: UtcMillis,
        serial: &Serial,
        sink: &mut impl Sink,
    ) -> Result<bool, TopicError> {
        let topic = self
            .node
            .device_topic(MessageType::DDeath, serial.as_str())?;
        let Session::Live(live) = &mut self.session else {
            return Ok(false);
        };
        // `tck-id-topics-ddeath-seq-num`: the DDEATH carries the next sequence
        // number like any other message. `LiveSession::device_death` allocates
        // it; the payload carries no metrics, by the same rule.
        let payload = live.device_death(millis(now));
        sink.emit(Outbound {
            topic,
            payload: encode(&payload),
            message: MessageType::DDeath,
        });
        self.declared.remove(serial);
        Ok(true)
    }

    /// Publishes one judged reading as device DATA.
    ///
    /// The payload timestamp is the source `ValueDate` — when the values were
    /// TRUE, not when we happened to send them — and every metric carries the
    /// oracle's verdict, so a stale reading is never presented as live.
    pub fn publish(
        &mut self,
        update: &MeterUpdate,
        sink: &mut impl Sink,
    ) -> Result<Published, TopicError> {
        let serial = update.measurement.serial.clone();
        let Session::Live(live) = &mut self.session else {
            return Ok(Published::DroppedBeforeBirth);
        };
        if !self.declared.contains_key(&serial) {
            return Ok(Published::DroppedUndeclaredDevice { serial });
        }
        let topic = self
            .node
            .device_topic(MessageType::DData, serial.as_str())?;
        // NOT ROUTED THROUGH THE TABLE, and the reason is stronger than the
        // table: `publish` is handed no clock at all. `PublicationInstant` is
        // unrepresentable here — there is nothing to read it from — so
        // `Emission::DeviceData`'s row is enforced by this function's SIGNATURE
        // rather than by a branch. Adding a clock parameter to make the table
        // reachable would be adding a capability in order to satisfy a test, and
        // the capability is exactly the one ADR 0013 refuses.
        //
        // (Epic 3's action D3: an "unreachable" cites its enforcer or is not
        // written. The enforcer is the signature, one line above.)
        let timestamp = millis(update.measurement.value_date);
        let payload =
            live.device_data(timestamp, metrics_for(&update.measurement, update.verdicts));
        sink.emit(Outbound {
            topic,
            payload: encode(&payload),
            message: MessageType::DData,
        });
        self.declared.insert(serial, Some(update.clone()));
        Ok(Published::Emitted)
    }
}

/// A node topic for a node message type — infallible by construction.
fn node_topic(node: &EdgeNode, message: MessageType) -> String {
    node.node_topic(message)
        .unwrap_or_else(|_| unreachable!("node message types address node topics"))
}

/// Everything the NODE birth declares about itself, for both session arms.
///
/// # Why one function and not two call sites
///
/// `birth()` has two arms — `Session::Pending` for the first birth and
/// `Session::Live` for every reconnect and every rebirth answer — and both
/// publish an NBIRTH. `tck-id-payloads-nbirth-rebirth-req` binds *"Every
/// NBIRTH"*, so a metric added to one arm only yields a clause that holds on the
/// first birth and fails on every later one: conformant at start-up, silently
/// non-conformant for the rest of the process's life, and visible in no log.
///
/// Building the list once removes the omission rather than testing for it.
/// `every_node_birth_declares_the_rebirth_command` still asserts both arms —
/// the shape is what makes the property true today, and the test is what keeps
/// it true after someone splits this back apart.
fn node_metrics(timestamp_ms: u64) -> Vec<Metric> {
    vec![contract_metric(timestamp_ms), rebirth_metric(timestamp_ms)]
}

/// The rebirth command endpoint, declared in every node BIRTH.
///
/// Five MUST clauses in three chapters converge on one metric:
/// `tck-id-topics-nbirth-rebirth-metric` (`Sparkplug_4_Topics.adoc:215-217`),
/// `tck-id-payloads-nbirth-rebirth-req` (`Sparkplug_6_Payloads.adoc:1082-1084`)
/// and `tck-id-operational-behavior-data-commands-rebirth-name`, `-datatype`,
/// `-value` (`Sparkplug_5_Operational_Behavior.adoc:955-965`). Name exactly
/// `Node Control/Rebirth`, datatype `Boolean`, value `false`.
///
/// **`Boolean(false)`, never `Null(DataType::Boolean)`.** A null metric declares
/// the type a tag WOULD have and carries no value; the clause is a MUST on the
/// value `false`. The null form would satisfy `-rebirth-datatype` and fail
/// `-rebirth-value`, and the two are easy to conflate because both mention
/// `Boolean`.
///
/// **No alias, and that is load-bearing rather than incidental.**
/// `encode_metric` hard-codes `alias: None` for every metric this bridge sends,
/// which satisfies `-rebirth-name-aliases` by construction — so the clause
/// stays `n/a` in the conformance matrix, alongside the three chapter-6 alias
/// clauses, rather than becoming `conformant`. The reason the norm forbids an
/// alias here is that a host must be able to request a rebirth *"without
/// knowledge of any potential alias"*, which is also why the handler in
/// `mqtt_driver.rs` matches on the name alone.
///
/// `Good`, for `contract_metric`'s reason and not by copying it: this is a fact
/// about the running software rather than a reading of the world. There is no
/// cloud call behind it and no clock that can make it old, so there is no state
/// in which the bridge holds it and cannot vouch for it.
fn rebirth_metric(timestamp_ms: u64) -> Metric {
    Metric::new(
        METRIC_NODE_CONTROL_REBIRTH,
        MetricValue::Boolean(false),
        timestamp_ms,
    )
    .with_quality_code(ignition_quality_code(Quality::Good))
}

/// The contract version, declared in the node BIRTH so a consumer can SEE a
/// contract change instead of having to infer one from vanished tags. It is
/// `Good` because it is a fact about the running software, always known — a
/// compile-time constant is never stale.
fn contract_metric(timestamp_ms: u64) -> Metric {
    Metric::new(
        METRIC_CONTRACT_VERSION,
        MetricValue::Int64(CONTRACT_VERSION),
        timestamp_ms,
    )
    .with_quality_code(ignition_quality_code(Quality::Good))
}

/// The tag set declared for a device with no reading yet: named, unit-carrying,
/// valueless and stale.
///
/// **This BIRTH is what makes the `Cause` property exist on the host at all**
/// (ADR 0043), so the property is declared here, and it names
/// [`Cause::NoReadingYet`] rather than [`CAUSE_NONE`]. These metrics are `Stale`;
/// answering *no cause* for a non-good metric would be the lie this whole
/// property exists to prevent. Until v11 they were the one non-good pair in this
/// bridge that named no reason at all, so an operator seeing `Bad_Stale` seconds
/// after a start could not tell a fresh bridge from a broken feed.
fn cold_start_metrics(timestamp_ms: u64) -> Vec<Metric> {
    vec![
        Metric::new(
            METRIC_POWER,
            MetricValue::Null(DataType::Double),
            timestamp_ms,
        )
        .with_quality_code(ignition_quality_code(Quality::Stale))
        .with_engineering_unit(UNIT_POWER),
        Metric::new(
            METRIC_ENERGY,
            MetricValue::Null(DataType::Double),
            timestamp_ms,
        )
        .with_quality_code(ignition_quality_code(Quality::Stale))
        .with_engineering_unit(UNIT_ENERGY),
        cause_metric(
            METRIC_CAUSE_POWER,
            Verdict::stale(Cause::NoReadingYet),
            timestamp_ms,
        ),
        cause_metric(
            METRIC_CAUSE_ENERGY,
            Verdict::stale(Cause::NoReadingYet),
            timestamp_ms,
        ),
    ]
}

/// The mapping — the one and only `Measurement` → Sparkplug metric translation
/// in the tree.
///
/// `Bad` and `Stale` are treated differently on purpose. `Bad` means "this
/// number is not a reading", so no number is published at all: a consumer that
/// ignores the quality flag would otherwise record a real-looking value. `Stale`
/// means "this WAS a reading, it is just no longer current" — the value is true
/// history, so it is published, flagged, with its own `ValueDate` as the payload
/// timestamp. The timestamp is what keeps that honest: a stale reading is
/// visibly old even to a consumer that ignores the flag.
/// [`metrics_for`], for a test in another module that needs to assert on the
/// PUBLISHED metrics rather than on an in-process verdict.
///
/// **Exists because of a defect, and is narrow on purpose.** Story 2.3's review
/// found that every test reaching `metrics_for` handed it a `Verdicts::uniform`,
/// where the pre-2.3 code and the new one agree on every output — so the whole of
/// ADR 0031 could be reverted with the suite green. A test that owns the source
/// path and the wire path in one place is what stops that recurring, and it needs
/// this function to be reachable.
#[cfg(test)]
pub fn metrics_for_test(measurement: &Measurement, verdicts: Verdicts) -> Vec<Metric> {
    metrics_for(measurement, verdicts)
}

fn metrics_for(measurement: &Measurement, verdicts: Verdicts) -> Vec<Metric> {
    let timestamp = millis(measurement.value_date);

    // One metric, judged on its OWN verdict. Before Story 2.3 both metrics took
    // the reading's single verdict, so an energy-only refusal nulled the power
    // value and stamped it with the energy's cause — a number the bridge had no
    // complaint about, withheld and then blamed.
    // **The verdict as PUBLISHED, which is not always the verdict as judged.**
    // Computed once and used by both the value metric and its cause metric — a
    // separation that cost a defect the moment it existed: the first version of
    // ADR 0044 degraded the value inside the builder and left the cause metric
    // reading the ORIGINAL verdict, so an absent value went out `Bad` with a
    // cause of `no-cause`. Caught by `an_absent_value_is_never_published_as_good`.
    let published_verdict = |metric: Measured, value: Option<f64>| {
        // AN ABSENT VALUE CANNOT BE GOOD, and saying it could was a defect.
        //
        // **Found by this story's own review, 2026-08-12.** The first version
        // published the null with whatever quality the verdict carried, and called
        // that "a second lock". It was not one: with a `Good` verdict it put
        // `Null` on the wire at quality 192, which tells a consumer *this
        // measurement is good, and it is nothing*. A lock that publishes "good" is
        // not a lock.
        //
        // Unreachable today — `map_device` pairs every `None` with a fault, so the
        // composition always refuses it — which is precisely why it is degraded
        // here rather than asserted away: the invariant lives in another module,
        // and this one must not depend on it silently. `ValueUnusable` is the
        // honest cause, meaning exactly *not one usable number*, which is what an
        // absent value is.
        match (verdicts.for_metric(metric), value) {
            (v, None) if v.quality() != Quality::Bad => Verdict::bad(Cause::ValueUnusable),
            (v, _) => v,
        }
    };

    let build = |metric: Measured, name: &'static str, unit: &'static str, value: Option<f64>| {
        let published = published_verdict(metric, value).quality();
        let carried = match (published, value) {
            // `Bad` withholds the number. That is the point of `Bad` rather than
            // `Stale`: a consumer must not be handed a value it would compute
            // with, and the datatype is kept so the tag does not change shape.
            (Quality::Bad, _) => MetricValue::Null(DataType::Double),
            (_, None) => MetricValue::Null(DataType::Double),
            (_, Some(value)) => MetricValue::Double(value),
        };
        Metric::new(name, carried, timestamp)
            .with_quality_code(ignition_quality_code(published))
            .with_engineering_unit(unit)
    };

    vec![
        build(
            Measured::Power,
            METRIC_POWER,
            UNIT_POWER,
            measurement.power.map(|p| p.0),
        ),
        build(
            Measured::Energy,
            METRIC_ENERGY,
            UNIT_ENERGY,
            measurement.energy.map(|e| e.0),
        ),
        cause_metric(
            METRIC_CAUSE_POWER,
            published_verdict(Measured::Power, measurement.power.map(|p| p.0)),
            timestamp,
        ),
        cause_metric(
            METRIC_CAUSE_ENERGY,
            published_verdict(Measured::Energy, measurement.energy.map(|e| e.0)),
            timestamp,
        ),
    ]
}

/// The cause of one metric's verdict, as a metric of its own (ADR 0044).
///
/// **`Good` quality, always, and it is not a copy of the measurement's.** This is
/// a fact about the bridge's own judgement rather than a reading of the world:
/// there is no cloud call behind it and no clock that can make it old, so there
/// is no state in which the bridge holds it and cannot vouch for it. Marking it
/// `Stale` because the metric it describes is stale would be the same category
/// error as stamping a diagnosis with the patient's temperature — and it would
/// make the one tag that explains a fault unreadable exactly when it matters.
///
/// A `Good` measurement's cause metric reads [`CAUSE_NONE`]. It is published
/// every time, like any metric: a consumer reading a tag reads its current value,
/// and there is no version of this that goes stale in silence.
fn cause_metric(name: &str, verdict: Verdict, timestamp_ms: u64) -> Metric {
    let cause = verdict.cause().map_or(CAUSE_NONE, |c| c.as_str());
    Metric::new(name, MetricValue::String(cause.to_string()), timestamp_ms)
        .with_quality_code(ignition_quality_code(Quality::Good))
}

/// A verdict that has not been re-computed cannot be re-asserted: `Good`
/// becomes `Stale`, and anything already worse stays as bad as it was — keeping
/// the cause it already had, because that cause is still the true one.
fn degrade(verdict: Verdict) -> Verdict {
    match verdict.quality() {
        Quality::Good => Verdict::stale(Cause::NotRevalidated),
        _ => verdict,
    }
}

/// Which clock a payload timestamp speaks, per thing this bridge publishes
/// (story 4.12 AC3).
///
/// # Why this table exists at all
///
/// The invariant — *a stale reading must read as old even to a consumer that
/// ignores the quality flag* — lives in two call sites and nothing stops a third
/// reaching for the wrong clock. This is [`qos_for`]'s pattern applied to time:
/// a table, pinned by a test, with the clause or the ADR named per row, so that
/// moving a row means saying so.
///
/// [`qos_for`]: crate::app::mqtt_driver
///
/// # The specification's own split, read rather than remembered
///
/// The norm puts acquisition time in the **metric** timestamp — *"this timestamp
/// represents the time at which the value of a metric was captured"*, which is the
/// **non-normative comment** under `tck-id-payloads-metric-timestamp-in-UTC`
/// (`Sparkplug_6:479-482`); the clause itself binds only *"The timestamp MUST be in
/// UTC"*, and the attribution is separated here because this repository cites
/// clauses, not the prose around them (2026-08-19 review). The **payload**
/// timestamp is another matter and is bound outright: it MUST denote *"the time at
/// which the message was published"*, in identical words for NBIRTH, DBIRTH, NDATA,
/// DDATA and DDEATH.
///
/// **This bridge deviates on two of those, deliberately, and [ADR 0013] is why.**
/// Stamping `now` on a re-declared 45-minute-old reading would hand a consumer
/// that reads the payload timestamp and ignores `Quality` a stale value wearing a
/// fresh one — the precise silent lie this project exists to prevent, and it
/// would be *conformant*. That consumer is not hypothetical: contract v1 shipped
/// quality codes a live Ignition displayed as `Good(500)` (ADR 0012).
///
/// [ADR 0013]: ../../../docs/adr/0013-payload-timestamp-is-acquisition-time.md
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampSource {
    /// The reading's own `ValueDate` — when the values were TRUE.
    ///
    /// A recorded deviation from the norm wherever it is used. See [ADR 0013].
    ///
    /// [ADR 0013]: ../../../docs/adr/0013-payload-timestamp-is-acquisition-time.md
    ReadingValueDate,
    /// The instant the message was built — what the norm asks for.
    PublicationInstant,
}

/// Everything this bridge publishes. **Six, and NDATA is not among them.**
///
/// The delivery table in `mqtt_driver` carries an NDATA row because the norm
/// binds one; this enum carries what the publisher actually emits, and it emits
/// no node-level data at all. Listing a seventh here would describe a message
/// nobody sends — the shape of the four Epic 1 tests this repository threw away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emission {
    /// NBIRTH — the first birth of a session, and every rebirth.
    NodeBirth,
    /// NDEATH — the will registered at CONNECT, and the death published on the
    /// way out.
    NodeDeath,
    /// DBIRTH for a device with **no reading yet**: nothing to declare but its
    /// existence.
    DeviceBirthColdStart,
    /// DBIRTH **re-declaring a reading already known** — a rebirth, or a device
    /// re-announced mid-session.
    DeviceBirthRedeclaring,
    /// DDATA — one judged reading.
    DeviceData,
    /// DDEATH — a device that has ended.
    DeviceDeath,
}

/// The clock each emission's PAYLOAD timestamp speaks (story 4.12 AC3).
///
/// Metric timestamps are not this function's subject: they always carry the
/// reading's `ValueDate`, which is what the norm asks of them.
pub fn timestamp_source_for(emission: Emission) -> TimestampSource {
    match emission {
        // CONFORMANT. A node birth announces a SESSION; there is no reading whose
        // time it could carry, and `tck-id-payloads-nbirth-timestamp`
        // (`Sparkplug_6:1064`) asks for the publication instant.
        Emission::NodeBirth => TimestampSource::PublicationInstant,
        // OURS TO CHOOSE, and worth knowing: there is **no**
        // `tck-id-payloads-ndeath-timestamp` in the norm — NDEATH's clauses
        // govern `seq`, `bdSeq` and the will, and none of them the payload
        // timestamp. DDEATH has one; NDEATH does not. We publish the instant for
        // the same reason the norm gives everywhere else.
        Emission::NodeDeath => TimestampSource::PublicationInstant,
        // CONFORMANT, and it costs nothing: a device with no reading has no
        // acquisition time to carry (`tck-id-payloads-dbirth-timestamp`,
        // `Sparkplug_6:1176`).
        Emission::DeviceBirthColdStart => TimestampSource::PublicationInstant,
        // DEVIATION, recorded — `tck-id-payloads-dbirth-timestamp`, ADR 0013.
        // This is the reconnection case: the rebirth that follows a 45-minute
        // outage re-declares history, and history stamped `now` is a lie.
        Emission::DeviceBirthRedeclaring => TimestampSource::ReadingValueDate,
        // DEVIATION, recorded — `tck-id-payloads-ddata-timestamp`
        // (`Sparkplug_6:1302` is NDATA's; DDATA's is `:1359`), ADR 0013.
        Emission::DeviceData => TimestampSource::ReadingValueDate,
        // CONFORMANT. A death is an event, not a measurement
        // (`tck-id-payloads-ddeath-timestamp`, `Sparkplug_6:1582`).
        Emission::DeviceDeath => TimestampSource::PublicationInstant,
    }
}

/// Sparkplug timestamps are unsigned epoch-millis; a pre-epoch instant cannot be
/// represented and is clamped to the epoch, where the consumer's own
/// plausibility check catches it.
fn millis(t: UtcMillis) -> u64 {
    u64::try_from(t.0).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use crate::core::oracle::{Cause, Verdict};

    /// Story 2.1 AC3 — the cause reaches the metric, under its own key, and only
    /// when there is one.
    ///
    /// **Why not inside `Quality`:** `tck-id-payloads-propertyset-quality-value-value`
    /// admits only `0`, `192` and `500` there, and this bridge already deviates
    /// from that clause deliberately (ADR 0012). A fourth value would deepen a
    /// deviation accepted only because the alternative was a silent lie.
    ///
    /// The third assertion is the one that would be missed: a GOOD metric must
    /// carry no cause at all. A cause published beside every good value is noise a
    /// consumer learns to ignore, and then misses the day it means something.
    ///
    /// FALSIFIED 2026-08-10: making `with_cause` unconditional — attaching
    /// `cause.unwrap_or(Cause::NotRevalidated)` — turns the good-metric assertion
    /// red while the two degraded ones stay green.
    #[test]
    fn a_non_good_metric_names_its_cause_and_a_good_one_says_it_has_none() {
        let m = super::super::sparkplug_publisher::tests::measurement(super::Quality::Good);

        // From contract v12 the cause is a METRIC of its own, not a property
        // (ADR 0044): a property is written by a BIRTH and never updated by a
        // DDATA, measured against a live host, so a cause carried as one stands
        // frozen at its birth value for ever.
        let cause_named = |ms: &[super::Metric], name: &str| -> Option<String> {
            ms.iter().find(|m| m.name == name).map(|m| match &m.value {
                super::MetricValue::String(v) => v.clone(),
                other => panic!("a cause is a string, got {other:?}"),
            })
        };

        let degraded =
            super::metrics_for(&m, Verdicts::uniform(Verdict::stale(Cause::ReadingTooOld)));
        for name in [super::METRIC_CAUSE_POWER, super::METRIC_CAUSE_ENERGY] {
            assert_eq!(
                cause_named(&degraded, name).as_deref(),
                Some("reading-too-old"),
                "{name} must name why its measurement is not good"
            );
        }
        for name in [super::METRIC_POWER, super::METRIC_ENERGY] {
            let metric = degraded.iter().find(|m| m.name == name).expect("published");
            assert!(
                metric.properties.is_empty(),
                "{name} carries no property at all now — the cause moved out of it"
            );
            // And the quality property is untouched by all this: still exactly
            // what ADR 0012 chose.
            assert_eq!(
                metric.quality_code,
                Some(super::ignition_quality_code(super::Quality::Stale))
            );
        }

        let refused = super::metrics_for(&m, Verdicts::uniform(Verdict::bad(Cause::SourceRefused)));
        assert_eq!(
            cause_named(&refused, super::METRIC_CAUSE_POWER).as_deref(),
            Some("source-refused")
        );

        // A good measurement says so explicitly rather than falling silent: a
        // consumer reads a tag's current value, and an absent tag is a hole.
        let good = super::metrics_for(&m, Verdicts::uniform(Verdict::good()));
        for name in [super::METRIC_CAUSE_POWER, super::METRIC_CAUSE_ENERGY] {
            assert_eq!(
                cause_named(&good, name).as_deref(),
                Some(super::CAUSE_NONE),
                "{name} is good and must say so explicitly"
            );
        }
        // The cause metric is GOOD even when the metric it describes is not: it is
        // a fact about our judgement, not a reading of the world.
        let cause_tag = degraded
            .iter()
            .find(|m| m.name == super::METRIC_CAUSE_POWER)
            .expect("published");
        assert_eq!(
            cause_tag.quality_code,
            Some(super::ignition_quality_code(super::Quality::Good)),
            "the tag that EXPLAINS a fault must not be unreadable exactly when it matters"
        );
    }

    /// **An absent value is never published as good** (review of story 2.5,
    /// 2026-08-12).
    ///
    /// The first version of the `None` arm published the null with the verdict's
    /// own quality, so a `Good` verdict put `Null` on the wire at quality 192 —
    /// *this measurement is good, and it is nothing*. Not reachable through
    /// `map_device`, which pairs every absent field with a fault; asserted anyway,
    /// because this module must not depend silently on an invariant that lives in
    /// another one.
    ///
    /// FALSIFIED 2026-08-12: removing the degradation restores quality 192 beside
    /// the null and the assertion goes red naming the code.
    #[test]
    fn an_absent_value_is_never_published_as_good() {
        let mut m = measurement(Quality::Good);
        m.power = None;
        let metrics = super::metrics_for(&m, Verdicts::uniform(Verdict::good()));
        let power = metrics
            .iter()
            .find(|x| x.name == super::METRIC_POWER)
            .expect("power is published");
        assert!(matches!(power.value, MetricValue::Null(_)));
        assert_eq!(
            power.quality_code,
            Some(super::ignition_quality_code(super::Quality::Bad)),
            "a null published at a GOOD quality tells a consumer the absence IS \
             the measurement"
        );
        let cause = metrics
            .iter()
            .find(|m| m.name == super::METRIC_CAUSE_POWER)
            .expect("the cause travels as its own metric since v12");
        assert!(
            matches!(&cause.value, MetricValue::String(v) if v == "value-unusable"),
            "and a non-good measurement names its cause, here the one that means \
             exactly `not one usable number`. Got {:?}",
            cause.value
        );
    }

    /// **Story 2.3 AC1, ON THE PUBLISHED METRICS** — the pair that ADR 0031
    /// exists for, asserted where a consumer would see it.
    ///
    /// **ADDED 2026-08-11 by the review of story 2.3, which found this hole by
    /// running the mutation.** Every other test reaching `metrics_for` hands it a
    /// `Verdicts::uniform`, where the pre-2.3 code and the new one agree on every
    /// output — so `let verdict = verdicts.meter();` restored the exact old wire,
    /// `Power = null` labelled `counter-went-backwards`, and the WHOLE SUITE
    /// STAYED GREEN. The story's own AC1 test asserts on the in-process
    /// `MeterUpdate` and never inspects a `Metric`: it proves the core composes
    /// per metric, and nothing about what leaves the process.
    ///
    /// This is the behaviour `CONTRACT_VERSION = 6` is declared BREAKING for. It
    /// was pinned by nothing: not by this file, and not by `contract_golden`,
    /// which is a list of strings and cannot express which metric a verdict lands
    /// on.
    ///
    /// FALSIFIED 2026-08-11, three mutations, each red on its own message:
    /// `verdicts.for_metric(metric)` → `verdicts.meter()` (the whole of ADR 0031
    /// undone — the `Power` value assertion goes red); nulling on
    /// `verdicts.meter().quality()` instead of the metric's own (same);
    /// attaching the cause from `verdicts.meter()` (the "no cause on a good
    /// metric" assertion goes red).
    #[test]
    fn a_metric_refused_alone_is_the_only_one_nulled_on_the_wire() {
        let m = measurement(Quality::Good);
        let mixed = Verdicts::from_judgements(&[
            crate::core::oracle::Judgement::about_reading(Verdict::good()),
            crate::core::oracle::Judgement::about(
                Measured::Energy,
                Verdict::bad(Cause::CounterWentBackwards),
            ),
        ]);
        let metrics = super::metrics_for(&m, mixed);

        let power = metrics
            .iter()
            .find(|metric| metric.name == super::METRIC_POWER)
            .expect("power is published");
        let energy = metrics
            .iter()
            .find(|metric| metric.name == super::METRIC_ENERGY)
            .expect("energy is published");

        // THE REFUSED METRIC: no value at all, and it says why.
        assert!(
            matches!(energy.value, MetricValue::Null(DataType::Double)),
            "a refused metric must withhold its number — that is what `Bad`              means, and it is the whole reason a backwards counter is `Bad`              rather than `Stale`. Got {:?}",
            energy.value
        );
        assert_eq!(
            energy.quality_code,
            Some(super::ignition_quality_code(super::Quality::Bad))
        );
        let energy_cause = metrics
            .iter()
            .find(|m| m.name == super::METRIC_CAUSE_ENERGY)
            .expect("published");
        assert!(
            matches!(&energy_cause.value, MetricValue::String(v) if v == "counter-went-backwards"),
            "got {:?}",
            energy_cause.value
        );

        // THE METRIC NOBODY OBJECTED TO: its real value, at full trust, unlabelled.
        assert!(
            matches!(power.value, MetricValue::Double(v) if Some(v) == m.power.map(|p| p.0)),
            "the power reading is current and no oracle judged it; withholding it              publishes a fault where there is none. Got {:?}",
            power.value
        );
        assert_eq!(
            power.quality_code,
            Some(super::ignition_quality_code(super::Quality::Good))
        );
        let power_cause = metrics
            .iter()
            .find(|m| m.name == super::METRIC_CAUSE_POWER)
            .expect("published");
        assert!(
            matches!(&power_cause.value, MetricValue::String(v) if v == super::CAUSE_NONE),
            "a good measurement names NO cause — and since v11 it says so \
             explicitly rather than falling silent. What it must never carry is \
             its neighbour's, which is what the pre-2.3 wire did: `Power = null`, \
             cause `counter-went-backwards`, for a number the bridge had no \
             complaint about. Got {:?}",
            power_cause.value
        );
    }

    /// The property that outlives the exact constants: neither non-good quality
    /// may land on Ignition's *good* level, whatever its subcode. That was the
    /// contract-v1 defect, and it was invisible from inside this tree.
    #[test]
    fn no_non_good_quality_can_be_mistaken_for_good_by_ignition() {
        const BAD_LEVEL: u32 = 0x8000_0000;
        for quality in [super::Quality::Stale, super::Quality::Bad] {
            assert_ne!(
                super::ignition_quality_code(quality) & BAD_LEVEL,
                0,
                "{quality:?} must carry Ignition's bad level in its top bits, or \
                 the host reads it as good and shows an untrustworthy value as \
                 trustworthy"
            );
        }
        assert_eq!(
            super::ignition_quality_code(super::Quality::Good),
            192,
            "192 is Good in both encodings"
        );
    }

    /// The generic crate must stay specification-correct even though this
    /// bridge deviates: the deviation lives here, not there.
    #[test]
    fn the_generic_crate_still_publishes_the_specified_codes() {
        assert_eq!(sparkplug_b::Quality::Good.code(), 192);
        assert_eq!(sparkplug_b::Quality::Stale.code(), 500);
        assert_eq!(sparkplug_b::Quality::Bad.code(), 0);
    }

    use super::*;
    use crate::domain::{Kw, Kwh, MeterId};
    use sparkplug_b::protobuf::{Payload, payload};

    fn node() -> EdgeNode {
        EdgeNode::new("Site", "Bridge").expect("valid ids")
    }

    fn publisher() -> SparkplugPublisher {
        SparkplugPublisher::new(node(), None)
    }

    const SERIAL: &str = "30000001";

    fn measurement(quality: Quality) -> Measurement {
        Measurement {
            meter: MeterId::new("garage"),
            serial: Serial::new(SERIAL),
            power: Some(Kw(0.018)),
            energy: Some(Kwh(4_843.822)),
            value_date: UtcMillis(1_784_984_792_050),
            quality,
        }
    }

    /// A representative verdict for a quality, for the tests that only care which
    /// quality reaches the wire.
    ///
    /// The cause is arbitrary but not absent: a `Stale` or `Bad` verdict without
    /// one cannot be built, which is the point of Story 2.1 — every non-good
    /// quality names why. Tests that DO care about the cause construct the verdict
    /// themselves rather than going through here.
    fn verdict_of(quality: Quality) -> Verdict {
        match quality {
            Quality::Good => Verdict::good(),
            Quality::Stale => Verdict::stale(Cause::ReadingTooOld),
            Quality::Bad => Verdict::bad(Cause::ValueUnusable),
        }
    }

    fn update(published: Quality) -> MeterUpdate {
        MeterUpdate::uniform(
            MeterId::new("garage"),
            measurement(Quality::Good),
            verdict_of(published),
        )
    }

    fn born() -> (SparkplugPublisher, RecordingSink) {
        let mut p = publisher();
        let mut sink = RecordingSink::default();
        p.birth(UtcMillis(1_000), &[Serial::new(SERIAL)], &mut sink)
            .expect("valid topics");
        sink.emitted.clear();
        (p, sink)
    }

    fn decode(o: &Outbound) -> Payload {
        sparkplug_b::decode(&o.payload).expect("valid protobuf")
    }

    fn metric<'a>(p: &'a Payload, name: &str) -> &'a payload::Metric {
        p.metrics
            .iter()
            .find(|m| m.name.as_deref() == Some(name))
            .expect("metric present")
    }

    /// The cause published for one measurement, read from its own METRIC.
    ///
    /// Added by story 4.12 to tell "stale because it was not re-judged" from "bad
    /// because the counter went backwards", which a quality code alone cannot.
    /// **It read a property until contract v12**; the cause is a metric now (ADR
    /// 0044), because a property is written by a BIRTH and never updated.
    ///
    /// Returns `None` when the payload carries no such metric at all, which is a
    /// different answer from a metric reading `no-cause` — and the tests below
    /// depend on the difference.
    fn cause_of(p: &Payload, name: &str) -> Option<String> {
        let m = p.metrics.iter().find(|m| m.name.as_deref() == Some(name))?;
        match &m.value {
            Some(payload::metric::Value::StringValue(v)) => Some(v.clone()),
            _ => None,
        }
    }

    fn quality_of(m: &payload::Metric) -> u32 {
        let props = m.properties.as_ref().expect("properties");
        let idx = props
            .keys
            .iter()
            .position(|k| k == Quality::PROPERTY_KEY)
            .expect("quality property");
        match props.values[idx].value {
            Some(payload::property_value::Value::IntValue(v)) => v,
            _ => panic!("quality is an int property"),
        }
    }

    fn unit_of(m: &payload::Metric) -> String {
        let props = m.properties.as_ref().expect("properties");
        let idx = props
            .keys
            .iter()
            .position(|k| k == Metric::ENG_UNIT_KEY)
            .expect("engUnit property");
        match &props.values[idx].value {
            Some(payload::property_value::Value::StringValue(v)) => v.clone(),
            _ => panic!("engUnit is a string property"),
        }
    }

    /// **The load-bearing half of [#107]'s repair, and it survived the remedy
    /// changing under it.**
    ///
    /// Ignition builds its tag set from what a BIRTH declares. A metric that only
    /// ever appears in a DDATA is a tag the host never created — the same rule
    /// that made the `Cause` PROPERTY useless (ADR 0043, superseded) applies to a
    /// metric's existence, even though a metric's VALUE, unlike a property's, is
    /// then updated by every DDATA. That difference is the whole of ADR 0044, and
    /// it was measured against a live host on 2026-08-22 with the wire read at
    /// the same instant as the screen.
    ///
    /// So if the cold-start BIRTH stops declaring the cause metrics, the tags
    /// never exist, every later DDATA carrying them is discarded, and contract
    /// v4's operator-facing purpose silently stops working again — with the wire
    /// conformant and every other test green.
    ///
    /// It also pins the VALUE. A cold-start measurement is `Stale`, so declaring
    /// the neutral `no-cause` would be false, and false in the one direction this
    /// project exists to prevent.
    ///
    /// FALSIFIED 2026-08-22, both ways: dropping the two `cause_metric` calls from
    /// `cold_start_metrics` goes red with *"the cold-start BIRTH must DECLARE"*;
    /// publishing `CAUSE_NONE` there instead goes red naming `"no-cause"` where
    /// `"no-reading-yet"` belongs.
    #[test]
    fn the_cold_start_birth_declares_the_cause_metrics_and_does_not_lie_about_them() {
        let mut p = publisher();
        let mut sink = RecordingSink::default();
        p.birth(UtcMillis(1_000), &[Serial::new(SERIAL)], &mut sink)
            .expect("valid topics");

        let dbirth = decode(&sink.emitted[1]);
        for name in [METRIC_CAUSE_POWER, METRIC_CAUSE_ENERGY] {
            let cause = cause_of(&dbirth, name).unwrap_or_else(|| {
                panic!(
                    "the cold-start BIRTH must DECLARE {name}: Ignition builds its tag \
                     set from a BIRTH, so a metric that first appears in a DDATA is a \
                     tag the host never created — and the operator never learns why a \
                     value is not good"
                )
            });
            assert_eq!(
                cause,
                Cause::NoReadingYet.as_str(),
                "{name}'s measurement is STALE at cold start, so the declared cause \
                 must say why — the neutral value would be a lie about a non-good \
                 measurement"
            );
        }

        // And the cause tag is GOOD while what it describes is not: it is a fact
        // about our own judgement, always known.
        let tag = metric(&dbirth, METRIC_CAUSE_POWER);
        assert_eq!(quality_of(tag), ignition_quality_code(Quality::Good));
    }

    #[test]
    fn cold_start_birth_declares_tags_with_no_value_and_stale_quality() {
        let mut p = publisher();
        let mut sink = RecordingSink::default();
        p.birth(UtcMillis(1_000), &[Serial::new(SERIAL)], &mut sink)
            .expect("valid topics");

        assert_eq!(sink.emitted.len(), 2, "one NBIRTH + one DBIRTH");
        assert_eq!(sink.emitted[0].topic, "spBv1.0/Site/NBIRTH/Bridge");
        assert_eq!(
            sink.emitted[1].topic, "spBv1.0/Site/DBIRTH/Bridge/30000001",
            "the device is keyed by Serial"
        );

        let dbirth = decode(&sink.emitted[1]);
        for name in [METRIC_POWER, METRIC_ENERGY] {
            let m = metric(&dbirth, name);
            assert_eq!(
                quality_of(m),
                ignition_quality_code(Quality::Stale),
                "{name} is STALE at cold start, never GOOD-by-default"
            );
            assert_eq!(m.value, None, "{name} carries no fabricated value");
            assert_eq!(m.is_null, Some(true));
            assert_eq!(m.datatype, Some(DataType::Double.code()));
        }
        assert_eq!(unit_of(metric(&dbirth, METRIC_POWER)), UNIT_POWER);
        assert_eq!(unit_of(metric(&dbirth, METRIC_ENERGY)), UNIT_ENERGY);
    }

    #[test]
    fn the_node_birth_publishes_the_contract_version() {
        let mut p = publisher();
        let mut sink = RecordingSink::default();
        p.birth(UtcMillis(1_000), &[], &mut sink).unwrap();
        let nbirth = decode(&sink.emitted[0]);
        let m = metric(&nbirth, METRIC_CONTRACT_VERSION);
        assert_eq!(
            m.value,
            Some(payload::metric::Value::LongValue(CONTRACT_VERSION as u64)),
            "a consumer can SEE the contract version, not infer it"
        );
        assert_eq!(quality_of(m), ignition_quality_code(Quality::Good));
    }

    /// Story 4.7 / AC1 — EVERY node BIRTH declares the rebirth command.
    ///
    /// Five MUST clauses, in three chapters, say the same thing:
    /// `tck-id-topics-nbirth-rebirth-metric`,
    /// `tck-id-payloads-nbirth-rebirth-req` (*"EVERY NBIRTH"*), and
    /// `tck-id-operational-behavior-data-commands-rebirth-name`, `-datatype`
    /// and `-value`. Without it a host has no declared endpoint to address, so
    /// the handler this story adds is unreachable by a conformant host.
    ///
    /// Both births are asserted because `birth()` has TWO arms —
    /// `Session::Pending` for the first and `Session::Live` for every later one
    /// — and adding the metric to one of them yields a clause that holds on the
    /// first birth and fails on every reconnect and every rebirth. `-rebirth-req`
    /// says *"Every NBIRTH"*, so one arm is not conformance.
    ///
    /// Asserted against the DECODED payload, never against the builder's own
    /// expression: [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)
    /// lists eight invariants currently "proved" by comparing production code
    /// with itself.
    ///
    /// Falsified 2026-07-30: see the story's falsification table (mutations 1
    /// and 2 — omitting the metric from either arm turns this red, and the
    /// `Session::Live` omission is red ONLY on the second birth, which is the
    /// case a single-birth test would have missed).
    #[test]
    fn every_node_birth_declares_the_rebirth_command() {
        let mut p = publisher();
        let mut sink = RecordingSink::default();
        // First birth: the `Session::Pending` arm.
        p.birth(UtcMillis(1_000), &[Serial::new(SERIAL)], &mut sink)
            .expect("valid topics");
        // Second: the `Session::Live` arm — a reconnect or a rebirth answer.
        p.birth(UtcMillis(2_000), &[Serial::new(SERIAL)], &mut sink)
            .expect("valid topics");

        let births: Vec<&Outbound> = sink
            .emitted
            .iter()
            .filter(|o| o.message == MessageType::NBirth)
            .collect();
        assert_eq!(
            births.len(),
            2,
            "this test needs both the Pending and the Live arm to have run"
        );

        // The NAME the norm requires, spelled out ONCE, here, as a literal.
        //
        // Everything else in this test locates the metric with
        // `METRIC_NODE_CONTROL_REBIRTH` — which is the same constant the producer
        // uses, so it cannot witness the string itself. Rename the constant to
        // `"Node Control/Reburth"` and every assertion below still passes while
        // three chapters are violated and no conformant host can address the
        // endpoint. The Story 4.7 code review found the only fast witnesses of the
        // literal were incidental (a near-miss TRACE test in `mqtt_driver`), with
        // the rest behind Docker-gated chaos tests that `--fast` skips.
        //
        // `tck-id-operational-behavior-data-commands-rebirth-name` (`:955-956`),
        // `tck-id-topics-nbirth-rebirth-metric` (`Sparkplug_4_Topics.adoc:215-217`)
        // and `tck-id-payloads-nbirth-rebirth-req`
        // (`Sparkplug_6_Payloads.adoc:1082-1084`) all spell it this way.
        //
        // Falsified 2026-07-30: changing the constant's value turns THIS assertion
        // red and leaves every other assertion in the test green.
        assert_eq!(
            METRIC_NODE_CONTROL_REBIRTH, "Node Control/Rebirth",
            "the metric name is fixed by the specification, not by us: a host \
             addresses this exact string and cannot be asked to guess another"
        );

        for (nth, out) in births.iter().enumerate() {
            let nbirth = decode(out);
            // Not the `metric` helper: its `expect("metric present")` names
            // neither the metric nor WHICH birth lacked it, and the failure
            // that matters here is specifically "the second one". A test that
            // fails is not automatically a test that says what broke.
            let m = nbirth
                .metrics
                .iter()
                .find(|m| m.name.as_deref() == Some(METRIC_NODE_CONTROL_REBIRTH))
                .unwrap_or_else(|| {
                    let present: Vec<&str> = nbirth
                        .metrics
                        .iter()
                        .filter_map(|m| m.name.as_deref())
                        .collect();
                    panic!(
                        "NBIRTH #{nth} (0 = the first birth, 1 = the rebirth) declares \
                         no {METRIC_NODE_CONTROL_REBIRTH} metric; it carries {present:?}. \
                         tck-id-payloads-nbirth-rebirth-req binds EVERY NBIRTH, so a \
                         metric present only on #0 is not conformance — and it is \
                         invisible in production, where #0 happens once and #1 happens \
                         on every reconnect."
                    )
                });
            assert_eq!(
                m.datatype,
                Some(DataType::Boolean.code()),
                "NBIRTH #{nth}: -rebirth-datatype is a MUST on Boolean (code 11)"
            );
            assert_eq!(
                m.value,
                Some(payload::metric::Value::BooleanValue(false)),
                "NBIRTH #{nth}: -rebirth-value is a MUST on the value false"
            );
            assert_ne!(
                m.is_null,
                Some(true),
                "NBIRTH #{nth}: a null metric is the ABSENCE of a value, and the \
                 clause requires the value false — Null(Boolean) would satisfy \
                 the datatype clause while failing the value one"
            );
            assert_eq!(
                m.alias, None,
                "NBIRTH #{nth}: -rebirth-name-aliases forbids an alias here so \
                 that a host can request a rebirth without knowing one"
            );
        }
    }

    #[test]
    fn a_good_reading_carries_units_serial_and_the_source_timestamp() {
        let (mut p, mut sink) = born();
        assert_eq!(
            p.publish(&update(Quality::Good), &mut sink).unwrap(),
            Published::Emitted
        );

        assert_eq!(sink.emitted.len(), 1);
        let out = &sink.emitted[0];
        assert_eq!(out.topic, "spBv1.0/Site/DDATA/Bridge/30000001");
        assert_eq!(out.message, MessageType::DData);

        let ddata = decode(out);
        assert_eq!(
            ddata.timestamp,
            Some(1_784_984_792_050),
            "the payload timestamp IS the source ValueDate"
        );
        let power = metric(&ddata, METRIC_POWER);
        assert_eq!(quality_of(power), ignition_quality_code(Quality::Good));
        assert_eq!(unit_of(power), UNIT_POWER);
        assert_eq!(
            power.value,
            Some(payload::metric::Value::DoubleValue(0.018))
        );
        let energy = metric(&ddata, METRIC_ENERGY);
        assert_eq!(unit_of(energy), UNIT_ENERGY);
        assert_eq!(
            energy.value,
            Some(payload::metric::Value::DoubleValue(4_843.822)),
            "full counter resolution, Double never float32"
        );
    }

    #[test]
    fn a_stale_verdict_never_publishes_a_fresh_looking_metric() {
        let (mut p, mut sink) = born();
        // The SOURCE said the reading is fine; the oracle judged it stale.
        assert_eq!(
            p.publish(&update(Quality::Stale), &mut sink).unwrap(),
            Published::Emitted
        );

        let ddata = decode(&sink.emitted[0]);
        for name in [METRIC_POWER, METRIC_ENERGY] {
            assert_eq!(
                quality_of(metric(&ddata, name)),
                ignition_quality_code(Quality::Stale),
                "{name} must carry the oracle's verdict, not the source's"
            );
        }
        assert_eq!(
            ddata.timestamp,
            Some(1_784_984_792_050),
            "and it stays stamped with when it was true, so it reads as old"
        );
    }

    #[test]
    fn a_bad_verdict_publishes_no_value_at_all() {
        let (mut p, mut sink) = born();
        assert_eq!(
            p.publish(&update(Quality::Bad), &mut sink).unwrap(),
            Published::Emitted
        );

        let ddata = decode(&sink.emitted[0]);
        for name in [METRIC_POWER, METRIC_ENERGY] {
            let m = metric(&ddata, name);
            assert_eq!(quality_of(m), ignition_quality_code(Quality::Bad));
            assert_eq!(m.value, None, "{name}: no number a consumer could record");
            assert_eq!(m.is_null, Some(true));
        }
    }

    #[test]
    fn a_drop_before_the_birth_is_reported_not_silent() {
        let mut p = publisher();
        let mut sink = RecordingSink::default();
        let outcome = p.publish(&update(Quality::Good), &mut sink).unwrap();
        assert_eq!(outcome, Published::DroppedBeforeBirth);
        assert!(sink.emitted.is_empty());
    }

    #[test]
    fn a_reading_for_an_undeclared_device_is_reported_not_silent() {
        let (mut p, mut sink) = born();
        let mut stranger = update(Quality::Good);
        stranger.measurement.serial = Serial::new("99999999");
        let outcome = p.publish(&stranger, &mut sink).unwrap();
        assert_eq!(
            outcome,
            Published::DroppedUndeclaredDevice {
                serial: Serial::new("99999999")
            }
        );
        assert!(sink.emitted.is_empty());
    }

    /// **Story 3.1 AC2 and AC3, and nothing exercised either beyond one meter.**
    ///
    /// Every other `birth` test here passes one serial or none, because the
    /// runtime served one meter until 2026-08-06. The code was already written
    /// against a slice and is unchanged by the fleet — which is exactly why it
    /// needs a test: *"it always took a list"* is a claim about the signature,
    /// not about the messages.
    ///
    /// AC3 is the one at risk. A per-meter task design invites a per-meter
    /// counter, and `tck-id-topics-dbirth-seq` (`Sparkplug_4:386`) makes the
    /// sequence a property of the **Edge Node**: one greater than the previous
    /// message *from the node*, whichever device it concerned.
    ///
    /// The assertion is the FULL ordered list, not "increasing" and not "all
    /// different". A counter advancing by two, or restarting per device, or
    /// shared but read twice, each satisfies a weaker claim.
    ///
    /// FALSIFIED 2026-08-07 by giving each device its own counter — `live` is
    /// replaced by a fresh `Session` per device inside `birth`'s device loop.
    /// Copied from the run:
    ///
    /// ```text
    /// test adapters::sparkplug_publisher::tests::four_devices_share_one_node_sequence ... FAILED
    ///
    /// thread '…four_devices_share_one_node_sequence' (355) panicked at
    /// crates/smartme-bridge/src/adapters/sparkplug_publisher.rs:1072:9:
    /// assertion `left == right` failed: the sequence belongs to the NODE, not to a
    /// device: four devices born under one node must consume 1,2,3,4 after the NBIRTH's 0
    ///   left: [Some(0), Some(1), Some(1), Some(1), Some(1)]
    ///  right: [Some(0), Some(1), Some(2), Some(3), Some(4)]
    /// ```
    #[test]
    fn four_devices_share_one_node_sequence() {
        let mut p = publisher();
        let mut sink = RecordingSink::default();
        let fleet: Vec<Serial> = ["30000001", "30000002", "30000003", "30000004"]
            .iter()
            .map(|s| Serial::new(*s))
            .collect();

        p.birth(UtcMillis(1_000), &fleet, &mut sink)
            .expect("four legal serials");

        // AC2 — one NBIRTH, then one DBIRTH per meter, in that order.
        // `tck-id-message-flow-device-birth-publish-nbirth-wait`.
        let kinds: Vec<MessageType> = sink.emitted.iter().map(|o| o.message).collect();
        assert_eq!(
            kinds,
            vec![
                MessageType::NBirth,
                MessageType::DBirth,
                MessageType::DBirth,
                MessageType::DBirth,
                MessageType::DBirth
            ],
            "one node birth, then one device birth per enabled meter, and the node's \
             first: a DBIRTH before the NBIRTH is a device announced under a node the \
             host has not been told about"
        );

        // ...and each on its OWN device topic. Four DBIRTHs on one topic would
        // satisfy the shape assertion above while announcing one device four times.
        let topics: Vec<&str> = sink.emitted[1..].iter().map(|o| o.topic.as_str()).collect();
        for serial in &fleet {
            assert!(
                topics.iter().any(|t| t.ends_with(serial.as_str())),
                "no DBIRTH carried {serial:?}; got {topics:?}"
            );
        }

        // AC3 — the sequence is the NODE's, shared by every device.
        let seqs: Vec<Option<u64>> = sink.emitted.iter().map(|o| decode(o).seq).collect();
        assert_eq!(
            seqs,
            vec![Some(0), Some(1), Some(2), Some(3), Some(4)],
            "the sequence belongs to the NODE, not to a device: four devices born \
             under one node must consume 1,2,3,4 after the NBIRTH's 0"
        );
    }

    /// **Story 3.2 AC3** — on a fleet, each meter's verdict must reach its OWN
    /// device and no other.
    ///
    /// Story 3.2 made a failed poll republish the last known value with a
    /// non-good quality, so for the first time the wire carries **different
    /// qualities for different devices at the same moment**. Guy runs four meters
    /// with one permanently unplugged, so a mis-routed verdict would mark a
    /// working meter stale — or, worse, leave the broken one looking good.
    ///
    /// **The trap this exists for has already sprung twice today.** Story 3.1's
    /// first cadence test counted `[9, 0, 0]` because the shared `reading()`
    /// fixture hard-codes one meter id, so every task's update arrived labelled
    /// "garage". Any assertion about "the right verdict" is worthless if every
    /// message is addressed to the same device — so this test indexes the emitted
    /// messages BY TOPIC and asserts a full map, not a count and not a sample.
    ///
    /// FALSIFIED 2026-08-07 by routing every DDATA to the first declared device —
    /// `let serial = self.declared.keys().next().unwrap().clone();` in place of
    /// reading it from the update. Copied from the run:
    ///
    /// ```text
    /// test adapters::sparkplug_publisher::tests::each_meters_verdict_reaches_its_own_device ... FAILED
    ///
    /// thread '…each_meters_verdict_reaches_its_own_device' (355) panicked at
    /// crates/smartme-bridge/src/adapters/sparkplug_publisher.rs:1162:9:
    /// assertion `left == right` failed: every meter's verdict must land on its own
    /// device: a mis-routed quality marks a working meter stale, or leaves a broken
    /// one looking good
    ///   left: {"30000004": 192}
    ///  right: {"30000001": 192, "30000002": 192, "30000003": 2147484164, "30000004": 192}
    /// ```
    ///
    /// `left` has ONE entry: all four messages landed on one device, and the silent
    /// meter's `Bad_Stale` (2147484164) vanished from the wire entirely. That is
    /// the harm stated in the message, produced rather than argued.
    #[test]
    fn each_meters_verdict_reaches_its_own_device() {
        let mut p = publisher();
        let mut sink = RecordingSink::default();
        let fleet = ["30000001", "30000002", "30000003", "30000004"];
        let serials: Vec<Serial> = fleet.iter().map(|s| Serial::new(*s)).collect();
        p.birth(UtcMillis(1_000), &serials, &mut sink)
            .expect("four legal serials");
        sink.emitted.clear();

        // Three answering, one silent — the shape of Guy's deployment. Each
        // update carries its OWN serial, which is what production does and what
        // the shared fixture would have hidden.
        let verdicts = [
            ("30000001", Quality::Good),
            ("30000002", Quality::Good),
            ("30000003", Quality::Stale),
            ("30000004", Quality::Good),
        ];
        for (serial, published) in verdicts {
            let mut m = measurement(Quality::Good);
            m.serial = Serial::new(serial);
            m.meter = MeterId::new(serial);
            assert_eq!(
                p.publish(
                    &MeterUpdate::uniform(m.meter.clone(), m, verdict_of(published)),
                    &mut sink
                )
                .expect("a declared device"),
                Published::Emitted,
                "the premise: every one of these must actually reach the wire, or \
                 the map below would be asserted over an empty stream"
            );
        }

        // Indexed BY TOPIC, so a message addressed to the wrong device shows up as
        // a wrong map rather than as a right count.
        let seen: std::collections::BTreeMap<String, u32> = sink
            .emitted
            .iter()
            .map(|o| {
                let payload = decode(o);
                (
                    o.topic
                        .rsplit('/')
                        .next()
                        .expect("a device level")
                        .to_string(),
                    quality_of(metric(&payload, METRIC_POWER)),
                )
            })
            .collect();
        let expected: std::collections::BTreeMap<String, u32> = verdicts
            .iter()
            .map(|(s, q)| ((*s).to_string(), ignition_quality_code(*q)))
            .collect();
        assert_eq!(
            seen, expected,
            "every meter's verdict must land on its own device: a mis-routed \
             quality marks a working meter stale, or leaves a broken one looking good"
        );
        assert_eq!(
            sink.emitted.len(),
            4,
            "four meters, four messages: one device published twice would give the \
             right map and the wrong wire"
        );
    }

    #[test]
    fn an_illegal_serial_emits_nothing_at_all() {
        let mut p = publisher();
        let mut sink = RecordingSink::default();
        let err = p
            .birth(
                UtcMillis(1_000),
                &[Serial::new(SERIAL), Serial::new("30/00+1")],
                &mut sink,
            )
            .expect_err("a serial with topic separators must be refused");
        assert!(matches!(err, TopicError::IllegalCharacter { .. }));
        assert!(
            sink.emitted.is_empty(),
            "not even the NBIRTH: a half-declared node is worse than none"
        );
        // ...and the session is untouched, so a corrected retry is a clean birth.
        p.birth(UtcMillis(1_000), &[Serial::new(SERIAL)], &mut sink)
            .expect("the corrected birth succeeds");
        assert_eq!(decode(&sink.emitted[0]).seq, Some(0));
    }

    #[test]
    fn a_rebirth_redeclares_what_is_known_instead_of_blanking_it() {
        let (mut p, mut sink) = born();
        assert_eq!(
            p.publish(&update(Quality::Good), &mut sink).unwrap(),
            Published::Emitted
        );
        sink.emitted.clear();

        // A reconnect re-births the same session's devices.
        p.birth(UtcMillis(2_000), &[Serial::new(SERIAL)], &mut sink)
            .unwrap();
        let dbirth = decode(&sink.emitted[1]);
        let power = metric(&dbirth, METRIC_POWER);
        assert_eq!(
            power.value,
            Some(payload::metric::Value::DoubleValue(0.018)),
            "a transport blip must not blank a tag the bridge can account for"
        );
        assert_eq!(
            quality_of(power),
            ignition_quality_code(Quality::Stale),
            "...but a value that has not been re-judged is never re-asserted as Good"
        );
        assert_eq!(
            dbirth.timestamp,
            Some(1_784_984_792_050),
            "and it is stamped with when it was true, not with now"
        );
    }

    #[test]
    fn the_will_matches_the_session_before_and_after_the_birth() {
        let mut p = publisher();
        let will_before = p.will(UtcMillis(1_000));
        assert_eq!(will_before.topic, "spBv1.0/Site/NDEATH/Bridge");
        assert_eq!(will_before.message, MessageType::NDeath);
        assert_eq!(decode(&will_before).seq, None, "a DEATH is never numbered");

        let mut sink = RecordingSink::default();
        p.birth(UtcMillis(1_000), &[], &mut sink).unwrap();
        let will_after = p.will(UtcMillis(1_000));
        assert_eq!(
            will_after.payload, will_before.payload,
            "the will is the same certificate whether the node has been born or not"
        );
        let nbirth = decode(&sink.emitted[0]);
        assert_eq!(
            decode(&will_before).metrics[0].value,
            nbirth.metrics[0].value
        );
    }

    /// `tck-id-topics-nbirth-bdseq-increment` states TWO obligations — *"The
    /// bdSeq number MUST start at zero and increment by one on every new MQTT
    /// CONNECT packet"* — and this repository honoured only the second until
    /// 2026-08-22 ([#100]). A bridge with an empty state directory published 1.
    ///
    /// **What makes this a test rather than a restatement**: it is written from
    /// the state that produces the fault — no persisted number at all — and it
    /// asserts on the number the WIRE carries, not on the constructor's
    /// argument. Falsified by restoring the sentinel: `SparkplugPublisher::new`
    /// taking `Some(BdSeq::new(0))` here makes the BIRTH carry 1 and this test
    /// go red naming the value it saw.
    #[test]
    fn a_bridge_that_has_never_connected_births_under_bd_seq_zero() {
        let p = SparkplugPublisher::new(node(), None);
        assert_eq!(
            p.bd_seq().value(),
            0,
            "a node with no previous session starts at zero, not past it"
        );

        let will = decode(&p.will(UtcMillis(1_000)));
        let bd_seq_metric = will
            .metrics
            .iter()
            .find(|m| m.name.as_deref() == Some(sparkplug_b::BD_SEQ_METRIC))
            .expect("the will carries bdSeq");
        assert_eq!(
            bd_seq_metric.value,
            Some(payload::metric::Value::LongValue(0)),
            "the number on the wire is the one the clause governs"
        );
    }

    #[test]
    fn a_new_session_advances_the_number_so_an_old_will_cannot_bury_it() {
        let mut p = publisher();
        let first = p.bd_seq();
        p.new_session();
        assert_eq!(p.bd_seq().value(), first.value().wrapping_add(1));
    }

    #[test]
    fn sequence_numbering_is_continuous_across_node_and_device_messages() {
        let mut p = publisher();
        let mut sink = RecordingSink::default();
        p.birth(UtcMillis(1), &[Serial::new(SERIAL)], &mut sink)
            .unwrap();
        assert_eq!(
            p.publish(&update(Quality::Good), &mut sink).unwrap(),
            Published::Emitted
        );
        assert_eq!(
            p.publish(&update(Quality::Good), &mut sink).unwrap(),
            Published::Emitted
        );

        let seqs: Vec<Option<u64>> = sink.emitted.iter().map(|o| decode(o).seq).collect();
        assert_eq!(
            seqs,
            vec![Some(0), Some(1), Some(2), Some(3)],
            "NBIRTH then DBIRTH then DDATA share one edge-node sequence"
        );
    }

    // ================================================================
    // Story 4.12 — anti-replay at the down→up instant (FR22, AR7)
    // ================================================================

    /// **AC3 — which clock each message speaks, per row, with its clause.**
    ///
    /// The invariant lives in two call sites and nothing stopped a third reaching
    /// for the wrong clock. This is `qos_for`'s pattern applied to time: story
    /// 4.17 showed what a table like this is worth — it turned a QoS violation
    /// that had shipped, with a unit test locking it in, from invisible into red.
    ///
    /// **Two of the six rows are deviations, and they are the point.** They are
    /// separated from the conformant ones deliberately: a single list would let a
    /// future edit move a MUST while looking like a preference, which is exactly
    /// what happened to `the_delivery_table_matches_the_specification_clause_by_clause`
    /// on the day it was written.
    ///
    /// FALSIFIED 2026-08-18 — mutation RUN, output copied: answering
    /// `ReadingValueDate` for `NodeBirth` goes red with `NodeBirth is fixed by the
    /// SPECIFICATION, not by us … left: ReadingValueDate, right: PublicationInstant`.
    #[test]
    fn the_timestamp_table_says_which_clock_each_message_speaks() {
        // CONFORMANT. Each row names the clause that fixes it; changing one means
        // the norm changed, and the norm is pinned at `docs/spec/sparkplug-b-3.0.0/`.
        let conformant = [
            // `tck-id-payloads-nbirth-timestamp` (`Sparkplug_6:1064`) — "a payload
            // timestamp that denotes the time at which the message was published".
            Emission::NodeBirth,
            // `tck-id-payloads-dbirth-timestamp` (`Sparkplug_6:1176`), same words.
            Emission::DeviceBirthColdStart,
            // `tck-id-payloads-ddeath-timestamp` (`Sparkplug_6:1582`), same words.
            Emission::DeviceDeath,
        ];
        for emission in conformant {
            assert_eq!(
                timestamp_source_for(emission),
                TimestampSource::PublicationInstant,
                "{emission:?} is fixed by the SPECIFICATION, not by us"
            );
        }

        // OURS, BECAUSE THE NORM IS SILENT. There is no
        // `tck-id-payloads-ndeath-timestamp` anywhere in chapter 6 — NDEATH's
        // clauses govern `seq`, `bdSeq` and the will, and none of them the payload
        // timestamp. DDEATH has one; NDEATH does not. Asserted separately so the
        // asymmetry is on the record rather than discovered again.
        assert_eq!(
            timestamp_source_for(Emission::NodeDeath),
            TimestampSource::PublicationInstant,
            "NDEATH's payload timestamp is unconstrained by the norm and we chose \
             the publication instant"
        );

        // DEVIATIONS, RECORDED — ADR 0013, and both are MUST violations we know
        // about. Stamping `now` here would hand a consumer that reads the payload
        // timestamp and ignores `Quality` a 45-minute-old value wearing a fresh
        // one: conformant, and the exact silent lie this project exists to
        // prevent.
        for emission in [Emission::DeviceData, Emission::DeviceBirthRedeclaring] {
            assert_eq!(
                timestamp_source_for(emission),
                TimestampSource::ReadingValueDate,
                "{emission:?} DEVIATES from the norm on purpose (ADR 0013); moving \
                 this row silently un-decides that ADR"
            );
        }

        // AND THE TWO COLUMNS ARE DISJOINT. Without this, a table answering
        // `ReadingValueDate` for everything would satisfy every deviation
        // assertion above, and a table answering `PublicationInstant` for
        // everything would satisfy every conformant one.
        assert_ne!(
            timestamp_source_for(Emission::DeviceData),
            timestamp_source_for(Emission::NodeBirth),
            "a table with one answer for every row decides nothing"
        );
    }

    /// **AC1 + AC2 — an hour passes, and the rebirth still tells the truth.**
    ///
    /// # The clock advance IS the test
    ///
    /// With a clock that does not move, a publisher stamping `now` and one
    /// stamping `value_date` are indistinguishable — that is the fake clock that
    /// never advanced, one of the four Epic 1 tests this repository had to throw
    /// away. Here the wall clock jumps an hour between the reading and the
    /// rebirth, which is what a broker outage looks like, and the assertion is
    /// that the re-declared payload did NOT follow it.
    ///
    /// The existing `a_rebirth_redeclares_what_is_known_instead_of_blanking_it`
    /// asserts the VALUES survive; nothing asserted their TIME until this story.
    ///
    /// FALSIFIED 2026-08-18 — three mutations RUN, output copied.
    ///
    /// **The first proves the table is a MECHANISM and not a comment**: moving
    /// `DeviceBirthRedeclaring` to `PublicationInstant` changes what is EMITTED —
    /// `left: Some(1784988392050), right: Some(1784984792050)`, exactly one hour
    /// apart, which is the outage this test simulates.
    ///
    /// Dropping `.map(degrade)` from the rebirth metrics goes red with `a reading
    /// not re-judged against now is published stale … left: 192, right:
    /// 2147484164` — 192 being Ignition's `Good`, which is the lie.
    ///
    /// Stamping the DDATA with a fixed instant instead of the reading's time goes
    /// red with `the DDATA payload timestamp IS the source ValueDate (ADR 0013) …
    /// left: Some(9000000000000), right: Some(1784984792050)`.
    #[test]
    fn an_hour_of_outage_does_not_move_the_re_declared_reading_forward() {
        const READING_AT: i64 = 1_784_984_792_050;
        const AN_HOUR_LATER: i64 = READING_AT + 3_600_000;

        let (mut p, mut sink) = born();
        assert_eq!(
            p.publish(&update(Quality::Good), &mut sink).unwrap(),
            Published::Emitted
        );
        // The premise: the DDATA itself carries the reading's own time.
        let ddata = decode(&sink.emitted[0]);
        assert_eq!(
            ddata.timestamp,
            Some(READING_AT as u64),
            "the DDATA payload timestamp IS the source ValueDate (ADR 0013)"
        );
        sink.emitted.clear();

        // The outage. The broker returns an hour later and the session re-births.
        p.new_session();
        p.birth(UtcMillis(AN_HOUR_LATER), &[Serial::new(SERIAL)], &mut sink)
            .expect("valid topics");

        // The NBIRTH speaks NOW — it announces a session, not a measurement.
        let nbirth = decode(&sink.emitted[0]);
        assert_eq!(
            nbirth.timestamp,
            Some(AN_HOUR_LATER as u64),
            "the node birth is an event and carries the instant it happened"
        );

        // And the DBIRTH re-declaring the reading does NOT.
        let dbirth = decode(&sink.emitted[1]);
        assert_eq!(
            dbirth.timestamp,
            Some(READING_AT as u64),
            "a reading re-declared after an hour of outage must still say WHEN IT \
             WAS TRUE. Stamping the reconnection instant turns a 45-minute-old \
             value into a fresh-looking one for any consumer reading the payload \
             timestamp — conformant, and the precise lie ADR 0013 refuses"
        );
        assert_ne!(
            dbirth.timestamp, nbirth.timestamp,
            "if the two agree, the re-declared reading followed the clock"
        );

        // AC2's other half: re-declared, never re-asserted as good.
        let power = metric(&dbirth, METRIC_POWER);
        assert_eq!(
            quality_of(power),
            ignition_quality_code(Quality::Stale),
            "a reading not re-judged against now is published stale"
        );
        assert_eq!(
            cause_of(&dbirth, METRIC_CAUSE_POWER).as_deref(),
            Some("not-revalidated"),
            "and it says WHY it is stale, rather than leaving the host to guess"
        );
        assert_eq!(
            power.timestamp,
            Some(READING_AT as u64),
            "the METRIC timestamp is the acquisition time too — which is the one \
             place the norm actually asks for it"
        );
    }

    /// **AC2 — a reading already refused keeps its own cause across the rebirth.**
    ///
    /// `degrade` touches `Good` and nothing else. Without this, a rebirth would
    /// flatten a `Bad` counter-went-backwards into a generic `Stale`, and an
    /// operator whose meter is lying would be told only that it is old — the
    /// distinction story 2.3 exists for, undone by the reconnection.
    ///
    /// FALSIFIED 2026-08-18 — mutation RUN, output copied: widening `degrade` to
    /// map every quality to `Stale(NotRevalidated)` goes red with `a refused
    /// reading stays refused across a reconnect … left: 2147484164, right:
    /// 2147484160` — the bridge's `Stale` code standing where its `Bad` belongs.
    /// (The note first predicted `500` and `0`; the run said otherwise, and the
    /// run is what is written here.)
    #[test]
    fn a_rebirth_does_not_flatten_a_refusal_into_mere_staleness() {
        let (mut p, mut sink) = born();
        let mut refused = update(Quality::Good);
        refused.verdicts = Verdicts::uniform(Verdict::bad(Cause::CounterWentBackwards));
        assert_eq!(p.publish(&refused, &mut sink).unwrap(), Published::Emitted);
        sink.emitted.clear();

        p.new_session();
        p.birth(
            UtcMillis(9_000_000_000_000),
            &[Serial::new(SERIAL)],
            &mut sink,
        )
        .expect("valid topics");
        let dbirth = decode(&sink.emitted[1]);
        let power = metric(&dbirth, METRIC_POWER);

        assert_eq!(
            quality_of(power),
            ignition_quality_code(Quality::Bad),
            "a refused reading stays refused across a reconnect: degrading it to \
             Stale would tell an operator their meter is merely old"
        );
        assert_eq!(
            cause_of(&dbirth, METRIC_CAUSE_POWER).as_deref(),
            Some("counter-went-backwards"),
            "and it keeps the cause that refused it"
        );
    }
}
