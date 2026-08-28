//! The inter-task message (Story 1.10).
//!
//! Defined BEFORE either task exists so neither can invent its own shape. The
//! poll task decides truth and sends this; the mqtt task transports it and
//! decides nothing. PURE: no tokio, no transport — the channel TYPE is not the
//! channel IMPLEMENTATION.

use crate::core::oracle::{Measured, Verdict, Verdicts};
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
    ///
    /// **Read by production code, and it was not always so** ([#66], finding 3).
    /// The 2026-08-09 review found this field used only by tests — the publisher
    /// routes on `measurement.serial` and the driver traced the same — and asked
    /// whether it should earn a reader or go, on the reasoning that a field which
    /// looks like a safety check but is read by nothing is the worst of the three
    /// outcomes.
    ///
    /// It earned one. `mqtt_driver` counts a lost reading against **this** name —
    /// `count_loss(&pulse, &update.meter, &loss)` — and carries it on the single
    /// warn line that reports the loss. That is deliberate: the loss counter is
    /// per meter as an operator names it, not per serial, so it survives a meter
    /// being replaced and lines up with the tags they are looking at. Deleting the
    /// field would silently break both.
    pub meter: MeterId,
    /// The reading itself, carrying the source-level quality.
    pub measurement: Measurement,
    /// What to publish on each metric, and what the meter as a whole is worth.
    ///
    /// **Was a single `Verdict` until Story 2.3.** One verdict per reading meant
    /// an oracle that judged only the energy index nulled the power value beside
    /// it and labelled it with the energy's cause — so a fault in one number
    /// withheld another the bridge had no complaint about.
    pub verdicts: Verdicts,
}

impl MeterUpdate {
    /// Assembles an update from a judged reading.
    pub fn new(meter: MeterId, measurement: Measurement, verdicts: Verdicts) -> Self {
        Self {
            meter,
            measurement,
            verdicts,
        }
    }

    /// Assembles an update whose every metric carries the same verdict.
    ///
    /// For the paths where one judgement genuinely covers the whole reading: a
    /// failed fetch, a republication, a cold start. Naming it rather than letting
    /// callers reach for [`Verdicts::uniform`] keeps the *deliberate* uniform
    /// cases distinguishable from a per-metric one that lost its detail.
    pub fn uniform(meter: MeterId, measurement: Measurement, verdict: Verdict) -> Self {
        Self::new(meter, measurement, Verdicts::uniform(verdict))
    }

    /// What the METER is worth — the worst verdict across its metrics.
    ///
    /// This is what a latch decision, `/healthz` and every operator screen must
    /// read. A per-metric verdict answers *"can I use this number"*; this answers
    /// *"is this meter telling me the truth"*, and the second question is the one
    /// a status page is asking.
    pub fn verdict(&self) -> Verdict {
        self.verdicts.meter()
    }

    /// The quality to stamp on the wire for `metric`.
    pub fn published_for(&self, metric: Measured) -> Quality {
        self.verdicts.for_metric(metric).quality()
    }

    /// The meter-level quality — what a screen shows beside the meter's name.
    pub fn published(&self) -> Quality {
        self.verdicts.meter().quality()
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
            power: Some(Kw(1.0)),
            energy: Some(Kwh(2.0)),
            value_date: UtcMillis(1_000),
            quality,
        }
    }

    #[test]
    fn the_two_qualities_stay_distinct() {
        // A source-Good reading the oracle judged Stale: both facts survive.
        let update = MeterUpdate::uniform(
            MeterId::new("m1"),
            measurement(Quality::Good),
            Verdict::stale(Cause::ReadingTooOld),
        );
        assert_eq!(update.measurement.quality, Quality::Good);
        assert_eq!(update.published(), Quality::Stale);
        // Story 2.1: and the reason survives alongside, which a bare quality could
        // not carry — this is the whole difference the verdict makes here.
        assert_eq!(update.verdict().cause(), Some(Cause::ReadingTooOld));
    }
}
