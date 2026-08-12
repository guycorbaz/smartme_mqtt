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
//! the encoder's own tests and by the chaos suite. This file is about what a
//! consumer has to understand to read the wire — the names it binds tags to, the
//! quality codes, and the cause vocabulary — which is the part that changes
//! meaning silently.
//!
//! # FALSIFICATION
//!
//! The repository's rule is that a test asserting an invariant must have been run
//! against deliberately broken code and seen to fail, recorded next to the test.
//! Story 2.1 ran three mutations and recorded them in the story file only; the
//! 2026-08-11 review called that out, and this is the record moved to where
//! someone editing the file will read it.
//!
//! **2026-08-10, on the file as first written** — three red, one deliberate
//! green: moving a quality code without a version bump; adding a cause without
//! one; and `CONTRACT_VERSION` advanced past its golden, which must panic with
//! the explicit refusal rather than silently pass.
//!
//! **2026-08-11, on the three things the review found unpinned** — each red on
//! its own, each naming the right thing:
//!
//! - `METRIC_PROPERTY_CAUSE` renamed `"Cause"` → `"Reason"`. Before this run the
//!   whole suite stayed green while every consumer's tag binding broke — and it
//!   is the change v4 was struck for in the first place. Now: `left: "Reason",
//!   right: "Cause"`.
//! - `Cause::CounterWentBackwards`'s published quality moved `Bad` → `Stale`,
//!   i.e. the bridge starts handing over the very number story 2.2 exists to
//!   withhold. Now: `left: Stale, right: Bad`.
//! - a cause appended to `Cause` and to `successor`/`as_str` but not to
//!   `Cause::ALL` — the mutation the old `every_cause_is_in_all` blessed. Caught
//!   in `oracle.rs`, before this file is even reached.
//!
//! What none of these can catch is a change that moves the golden and the code
//! together in one edit. Nothing in a repository can; that is what review is for.

use smartme_bridge::adapters::sparkplug_publisher::{
    CONTRACT_VERSION, METRIC_ENERGY, METRIC_NODE_CONTROL_REBIRTH, METRIC_POWER,
    METRIC_PROPERTY_CAUSE, UNIT_ENERGY, UNIT_POWER, ignition_quality_code,
};
use smartme_bridge::core::oracle::Cause;
use smartme_bridge::domain::Quality;

/// The quality-code half of a golden: each quality and the integer published for it.
type QualityGolden = &'static [(Quality, u32)];
/// The cause half: each cause's wire string, which side of the latch/degrade rule
/// it sits on, and **the quality it publishes**.
type CauseGolden = &'static [(&'static str, bool, Quality)];
/// The names a consumer binds tags to: metric names, engineering units, and the
/// property key a cause travels under.
type NameGolden = &'static [(&'static str, &'static str)];

/// The quality codes, as of contract v4. Deviates from the specification's
/// `0/192/500` on purpose — see ADR 0012.
const GOLDEN_QUALITY_V4: QualityGolden = &[
    (Quality::Good, 192),
    (Quality::Stale, 0x8000_0000 | 516),
    (Quality::Bad, 0x8000_0000 | 512),
];

/// The quality codes, as of contract v5.
///
/// **Written out rather than aliased to [`GOLDEN_QUALITY_V4`] (2026-08-11).**
/// The two were the same `const` shared by reference, which meant that editing
/// it to accommodate a future change would silently rewrite what v4 attested to.
/// A golden is a historical record: v4's row must keep saying what v4 said even
/// after nobody runs v4 any more, so each version owns its own copy and the
/// duplication is the point.
const GOLDEN_QUALITY_V5: QualityGolden = &[
    (Quality::Good, 192),
    (Quality::Stale, 0x8000_0000 | 516),
    (Quality::Bad, 0x8000_0000 | 512),
];

/// The cause vocabulary, as of contract v4. Order is `Cause::ALL`'s order.
const GOLDEN_CAUSES_V4: CauseGolden = &[
    // (wire string, latches, published quality)
    ("source-unreachable", false, Quality::Stale),
    ("source-refused", true, Quality::Bad),
    ("host-clock-unsynced", false, Quality::Stale),
    ("no-freshness-proof", false, Quality::Stale),
    ("source-clock-implausible", false, Quality::Stale),
    ("timestamps-disagree", false, Quality::Stale),
    ("reading-too-old", false, Quality::Stale),
    ("value-unusable", false, Quality::Bad),
    ("source-marked-stale", false, Quality::Stale),
    ("not-revalidated", false, Quality::Stale),
];

