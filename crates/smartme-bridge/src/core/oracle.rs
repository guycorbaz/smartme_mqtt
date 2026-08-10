//! Where several judgements about one reading become one published verdict
//! (Story 2.1).
//!
//! # Why this exists before any oracle does
//!
//! Until this module, exactly one thing produced a verdict: [`Policy::step`]
//! returned `(State, Quality)` behind a first-match-wins table. Epic 2 adds three
//! more producers — physical bounds, energy-counter monotonicity, payload domain
//! — and a fourth is already live without ever having been called an oracle
//! (ADR 0029's serial-identity check, which arrived because a real fault demanded
//! it). Four producers and no composition rule is how a guarantee rots: each new
//! oracle answers *"what quality do I publish?"* on its own and the answers drift.
//!
//! [`Policy::step`]: crate::core::state_machine::Policy::step
//!
//! # The three rules this module fixes, and none of them may be re-chosen
//!
//! **1. Composition is worst-wins over `Good < Stale < Bad`, not first-match.**
//! First-match is right for a *single* producer whose guards are ordered by
//! intent, which is what `Policy::step`'s table is. With several independent
//! producers, evaluation order becomes an accident of registration and
//! first-match would make the published verdict depend on it. Worst-wins is
//! order-independent, which is the property that matters once the set of oracles
//! can grow.
//!
//! `judge_reading` already behaved this way for the one case it had, and even
//! argued it — *"`Bad` is judged BEFORE the timestamp guards: 'do not use this
//! value' must never be relabeled as the milder 'old value'"*. That sentence was
//! the rule. It had never been stated as one.
//!
//! **2. The cause survives to the wire, and NOT inside the `Quality` property.**
//! `tck-id-payloads-propertyset-quality-value-value`
//! (`Sparkplug_6_Payloads.adoc:634-636`) restricts that property to the values
//! `0`, `192` or `500`. The bridge already deviates there deliberately (ADR 0012:
//! the conformant codes display as `Good` on Ignition, which is the exact lie this
//! project exists to prevent), and inventing a fourth value to encode a cause
//! would deepen a deviation accepted only because the alternative was a silent
//! lie. So the cause travels under its own property key, which costs nothing in
//! conformance — a `PropertySet` constrains only that keys and values have equal
//! length (`Sparkplug_6_Payloads.adoc:571,577`).
//!
//! **3. Latching follows identity; degrading follows value.** A reading that came
//! from the wrong meter means no later reading from that misconfiguration is
//! trustworthy either, so it latches (`State::Failed`, restart-only, ADR 0009's
//! "stop + surface"). A power value outside physical bounds says nothing about the
//! next one, so it degrades that reading and nothing more. The rule follows the
//! *kind* of contradiction rather than its severity, and it is written here so
//! that stories 2.2–2.4 do not each have to guess it.
//!
//! ADR 0029's serial-identity check was decided before this rule existed and is
//! its first instance rather than an exception to it — see [`Cause::latches`].

use crate::domain::Quality;

/// Why a reading was degraded or refused: the half of a verdict that reaches a
/// consumer, under its own property key.
///
/// Every variant here corresponds to a judgement that **already existed** before
/// Story 2.1 — the eight rows of `Policy::step`'s table. The story adds no oracle;
/// it gives the existing ones a shared vocabulary so the ones Epic 2 still owes
/// can join without inventing their own.
///
/// The wire string is deliberately short and stable: it is read by a human staring
/// at a tag browser at an unhelpful hour, and it is part of the versioned contract,
/// so changing one is a `CONTRACT_VERSION` bump (see `contract_golden`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cause {
    /// The fetch did not complete: timeout, or a transient transport failure.
    SourceUnreachable,
    /// The source refused us, fatally — rejected credentials, a configuration the
    /// source contradicts, or a serial that is not the one smart-me reports
    /// (ADR 0029). **Latches.**
    SourceRefused,
    /// The host wall clock is below the plausibility floor, so nothing it stamps
    /// can be trusted — including the judgement that would otherwise be made.
    HostClockUnsynced,
    /// The response carried no usable `Date` header, so there is no freshness
    /// proof at all. Absence of evidence, published as such.
    NoFreshnessProof,
    /// The source's own clock is implausible: a stamp from before 2020 cannot be a
    /// live reading, however self-consistent the pair is.
    SourceClockImplausible,
    /// `http_date` precedes `value_date`: the two clock domains disagree, and a
    /// negative age is not a fresher reading.
    TimestampsDisagree,
    /// The reading is older than the allowance. The ordinary staleness case.
    ReadingTooOld,
    /// The values themselves are unusable — an unknown unit or a non-finite
    /// number — so the timestamps prove only that we were promptly given
    /// something we cannot use (Story 1.7's fail-closed conversion).
    ValueUnusable,
    /// The source handed us a value it had already marked as not fresh.
    ///
    /// **Nothing produces this today** — `map_device` yields `Good` or `Bad` and
    /// never `Stale`. The arm exists because `Measurement::quality` is a field any
    /// future source adapter can set, and the migration of `Policy::step` (Story
    /// 2.1, AC7) had to give every row of its table a cause. Naming it rather than
    /// borrowing [`Cause::ValueUnusable`] keeps the wire honest the day something
    /// does produce it: "the source said so" and "we could not convert it" are
    /// different diagnoses and would send an operator to different places.
    SourceMarkedStale,
}

