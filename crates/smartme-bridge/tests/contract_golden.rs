//! AR16's guard: the published contract cannot move without its version moving
//! (Story 2.1).
//!
//! # What this exists to stop
//!
//! `CONTRACT_VERSION` is published in the node BIRTH and the Tier-3 runbook
//! indexes its run table by it, on one promise: **two runs sharing a version
//! number attest to the same tag set**. Until this file, nothing enforced that.
//! The number was maintained by whoever remembered, and AR16 had asked for
//! exactly this test since the architecture was written.
//!
//! The Epic 5 retrospective (2026-08-10) named the pattern this closes: the
//! repository repairs instances by hand and numbers its own recurrences, while
//! the two things it gave a *mechanism* — `arch_purity` and `reconfigure::
//! classify`'s exhaustive destructure — never recurred. Story 2.1 defines the
//! first oracle→quality mapping; building it without its guard would have been
//! the same mistake with a fresh subject.
//!
//! # How it works
//!
//! The golden below is the contract **as of a version number**. The test rebuilds
//! the live mapping and compares. Three ways to fail, and each says which:
//!
//! - a mapping entry changed while `CONTRACT_VERSION` did not → the diff names it;
//! - a cause was added or removed → the count no longer matches;
//! - `CONTRACT_VERSION` moved with no golden written for it → an explicit refusal,
//!   because a version nobody pinned protects nothing.
//!
//! # What it deliberately does NOT cover
//!
//! Metric *values*, timestamps, topics and sequence numbers. Those are covered by
//! the encoder's own tests and by the chaos suite. This file is about the
//! vocabulary a consumer has to understand — quality codes and cause strings —
//! which is the part that changes meaning silently.

use smartme_bridge::adapters::sparkplug_publisher::{CONTRACT_VERSION, ignition_quality_code};
use smartme_bridge::core::oracle::Cause;
use smartme_bridge::domain::Quality;

/// The quality-code half of a golden: each quality and the integer published for it.
type QualityGolden = &'static [(Quality, u32)];
/// The cause half: each cause's wire string, and which side of the latch/degrade
/// rule it sits on.
type CauseGolden = &'static [(&'static str, bool)];

/// The quality codes, as of contract v4. Deviates from the specification's
/// `0/192/500` on purpose — see ADR 0012.
const GOLDEN_QUALITY_V4: QualityGolden = &[
    (Quality::Good, 192),
    (Quality::Stale, 0x8000_0000 | 516),
    (Quality::Bad, 0x8000_0000 | 512),
];

/// The cause vocabulary, as of contract v4. Order is `Cause::ALL`'s order.
const GOLDEN_CAUSES_V4: CauseGolden = &[
    // (wire string, latches)
    ("source-unreachable", false),
    ("source-refused", true),
    ("host-clock-unsynced", false),
    ("no-freshness-proof", false),
    ("source-clock-implausible", false),
    ("timestamps-disagree", false),
    ("reading-too-old", false),
    ("value-unusable", false),
    ("source-marked-stale", false),
    ("not-revalidated", false),
];

/// The cause vocabulary, as of contract v5: v4 plus `counter-went-backwards`
/// (Story 2.2). Additive — nothing a v4 consumer understood changed meaning.
const GOLDEN_CAUSES_V5: CauseGolden = &[
    ("source-unreachable", false),
    ("source-refused", true),
    ("host-clock-unsynced", false),
    ("no-freshness-proof", false),
    ("source-clock-implausible", false),
    ("timestamps-disagree", false),
    ("reading-too-old", false),
    ("value-unusable", false),
    ("source-marked-stale", false),
    ("not-revalidated", false),
    ("counter-went-backwards", false),
];

/// The one place a version is bound to its golden. Adding a version without its
/// golden is what the `None` arm refuses.
fn golden_for(version: i64) -> Option<(QualityGolden, CauseGolden)> {
    match version {
        // The quality codes have not moved since v4; the cause vocabulary has.
        5 => Some((GOLDEN_QUALITY_V4, GOLDEN_CAUSES_V5)),
        4 => Some((GOLDEN_QUALITY_V4, GOLDEN_CAUSES_V4)),
        _ => None,
    }
}

#[test]
fn the_published_contract_matches_its_version() {
    let Some((golden_quality, golden_causes)) = golden_for(CONTRACT_VERSION) else {
        panic!(
            "CONTRACT_VERSION is {CONTRACT_VERSION} and no golden is written for it.\n\
             \n\
             A version number nobody pinned protects nothing: the Tier-3 runbook indexes its\n\
             run table by this number on the promise that two runs sharing it attest to the\n\
             same tag set. Add a GOLDEN_*_V{CONTRACT_VERSION} pair above and list it in\n\
             `golden_for`, recording what this version means."
        );
    };

    // --- quality codes -------------------------------------------------------
    for (quality, expected) in golden_quality {
        let actual = ignition_quality_code(*quality);
        assert_eq!(
            actual, *expected,
            "the code for {quality:?} moved from {expected} to {actual} without \
             CONTRACT_VERSION moving.\n\
             A consumer that learned this vocabulary under v{CONTRACT_VERSION} would now \
             read a different meaning from the same number — which is the failure ADR 0012 \
             was written about, arriving from the other direction."
        );
    }

    // --- cause vocabulary ----------------------------------------------------
    assert_eq!(
        Cause::ALL.len(),
        golden_causes.len(),
        "the cause vocabulary changed size ({} live, {} in the v{CONTRACT_VERSION} golden) \
         without CONTRACT_VERSION moving.\n\
         A cause is a string a consumer reads off a degraded metric; adding or removing one \
         changes what v{CONTRACT_VERSION} means.",
        Cause::ALL.len(),
        golden_causes.len()
    );

    for (cause, (wire, latches)) in Cause::ALL.iter().zip(golden_causes) {
        assert_eq!(
            cause.as_str(),
            *wire,
            "{cause:?} publishes {:?} but v{CONTRACT_VERSION} promised {wire:?}",
            cause.as_str()
        );
        assert_eq!(
            cause.latches(),
            *latches,
            "{cause:?} changed side of the latch/degrade rule without a version bump.\n\
             That is not a cosmetic change: a latching cause takes the meter off the wire \
             until a restart, and a degrading one does not."
        );
    }
}

/// The half a golden test usually forgets: that it can pass for the right reason.
///
/// A guard only ever demonstrated red proves it can fail, not that it can pass —
/// which is how a test that fails for an unrelated reason gets read as working.
/// This asserts the shape of the binding itself rather than any particular value.
#[test]
fn a_version_without_a_golden_is_refused_and_one_with_a_golden_is_not() {
    assert!(
        golden_for(CONTRACT_VERSION).is_some(),
        "the shipped version must have a golden"
    );
    assert!(
        golden_for(CONTRACT_VERSION + 1).is_none(),
        "an unwritten version must be refused rather than silently accepted"
    );
    assert!(
        golden_for(3).is_none(),
        "v3 predates this guard and has no golden; it must not appear to have one"
    );
}
