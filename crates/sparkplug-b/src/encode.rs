//! Payload builders and the session lifecycle (BIRTH / DATA / DEATH).
//!
//! The session types own the numbering invariants so a caller cannot get them
//! wrong. The order is enforced by the type system, not by documentation:
//! [`NodeSession`] can only produce a will and a BIRTH; the BIRTH consumes it
//! and yields a [`LiveSession`], which is the only thing that can produce DATA.
//! A DATA message before a BIRTH — which would carry `seq = 0` and be
//! indistinguishable from a BIRTH on the wire — is unrepresentable.
//!
//! Nothing here does I/O or reads a clock: timestamps and the persisted
//! [`BdSeq`] come from the caller, which keeps the whole lifecycle testable as a
//! pure function of its inputs.
//!
//! ```
//! use sparkplug_b::{BdSeq, Metric, MetricValue, NodeSession, Quality};
//!
//! let session = NodeSession::start(BdSeq::new(7));
//! // The will is registered with the broker BEFORE connecting...
//! let will = session.will(1_000);
//! // ...then the BIRTH opens the session and unlocks DATA.
//! let (mut live, birth) = session.birth(1_000, vec![]);
//! let data = live.data(2_000, vec![]);
//!
//! assert_eq!(birth.seq, Some(0));
//! assert_eq!(data.seq, Some(1));
//! assert_eq!(will.seq, None);
//! ```

use prost::Message;

use crate::model::{Metric, MetricValue, Quality};
use crate::protobuf::{Payload, payload};
use crate::seq::{BD_SEQ_METRIC, BdSeq, SeqCounter};

/// A session that has not yet published its BIRTH.
///
/// Its number is chosen at construction and never reused: [`NodeSession::start`]
/// is the only constructor, and it always advances past the previous session.
/// Reusing a number would let a broker deliver an old will against the live
/// session — the node would be marked dead while it is publishing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSession {
    bd_seq: BdSeq,
}

/// A session that has published its BIRTH and may now publish DATA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    bd_seq: BdSeq,
    seq: SeqCounter,
}

impl NodeSession {
    /// Opens the session that follows `previous`.
    ///
    /// `previous` is the number the last session used — persist it before
    /// connecting and pass it back here, so a restart continues the sequence
    /// instead of replaying a number a consumer has already seen. A brand-new
    /// node passes [`BdSeq::before_first`].
    pub const fn start(previous: BdSeq) -> Self {
        Self {
            bd_seq: previous.next_session(),
        }
    }

    /// This session's number — persist it before connecting.
    pub const fn bd_seq(&self) -> BdSeq {
        self.bd_seq
    }

    /// The DEATH payload to register as the connection's last will, BEFORE
    /// connecting. See [`LiveSession::death`] for what the timestamp means.
    #[must_use]
    pub fn will(&self, timestamp_ms: u64) -> Payload {
        death_payload(self.bd_seq, timestamp_ms)
    }

    /// Publishes the BIRTH and opens the session for DATA.
    ///
    /// The BIRTH carries `seq = 0` (which is what tells a consumer the
    /// numbering restarted) and the [`BdSeq`] metric, so the birth and the
    /// matching death can be paired.
    ///
    /// The `metrics` should declare EVERY metric the session will later
    /// publish, with names, engineering units and quality: a BIRTH is what lets
    /// a consumer discover the tag set instead of being configured with it, and
    /// a consumer may discard DATA for a metric the BIRTH never declared.
    #[must_use]
    pub fn birth(self, timestamp_ms: u64, metrics: Vec<Metric>) -> (LiveSession, Payload) {
        let mut live = LiveSession {
            bd_seq: self.bd_seq,
            seq: SeqCounter::new(),
        };
        let payload = live.build_birth(timestamp_ms, metrics);
        (live, payload)
    }
}

impl LiveSession {
    /// This session's number.
    pub const fn bd_seq(&self) -> BdSeq {
        self.bd_seq
    }

    /// The sequence number the next message will carry, without consuming it.
    pub const fn peek_seq(&self) -> u64 {
        self.seq.peek()
    }