/// The quality codes, as of contract v7. Its own copy, again: a golden is a
/// historical record, and sharing one array between two versions means editing v6
/// retroactively rewrites what v6 attested to.
const GOLDEN_QUALITY_V7: QualityGolden = &[
    (Quality::Good, 192),
    (Quality::Stale, 0x8000_0000 | 516),
    (Quality::Bad, 0x8000_0000 | 512),
];

/// The cause vocabulary, as of contract v7: v6 plus the four story 2.5 added.
///
/// **Additive.** Nothing a v6 consumer understood changed meaning; four strings
/// joined the vocabulary. What they replace is not a cause but an ABSENCE of one —
/// `value-unusable` used to be published for an unrecognised unit, a non-finite
/// number, an arithmetic overflow AND an unparseable timestamp, four faults
/// repaired in four different places under one word that named no field.
///
/// `value-unusable` survives with its meaning narrowed to exactly one case: not
/// one usable number in the whole reading. A v6 consumer keyed on it still sees it,
/// less often and more precisely.
const GOLDEN_CAUSES_V7: CauseGolden = &[
    ("source-unreachable", false, Quality::Stale),
    ("source-refused", true, Quality::Bad),
    ("host-clock-unsynced", false, Quality::Stale),
    ("no-freshness-proof", false, Quality::Stale),
    ("source-clock-implausible", false, Quality::Stale),
    ("timestamps-disagree", false, Quality::Stale),
    ("reading-too-old", false, Quality::Stale),
    ("value-unusable", false, Quality::Bad),
    ("source-marked-stale", false, Quality::Stale),
    ("not-revalidated", false, Quality::Stale),
    ("counter-went-backwards", false, Quality::Bad),
    ("unit-not-recognised", false, Quality::Bad),
    ("value-not-finite", false, Quality::Bad),
    ("value-overflowed", false, Quality::Bad),
    ("source-timestamp-unparseable", false, Quality::Bad),
];

/// The names and units, as of v7. Unchanged, and its own copy.
const GOLDEN_NAMES_V7: NameGolden = &[
    ("metric.power", "Power"),
    ("metric.energy", "Energy"),
    ("metric.rebirth", "Node Control/Rebirth"),
    ("unit.power", "kW"),
    ("unit.energy", "kWh"),
    ("property.cause", "Cause"),
];

/// The quality codes, as of contract v6. Its own copy — see [`GOLDEN_QUALITY_V5`].
const GOLDEN_QUALITY_V6: QualityGolden = &[
    (Quality::Good, 192),
    (Quality::Stale, 0x8000_0000 | 516),
    (Quality::Bad, 0x8000_0000 | 512),
];

/// The cause vocabulary, as of contract v6.
///
/// **Identical to v5, and that is the point of writing it out.** Story 2.3 does
/// not touch the vocabulary: it changes WHICH METRIC a cause lands on, which no
/// list of strings can express. A reader comparing v5 and v6 here sees no
/// difference and must go to `CONTRACT_VERSION`'s own doc to find out what moved
/// — so that doc carries the explanation, and this file carries the proof that
/// nothing else moved with it.
const GOLDEN_CAUSES_V6: CauseGolden = &[
    ("source-unreachable", false, Quality::Stale),
    ("source-refused", true, Quality::Bad),
    ("host-clock-unsynced", false, Quality::Stale),
    ("no-freshness-proof", false, Quality::Stale),
    ("source-clock-implausible", false, Quality::Stale),
    ("timestamps-disagree", false, Quality::Stale),
    ("reading-too-old", false, Quality::Stale),
    ("value-unusable", false, Quality::Bad),
    ("source-marked-stale", false, Quality::Stale),
    ("not-revalidated", false, Quality::Stale),
    ("counter-went-backwards", false, Quality::Bad),
];

/// The names and units, as of v6. Unchanged, and its own copy.
const GOLDEN_NAMES_V6: NameGolden = &[
    ("metric.power", "Power"),
    ("metric.energy", "Energy"),
    ("metric.rebirth", "Node Control/Rebirth"),
    ("unit.power", "kW"),
    ("unit.energy", "kWh"),
    ("property.cause", "Cause"),
];

/// The cause vocabulary, as of contract v5: v4 plus `counter-went-backwards`
/// (Story 2.2). Additive — nothing a v4 consumer understood changed meaning.
const GOLDEN_CAUSES_V5: CauseGolden = &[
    ("source-unreachable", false, Quality::Stale),
    ("source-refused", true, Quality::Bad),
    ("host-clock-unsynced", false, Quality::Stale),
    ("no-freshness-proof", false, Quality::Stale),
    ("source-clock-implausible", false, Quality::Stale),
    ("timestamps-disagree", false, Quality::Stale),
    ("reading-too-old", false, Quality::Stale),
    ("value-unusable", false, Quality::Bad),
    ("source-marked-stale", false, Quality::Stale),
    ("not-revalidated", false, Quality::Stale),
    ("counter-went-backwards", false, Quality::Bad),
];

