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
pub const CONTRACT_VERSION: i64 = 9;

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
pub const METRIC_PROPERTY_CAUSE: &str = "Cause";
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
    /// Opens the session that follows `previous_bd_seq` (restored from storage).
    pub fn new(node: EdgeNode, previous_bd_seq: BdSeq) -> Self {
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
        self.session = Session::Pending(NodeSession::start(previous));
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
            let payload_ts = match &known {
                Some(update) => millis(update.measurement.value_date),
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
    let build = |metric: Measured, name: &'static str, unit: &'static str, value: Option<f64>| {
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
        let verdict = match (verdicts.for_metric(metric), value) {
            (v, None) if v.quality() != Quality::Bad => Verdict::bad(Cause::ValueUnusable),
            (v, _) => v,
        };
        let published = verdict.quality();
        let carried = match (published, value) {
            // `Bad` withholds the number. That is the point of `Bad` rather than
            // `Stale`: a consumer must not be handed a value it would compute
            // with, and the datatype is kept so the tag does not change shape.
            (Quality::Bad, _) => MetricValue::Null(DataType::Double),
            (_, None) => MetricValue::Null(DataType::Double),
            (_, Some(value)) => MetricValue::Double(value),
        };
        let built = Metric::new(name, carried, timestamp)
            .with_quality_code(ignition_quality_code(published))
            .with_engineering_unit(unit);
        match verdict.cause() {
            Some(cause) => built.with_property(METRIC_PROPERTY_CAUSE, cause.as_str()),
            // A good metric carries no cause, by construction. Publishing an
            // empty one would be noise a consumer must learn to ignore, and the
            // day it meant something nobody would notice.
            None => built,
        }
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
    ]
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
    fn a_non_good_metric_names_its_cause_and_a_good_one_does_not() {
        let m = super::super::sparkplug_publisher::tests::measurement(super::Quality::Good);

        let degraded =
            super::metrics_for(&m, Verdicts::uniform(Verdict::stale(Cause::ReadingTooOld)));
        for metric in &degraded {
            assert_eq!(
                metric.properties,
                vec![(
                    super::METRIC_PROPERTY_CAUSE.to_string(),
                    "reading-too-old".to_string()
                )],
                "{} must name why it is not good",
                metric.name
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
            refused[0].properties,
            vec![(
                super::METRIC_PROPERTY_CAUSE.to_string(),
                "source-refused".to_string()
            )]
        );

        let good = super::metrics_for(&m, Verdicts::uniform(Verdict::good()));
        for metric in &good {
            assert!(
                metric.properties.is_empty(),
                "{} is good and must carry no cause",
                metric.name
            );
        }
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
        assert_eq!(
            power.properties,
            vec![(
                super::METRIC_PROPERTY_CAUSE.to_string(),
                "value-unusable".to_string()
            )],
            "and a non-good metric names its cause, here the one that means \
             exactly `not one usable number`"
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
        assert_eq!(
            energy.properties,
            vec![(
                super::METRIC_PROPERTY_CAUSE.to_string(),
                "counter-went-backwards".to_string()
            )]
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
        assert!(
            power.properties.is_empty(),
            "a good metric carries no cause — least of all its neighbour's, which              is what the pre-2.3 wire did: `Power = null`, cause              `counter-went-backwards`, for a number the bridge had no complaint              about"
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
        SparkplugPublisher::new(node(), BdSeq::before_first())
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
}
