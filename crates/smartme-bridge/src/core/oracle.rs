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

use crate::domain::{Kwh, Quality};

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
    /// The value is the last one we judged, republished without a new judgement.
    ///
    /// ADR 0027 requires every poll cycle to publish a verdict for every enabled
    /// meter — never silence — so a meter that has not produced a fresh reading is
    /// republished with its last known values. A verdict that has not been
    /// re-computed cannot be re-asserted, so `Good` degrades to `Stale` here; this
    /// cause is what says *why* it degraded, rather than leaving a consumer to read
    /// it as a reading that arrived late.
    NotRevalidated,
    /// The energy index went backwards: a reset, a rollover, or a meter that was
    /// replaced (Story 2.2, FR15/NFR6).
    ///
    /// **One cause for three events, on purpose.** Nothing available to the bridge
    /// tells them apart: a rollover is a reset with a particular arithmetic, and a
    /// replacement is a reset with a different serial — which ADR 0029 already
    /// refuses upstream, so the replacement that reaches here is one an operator
    /// re-serialised on purpose. Publishing three causes would claim a
    /// discrimination we cannot make.
    CounterWentBackwards,
}

impl Cause {
    /// Every cause, in a fixed order.
    ///
    /// Kept honest by [`Cause::successor`], whose exhaustive `match` stops the
    /// build when a variant is added — so a new cause cannot join the enum and
    /// quietly miss the golden contract test.
    ///
    /// # The hole this used to have, and why it was the natural case
    ///
    /// **Corrected 2026-08-11 by the review of story 2.1.** This list was
    /// previously said to be kept honest by [`Cause::discriminant`], and it was
    /// not. `discriminant` is an exhaustive `match`, so a new variant did stop
    /// the build — but the repair was to add one arm to `discriminant` and one
    /// to [`Cause::as_str`], and **neither of those touches `ALL`**.
    /// `every_cause_is_in_all` walked `ALL` checking `discriminant() == position`
    /// and asserted `ALL.len() == 11`, so APPENDING a variant and forgetting
    /// `ALL` left positions 0..=10 still aligned, the length still 11, and the
    /// golden comparing 11 to 11. Everything green, and the new string on the
    /// wire under an unchanged `CONTRACT_VERSION`.
    ///
    /// The old test caught only a MID-LIST insertion, which is not how a cause
    /// has ever been added here — `CounterWentBackwards` was appended, as the
    /// next one will be. [`Cause::successor`] closes it: the last variant is the
    /// one that returns `None`, so appending forces an edit to two arms, and
    /// `every_cause_is_in_all` now rebuilds the list by walking that chain and
    /// compares it to this one element by element.
    pub const ALL: &'static [Cause] = &[
        Cause::SourceUnreachable,
        Cause::SourceRefused,
        Cause::HostClockUnsynced,
        Cause::NoFreshnessProof,
        Cause::SourceClockImplausible,
        Cause::TimestampsDisagree,
        Cause::ReadingTooOld,
        Cause::ValueUnusable,
        Cause::SourceMarkedStale,
        Cause::NotRevalidated,
        Cause::CounterWentBackwards,
    ];

    /// The next cause in [`Cause::ALL`]'s order, or `None` for the last one.
    ///
    /// **This is what actually forces `ALL` to be complete**, and it replaces a
    /// `discriminant` that could not (see [`Cause::ALL`]). The chain is a linked
    /// list expressed as an exhaustive `match`, and it has the one property a
    /// positional index lacks: **there is exactly one `None`**, so a variant
    /// cannot be appended without editing the arm that used to end the chain.
    /// Whoever adds a cause must therefore say where it goes, and
    /// `every_cause_is_in_all` rebuilds `ALL` from this chain and compares.
    ///
    /// Adding a variant to [`Cause`] without adding it here does not compile.
    /// Adding it here but not to [`Cause::ALL`] — or vice versa — fails that
    /// test. Between the two, a cause cannot reach the wire without passing the
    /// golden contract test.
    ///
    /// `#[cfg(test)]` because production has no use for it and `clippy -D warnings`
    /// is right to say so. The guard still bites where it matters: the CI compiles
    /// the tests on every run, so a variant added to the enum and nowhere else
    /// fails the build rather than slipping past.
    #[cfg(test)]
    const fn successor(self) -> Option<Cause> {
        match self {
            Cause::SourceUnreachable => Some(Cause::SourceRefused),
            Cause::SourceRefused => Some(Cause::HostClockUnsynced),
            Cause::HostClockUnsynced => Some(Cause::NoFreshnessProof),
            Cause::NoFreshnessProof => Some(Cause::SourceClockImplausible),
            Cause::SourceClockImplausible => Some(Cause::TimestampsDisagree),
            Cause::TimestampsDisagree => Some(Cause::ReadingTooOld),
            Cause::ReadingTooOld => Some(Cause::ValueUnusable),
            Cause::ValueUnusable => Some(Cause::SourceMarkedStale),
            Cause::SourceMarkedStale => Some(Cause::NotRevalidated),
            Cause::NotRevalidated => Some(Cause::CounterWentBackwards),
            // The end of the chain. A new cause appended to `ALL` replaces this
            // arm's `None` with a `Some`, which is the edit the old positional
            // `discriminant` never demanded.
            Cause::CounterWentBackwards => None,
        }
    }

    /// The list [`Cause::ALL`] must equal, rebuilt from [`Cause::successor`].
    #[cfg(test)]
    fn walk_from_first() -> Vec<Cause> {
        let mut walked = vec![Cause::SourceUnreachable];
        while let Some(next) = walked[walked.len() - 1].successor() {
            walked.push(next);
            assert!(
                walked.len() <= 64,
                "`successor` has a cycle — the chain must end at exactly one `None`"
            );
        }
        walked
    }

    /// The quality a verdict carrying this cause publishes.
    ///
    /// **This is the oracle→quality mapping AC5 named, and until 2026-08-11 it
    /// existed nowhere.** `contract_golden` pinned each quality's integer code
    /// and each cause's wire string, but never *which quality a cause produces* —
    /// so turning `Verdict::stale(ReadingTooOld)` into `Verdict::bad(…)`, or
    /// making [`energy_is_monotonic`] return `Stale`, changed what every consumer
    /// reads with the guard fully green. That is a contract change of the most
    /// consequential kind: `Stale` says *this value was true and may be old* and
    /// `Bad` withholds the value entirely.
    ///
    /// Declaring it here rather than deriving it from the producers is
    /// deliberate. Derived, it would restate whatever the code does and could
    /// never disagree with it; declared, it is a promise the producers are
    /// checked against — `the_table_publishes_the_quality_each_cause_promises`
    /// in `state_machine`, and `every_cause_keeps_its_promised_quality` below.
    ///
    /// The `match` is exhaustive on purpose: a new cause must be given a quality
    /// here, in the open, or the build stops.
    pub const fn published_quality(self) -> Quality {
        match self {
            // Identity and value refusals: the number must not be handed over.
            Cause::SourceRefused => Quality::Bad,
            Cause::ValueUnusable => Quality::Bad,
            Cause::CounterWentBackwards => Quality::Bad,
            // Freshness refusals: the value was true, and may be old. The
            // difference from the three above is the whole reason a consumer is
            // told which of the two it is holding.
            Cause::SourceUnreachable => Quality::Stale,
            Cause::HostClockUnsynced => Quality::Stale,
            Cause::NoFreshnessProof => Quality::Stale,
            Cause::SourceClockImplausible => Quality::Stale,
            Cause::TimestampsDisagree => Quality::Stale,
            Cause::ReadingTooOld => Quality::Stale,
            Cause::SourceMarkedStale => Quality::Stale,
            Cause::NotRevalidated => Quality::Stale,
        }
    }

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
            Cause::NotRevalidated => "not-revalidated",
            Cause::CounterWentBackwards => "counter-went-backwards",
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