    /// A DATA message carrying the next sequence number.
    ///
    /// Messages must be published in construction order on one connection: the
    /// number is allocated here, so publishing out of order shows a consumer a
    /// gap that did not happen.
    #[must_use]
    pub fn data(&mut self, timestamp_ms: u64, metrics: Vec<Metric>) -> Payload {
        Payload {
            timestamp: Some(timestamp_ms),
            metrics: metrics.iter().map(encode_metric).collect(),
            seq: Some(self.seq.take()),
            uuid: None,
            body: None,
        }
    }

    /// Re-publishes the BIRTH on the same session (a consumer asked to
    /// resynchronise, or the transport reconnected without a new session):
    /// restarts the numbering at 0 and re-declares the tag set.
    #[must_use]
    pub fn rebirth(&mut self, timestamp_ms: u64, metrics: Vec<Metric>) -> Payload {
        self.build_birth(timestamp_ms, metrics)
    }

    /// A device BIRTH: declares one device's tag set. It takes the next
    /// sequence number like any other message — the sequence is per EDGE NODE
    /// and shared by node and device messages, so a consumer sees one
    /// uninterrupted numbering across both.
    ///
    /// Unlike a node BIRTH it does NOT reset the numbering and carries no
    /// `bdSeq`: the session it belongs to is the node's.
    #[must_use]
    pub fn device_birth(&mut self, timestamp_ms: u64, metrics: Vec<Metric>) -> Payload {
        self.data(timestamp_ms, metrics)
    }

    /// A device DATA message.
    #[must_use]
    pub fn device_data(&mut self, timestamp_ms: u64, metrics: Vec<Metric>) -> Payload {
        self.data(timestamp_ms, metrics)
    }

    /// A device DEATH: says THIS device is gone while the node stays alive —
    /// the granularity a node-level death cannot express.
    #[must_use]
    pub fn device_death(&mut self, timestamp_ms: u64) -> Payload {
        Payload {
            timestamp: Some(timestamp_ms),
            metrics: Vec::new(),
            seq: Some(self.seq.take()),
            uuid: None,
            body: None,
        }
    }

    /// The DEATH payload for this session — identical to the will registered
    /// before connecting, for the case where the node dies politely and
    /// publishes it itself.
    ///
    /// `timestamp_ms` is the moment the payload is BUILT, not the moment of
    /// death: when the broker publishes a registered will it does not rewrite
    /// the payload, so a consumer must treat the death's timestamp as "no later
    /// than this" rather than as the instant the node stopped.
    #[must_use]
    pub fn death(&self, timestamp_ms: u64) -> Payload {
        death_payload(self.bd_seq, timestamp_ms)
    }

    fn build_birth(&mut self, timestamp_ms: u64, metrics: Vec<Metric>) -> Payload {
        self.seq.reset();
        let mut encoded = Vec::with_capacity(metrics.len() + 1);
        encoded.push(bd_seq_metric(self.bd_seq, timestamp_ms));
        encoded.extend(metrics.iter().map(encode_metric));
        Payload {
            timestamp: Some(timestamp_ms),
            metrics: encoded,
            seq: Some(self.seq.take()),
            uuid: None,
            body: None,
        }
    }
}

/// Serialises a payload to protobuf bytes, ready to publish.
///
/// The payload type is `prost`-generated and re-exported from
/// [`crate::protobuf`]: `prost` is therefore a PUBLIC dependency of this crate,
/// and a major `prost` bump is a breaking change here too.
#[must_use]
pub fn encode(payload: &Payload) -> Vec<u8> {
    payload.encode_to_vec()
}

/// Parses protobuf bytes back into a payload.
///
/// Provided so a consumer — a test asserting what was actually put on the wire,
/// or a subscriber — does not have to depend on `prost` directly just to look
/// at a payload this crate produced.
pub fn decode(bytes: &[u8]) -> Result<Payload, prost::DecodeError> {
    Payload::decode(bytes)
}

/// A DEATH payload: the session's number, and deliberately NO sequence number —
/// the broker publishes it at a moment the node cannot number, and a fabricated
/// number would corrupt a consumer's gap detection.
fn death_payload(bd_seq: BdSeq, timestamp_ms: u64) -> Payload {
    Payload {
        timestamp: Some(timestamp_ms),
        metrics: vec![bd_seq_metric(bd_seq, timestamp_ms)],
        seq: None,
        uuid: None,
        body: None,
    }
}