impl Cause {
    /// The string a consumer sees. Stable, and part of the versioned contract.
    pub const fn as_str(self) -> &'static str {
        match self {
            Cause::SourceUnreachable => "source-unreachable",
            Cause::SourceRefused => "source-refused",
            Cause::HostClockUnsynced => "host-clock-unsynced",
            Cause::NoFreshnessProof => "no-freshness-proof",
            Cause::SourceClockImplausible => "source-clock-implausible",
            Cause::TimestampsDisagree => "timestamps-disagree",
            Cause::ReadingTooOld => "reading-too-old",
            Cause::ValueUnusable => "value-unusable",
            Cause::SourceMarkedStale => "source-marked-stale",
        }
    }

    /// Whether this contradiction poisons every later reading, or only this one.
    ///
    /// **Identity latches; value degrades.** [`Cause::SourceRefused`] is the only
    /// latching cause today, and it covers the three things that mean *the answer
    /// we are getting is not the answer to our question*: rejected credentials, a
    /// configuration the source contradicts, and ADR 0029's serial mismatch.
    /// Retrying any of them with the same configuration would keep lying, and the
    /// configuration is restart-only — so `State::Failed` is absorbing and only a
    /// fresh process leaves it.
    ///
    /// Everything else describes *this* reading. A reading that is too old says
    /// nothing about the next one; neither does one whose value could not be
    /// converted. Story 2.2's counter-went-backwards and 2.3's out-of-bounds will
    /// join this half, and the test
    /// `a_degrading_cause_does_not_poison_the_next_reading` is what holds them
    /// there.
    pub const fn latches(self) -> bool {
        matches!(self, Cause::SourceRefused)
    }
}

/// One judgement: a quality, and — unless it is `Good` — why.
///
/// A `Good` verdict carries no cause by construction. That is not tidiness: a
/// cause published beside a good value is noise a host would have to learn to
/// ignore, and the day it means something nobody would notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    quality: Quality,
    cause: Option<Cause>,
}

impl Verdict {
    /// Nothing to object to.
    pub const fn good() -> Self {
        Verdict {
            quality: Quality::Good,
            cause: None,
        }
    }

    /// Old, or unproven: the last known value is still worth publishing, marked.
    pub const fn stale(cause: Cause) -> Self {
        Verdict {
            quality: Quality::Stale,
            cause: Some(cause),
        }
    }

    /// Do not use this value.
    pub const fn bad(cause: Cause) -> Self {
        Verdict {
            quality: Quality::Bad,
            cause: Some(cause),
        }
    }

    /// What gets stamped on the metric.
    pub const fn quality(self) -> Quality {
        self.quality
    }

    /// Why, or `None` for a good reading.
    pub const fn cause(self) -> Option<Cause> {
        self.cause
    }

    /// Whether this verdict puts the meter in `Failed` until a restart.
    ///
    /// Delegates to [`Cause::latches`] so the rule lives in exactly one place; a
    /// verdict cannot latch for a reason its cause does not.
    pub const fn latches(self) -> bool {
        match self.cause {
            Some(cause) => cause.latches(),
            None => false,
        }
    }
}

/// How bad a quality is. Private on purpose: the order is the composition rule,
/// not a general fact about `Quality`, and exporting it would invite a second
/// interpretation somewhere else.
const fn severity(quality: Quality) -> u8 {
    match quality {
        Quality::Good => 0,
        Quality::Stale => 1,
        Quality::Bad => 2,
    }
}

