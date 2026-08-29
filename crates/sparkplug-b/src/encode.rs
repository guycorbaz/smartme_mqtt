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
//! let session = NodeSession::start(Some(BdSeq::new(7)));
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
    /// Opens the session that follows `previous`, or the node's FIRST session
    /// when there is none.
    ///
    /// `Some(previous)` is the number the last session used — persist it before
    /// connecting and pass it back here, so a restart continues the sequence
    /// instead of replaying a number a consumer has already seen.
    ///
    /// **`None` means this node has never connected, and its first session is
    /// numbered 0.** `tck-id-topics-nbirth-bdseq-increment`
    /// (`Sparkplug_4_Topics.adoc`) states two obligations — *"The bdSeq number
    /// MUST **start at zero** and increment by one on every new MQTT CONNECT
    /// packet"* — and until 2026-08-22 this crate honoured only the second: the
    /// caller passed a `BdSeq::before_first()` sentinel of 0 and this function
    /// advanced past it, so a brand-new node published 1. The absence of a
    /// previous session cannot be spelled as a number without replaying one, so
    /// it is spelled as the absence it is ([#100], ADR 0042).
    pub const fn start(previous: Option<BdSeq>) -> Self {
        Self {
            bd_seq: match previous {
                Some(previous) => previous.next_session(),
                None => BdSeq::new(0),
            },
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
            metrics: metrics
                .iter()
                .map(|m| encode_metric(m, Datatype::Omitted))
                .collect(),
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
    ///
    /// **It shares the shape of [`Self::data`] and NOT its metric encoding**, and
    /// that difference is the whole of [#28]. A DBIRTH declares each metric's
    /// `datatype` (`tck-id-payloads-metric-datatype-req`, a MUST); a DDATA does
    /// not (`-not-req`, a SHOULD NOT). Until 2026-08-29 this method delegated to
    /// `data`, which was harmless while one encoder served both — and is the one
    /// edit that would silently strip the DBIRTH of the field a consumer learns
    /// the tag set from. `a_device_birth_declares_datatypes_a_device_data_omits`
    /// exists for that edit and nothing else.
    ///
    #[must_use]
    pub fn device_birth(&mut self, timestamp_ms: u64, metrics: Vec<Metric>) -> Payload {
        Payload {
            timestamp: Some(timestamp_ms),
            metrics: metrics
                .iter()
                .map(|m| encode_metric(m, Datatype::Included))
                .collect(),
            seq: Some(self.seq.take()),
            uuid: None,
            body: None,
        }
    }

    /// Gives back the sequence number the last message took, because that
    /// message **never reached the wire**.
    ///
    /// A thin pass-through to [`SeqCounter::give_back`], and its condition of use
    /// is that one, in full: a single message in flight, refused by the
    /// transport. Replaying a number that did reach the wire is worse than the
    /// hole it repairs. See [ADR 0046] for the one call site this exists for.
    ///
    /// [ADR 0046]: ../../../docs/adr/0046-a-publication-is-confirmed-by-the-transport-or-taken-back.md
    pub fn give_back_seq(&mut self) {
        self.seq.give_back();
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
        encoded.extend(metrics.iter().map(|m| encode_metric(m, Datatype::Included)));
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
///
/// **The error is this crate's own** (story 8.2, NFR19). It returned
/// `prost::DecodeError` until 2026-08-20, which meant a consumer that wanted to
/// match on a failure had to depend on `prost` — at the version this crate pins —
/// for ever, and a `prost` major bump broke their code as well as ours. The
/// message is preserved verbatim; what is no longer borrowed is the type.
pub fn decode(bytes: &[u8]) -> Result<Payload, DecodeError> {
    Payload::decode(bytes).map_err(|error| DecodeError {
        detail: error.to_string(),
    })
}

/// Why a byte string could not be read as a Sparkplug payload.
///
/// Opaque on purpose: the reason is a diagnostic for a human or a log, not
/// something to branch on. There is exactly one way to fail — the bytes are not
/// this protobuf — so a variant per cause would be an enum with one arm and a
/// promise of more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    detail: String,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "not a Sparkplug B payload: {}", self.detail)
    }
}

impl std::error::Error for DecodeError {}

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
    encode_metric(
        &Metric::new(
            BD_SEQ_METRIC,
            MetricValue::Int64(bd_seq.wire_value()),
            timestamp_ms,
        ),
        // NBIRTH requires it, and NDEATH is outside the SHOULD NOT's list of
        // four — see [`Datatype`].
        Datatype::Included,
    )
}

