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

    /// The conventional numeric quality code recognised by SCADA consumers
    /// (the OPC-style triple): `192` good, `500` stale/uncertain, `0` bad.
    ///
    /// Encoding this HERE — beside the enum, in the crate that owns the wire
    /// format — is deliberate: a mapping invented per-consumer is exactly the
    /// drift a single definition exists to prevent.
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
    /// Quality, published as a metric property. `None` omits the property
    /// entirely (a consumer then applies its own default) — prefer being
    /// explicit.
    pub quality: Option<Quality>,
    /// Engineering unit (for example `"kW"`), published under
    /// [`Metric::ENG_UNIT_KEY`] so a consumer can auto-discover what the number
    /// means instead of hard-coding it.
    pub engineering_unit: Option<String>,
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
            quality: None,
            engineering_unit: None,
        }
    }

    /// Attaches the quality property.
    #[must_use]
    pub fn with_quality(mut self, quality: Quality) -> Self {
        self.quality = Some(quality);
        self
    }

    /// Attaches the engineering-unit property, making the metric
    /// self-describing.
    #[must_use]
    pub fn with_engineering_unit(mut self, unit: impl Into<String>) -> Self {
        self.engineering_unit = Some(unit.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_codes_are_the_conventional_triple() {
        assert_eq!(Quality::Good.code(), 192);
        assert_eq!(Quality::Stale.code(), 500);
        assert_eq!(Quality::Bad.code(), 0);
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
        assert_eq!(m.quality, Some(Quality::Good));
        assert_eq!(m.engineering_unit.as_deref(), Some("kWh"));
        assert_eq!(m.timestamp_ms, 1_000);
    }
}