/// The one composition: **worst wins**, over `Good < Stale < Bad`.
///
/// Every producer of a verdict passes through here. Ties keep the FIRST verdict
/// of that severity, so the reported cause is stable rather than dependent on how
/// the oracles happen to be ordered — but which of two equally severe causes is
/// reported is arbitrary by construction, and no caller may rely on it. If two
/// causes at the same severity ever need to be distinguished, that is a decision
/// to take then, in the open, rather than a property to discover.
///
/// An empty iterator composes to [`Verdict::good`]: nothing objected. That is the
/// honest reading — "no oracle refused" — and it is what makes registering zero
/// oracles behave like the bridge did before this module existed.
pub fn compose(verdicts: impl IntoIterator<Item = Verdict>) -> Verdict {
    verdicts.into_iter().fold(Verdict::good(), |worst, next| {
        if severity(next.quality()) > severity(worst.quality()) {
            next
        } else {
            worst
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC2 — worst wins, and it does so independently of the order the verdicts
    /// arrive in.
    ///
    /// **The mutation this is written against** is the one a reasonable
    /// implementation would make: return the FIRST non-good verdict instead of the
    /// worst. It is invisible whenever the worst happens to come first, so the
    /// case that matters is the one where evaluation order and severity order
    /// DISAGREE — a `Stale` registered before a `Bad`.
    ///
    /// FALSIFIED 2026-08-10: replacing the fold body with `if worst.quality() ==
    /// Quality::Good { next } else { worst }` (first-non-good-wins) turns the
    /// second assertion red — `Stale` is published where `Bad` was owed, which is
    /// precisely "do not use this value" relabelled as the milder "old value".
    #[test]
    fn the_worst_verdict_wins_whatever_order_it_arrives_in() {
        let bad = Verdict::bad(Cause::ValueUnusable);
        let stale = Verdict::stale(Cause::ReadingTooOld);

        // Worst first: every implementation gets this right, which is why it
        // cannot be the only case.
        assert_eq!(compose([bad, stale]).quality(), Quality::Bad);

        // Worst LAST. This is the assertion that discriminates.
        assert_eq!(compose([stale, bad]).quality(), Quality::Bad);

        // And the cause travels with the quality it belongs to, rather than being
        // picked up from whichever verdict happened to be first.
        assert_eq!(compose([stale, bad]).cause(), Some(Cause::ValueUnusable));
    }

    /// AC2 — the degenerate ends, stated rather than assumed.
    #[test]
    fn nothing_objecting_is_good_and_carries_no_cause() {
        assert_eq!(compose([]).quality(), Quality::Good);
        assert_eq!(compose([]).cause(), None);
        assert_eq!(compose([Verdict::good(), Verdict::good()]), Verdict::good());
    }

    /// AC4 — the latch/degrade rule, from both directions.
    ///
    /// The second half is the one that matters for the oracles Epic 2 still owes:
    /// a value-level refusal must NOT put the meter in `Failed`, or story 2.3's
    /// first out-of-bounds power reading would take a meter off the wire until
    /// somebody restarted the container.
    ///
    /// FALSIFIED 2026-08-10: widening `Cause::latches` to
    /// `!matches!(self, Cause::SourceUnreachable)` — the plausible "anything worse
    /// than a timeout is fatal" reading — turns the second assertion red on every
    /// degrading cause.
    #[test]
    fn identity_latches_and_value_does_not() {
        // Identity: the answer is not the answer to our question.
        assert!(Cause::SourceRefused.latches());
        assert!(Verdict::bad(Cause::SourceRefused).latches());

        // Value: this reading only.
        for cause in [
            Cause::SourceUnreachable,
            Cause::HostClockUnsynced,
            Cause::NoFreshnessProof,
            Cause::SourceClockImplausible,
            Cause::TimestampsDisagree,
            Cause::ReadingTooOld,
            Cause::ValueUnusable,
            Cause::SourceMarkedStale,
        ] {
            assert!(
                !cause.latches(),
                "{cause:?} describes a reading, not an identity, so it must not latch"
            );
        }
    }

    /// AC4 — a degrading verdict leaves the next reading free to be `Good`.
    ///
    /// Composition carries no memory, and this asserts that plainly: composing a
    /// degraded verdict and then composing a fresh set does not drag the earlier
    /// refusal along. The state machine's `Failed` latch is the only memory in the
    /// system, and it is reached through [`Verdict::latches`] alone.
    #[test]
    fn a_degrading_cause_does_not_poison_the_next_reading() {
        let degraded = compose([Verdict::bad(Cause::ValueUnusable)]);
        assert_eq!(degraded.quality(), Quality::Bad);
        assert!(!degraded.latches());

        let next = compose([Verdict::good()]);
        assert_eq!(next.quality(), Quality::Good);
        assert_eq!(next.cause(), None);
    }

    /// A `Good` verdict cannot be given a cause — enforced by there being no
    /// constructor that does it, which this test records rather than proves.
    #[test]
    fn a_good_verdict_has_no_cause() {
        assert_eq!(Verdict::good().cause(), None);
        assert_eq!(Verdict::good().quality(), Quality::Good);
        assert!(!Verdict::good().latches());
    }

    /// Every cause has a distinct wire string, and none of them is empty.
    ///
    /// Two causes sharing a string would make the property useless for the thing
    /// it exists for — telling an operator WHICH oracle refused — and the
    /// collision would be invisible in every other test.
    #[test]
    fn every_cause_has_its_own_wire_string() {
        let all = [
            Cause::SourceUnreachable,
            Cause::SourceRefused,
            Cause::HostClockUnsynced,
            Cause::NoFreshnessProof,
            Cause::SourceClockImplausible,
            Cause::TimestampsDisagree,
            Cause::ReadingTooOld,
            Cause::ValueUnusable,
            Cause::SourceMarkedStale,
        ];
        let mut seen = std::collections::BTreeSet::new();
        for cause in all {
            assert!(!cause.as_str().is_empty(), "{cause:?} has no wire string");
            assert!(
                seen.insert(cause.as_str()),
                "{cause:?} shares its wire string with another cause"
            );
        }
        assert_eq!(seen.len(), all.len());
    }
}
