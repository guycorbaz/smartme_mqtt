//! Hand-written Sparkplug model types, complementing the generated protobuf bindings.
//!
//! [`Quality`] (Story 1.2) plus the metric model that the payload builders encode
//! (Story 1.8). These types are the crate's vocabulary: a caller describes WHAT it
//! wants to publish, and [`mod@crate::encode`] turns that into spec-shaped protobuf.

use crate::datatype::DataType;

/// Quality of a metric value carried in a Sparkplug payload.
///
/// One canonical, three-state classification:
///
/// - [`Good`](Quality::Good) — the value is fresh and trusted.
/// - [`Stale`](Quality::Stale) — the value was valid once but is no longer current
///   (for example, the source stopped updating); consumers must not act on it as
///   live data.
/// - [`Bad`](Quality::Bad) — the value could not be read or failed validation and
///   must not be used.
///
/// Deliberately no `Default`: a quality is always an explicit decision, never a
/// silently substituted value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Fresh, trusted value.
    Good,
    /// Previously valid value that is no longer current.
    Stale,
    /// Unusable value: read failure or failed validation.
    Bad,
}

impl Quality {
    /// The property key under which quality travels on the wire.
    ///
    /// A metric's quality is carried as a metric property, not as a value: the
    /// value stays the value, and consumers that understand this key learn
    /// whether they may act on it.
    pub const PROPERTY_KEY: &'static str = "Quality";

    /// The **specification-mandated** quality code, published under
    /// [`Self::PROPERTY_KEY`] with wire type `Int32`.
    ///
    /// Sparkplug B v3.0.0 admits exactly three values
    /// (`tck-id-payloads-propertyset-quality-value-value`):
    ///
    /// > The 'value' of the Property Value MUST be an int_value and be one of
    /// > the valid quality codes of **0, 192, or 500**.
    ///
    /// # When a host does not honour them
    ///
    /// Some host implementations classify the quality property by their own
    /// encoding rather than by this enumeration, and read `500` or `0` as
    /// *good*. Against such a host, publishing the specified codes is
    /// conformant and still misleading — an unusable value is displayed as
    /// trustworthy.
    ///
    /// This crate does not resolve that conflict, because it cannot: the right
    /// answer depends on the consumer, and a generic library that silently
    /// emitted one supplier's encoding while claiming to implement the
    /// specification would be the worst of both. Use
    /// [`Metric::with_quality_code`] to publish a host-specific value, at the
    /// call site, where the deviation is visible and can be justified.
    pub const fn code(self) -> u32 {
        match self {
            Quality::Good => 192,
            Quality::Stale => 500,
            Quality::Bad => 0,
        }
    }
}

/// A typed metric value. The variant determines the [`DataType`] on the wire, so
/// a value can never be tagged with a type it does not have.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MetricValue {
    /// 64-bit signed integer. Transported in the same wire field as
    /// [`MetricValue::UInt64`]; a decoder tells them apart by the metric's
    /// declared data type.
    Int64(i64),
    /// 64-bit unsigned integer.
    UInt64(u64),
    /// 64-bit float — the only float variant, on purpose: see
    /// [`MetricValue::datatype`].
    Double(f64),
    /// Boolean.
    Boolean(bool),
    /// UTF-8 string.
    String(String),
    /// No value at all, while still declaring what type the metric IS.
    ///
    /// This is what an unreadable sensor should publish: the encoder marks the
    /// metric null and omits the value entirely, so a consumer cannot mistake a
    /// placeholder number for a reading. Publishing `0.0` with a bad quality
    /// flag instead means every consumer that ignores the flag — and there is
    /// always one — records a real-looking zero.
    Null(DataType),
}

impl MetricValue {
    /// The Sparkplug data type this value encodes as.
    ///
    /// There is deliberately no 32-bit float variant: a cumulative counter that
    /// has run for years needs more than `f32`'s ~7 significant digits, and a
    /// silently-rounded counter is a lie a consumer cannot detect. Callers that
    /// hold an `f32` widen it explicitly.
    pub const fn datatype(&self) -> DataType {
        match self {
            MetricValue::Int64(_) => DataType::Int64,
            MetricValue::UInt64(_) => DataType::UInt64,
            MetricValue::Double(_) => DataType::Double,
            MetricValue::Boolean(_) => DataType::Boolean,
            MetricValue::String(_) => DataType::String,
            // A null metric still declares the type the tag would have.
            MetricValue::Null(datatype) => *datatype,
        }
    }
}

/// One metric to publish: a name, a typed value, its acquisition timestamp, and
/// the optional self-describing properties a consumer needs to interpret it.
#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    /// Metric name (the consumer-visible tag name).
    pub name: String,
    /// The typed value.
    pub value: MetricValue,
    /// Acquisition time, milliseconds since the UNIX epoch — when the value was
    /// TRUE, not when it was sent.
    pub timestamp_ms: u64,
    /// The quality code published as a metric property, or `None` to omit the
    /// property entirely (a consumer then applies its own default) — prefer
    /// being explicit.
    ///
    /// Stored as the raw code rather than as a [`Quality`], because a caller
    /// may have to publish a host-specific value the enumeration cannot name.
    /// See [`Metric::with_quality`] and [`Metric::with_quality_code`].
    pub quality_code: Option<u32>,
    /// Engineering unit (for example `"kW"`), published under
    /// [`Metric::ENG_UNIT_KEY`] so a consumer can auto-discover what the number
    /// means instead of hard-coding it.
    pub engineering_unit: Option<String>,
    /// Additional string-valued properties, in insertion order.
    ///
    /// Sparkplug's `PropertySet` is open: beyond the keys the specification names
    /// (`Quality`) and the ones convention supplies (`engUnit`), a publisher may
    /// attach whatever a consumer needs, subject only to keys and values being
    /// equal in number (`tck-id-payloads-propertyset-keys-value` and
    /// `-values-value`). This is the door to that, so a caller does not have to
    /// choose between the specification's vocabulary and its own.
    ///
    /// Deliberately NOT a map: the wire is an ordered pair of arrays, and a map
    /// would make the encoded bytes depend on a hash seed — which a golden test
    /// would then have to tolerate or a consumer diff would show as churn.
    pub properties: Vec<(String, String)>,
}