/// The `bdSeq` metric carried by every BIRTH and DEATH.
fn bd_seq_metric(bd_seq: BdSeq, timestamp_ms: u64) -> payload::Metric {
    encode_metric(&Metric::new(
        BD_SEQ_METRIC,
        MetricValue::Int64(bd_seq.wire_value()),
        timestamp_ms,
    ))
}

fn encode_metric(metric: &Metric) -> payload::Metric {
    let (value, is_null) = match &metric.value {
        MetricValue::Null(_) => (None, Some(true)),
        value => (Some(encode_value(value)), None),
    };
    payload::Metric {
        name: Some(metric.name.clone()),
        alias: None,
        timestamp: Some(metric.timestamp_ms),
        datatype: Some(metric.value.datatype().code()),
        is_historical: None,
        is_transient: None,
        is_null,
        metadata: None,
        properties: encode_properties(metric),
        value,
    }
}

fn encode_value(value: &MetricValue) -> payload::metric::Value {
    match value {
        // `long_value` is a protobuf `uint64`; a signed 64-bit metric travels as
        // its two's-complement bit pattern, which is exactly what this cast is.
        // A decoder disambiguates Int64 from UInt64 via the `datatype` field.
        MetricValue::Int64(v) => payload::metric::Value::LongValue(*v as u64),
        MetricValue::UInt64(v) => payload::metric::Value::LongValue(*v),
        // Always the 64-bit field: a counter encoded as float32 loses digits a
        // consumer cannot recover, and cannot tell it lost them.
        MetricValue::Double(v) => payload::metric::Value::DoubleValue(*v),
        MetricValue::Boolean(v) => payload::metric::Value::BooleanValue(*v),
        MetricValue::String(v) => payload::metric::Value::StringValue(v.clone()),
        // Handled by the caller: a null metric carries no value at all.
        MetricValue::Null(_) => unreachable!("null values are encoded as is_null"),
    }
}

/// Builds the property set from whichever self-describing properties are set.
/// Returns `None` when there are none — an empty property set on the wire says
/// something different from no property set at all.
fn encode_properties(metric: &Metric) -> Option<payload::PropertySet> {
    let mut keys = Vec::new();
    let mut values = Vec::new();
    if let Some(code) = metric.quality_code {
        keys.push(Quality::PROPERTY_KEY.to_string());
        values.push(int_property(code));
    }
    if let Some(unit) = &metric.engineering_unit {
        keys.push(Metric::ENG_UNIT_KEY.to_string());
        values.push(string_property(unit));
    }
    if keys.is_empty() {
        return None;
    }
    Some(payload::PropertySet { keys, values })
}

fn int_property(value: u32) -> payload::PropertyValue {
    payload::PropertyValue {
        r#type: Some(crate::datatype::DataType::Int32.code()),
        is_null: None,
        value: Some(payload::property_value::Value::IntValue(value)),
    }
}

