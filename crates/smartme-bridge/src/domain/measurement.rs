//! Canonical `Measurement` and unit-carrying newtypes (Story 1.2).
//!
//! No serial, topic, or physical quantity is ever a bare `String`/`f64` (AR14): the
//! unit and the identity travel in the type, so the compiler rejects a mix-up. No
//! validation lives here — well-formedness oracles are Epic 2; the private-field
//! string newtypes funnel every construction through `new()`, the one place that
//! validation will land.
//! This module is PURE — enforced by `tests/arch_purity.rs`.

use std::fmt;
use std::ops::Sub;

use super::quality::Quality;

/// Active power in kilowatts.
///
/// The inner value is public: adapters and tests read `.0`. Deliberately no
/// `From<f64>` — an implicit conversion would hide the unit at the call site.
/// Finiteness is NOT checked here: the source adapter (Story 1.7) rejects
/// non-finite values fail-closed before a `Kw` ever exists — `PartialEq` on `f64`
/// is bitwise, so a NaN would make a reading unequal to itself. A bare `f64` is
/// rejected wherever `Kw` is expected:
///
/// ```compile_fail,E0308
/// fn publish(power: smartme_bridge::domain::Kw) {}
/// publish(1.5); // no unit carried — does not compile
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kw(pub f64);

/// Cumulative energy in kilowatt-hours. Same design as [`Kw`]: public inner value,
/// no `From<f64>`, finiteness owned by the source adapter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kwh(pub f64);

/// UTC wall-clock instant as epoch-milliseconds (AR15: `i64` epoch-millis in
/// Sparkplug payloads, never local time). ISO-8601 rendering is a wire/UI concern
/// of later epics, not domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UtcMillis(pub i64);

/// Signed difference in milliseconds. Story 1.5 computes
/// `age = http_date − value_date`, which is negative when the source's clock runs
/// ahead of ours (`age < 0` → STALE) — hence `i64`, not a duration type. Saturates
/// instead of wrapping: a garbage timestamp near `i64::MIN`/`MAX` from the external
/// API must not flip the sign the STALE guard reads.
impl Sub for UtcMillis {
    type Output = i64;

    fn sub(self, rhs: Self) -> i64 {
        self.0.saturating_sub(rhs.0)
    }
}

macro_rules! string_newtype {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            /// Wraps the raw string. No well-formedness check yet — that is Epic
            /// 2/5 work. The field stays private so every construction funnels
            /// through here: validation lands in exactly one place when it comes.
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            /// The underlying string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_newtype!(
    /// Logical meter identity (the configuration key). `Eq + Hash` because Story
    /// 1.10 keys per-meter state by it; its channel message carries
    /// `(MeterId, Measurement, Quality)`.
    MeterId
);

string_newtype!(
    /// Physical device serial as reported by the meter; Story 1.9 keys the
    /// Sparkplug device by it.
    Serial
);

string_newtype!(
    /// An MQTT topic path — never a bare `String`, so a serial or an id cannot be
    /// published as a topic by accident.
    TopicPath
);

/// The canonical reading: one meter, one instant, quality always explicit — never a
/// substituted value (no `Default`, no `serde`; nothing persists a `Measurement`).
///
/// `Eq` is impossible here: [`Kw`] wraps an `f64`.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// Logical meter this reading belongs to.
    pub meter: MeterId,
    /// Physical device serial reported with the reading.
    pub serial: Serial,
    /// Active power at `value_date`.
    pub power: Kw,
    /// Cumulative energy counter at `value_date`.
    pub energy: Kwh,
    /// When the meter says the values were true (source timestamp, UTC).
    pub value_date: UtcMillis,
    /// Explicit quality decision for this reading.
    pub quality: Quality,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_newtypes_round_trip() {
        let m = MeterId::new("garage");
        assert_eq!(m.as_str(), "garage");
        assert_eq!(m.to_string(), "garage");

        let s = Serial::new(String::from("S-123"));
        assert_eq!(s.as_str(), "S-123");
        assert_eq!(s.to_string(), "S-123");

        let t = TopicPath::new("site/meters/garage");
        assert_eq!(t.as_str(), "site/meters/garage");
        assert_eq!(t.to_string(), "site/meters/garage");
    }

    #[test]
    fn identifier_newtypes_are_map_keys() {
        let mut by_serial = std::collections::HashMap::new();
        by_serial.insert(Serial::new("S-123"), 1u32);
        assert_eq!(by_serial.get(&Serial::new("S-123")), Some(&1));
        assert_eq!(by_serial.get(&Serial::new("S-999")), None);
    }

    #[test]
    fn utc_millis_subtraction_is_signed() {
        assert_eq!(UtcMillis(2_000) - UtcMillis(500), 1_500);
        assert_eq!(UtcMillis(1_000) - UtcMillis(1_000), 0);
        // http_date behind value_date → negative age (Story 1.5: age < 0 → STALE)
        assert_eq!(UtcMillis(500) - UtcMillis(2_000), -1_500);
        // saturates: a wrap would flip the sign the STALE guard reads
        assert_eq!(UtcMillis(i64::MIN) - UtcMillis(1), i64::MIN);
        assert_eq!(UtcMillis(i64::MAX) - UtcMillis(-1), i64::MAX);
    }

    #[test]
    fn measurement_carries_typed_fields() {
        let m = Measurement {
            meter: MeterId::new("garage"),
            serial: Serial::new("S-123"),
            power: Kw(1.2),
            energy: Kwh(34.5),
            value_date: UtcMillis(1_700_000_000_000),
            quality: Quality::Good,
        };
        assert_eq!(m.power, Kw(1.2));
        assert_eq!(m.energy, Kwh(34.5));
        assert_eq!(m.clone(), m);
    }
}
