//! The inter-task message (Story 1.10).
//!
//! Defined BEFORE either task exists so neither can invent its own shape. The
//! poll task decides truth and sends this; the mqtt task transports it and
//! decides nothing. PURE: no tokio, no transport — the channel TYPE is not the
//! channel IMPLEMENTATION.

use crate::core::oracle::Verdict;
use crate::domain::{Measurement, MeterId, Quality};

/// One judged reading on its way to the wire.
///
/// The two qualities are deliberately distinct and both travel:
///
/// - `measurement.quality` is what the SOURCE could tell us about the value
///   (Story 1.7: a unit it could not convert arrives `Bad`).
/// - [`MeterUpdate::verdict`] is the ORACLE LAYER's composed verdict — the
///   quality that must be stamped on the wire, and, unless it is `Good`, the
///   cause that goes beside it (Story 2.1).
///
/// They differ in exactly the cases that matter: a source-Good value whose
/// timestamps prove it stale is published `Stale`, and collapsing the two here
/// would throw away the distinction the whole state machine exists to compute.
///
/// **The verdict replaced a bare `Quality` in Story 2.1.** A quality alone could
/// say a reading was unusable but never why, so every consumer — the wire, the
/// screens, the log — had to re-derive the reason or do without it. Carrying the
/// cause here means it is decided once, by whoever judged, rather than guessed
/// downstream.
#[derive(Debug, Clone, PartialEq)]
pub struct MeterUpdate {
    /// Which meter this reading belongs to.
    pub meter: MeterId,
    /// The reading itself, carrying the source-level quality.
    pub measurement: Measurement,
    /// What to publish, and why — the composed verdict.
    pub verdict: Verdict,
}

impl MeterUpdate {
    /// Assembles an update from a judged reading.
    pub fn new(meter: MeterId, measurement: Measurement, verdict: Verdict) -> Self {
        Self {
            meter,
            measurement,
            verdict,
        }
    }

    /// The quality to stamp on the wire.
    ///
    /// A convenience over `self.verdict.quality()`, kept because the great
    /// majority of readers want exactly this and nothing else — but the verdict
    /// remains the field, so a reader that needs the cause cannot be handed a
    /// value that has quietly dropped it.
    pub fn published(&self) -> Quality {
        self.verdict.quality()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::oracle::Cause;
    use crate::domain::{Kw, Kwh, Serial, UtcMillis};

    fn measurement(quality: Quality) -> Measurement {
        Measurement {
            meter: MeterId::new("m1"),
            serial: Serial::new("S-1"),
            power: Kw(1.0),
            energy: Kwh(2.0),
            value_date: UtcMillis(1_000),
            quality,
        }
    }

    #[test]
    fn the_two_qualities_stay_distinct() {
        // A source-Good reading the oracle judged Stale: both facts survive.
        let update = MeterUpdate::new(
            MeterId::new("m1"),
            measurement(Quality::Good),
            Verdict::stale(Cause::ReadingTooOld),
        );
        assert_eq!(update.measurement.quality, Quality::Good);
        assert_eq!(update.published(), Quality::Stale);
        // Story 2.1: and the reason survives alongside, which a bare quality could
        // not carry — this is the whole difference the verdict makes here.
        assert_eq!(update.verdict.cause(), Some(Cause::ReadingTooOld));
    }
}