fn string_property(value: &str) -> payload::PropertyValue {
    payload::PropertyValue {
        r#type: Some(crate::datatype::DataType::String.code()),
        is_null: None,
        value: Some(payload::property_value::Value::StringValue(
            value.to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datatype::DataType;

    fn energy(value: f64, ts: u64) -> Metric {
        Metric::new("Energy", MetricValue::Double(value), ts)
            .with_quality(Quality::Good)
            .with_engineering_unit("kWh")
    }

    fn live_from(bd: u8) -> (LiveSession, Payload) {
        NodeSession::start(BdSeq::new(bd)).birth(1_000, vec![energy(1.5, 1_000)])
    }

    #[test]
    fn birth_carries_seq_zero_and_the_session_number() {
        let (live, p) = live_from(7);
        assert_eq!(p.seq, Some(0), "a BIRTH must restart the numbering");
        assert_eq!(live.bd_seq(), BdSeq::new(8));
        let bd = &p.metrics[0];
        assert_eq!(bd.name.as_deref(), Some(BD_SEQ_METRIC));
        assert_eq!(bd.value, Some(payload::metric::Value::LongValue(8)));
        assert_eq!(bd.datatype, Some(DataType::Int64.code()));
    }

    #[test]
    fn rebirth_resets_the_numbering_after_data() {
        let (mut live, _) = live_from(0);
        let _ = live.data(2, vec![]);
        let _ = live.data(3, vec![]);
        assert_eq!(live.peek_seq(), 3);
        let rebirth = live.rebirth(4, vec![]);
        assert_eq!(rebirth.seq, Some(0));
        assert_eq!(live.peek_seq(), 1);
    }

    #[test]
    fn data_messages_take_consecutive_sequence_numbers() {
        let (mut live, _) = live_from(0);
        assert_eq!(live.data(2, vec![]).seq, Some(1));
        assert_eq!(live.data(3, vec![]).seq, Some(2));
        assert_eq!(live.data(4, vec![]).seq, Some(3));
    }

    #[test]
    fn the_will_matches_the_birth_and_carries_no_sequence() {
        let session = NodeSession::start(BdSeq::new(3));
        let will = session.will(500);
        let (live, birth) = session.birth(1_000, vec![]);
        let death = live.death(2_000);

        assert_eq!(will.seq, None, "a DEATH must not be numbered");
        assert_eq!(death.seq, None);
        assert_eq!(will.metrics.len(), 1, "a DEATH carries only bdSeq");
        assert_eq!(will.metrics[0].value, birth.metrics[0].value);
        assert_eq!(will.metrics[0].value, death.metrics[0].value);
    }

    #[test]
    fn a_counter_is_encoded_as_double_never_float32() {
        let (mut live, _) = live_from(0);
        // A value that f32 cannot represent exactly.
        let precise = 40_437.819_123_456_79_f64;
        let p = live.data(1_000, vec![energy(precise, 1_000)]);
        let m = &p.metrics[0];
        assert_eq!(m.datatype, Some(DataType::Double.code()));
        match m.value {
            Some(payload::metric::Value::DoubleValue(v)) => assert_eq!(v, precise),
            ref other => panic!("expected a double value, got {other:?}"),
        }
        // Round-tripping through the wire must not round it.
        let decoded = Payload::decode(encode(&p).as_slice()).expect("decodes");
        match decoded.metrics[0].value {
            Some(payload::metric::Value::DoubleValue(v)) => {
                assert_eq!(v, precise, "full resolution survives the wire")
            }
            ref other => panic!("expected a double value, got {other:?}"),
        }
    }

    #[test]
    fn a_birth_is_self_describing() {
        let (_, p) = live_from(0);
        let m = p
            .metrics
            .iter()
            .find(|m| m.name.as_deref() == Some("Energy"))
            .expect("the metric is named");
        let props = m.properties.as_ref().expect("properties present");
        assert_eq!(
            props.keys,
            vec![
                Quality::PROPERTY_KEY.to_string(),
                Metric::ENG_UNIT_KEY.to_string()
            ]
        );
        assert_eq!(
            props.values[0].value,
            Some(payload::property_value::Value::IntValue(192))
        );
        assert_eq!(
            props.values[1].value,
            Some(payload::property_value::Value::StringValue(
                "kWh".to_string()
            ))
        );
    }

    #[test]
    fn quality_travels_as_a_property_on_every_message() {
        let (mut live, _) = live_from(0);
        let stale = Metric::new("Power", MetricValue::Double(0.0), 1)
            .with_quality(Quality::Stale)
            .with_engineering_unit("kW");
        let p = live.data(1, vec![stale]);
        let props = p.metrics[0].properties.as_ref().expect("properties");
        // Deliberately expressed through the enum rather than a literal: the
        // exact bit pattern is pinned once, next to the enum, by the test that
        // records what a real host was measured to honour. Repeating it here
        // would just be a second place to get it wrong.
        assert_eq!(
            props.values[0].value,
            Some(payload::property_value::Value::IntValue(
                Quality::Stale.code()
            ))
        );
        assert_eq!(
            props.values[0].r#type,
            Some(crate::datatype::DataType::Int32.code()),
            "quality must be typed Int32 on the wire, or a consumer reads the \
             field as something else"
        );
    }

    #[test]
    fn a_metric_without_properties_omits_the_property_set() {
        let (mut live, _) = live_from(0);
        let bare = Metric::new("Raw", MetricValue::Int64(1), 1);
        let p = live.data(1, vec![bare]);
        assert!(p.metrics[0].properties.is_none());
    }

    #[test]
    fn a_null_metric_carries_no_value_but_keeps_its_datatype() {
        let (mut live, _) = live_from(0);
        let unreadable = Metric::new("Power", MetricValue::Null(DataType::Double), 1)
            .with_quality(Quality::Bad)
            .with_engineering_unit("kW");
        let p = live.data(1, vec![unreadable]);
        let m = &p.metrics[0];
        assert_eq!(m.value, None, "no fabricated number");
        assert_eq!(m.is_null, Some(true));
        assert_eq!(
            m.datatype,
            Some(DataType::Double.code()),
            "the tag keeps its declared type"
        );
        // And it survives the wire.
        let decoded = Payload::decode(encode(&p).as_slice()).expect("decodes");
        assert_eq!(decoded.metrics[0].is_null, Some(true));
        assert_eq!(decoded.metrics[0].value, None);
    }

    #[test]
    fn negative_signed_metrics_round_trip_through_the_unsigned_field() {
        let (mut live, _) = live_from(0);
        for v in [-1_i64, i64::MIN, i64::MAX, 0] {
            let p = live.data(1, vec![Metric::new("N", MetricValue::Int64(v), 1)]);
            let decoded = Payload::decode(encode(&p).as_slice()).expect("decodes");
            match decoded.metrics[0].value {
                Some(payload::metric::Value::LongValue(raw)) => {
                    assert_eq!(raw as i64, v, "two's-complement round trip for {v}")
                }
                ref other => panic!("expected a long value, got {other:?}"),
            }
            assert_eq!(decoded.metrics[0].datatype, Some(DataType::Int64.code()));
        }
    }

    #[test]
    fn non_finite_doubles_are_passed_through_bit_exactly() {
        // The crate does not editorialise about values: NaN and infinity encode
        // and decode bit-identically. (Note that `Payload`'s derived `PartialEq`
        // is float equality, so a NaN payload is never `==` itself — compare
        // bits, as here, not payloads.)
        let (mut live, _) = live_from(0);
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let p = live.data(1, vec![Metric::new("X", MetricValue::Double(v), 1)]);
            let decoded = Payload::decode(encode(&p).as_slice()).expect("decodes");
            match decoded.metrics[0].value {
                Some(payload::metric::Value::DoubleValue(got)) => {
                    assert_eq!(got.to_bits(), v.to_bits(), "bit-exact passthrough")
                }
                ref other => panic!("expected a double value, got {other:?}"),
            }
        }
    }

    #[test]
    fn device_messages_share_the_edge_node_numbering() {
        let (mut live, birth) = live_from(0);
        assert_eq!(birth.seq, Some(0));
        // Node and device messages draw from ONE counter: a consumer must see
        // an uninterrupted sequence across both.
        assert_eq!(live.device_birth(1, vec![]).seq, Some(1));
        assert_eq!(live.data(2, vec![]).seq, Some(2));
        assert_eq!(live.device_data(3, vec![]).seq, Some(3));
        assert_eq!(live.device_death(4).seq, Some(4));
        assert_eq!(live.data(5, vec![]).seq, Some(5));
    }

    #[test]
    fn a_device_death_carries_no_bdseq() {
        let (mut live, _) = live_from(0);
        let d = live.device_death(1);
        assert!(
            d.metrics.is_empty(),
            "the session number belongs to the node, not the device"
        );
    }

    #[test]
    fn payloads_round_trip_through_protobuf() {
        let session = NodeSession::start(BdSeq::new(11));
        let (_, p) = session.birth(
            1_700_000_000_000,
            vec![energy(4_843.822, 1_700_000_000_000)],
        );
        let bytes = encode(&p);
        let decoded = Payload::decode(bytes.as_slice()).expect("valid protobuf");
        assert_eq!(decoded, p);
        assert_eq!(decoded.timestamp, Some(1_700_000_000_000));
    }
}
