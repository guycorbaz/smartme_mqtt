//! The inter-task message (Story 1.10).
//!
//! Defined BEFORE either task exists so neither can invent its own shape. The
//! poll task decides truth and sends this; the mqtt task transports it and
//! decides nothing. PURE: no tokio, no transport — the channel TYPE is not the
//! channel IMPLEMENTATION.

use crate::domain::{Measurement, MeterId, Quality};

/// One judged reading on its way to the wire.
///
/// The two qualities are deliberately distinct and both travel:
///
/// - `measurement.quality` is what the SOURCE could tell us about the value
///   (Story 1.7: a unit it could not convert arrives `Bad`).
/// - [`MeterUpdate::published`] is the ORACLE's verdict — the effect returned by
///   the state machine, and the one that must be stamped on the wire.
///
/// They differ in exactly the cases that matter: a source-Good value whose
/// timestamps prove it stale is published `Stale`, and collapsing the two here
/// would throw away the distinction the whole state machine exists to compute.
#[derive(Debug, Clone, PartialEq)]
pub struct MeterUpdate {
    /// Which meter this reading belongs to.
    pub meter: MeterId,
    /// The reading itself, carrying the source-level quality.
    pub measurement: Measurement,
    /// The quality to publish — the state machine's verdict.
    pub published: Quality,
}

impl MeterUpdate {
    /// Assembles an update from a judged reading.
    pub fn new(meter: MeterId, measurement: Measurement, published: Quality) -> Self {
        Self {
            meter,
            measurement,
            published,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            Quality::Stale,
        );
        assert_eq!(update.measurement.quality, Quality::Good);
        assert_eq!(update.published, Quality::Stale);
    }
}