/// Whether a metric carries its `datatype`, which is the ONE field a message
/// type decides here.
///
/// The specification asks for opposite things, and names the message types on
/// each side explicitly:
///
/// > *"The datatype MUST be included with each metric definition in NBIRTH and
/// > DBIRTH messages."* — `tck-id-payloads-metric-datatype-req`
///
/// > *"The datatype SHOULD NOT be included with metric definitions in NDATA,
/// > NCMD, DDATA, and DCMD messages."* — `tck-id-payloads-metric-datatype-not-req`
///
/// Until [#28] one encoder served every message type, so a single line satisfied
/// the MUST and violated the SHOULD NOT — on the message that repeats forever,
/// for a field the consumer already learned from the BIRTH.
///
/// **The death payloads keep it, and that is a reading of the list rather than
/// an oversight.** The SHOULD NOT enumerates four message types; NDEATH and
/// DDEATH are in neither clause, and the specification's own NDEATH payload
/// example carries `"dataType": "UInt64"` on its `bdSeq` metric
/// (`Sparkplug_6_Payloads.adoc:1564`). A metric a host reads to reconcile a
/// death with its birth is not the repetition the clause is aimed at.
///
/// **And the specification contradicts itself on the DDATA half**: the very
/// chapter that states the SHOULD NOT prints a DDATA example whose two metrics
/// both carry `"dataType": "Boolean"` (`:1391` and `:1396`). The clause is
/// normative and the example is not, so the clause wins — but a host built
/// against the example is a real possibility, which is why [#28]'s repair is
/// attested against a live host before it ships. See [ADR 0053].
///
/// [ADR 0053]: ../../../docs/adr/0053-the-datatype-leaves-the-data-messages.md
///
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Datatype {
    /// NBIRTH, DBIRTH, NDEATH and DDEATH: the field travels.
    Included,
    /// NDATA, NCMD, DDATA and DCMD — the four the SHOULD NOT names.
    Omitted,
}

