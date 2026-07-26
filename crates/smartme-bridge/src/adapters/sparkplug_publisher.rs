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
use crate::domain::{Measurement, Serial, UtcMillis};

/// Version of the topic/metric contract with the SCADA host. Bump on ANY change
/// to the topic grammar, to a metric name or unit, or to the meaning of a
/// published quality code.
///
/// - **2** — quality codes corrected. Version 1 published `500` for stale and
///   `0` for bad; a Tier-3 run showed a real host reading both as *good*, so a
///   v1 consumer was being told an unusable value was trustworthy. See
///   `sparkplug_b::Quality::code`.
/// - **1** — initial contract.
pub const CONTRACT_VERSION: i64 = 2;

/// Metric under which [`CONTRACT_VERSION`] is published in the node BIRTH.
pub const METRIC_CONTRACT_VERSION: &str = "Contract/Version";
/// Metric name for instantaneous power.
pub const METRIC_POWER: &str = "Power";
/// Engineering unit published with [`METRIC_POWER`].
pub const UNIT_POWER: &str = "kW";
/// Metric name for the cumulative energy counter.
pub const METRIC_ENERGY: &str = "Energy";
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
                let (live, payload) = pending.birth(timestamp, vec![contract_metric(timestamp)]);
                sink.emit(Outbound {
                    topic: node_topic(&self.node, MessageType::NBirth),
                    payload: encode(&payload),
                    message: MessageType::NBirth,
                });
                live
            }
            Session::Live(mut live) => {
                let payload = live.rebirth(timestamp, vec![contract_metric(timestamp)]);
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
                Some(update) => metrics_for(&update.measurement, degrade(update.published)),
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
        let payload = live.device_data(
            timestamp,
            metrics_for(&update.measurement, update.published),
        );
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
    .with_quality(Quality::Good)
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
        .with_quality(Quality::Stale)
        .with_engineering_unit(UNIT_POWER),
        Metric::new(
            METRIC_ENERGY,
            MetricValue::Null(DataType::Double),
            timestamp_ms,
        )
        .with_quality(Quality::Stale)
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
fn metrics_for(measurement: &Measurement, published: Quality) -> Vec<Metric> {
    let timestamp = millis(measurement.value_date);
    let (power, energy) = match published {
        Quality::Bad => (
            MetricValue::Null(DataType::Double),
            MetricValue::Null(DataType::Double),
        ),
        _ => (
            MetricValue::Double(measurement.power.0),
            MetricValue::Double(measurement.energy.0),
        ),
    };
    vec![
        Metric::new(METRIC_POWER, power, timestamp)
            .with_quality(published)
            .with_engineering_unit(UNIT_POWER),
        Metric::new(METRIC_ENERGY, energy, timestamp)
            .with_quality(published)
            .with_engineering_unit(UNIT_ENERGY),
    ]
}

/// A verdict that has not been re-computed cannot be re-asserted: `Good`
/// becomes `Stale`, and anything already worse stays as bad as it was.
fn degrade(published: Quality) -> Quality {
    match published {
        Quality::Good => Quality::Stale,
        other => other,
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
            power: Kw(0.018),
            energy: Kwh(4_843.822),
            value_date: UtcMillis(1_784_984_792_050),
            quality,
        }
    }

    fn update(published: Quality) -> MeterUpdate {
        MeterUpdate::new(
            MeterId::new("garage"),
            measurement(Quality::Good),
            published,
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
                Quality::Stale.code(),
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
        assert_eq!(quality_of(m), Quality::Good.code());
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
        assert_eq!(quality_of(power), Quality::Good.code());
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
                Quality::Stale.code(),
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
            assert_eq!(quality_of(m), Quality::Bad.code());
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
            Quality::Stale.code(),
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