/// The names and units a consumer binds to, as of v4 — unchanged in v5.
///
/// **ADDED 2026-08-11.** `CONTRACT_VERSION`'s own doc promises a bump *"on ANY
/// change to the topic grammar, to a metric name or unit, or to the meaning of a
/// published quality code"*, and the guard checked only the last of the three.
/// `METRIC_PROPERTY_CAUSE` is the sharpest case: it is **the change v4 was
/// struck for**, and renaming `"Cause"` to `"Reason"` broke every consumer's tag
/// binding while leaving this file green.
/// **The right-hand side is a LITERAL, never the constant.** Writing
/// `("metric.power", METRIC_POWER)` would compare the constant to itself and
/// pass through any rename — the "bdSeq compared against itself" shape this
/// repository has already had to throw four tests away for. The literal is the
/// promise; the constant is what is checked against it.
const GOLDEN_NAMES_V4: NameGolden = &[
    ("metric.power", "Power"),
    ("metric.energy", "Energy"),
    ("metric.rebirth", "Node Control/Rebirth"),
    ("unit.power", "kW"),
    ("unit.energy", "kWh"),
    ("property.cause", "Cause"),
];

/// The names and units, as of v5. Its own copy, for the reason
/// [`GOLDEN_QUALITY_V5`] gives.
const GOLDEN_NAMES_V5: NameGolden = &[
    ("metric.power", "Power"),
    ("metric.energy", "Energy"),
    ("metric.rebirth", "Node Control/Rebirth"),
    ("unit.power", "kW"),
    ("unit.energy", "kWh"),
    ("property.cause", "Cause"),
];

/// The live names, paired with the labels the goldens use. This is the only
/// place the constants appear, so the comparison below is live-against-promise.
fn live_names() -> Vec<(&'static str, &'static str)> {
    vec![
        ("metric.power", METRIC_POWER),
        ("metric.energy", METRIC_ENERGY),
        ("metric.rebirth", METRIC_NODE_CONTROL_REBIRTH),
        ("unit.power", UNIT_POWER),
        ("unit.energy", UNIT_ENERGY),
        ("property.cause", METRIC_PROPERTY_CAUSE),
    ]
}

/// What one version of the contract promises a consumer.
struct Golden {
    quality: QualityGolden,
    causes: CauseGolden,
    names: NameGolden,
}

/// The one place a version is bound to its golden. Adding a version without its
/// golden is what the `None` arm refuses.
fn golden_for(version: i64) -> Option<Golden> {
    match version {
        7 => Some(Golden {
            quality: GOLDEN_QUALITY_V7,
            causes: GOLDEN_CAUSES_V7,
            names: GOLDEN_NAMES_V7,
        }),
        6 => Some(Golden {
            quality: GOLDEN_QUALITY_V6,
            causes: GOLDEN_CAUSES_V6,
            names: GOLDEN_NAMES_V6,
        }),
        5 => Some(Golden {
            quality: GOLDEN_QUALITY_V5,
            causes: GOLDEN_CAUSES_V5,
            names: GOLDEN_NAMES_V5,
        }),
        4 => Some(Golden {
            quality: GOLDEN_QUALITY_V4,
            causes: GOLDEN_CAUSES_V4,
            names: GOLDEN_NAMES_V4,
        }),
        _ => None,
    }
}