fn encode_metric(metric: &Metric, datatype: Datatype) -> payload::Metric {
    let (value, is_null) = match &metric.value {
        MetricValue::Null(_) => (None, Some(true)),
        value => (Some(encode_value(value)), None),
    };
    payload::Metric {
        name: Some(metric.name.clone()),
        alias: None,
        timestamp: Some(metric.timestamp_ms),
        datatype: match datatype {
            Datatype::Included => Some(metric.value.datatype().code()),
            Datatype::Omitted => None,
        },
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
        // A decoder disambiguates Int64 from UInt64 via the `datatype` field —
        // which since [#28] is present in the BIRTH and in neither DATA message,
        // so the disambiguation is the tag set's job, not each message's.
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
    // Caller-supplied properties last, in insertion order: the two the
    // specification and convention name keep their position whatever a caller
    // adds, so a consumer reading by index does not shift under it.
    for (key, value) in &metric.properties {
        keys.push(key.clone());
        values.push(string_property(value));
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

    /// [#30] — the four encoder invariants that were correct by construction and
    /// asserted by nothing.
    ///
    /// # Why "correct by construction" is not enough here
    ///
    /// It is the rule the Epic 1 retrospective bought with contract v1: *148 green
    /// tests all agreed with each other and the wire was wrong*. Each of these
    /// four is a MUST that this encoder happens to satisfy, and nothing in the
    /// tree would notice if it stopped.
    ///
    /// # The four, and where each was blind
    ///
    /// **The array lengths** —
    /// `tck-id-payloads-propertyset-keys-array-size` and `-values-array-size`
    /// (`Sparkplug_6_Payloads.adoc:570` and `:576`): the two arrays MUST hold the
    /// same number of entries. `encode_properties` pushes a key and a value
    /// together in each branch, so they cannot diverge.
    /// `a_birth_is_self_describing` indexes `values[0]` and `values[1]`, which
    /// would panic if values were SHORT — and pass silently if values were LONG.
    ///
    /// **The property types** — `-metric-propertyvalue-type-value` (`:588`) and
    /// `-type-req`, plus `-propertyset-quality-value-type`: a property value's
    /// `type` MUST be present and MUST be one of the enumerated types. The
    /// quality half was asserted against `DataType::Int32.code()`, which is
    /// production's own expression and follows it wherever it goes; the `engUnit`
    /// half was not asserted at all, because `unit_of` matches the VALUE variant
    /// and never the type field. Hence the LITERAL `3` and `12` below, and a walk
    /// over every pair rather than a check on one constructor.
    ///
    /// **The metric timestamp** — `-name-birth-data-requirement` (`:475`): it MUST
    /// be included with every metric in all NBIRTH, DBIRTH, NDATA and DDATA. (The
    /// id says `name`; the clause is about the timestamp — the specification's id
    /// is misleading, not our filing.) Every timestamp assertion in the tree reads
    /// the PAYLOAD level; nothing read the encoded metric's own field.
    ///
    /// **FALSIFIED 2026-08-24, four mutations RUN, each against its own assertion:**
    ///
    /// - `values.push(…)` duplicated in the unit branch — the arrays diverge: RED;
    /// - `r#type` dropped from `string_property`: RED — the mutation the matrix
    ///   recorded as staying green;
    /// - `r#type` dropped from `int_property`: RED — the other half, which
    ///   `-propertyvalue-type-req` was waiting for;
    /// - `timestamp: None` in `encode_metric`: RED.
    ///
    /// `datatype` is NOT covered here and not claimed: `a_birth_is_self_describing`
    /// already pins it.
    #[test]
    fn the_four_encoder_invariants_nothing_was_watching() {
        let mut metric = Metric::new("Power", MetricValue::Double(0.018), 1_784_984_792_050);
        metric.quality_code = Some(Quality::Good.code());
        metric.engineering_unit = Some("kW".to_string());
        metric
            .properties
            .push(("Cause".to_string(), "no-cause".to_string()));

        // Both sides of [`Datatype`]: the clause below covers NBIRTH, DBIRTH,
        // NDATA and DDATA alike, so [#28]'s split must not have moved it.
        for side in [Datatype::Included, Datatype::Omitted] {
            assert_eq!(
                encode_metric(&metric, side).timestamp,
                Some(1_784_984_792_050),
                "-name-birth-data-requirement is a MUST on {side:?} messages too"
            );
        }
        let encoded = encode_metric(&metric, Datatype::Included);

        // 4. The metric-level timestamp, read from the ENCODED metric.
        assert_eq!(
            encoded.timestamp,
            Some(1_784_984_792_050),
            "every metric of every BIRTH and DATA must carry its own timestamp: \
             the payload-level one says when the message was built, not when this \
             number was true"
        );

        let properties = encoded.properties.expect("three properties were set");

        // 1–2. The two arrays, in both directions.
        assert_eq!(
            properties.keys.len(),
            properties.values.len(),
            "a PropertySet whose arrays disagree is unreadable by index, which is \
             the only way a consumer can read it"
        );
        assert_eq!(
            properties.keys.len(),
            3,
            "quality, engineering unit, and the caller's own — the count is stated \
             so a LONGER values array cannot satisfy the equality above by \
             accident"
        );

        // 3. Every property value names its type, and names it correctly.
        for (key, value) in properties.keys.iter().zip(&properties.values) {
            // THE LITERAL CODES, not `DataType::Int32.code()`. This document
            // records that the quality property's `type` was once "asserted by a
            // test that compared production's own expression against itself" —
            // which follows the code wherever it goes and witnesses nothing. `3`
            // and `12` are the specification's numbers
            // (`Sparkplug_6_Payloads.adoc`, the DataType enumeration), so a
            // change to `DataType` has to answer to them.
            let expected = if key == Quality::PROPERTY_KEY { 3 } else { 12 };
            assert_eq!(
                value.r#type,
                Some(expected),
                "property {key:?} must declare its type: a consumer reads the \
                 value variant THROUGH it, and the string properties were asserted \
                 by their variant alone"
            );
        }
    }

    use super::*;
    use crate::datatype::DataType;

    fn energy(value: f64, ts: u64) -> Metric {
        Metric::new("Energy", MetricValue::Double(value), ts)
            .with_quality(Quality::Good)
            .with_engineering_unit("kWh")
    }

    fn live_from(bd: u8) -> (LiveSession, Payload) {
        NodeSession::start(Some(BdSeq::new(bd))).birth(1_000, vec![energy(1.5, 1_000)])
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
        let session = NodeSession::start(Some(BdSeq::new(3)));
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
        // A DEVICE BIRTH, because that is where the declared type now lives
        // ([#28]): the DDATA half of the same claim is the value round trip
        // below, which is message-independent.
        let p = live.device_birth(1_000, vec![energy(precise, 1_000)]);
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

    /// A caller-supplied property reaches the protobuf, with its key, its value
    /// and its type — and it does not displace the two the crate adds itself.
    ///
    /// **ADDED 2026-08-11. Nothing traversed this loop.** Story 2.1 put the
    /// bridge's `Cause` on the wire through `Metric::with_property`, and asserted
    /// it on `metric.properties` — the model's `Vec<(String, String)>`, in
    /// memory, before any encoding happens. `a_birth_is_self_describing` and
    /// `quality_travels_as_a_property_on_every_message` reach the protobuf but
    /// only ever look at quality and `engUnit`, and `model.rs`'s own property
    /// test never calls `with_property`. So the `for (key, value) in
    /// &metric.properties` loop in `encode_properties` could be DELETED and the
    /// entire suite stayed green, while the cause silently stopped reaching any
    /// consumer — the exact failure story 2.1 was written to prevent, one layer
    /// below where it was checked.
    ///
    /// The array-length assertion is not decoration: equal `keys`/`values`
    /// lengths is the one thing the norm actually mandates about a `PropertySet`
    /// (`tck-id-payloads-propertyset-keys-array-size` and
    /// `-values-array-size`, `Sparkplug_6_Payloads.adoc:570,576`), and a loop
    /// that pushed to one vector and not the other would produce a payload no
    /// conformant host must accept.
    ///
    /// FALSIFIED 2026-08-11, three mutations, each red on its own message:
    /// deleting the caller-property loop (the key is absent); pushing the value
    /// as an int rather than a string (the type assertion); and moving the loop
    /// ABOVE the quality/unit pushes (the ordering assertion, which is what
    /// keeps a consumer reading by index from shifting under a new property).
    #[test]
    fn a_caller_supplied_property_reaches_the_wire_beside_the_crate_s_own() {
        let (mut live, _) = live_from(0);
        let refused = Metric::new("Energy", MetricValue::Double(4843.822), 1)
            .with_quality(Quality::Bad)
            .with_engineering_unit("kWh")
            .with_property("Cause", "counter-went-backwards");
        let p = live.data(1, vec![refused]);
        let props = p.metrics[0]
            .properties
            .as_ref()
            .expect("a metric with a caller property has a property set");

        assert_eq!(
            props.keys.len(),
            props.values.len(),
            "tck-id-payloads-propertyset-keys-array-size / -values-array-size: \
             the two arrays MUST be the same length"
        );

        // Ordering is the property `encode_properties` documents: the crate's
        // own two keep their positions whatever a caller adds.
        assert_eq!(
            props.keys,
            vec![
                Quality::PROPERTY_KEY.to_string(),
                Metric::ENG_UNIT_KEY.to_string(),
                "Cause".to_string(),
            ]
        );

        assert_eq!(
            props.values[2].value,
            Some(payload::property_value::Value::StringValue(
                "counter-went-backwards".to_string()
            )),
            "the cause must arrive as its wire string, not as anything a \
             consumer would have to decode"
        );
        assert_eq!(
            props.values[2].r#type,
            Some(crate::datatype::DataType::String.code()),
            "a caller property is typed String on the wire, or a host reads the \
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

    /// A null metric says "no value" in both messages, and declares its type in
    /// only one of them.
    ///
    /// # This is [#28]'s sharpest edge, and it is deliberate
    ///
    /// A null DDATA metric carries a name, `is_null`, its properties and
    /// **nothing else** — no value to infer a type from and no declared type. A
    /// consumer that did not read the DBIRTH cannot tell what kind of tag just
    /// went null. That is precisely the consumer Sparkplug says must not exist:
    /// *"a consumer may discard DATA for a metric the BIRTH never declared"*.
    ///
    /// It matters because a caller that withholds a number rather than shipping
    /// one it does not trust puts exactly this shape on the wire. That is not
    /// hypothetical — it is what [ADR 0053] decided, and what a live host is
    /// asked about before that decision ships.
    ///
    /// [ADR 0053]: ../../../docs/adr/0053-the-datatype-leaves-the-data-messages.md
    ///
    /// **FALSIFIED 2026-08-29, two mutations RUN, one per direction:**
    ///
    /// - `Datatype::Omitted` arm returning `Some(..)` — the DDATA half goes RED
    ///   (`left: Some(10), right: None`), which is the whole of [#28];
    /// - `device_birth` delegating back to `data`, the ORDINARY shape of the
    ///   fault since that is what it did until today — the BIRTH half goes RED
    ///   (`left: None, right: Some(10)`).
    ///
    #[test]
    fn a_null_metric_carries_no_value_and_declares_its_type_only_at_birth() {
        let (mut live, _) = live_from(0);
        let unreadable = || {
            Metric::new("Power", MetricValue::Null(DataType::Double), 1)
                .with_quality(Quality::Bad)
                .with_engineering_unit("kW")
        };

        let birth = live.device_birth(1, vec![unreadable()]);
        let data = live.device_data(1, vec![unreadable()]);

        for (what, p) in [("DBIRTH", &birth), ("DDATA", &data)] {
            let m = &p.metrics[0];
            assert_eq!(m.value, None, "no fabricated number in the {what}");
            assert_eq!(m.is_null, Some(true), "the {what} says so explicitly");
            // And it survives the wire, nulls being the shape a decoder is most
            // likely to drop.
            let decoded = Payload::decode(encode(p).as_slice()).expect("decodes");
            assert_eq!(decoded.metrics[0].is_null, Some(true));
            assert_eq!(decoded.metrics[0].value, None);
        }

        assert_eq!(
            birth.metrics[0].datatype,
            Some(DataType::Double.code()),
            "-metric-datatype-req: the DBIRTH declares the type, and it is the \
             ONLY place a null tag's type is stated"
        );
        assert_eq!(
            data.metrics[0].datatype, None,
            "-metric-datatype-not-req: the DDATA does not repeat it"
        );
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
            assert_eq!(
                decoded.metrics[0].datatype, None,
                "[#28]: a DDATA does not repeat the type"
            );
            // So the disambiguation this round trip depends on — Int64 from
            // UInt64, which share `long_value` — is the BIRTH's job now, and
            // only the BIRTH's. Asserted on the same value, in the same test,
            // because separating them is how the pair drifts apart.
            let born = live.device_birth(1, vec![Metric::new("N", MetricValue::Int64(v), 1)]);
            assert_eq!(born.metrics[0].datatype, Some(DataType::Int64.code()));
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

    /// The `datatype` is present in exactly the messages that declare a tag set,
    /// and absent from exactly the messages that update one ([#28]).
    ///
    /// # Why every builder, and not the two that were repaired
    ///
    /// There are five ways to reach [`encode_metric`], and the fault this guards
    /// against is not "the wrong constant" — it is **one builder quietly sharing
    /// another's body**. `device_birth` did exactly that until 2026-08-29: it
    /// returned `self.data(..)`, which was harmless while one encoder served
    /// both and becomes a stripped DBIRTH the moment they differ. A test that
    /// checked `device_birth` and `device_data` would have passed on the day the
    /// delegation was written and failed only later; a test that walks the whole
    /// set fails the moment any builder borrows another's encoding.
    ///
    /// The BIRTH side is what makes this a discrimination test rather than a
    /// one-directional one: a repair that reached too far — dropping the field
    /// everywhere — would satisfy the SHOULD NOT and violate the MUST, and is
    /// caught here rather than by a live host.
    ///
    /// **FALSIFIED 2026-08-29, three mutations RUN:**
    ///
    /// - `device_birth` delegating to `data` again, the ORDINARY shape (it is
    ///   the shape this file shipped for a month): RED here on the `DBIRTH` row
    ///   (`left: None, right: Some(10)`), and red in three sibling tests too;
    /// - `build_birth` encoding with `Datatype::Omitted` — the repair reaching
    ///   too far: RED here ALONE, on `NBIRTH`, which is what makes this guard
    ///   worth its length: no other test in the crate notices that direction;
    /// - `Datatype::Omitted` returning the code anyway: RED on the DATA rows
    ///   (`left: Some(10), right: None`).
    ///
    #[test]
    fn the_datatype_travels_with_the_declaration_and_with_nothing_else() {
        let declared = Some(DataType::Double.code());
        let one = || vec![energy(4_843.822, 1)];

        let session = NodeSession::start(Some(BdSeq::new(0)));
        // Taken BEFORE the birth, which is the order a caller must use: the will
        // is registered with the CONNECT.
        let will = session.will(1);
        let (mut live, nbirth) = session.birth(1, one());

        // Each row: what it is, the payload, and whether the type must travel.
        let cases: Vec<(&str, Payload, bool)> = vec![
            ("NBIRTH", nbirth, true),
            ("rebirth", live.rebirth(1, one()), true),
            ("DBIRTH", live.device_birth(1, one()), true),
            ("NDATA", live.data(1, one()), false),
            ("DDATA", live.device_data(1, one()), false),
        ];

        for (what, payload, declares) in &cases {
            let m = payload
                .metrics
                .iter()
                .find(|m| m.name.as_deref() == Some("Energy"))
                .unwrap_or_else(|| panic!("{what} carries the metric"));
            assert_eq!(
                m.datatype,
                if *declares { declared } else { None },
                "{what}: -metric-datatype-{} is the clause it answers to",
                if *declares {
                    "req (MUST)"
                } else {
                    "not-req (SHOULD NOT)"
                }
            );
            // The name and timestamp travel either way — so a row going red
            // above is about the datatype and not about a broken builder.
            assert_eq!(m.timestamp, Some(1), "{what} keeps the metric timestamp");
        }

        // And the `bdSeq` metric — carried by the NBIRTH and by the two death
        // certificates, the registered will and the explicit NDEATH — keeps its
        // type in all three: the SHOULD NOT names four message types and no
        // death is among them. (A DDEATH carries no metric at all.)
        for (what, payload) in [
            ("NBIRTH", &cases[0].1),
            ("will", &will),
            ("NDEATH", &live.death(1)),
        ] {
            let bd = payload
                .metrics
                .iter()
                .find(|m| m.name.as_deref() == Some(BD_SEQ_METRIC))
                .unwrap_or_else(|| panic!("{what} carries bdSeq"));
            assert_eq!(
                bd.datatype,
                Some(DataType::Int64.code()),
                "{what}: the metric a host reconciles a death by keeps its type"
            );
        }
    }

    #[test]
    fn payloads_round_trip_through_protobuf() {
        let session = NodeSession::start(Some(BdSeq::new(11)));
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