impl Metric {
    /// The property key conventionally used for engineering units.
    pub const ENG_UNIT_KEY: &'static str = "engUnit";

    /// A metric with a name, value and acquisition timestamp; no properties yet.
    pub fn new(name: impl Into<String>, value: MetricValue, timestamp_ms: u64) -> Self {
        Self {
            name: name.into(),
            value,
            timestamp_ms,
            quality_code: None,
            engineering_unit: None,
            properties: Vec::new(),
        }
    }

    /// Attaches the quality property using the **specification-mandated** code
    /// for `quality` — see [`Quality::code`].
    ///
    /// This is the right call unless you know your consumer does not honour
    /// those codes.
    #[must_use]
    pub fn with_quality(mut self, quality: Quality) -> Self {
        self.quality_code = Some(quality.code());
        self
    }

    /// Attaches the quality property using a **raw, host-specific** code.
    ///
    /// Sparkplug B admits only `0`, `192` and `500`
    /// (`tck-id-payloads-propertyset-quality-value-value`), so any other value
    /// is a deliberate deviation from the specification. It exists because some
    /// hosts classify quality by their own encoding and read the specified
    /// codes as *good*, which turns a conformant publication into a silent lie
    /// about the data.
    ///
    /// Deviating may well be the honest choice for a given deployment — but it
    /// is a choice, and it belongs at the call site with a comment saying which
    /// host required it, not hidden inside this crate.
    #[must_use]
    pub fn with_quality_code(mut self, code: u32) -> Self {
        self.quality_code = Some(code);
        self
    }

    /// Attaches the engineering-unit property, making the metric
    /// self-describing.
    #[must_use]
    pub fn with_engineering_unit(mut self, unit: impl Into<String>) -> Self {
        self.engineering_unit = Some(unit.into());
        self
    }

    /// Attaches an arbitrary string-valued property.
    ///
    /// Appends, so calling it twice with the same key publishes the key twice —
    /// which the specification permits and no consumer expects. The caller owns
    /// its key space; this crate does not deduplicate, because silently dropping
    /// a property a caller asked for is worse than publishing what they said.
    #[must_use]
    pub fn with_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.properties.push((key.into(), value.into()));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three codes the specification admits, and nothing else
    /// (`tck-id-payloads-propertyset-quality-value-value`).
    #[test]
    fn quality_codes_are_the_three_the_specification_admits() {
        assert_eq!(Quality::Good.code(), 192);
        assert_eq!(Quality::Stale.code(), 500);
        assert_eq!(Quality::Bad.code(), 0);
    }

    /// A host-specific code is expressible, and does not go through the enum —
    /// deviating is possible, but only by saying so.
    #[test]
    fn a_host_specific_code_bypasses_the_enumeration() {
        let m = Metric::new("x", MetricValue::Double(1.0), 1).with_quality_code(0x8000_0204);
        assert_eq!(m.quality_code, Some(0x8000_0204));
        let spec = Metric::new("x", MetricValue::Double(1.0), 1).with_quality(Quality::Stale);
        assert_eq!(spec.quality_code, Some(500));
    }

    #[test]
    fn value_variants_pin_their_datatype() {
        assert_eq!(MetricValue::Int64(1).datatype(), DataType::Int64);
        assert_eq!(MetricValue::UInt64(1).datatype(), DataType::UInt64);
        assert_eq!(MetricValue::Boolean(true).datatype(), DataType::Boolean);
        assert_eq!(
            MetricValue::String("x".to_string()).datatype(),
            DataType::String
        );
    }

    #[test]
    fn a_float_value_is_always_double_never_float32() {
        // The resolution guarantee: a long-running counter must not be rounded.
        assert_eq!(MetricValue::Double(1.0).datatype(), DataType::Double);
        assert_ne!(MetricValue::Double(1.0).datatype(), DataType::Float);
    }

    #[test]
    fn a_null_value_still_declares_its_type() {
        assert_eq!(
            MetricValue::Null(DataType::Double).datatype(),
            DataType::Double
        );
        assert_eq!(
            MetricValue::Null(DataType::Int64).datatype(),
            DataType::Int64
        );
    }

    #[test]
    fn builders_attach_self_describing_properties() {
        let m = Metric::new("Energy", MetricValue::Double(42.5), 1_000)
            .with_quality(Quality::Good)
            .with_engineering_unit("kWh");
        assert_eq!(m.quality_code, Some(Quality::Good.code()));
        assert_eq!(m.engineering_unit.as_deref(), Some("kWh"));
        assert_eq!(m.timestamp_ms, 1_000);
    }
}