#[test]
fn the_published_contract_matches_its_version() {
    let Some(golden) = golden_for(CONTRACT_VERSION) else {
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
    for (quality, expected) in golden.quality {
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

    // --- names, units and the property key -----------------------------------
    // These are what a consumer's tag bindings are made of: renaming any of them
    // breaks every binding silently, which is why CONTRACT_VERSION's own doc
    // promises a bump for them.
    let live = live_names();
    assert_eq!(
        live.len(),
        golden.names.len(),
        "a contract name was added or removed without CONTRACT_VERSION moving"
    );
    for ((label, actual), (golden_label, expected)) in live.iter().zip(golden.names) {
        assert_eq!(label, golden_label, "the golden name list is out of order");
        assert_eq!(
            actual, expected,
            "{label} is published as {actual:?} but v{CONTRACT_VERSION} promised \
             {expected:?}.\n\
             Every consumer binds its tags by this string: renaming it breaks all \
             of them at once, and does so silently — the metric simply stops \
             arriving under the name the host is watching."
        );
    }

    // --- cause vocabulary ----------------------------------------------------
    assert_eq!(
        Cause::ALL.len(),
        golden.causes.len(),
        "the cause vocabulary changed size ({} live, {} in the v{CONTRACT_VERSION} golden) \
         without CONTRACT_VERSION moving.\n\
         A cause is a string a consumer reads off a degraded metric; adding or removing one \
         changes what v{CONTRACT_VERSION} means.",
        Cause::ALL.len(),
        golden.causes.len()
    );

    for (cause, (wire, latches, quality)) in Cause::ALL.iter().zip(golden.causes) {
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
        // The oracle→quality mapping AC5 asked for, and the half this file was
        // missing until 2026-08-11: pinning each quality's code and each cause's
        // string says nothing about WHICH quality a cause produces.
        assert_eq!(
            cause.published_quality(),
            *quality,
            "{cause:?} now publishes {:?} where v{CONTRACT_VERSION} promised {quality:?}.\n\
             This is the most consequential change a contract can make without \
             renaming anything: `Stale` hands the consumer a value and says it may \
             be old, `Bad` withholds the value entirely. A consumer that learned \
             one of them under this version now reads the other.",
            cause.published_quality()
        );
    }
}

/// The half a golden test usually forgets: that it can pass for the right reason.
///
/// A guard only ever demonstrated red proves it can fail, not that it can pass —
/// which is how a test that fails for an unrelated reason gets read as working.
///
/// **STRENGTHENED 2026-08-11.** As first written this was close to a tautology:
/// `golden_for(CONTRACT_VERSION + 1).is_none()` and `golden_for(3).is_none()`
/// restate that the `match` has exactly two listed arms, and nothing in the
/// workflow would ever pre-write a golden for an unshipped version. Worse, the
/// load-bearing half — that the refusal actually PANICS rather than being
/// quietly skipped — was never exercised at all, so the one behaviour that
/// protects the runbook's promise had no test.
///
/// It now checks the two properties that can genuinely break: that every version
/// with a golden is internally consistent (a golden whose three parts disagree in
/// length would fail confusingly, at a line that names none of it), and that each
/// version's golden is its OWN, so editing one does not rewrite the history of
/// another.
///
/// FALSIFIED 2026-08-11: dropping a row from `GOLDEN_NAMES_V4` makes the
/// per-version completeness check red on v4 — a version nothing else in this
/// file reads, which is exactly the blind spot this test exists for.
#[test]
fn every_written_version_has_its_own_complete_golden() {
    let shipped = golden_for(CONTRACT_VERSION).expect("the shipped version must have a golden");
    assert!(
        golden_for(CONTRACT_VERSION + 1).is_none(),
        "an unwritten version must be refused rather than silently accepted"
    );
    assert!(
        golden_for(3).is_none(),
        "v3 predates this guard and has no golden; it must not appear to have one"
    );

    // Every written version, not just the shipped one: a golden is a historical
    // record and stays readable after nobody runs that version.
    let written: Vec<i64> = (4..=CONTRACT_VERSION).collect();
    assert!(
        written.len() >= 2,
        "this check is meaningless with fewer than two written versions"
    );
    for version in &written {
        let golden = golden_for(*version).unwrap_or_else(|| {
            panic!("v{version} is between the first golden and the shipped one but has none")
        });
        assert_eq!(
            golden.quality.len(),
            3,
            "v{version}: every quality must have a code, or a metric publishes one \
             the golden never saw"
        );
        assert_eq!(
            golden.names.len(),
            shipped.names.len(),
            "v{version}: a name was added or dropped between versions without the \
             golden saying so"
        );
        assert!(
            !golden.causes.is_empty(),
            "v{version}: a contract with no causes cannot describe a degraded metric"
        );
    }

    // NOT ASSERTED, and the attempt is worth recording so nobody re-writes it:
    // that each version's arrays are physically distinct. `GOLDEN_QUALITY_V4` and
    // `GOLDEN_QUALITY_V5` are now written out separately — the review found them
    // sharing one `const` by reference, which meant an edit made for v5 would
    // retroactively rewrite what v4 attested to. But a `std::ptr::eq` check on
    // them FAILS EVEN WHEN THEY ARE SEPARATE CONSTANTS: rustc is free to
    // coalesce two constants with identical contents to one address, so the
    // assertion reports aliasing that does not exist in the source and would go
    // green or red on an optimisation decision rather than on ours. Written,
    // run, seen to fail against correct code, and removed. The separation is
    // kept as a rule stated where the constants are, not as a runtime check —
    // which is the honest place for a property the language will not let a test
    // observe.
}