/// The energy-counter monotonicity oracle (Story 2.2 — FR15, NFR6).
///
/// A cumulative counter does not go down. When it does, the meter was reset,
/// rolled over, or replaced — and the number itself is not what makes that
/// dangerous. **The danger is the difference**: a consumer computing consumption
/// between two readings would get a negative delta and no reason to distrust it,
/// which is the "never lies" invariant failing on arithmetic the bridge never
/// performs.
///
/// # Why `Bad` and not `Stale`
///
/// `Stale` says *this value was true and may be old*. Here the value may be
/// perfectly current — a new meter really does read 12 kWh — and it is the
/// RELATION to the previous one that is broken. `Bad` publishes null values
/// (see `metrics_for`), which withholds exactly the number a consumer would
/// difference. Publishing it as `Stale` would hand over the number with a hint.
///
/// # Why no tolerance band
///
/// The comparison is a strict `<`, with no epsilon. A cumulative counter does not
/// go backwards *by a little*, so a band would be a number nobody measured,
/// chosen to suppress a signal rather than to model one. If real polling data ever
/// shows benign jitter, that measurement is what would justify a band.
///
/// # Why it does not latch
///
/// Story 2.1's rule: identity latches, value degrades. A broken counter history
/// is not a claim that this is the wrong meter, and a replaced meter legitimately
/// reads lower for ever after — latching would take a working meter off the wire
/// until somebody restarted the container. The caller therefore adopts the new
/// index as the reference; see `poll_publish`.
///
/// `None` for the reference means "no accepted reading yet", which cannot be
/// backwards from anything.
///
/// # Non-finite values, and why this function does not trust its caller
///
/// **Added 2026-08-11 by the story's review.** `reading.0 < previous.0` is
/// `false` when either side is NaN, so a naive `match` judged a NaN reading
/// monotonic-`Good` — and a NaN REFERENCE disabled the oracle permanently for
/// that meter, every later comparison being false, with no signal anywhere.
///
/// The only thing preventing it was an invariant in a different module: the
/// source adapter marks non-finite values `Bad`. That is a real guarantee and it
/// is not this function's to assume. This is a `pub` function in the pure core;
/// it is reachable from any future oracle or test, its doc enumerates the cases
/// it considered, and "somebody else already checked" is how a guarantee decays
/// into a coincidence. A non-finite READING is not a counter that went backwards
/// — it is a value that cannot be compared at all, so it is refused as
/// [`Cause::ValueUnusable`] rather than being silently blessed.
///
/// **A non-finite REFERENCE gets no guard, and that is deliberate.** The review
/// also asked for one, on the reasoning that a NaN reference makes every later
/// comparison false and so disables the oracle for good. Writing it showed the
/// premise is wrong twice over: an explicit `!previous.is_finite() => good()`
/// arm produces the *identical* result to falling through (`x < NaN` is already
/// `false`), so it could not be falsified — it was unfalsifiable decoration, the
/// very shape this repository throws tests away for. And "for good" does not
/// hold either: the caller re-adopts a reference on every accepted reading, so a
/// poisoned one survives exactly one tick. What actually protects this is the
/// reference-adoption rule in `poll_publish`, which is where it is tested.
pub fn energy_is_monotonic(reference: Option<Kwh>, reading: Kwh) -> Verdict {
    if !reading.0.is_finite() {
        return Verdict::bad(Cause::ValueUnusable);
    }
    match reference {
        Some(previous) if reading.0 < previous.0 => Verdict::bad(Cause::CounterWentBackwards),
        _ => Verdict::good(),
    }
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

        // Value: this reading only. DERIVED FROM `Cause::ALL` since 2026-08-11 —
        // this used to be a hand-copied duplicate of the enum, so a newly
        // appended cause was never put to the latch question at all and silently
        // took `latches()`'s `matches!` default of "does not latch". Deriving it
        // means a new cause must be classified here or the test names it.
        for cause in Cause::ALL.iter().filter(|c| **c != Cause::SourceRefused) {
            assert!(
                !cause.latches(),
                "{cause:?} describes a reading, not an identity, so it must not \
                 latch. If it genuinely does, it belongs beside SourceRefused \
                 above — and ADR 0029 records why that list is short."
            );
        }
    }

    /// AC4 — a degrading verdict leaves the next reading free to be `Good`.
    ///
    /// **REWRITTEN 2026-08-11.** The previous version called `compose([bad(…)])`
    /// and then `compose([good()])` and asserted the second was `Good` — which
    /// proves that a stateless `fold` is stateless. No reading was judged and no
    /// "next reading" was published, so the property AC4 asked for (*"the next
    /// good reading publishes `Good` again"*) was untouched; it was only really
    /// attested a story later, by 2.2's pipeline test. The story file names this
    /// trap in its own words, which is what makes it worth recording here.
    ///
    /// What can actually break is the LATCH: a degrading cause must not make
    /// `latches()` true, because that is the bit `Policy::step` and every future
    /// oracle read to decide whether a meter comes back. So the test now walks
    /// every degrading cause through composition — alone, and mixed with a
    /// worse-severity sibling — and asserts the composed verdict does not latch.
    ///
    /// FALSIFIED 2026-08-11: widening `Cause::latches` to
    /// `!matches!(self, Cause::SourceUnreachable)` turns it red on the first
    /// degrading cause, naming it. The old version stayed GREEN under that same
    /// mutation, because `ValueUnusable` was the only cause it ever looked at and
    /// it never asked whether the composed verdict latched.
    #[test]
    fn a_degrading_cause_does_not_poison_the_next_reading() {
        for cause in Cause::ALL.iter().filter(|c| !c.latches()) {
            let verdict = match cause.published_quality() {
                Quality::Bad => Verdict::bad(*cause),
                _ => Verdict::stale(*cause),
            };

            let alone = compose([verdict]);
            assert!(
                !alone.latches(),
                "{cause:?} degrades one reading, so composing it must not latch \
                 the meter — a latched meter stays off the wire until a restart"
            );

            // Mixed with a worse verdict from another oracle: the composed cause
            // may be the other one, and it still must not latch.
            let mixed = compose([verdict, Verdict::bad(Cause::ValueUnusable)]);
            assert!(
                !mixed.latches(),
                "{cause:?} composed with a degrading Bad must not latch either"
            );

            // And the next reading, judged afresh, is free to be Good.
            assert_eq!(compose([Verdict::good()]), Verdict::good());
        }
    }

    /// A non-finite index is refused rather than blessed (added 2026-08-11 by the
    /// story's review).
    ///
    /// `reading < previous` is `false` when either side is NaN, so the naive
    /// `match` fell through to `Verdict::good()` and published a NaN index as a
    /// monotonic-good measurement. The source adapter does mark non-finite values
    /// `Bad`, but that invariant lives in another module and this is a `pub`
    /// function in the pure core.
    ///
    /// **Only the READING is guarded, and the test only asserts what the guard
    /// changes.** A guard on the reference was written first and then removed:
    /// its explicit arm returned exactly what falling through returned, so no
    /// mutation could make an assertion about it fail. An assertion that cannot
    /// fail is the thing this repository deletes tests for, and writing one here
    /// while reviewing others for it would have been its own answer.
    ///
    /// FALSIFIED 2026-08-11: deleting the `is_finite` early return makes both
    /// assertions fail — `NaN` and `INFINITY` each come back `Good` with no
    /// cause, which is the silent blessing the guard exists to stop.
    #[test]
    fn a_non_finite_index_is_refused_rather_than_blessed() {
        // Cannot be compared to anything: refused, and NOT as a backwards
        // counter — nothing was ever ordered, so naming the ordering oracle
        // would send an operator to the wrong fault.
        let verdict = energy_is_monotonic(Some(Kwh(4843.822)), Kwh(f64::NAN));
        assert_eq!(verdict.quality(), Quality::Bad);
        assert_eq!(verdict.cause(), Some(Cause::ValueUnusable));

        let infinite = energy_is_monotonic(None, Kwh(f64::INFINITY));
        assert_eq!(
            infinite.cause(),
            Some(Cause::ValueUnusable),
            "an infinite index is no more comparable than a NaN one, and `None` \
             for the reference must not excuse it"
        );
    }

    /// A `Good` verdict cannot be given a cause — enforced by there being no
    /// constructor that does it, which this test records rather than proves.
    #[test]
    fn a_good_verdict_has_no_cause() {
        assert_eq!(Verdict::good().cause(), None);
        assert_eq!(Verdict::good().quality(), Quality::Good);
        assert!(!Verdict::good().latches());
    }

    /// `ALL` really is all of them.
    ///
    /// [`Cause::successor`]'s exhaustive match makes the compiler refuse a variant
    /// that was added to the enum and nowhere else; this closes the other half,
    /// where a variant joined the chain but never the list.
    ///
    /// **REWRITTEN 2026-08-11 — the previous version had a hole at the natural
    /// case.** It compared `cause.discriminant()` to the position in `ALL` and
    /// then asserted `ALL.len() == 11`. Appending a variant and forgetting `ALL`
    /// left positions 0..=10 aligned and the length unchanged, so it passed —
    /// while the new wire string reached a consumer under an unmoved
    /// `CONTRACT_VERSION`. It only ever caught a mid-list insertion, and no cause
    /// has ever been added that way. Deriving the expected list from the chain
    /// instead makes the two lists disagree the moment either one is edited
    /// alone, in both directions and at both ends.
    ///
    /// FALSIFIED 2026-08-11, both directions, each red:
    /// - appending a twelfth variant to `Cause` and to `successor`/`as_str` but
    ///   NOT to `ALL` — the exact mutation the old test blessed — fails on the
    ///   length mismatch naming the missing cause;
    /// - removing `Cause::ReadingTooOld` from `ALL` while leaving the chain
    ///   intact fails on the element comparison.
    #[test]
    fn every_cause_is_in_all() {
        assert_eq!(
            Cause::ALL,
            Cause::walk_from_first().as_slice(),
            "Cause::ALL and Cause::successor's chain disagree — one of them was \
             edited alone. Whichever is right, the golden contract test and \
             CONTRACT_VERSION both need looking at."
        );
    }

    /// Every cause has a distinct wire string, and none of them is empty.
    ///
    /// Two causes sharing a string would make the property useless for the thing
    /// it exists for — telling an operator WHICH oracle refused — and the
    /// collision would be invisible in every other test.
    ///
    /// **Iterates `Cause::ALL` since 2026-08-11.** It used to carry a hand-copied
    /// duplicate of the enum — a third list, after `ALL` and the one in
    /// `identity_latches_and_value_does_not`, with nothing keeping the three in
    /// agreement. A twelfth cause appended to the enum appeared in none of them,
    /// so it was never checked for a string collision and its latch decision was
    /// never reviewed, in a file whose stated purpose is that a new cause cannot
    /// slip past. `ALL` is now itself derived-and-checked (see
    /// `every_cause_is_in_all`), so deriving from it is a real chain of custody
    /// rather than one more copy.
    #[test]
    fn every_cause_has_its_own_wire_string() {
        let mut seen = std::collections::BTreeSet::new();
        for cause in Cause::ALL {
            assert!(!cause.as_str().is_empty(), "{cause:?} has no wire string");
            assert!(
                seen.insert(cause.as_str()),
                "{cause:?} shares its wire string with another cause"
            );
        }
        assert_eq!(seen.len(), Cause::ALL.len());
    }
}
