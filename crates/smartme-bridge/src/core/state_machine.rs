//! The pure `Fresh|Stale|Failed` staleness oracle (Story 1.5).
//!
//! "Is this a lie?" is decided HERE — a deterministic function over plain data,
//! off the network, outside any `async fn`. The freshness formula is
//! `age = http_date − value_date` (both cloud-domain, ADR 0004): the host clock is
//! out of the equation, except for one boot-sanity guard. When in doubt, the
//! verdict is STALE — the cheap honest default.

use crate::core::oracle::{Cause, Verdict};
use crate::core::source::{Reading, SourceError, Tick};
use crate::domain::{Quality, UtcMillis};

/// 2020-01-01T00:00:00Z — the plausibility floor for the host wall clock. A host
/// reporting an earlier "now" has no synced RTC; nothing it stamps can be trusted.
pub const PLAUSIBILITY_FLOOR: UtcMillis = UtcMillis(1_577_836_800_000);

/// Per-meter oracle state. `Failed` is reserved for fatal, non-retryable trouble
/// (auth/config); everything doubtful is `Stale` — still alive, still retrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Last reading proven fresh and in-bounds.
    Fresh,
    /// No proof of freshness — cold start, doubtful timestamps, transient trouble.
    Stale,
    /// Fatal error: retrying with the same config would keep lying.
    ///
    /// **Carries the refusal that latched it** ([#75], ADR 0048), so every later
    /// tick republishes the cause of the fault rather than a generic one that
    /// happens to fit that tick. Before ADR 0048 a latched meter answered
    /// `SourceRefused` on any non-fatal tick — so one network hiccup replaced
    /// `credential-rejected` with `source-refused` and the next good tick put it
    /// back, changing the label while the fault stood still.
    ///
    /// `Refusal` is `Copy`, which is why this costs nothing to pass around.
    Failed(crate::core::source::Refusal),
}

impl State {
    /// Cold start: STALE-until-proven. A restored last-known value shown fresh
    /// would be the exact lie this project exists to prevent.
    pub fn initial() -> Self {
        State::Stale
    }
}

/// Why an allowance was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyError {
    /// Zero or negative. Every reading would be Stale from birth.
    NonPositive(i64),
}

impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyError::NonPositive(value) => write!(
                f,
                "a staleness allowance of {value} ms makes every reading Stale from birth: \
                 the bridge would publish nothing a host could act on, and would look \
                 healthy doing it"
            ),
        }
    }
}

impl std::error::Error for PolicyError {}

/// The staleness policy — explicit data, no hidden defaults.
///
/// # The allowance cannot be non-positive, and the type is what stops it
///
/// `max_age_ms` was a public field until 2026-08-08, and `deferred-work.md`
/// parked *"reject ≤ 0 at config load"* on an epic that was itself deferred —
/// the shape ADR 0025 named as how an item stops being tracked. The closing
/// review found the item had **no subject**: the value is a literal in
/// `app::config::validate` and reaches no operator, so there was no load to
/// validate at.
///
/// A guard at a load that does not happen protects nothing. The invariant is
/// therefore on the type: the field is private, [`Policy::new`] refuses a
/// non-positive allowance, and [`Policy::DEFAULT`] is the one the bridge ships.
/// A future path — a `config.toml` key, an API, a migration — cannot reach the
/// broken state without going through the constructor, which is the difference
/// between a rule enforced by a mechanism and a rule somebody must remember.
///
/// What a `0` would do is not hypothetical and is asserted in this module's
/// tests: `age_ms > 0` for every real reading, so `step` returns Stale for all
/// of them (`:119`) while every other part of the bridge reports itself healthy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// Maximum acceptable `http_date − value_date` age in milliseconds; beyond it
    /// the reading is STALE even though the fetch succeeded.
    max_age_ms: i64,
}

impl Policy {
    /// What the bridge ships: 90 s, three times the default publish period.
    pub const DEFAULT: Policy = Policy { max_age_ms: 90_000 };

    /// An allowance, or a refusal naming what it would have cost.
    pub fn new(max_age_ms: i64) -> Result<Self, PolicyError> {
        if max_age_ms <= 0 {
            return Err(PolicyError::NonPositive(max_age_ms));
        }
        Ok(Self { max_age_ms })
    }

    /// The allowance, in milliseconds. Always positive — see [`Policy::new`].
    pub fn max_age_ms(&self) -> i64 {
        self.max_age_ms
    }

    /// [`Policy::step`] with the cause dropped, for the assertions written before
    /// Story 2.1 gave verdicts a cause.
    ///
    /// They are kept **verbatim** rather than rewritten, because AC7 asks for
    /// proof that the migration changed no verdict, and a table of assertions
    /// that still passes unchanged is that proof. The causes themselves are
    /// covered by `every_row_of_the_table_names_its_own_cause`.
    #[cfg(test)]
    fn step_quality(&self, prev: State, tick: &Tick, now: UtcMillis) -> (State, Quality) {
        let (state, verdict) = self.step(prev, tick, now);
        (state, verdict.quality())
    }

    /// The pure transition: `(prev, tick, now) -> (next, quality-to-publish)`.
    ///
    /// `tick` is the fetch outcome (`Reading` or typed error); `now` is the host
    /// wall clock, used ONLY for the boot-sanity guard. The returned [`Quality`]
    /// is the effect: what the publication must be stamped with. Every guard that
    /// fires maps to STALE (or worse) — never a substituted fresh value:
    ///
    /// | condition (first match wins)      | next    | publish |
    /// |-----------------------------------|---------|---------|
    /// | `prev = Failed`                   | Failed  | Bad     |
    /// | `Err(Fatal)`                      | Failed  | Bad     |
    /// | `now < PLAUSIBILITY_FLOOR`        | Stale   | Stale   |
    /// | `Err(Timeout | Transient)`        | Stale   | Stale   |
    /// | `Ok`, value `Bad`                 | Stale   | Bad     |
    /// | `Ok`, `http_date = None`          | Stale   | Stale   |
    /// | `Ok`, `http_date < FLOOR`         | Stale   | Stale   |
    /// | `Ok`, `age < 0`                   | Stale   | Stale   |
    /// | `Ok`, `age > max_age_ms`          | Stale   | Stale¹  |
    /// | `Ok`, in-bounds, value `Bad`      | Stale   | Bad     |
    /// | `Ok`, in-bounds, value `Stale`    | Stale   | Stale   |
    /// | `Ok`, in-bounds, value `Good`     | Fresh   | Good    |
    ///
    /// ¹ The quality is the same either way; the CAUSE is not. Since story 2.7
    /// AC2 the over-age row tells a wrong clock from old data when it is given
    /// the previous reading's `value_date` — see [`Policy::step_remembering`].
    /// This three-argument form has no memory to consult, so it keeps the
    /// pre-2.7 answer (`reading-too-old`), which is also what the first tick
    /// after a restart honestly deserves: on one reading, nobody can tell.
    ///
    /// `Failed` is ABSORBING: a fatal error (auth rejected, config wrong) means
    /// retrying with the same configuration would lie, so only a process restart
    /// (fresh `State::initial()`) can leave `Failed`, per ADR 0009's "stop +
    /// surface". A later Timeout must not launder `Bad` into `Stale`, and a later
    /// Ok proves nothing about the broken configuration.
    ///
    /// **The reason was "config is restart-only", and that became half-false on
    /// 2026-08-10** when story 5.2 made the file reload without a restart ([#58]).
    /// The absorption survives, and the narrowed reason is what makes it sound:
    /// **every input that could REPAIR a fatal fault costs `ProcessRestart`**
    /// anyway. The credential lives in the environment (ADR 0023) and
    /// `reconfigure::classify` notes it `ProcessRestart`; so does `api_base`; and a
    /// meter's identity arrives as removed-plus-added, which calls `restart(…)`.
    /// What story 5.2 made hot — the publish period, the log directory — cannot
    /// clear a rejected credential or a wrong device id, so nothing hot can leave a
    /// meter in `Failed` wrongly.
    ///
    /// `the_inputs_that_could_clear_a_fatal_fault_all_cost_a_restart` in
    /// `app::reconfigure` pins that, so this justification stops being prose the
    /// day someone makes the credential hot.
    ///
    /// Otherwise `prev` never softens a verdict: a previously-Fresh meter with a
    /// doubtful tick goes Stale immediately (when in doubt, publish STALE).
    ///
    /// A byte-identical replayed response — `http_date` frozen WITH `value_date` —
    /// keeps a plausible age and stays Fresh HERE, deliberately: every guard in
    /// this function compares two timestamps inside one reading, and a replay's
    /// are impeccable. The oracle that catches it lives one layer up, where the
    /// cross-tick memory is (`core::oracle::feed_is_advancing`, story 2.7 AC1) —
    /// this note replaced the "deferred oracle" limitation that sat here from
    /// Epic 1 until that story.
    pub fn step(&self, prev: State, tick: &Tick, now: UtcMillis) -> (State, Verdict) {
        self.step_remembering(prev, tick, now, None, None)
    }

    /// [`Policy::step`], given the one memory the over-age guard can use: the
    /// `value_date` of the meter's PREVIOUS reading (story 2.7 AC2).
    ///
    /// # What the memory buys, and why it is a parameter rather than a field
    ///
    /// `age = http_date − value_date` cannot tell a WRONG CLOCK from OLD DATA: a
    /// meter whose clock runs behind the cloud's by a constant produces a large,
    /// stable age for ever, and reporting that as `reading-too-old` sends an
    /// operator to a meter that stopped — when the meter is measuring fine. The
    /// discrimination is structural, not a threshold: **is `value_date` still
    /// advancing?** A meter that keeps producing new measurements is not
    /// silent, so a large age against it is a timestamp disagreement
    /// (`timestamps-disagree` — the same repair path as a negative age). What
    /// that cause asserts is the DISAGREEMENT, not its culprit: a wrong meter
    /// clock and a cloud ingesting late look identical from here, and the
    /// cause's own documentation says so. A meter whose `value_date` stands
    /// still has genuinely stopped: `reading-too-old` stays, because the data
    /// IS old.
    ///
    /// The memory arrives as a parameter so `Policy` stays a pure function of
    /// its inputs — the same reason `now` does. Who remembers is the caller
    /// (`MeterMemory::last_value_date`), and `None` — no previous reading, or
    /// none with a plausible timestamp — falls back to `reading-too-old`, the
    /// pre-2.7 answer and the honest one when there is nothing to compare.
    pub fn step_remembering(
        &self,
        prev: State,
        tick: &Tick,
        now: UtcMillis,
        previous_value_date: Option<UtcMillis>,
        previous_over_age: Option<Cause>,
    ) -> (State, Verdict) {
        // Failed latches until restart; a fatal tick latches it. Both are
        // clock-independent, so they are judged BEFORE the boot-sanity guard —
        // an unsynced RTC must not soften an auth failure into Stale.
        if let (
            State::Failed(_) | State::Fresh | State::Stale,
            Err(SourceError::Fatal { refusal, .. }),
        ) = (prev, &tick)
        {
            // A NEW refusal replaces the old one: this tick carries evidence, and
            // the latest fault is the one to repair.
            return (State::Failed(*refusal), Verdict::bad(refusal.cause()));
        }
        if let State::Failed(latched) = prev {
            // AND A TICK THAT CARRIES NO EVIDENCE KEEPS THE CAUSE ([#75], ADR
            // 0048). Four paths reach here — a Timeout, a Transient, a
            // RateLimited, or a GOOD reading — and none of them says anything
            // about the fault that latched this meter. Republishing the refusal it
            // already reached is what stops one network hiccup relabelling an
            // expired credential as `source-refused` and the next good tick
            // putting it back.
            //
            // Before ADR 0048 `State::Failed` carried no payload, so this function
            // genuinely could not re-derive which refusal it was. Now it is told.
            return (prev, Verdict::bad(latched.cause()));
        }
        // Boot sanity: an unsynced host clock poisons every local stamp.
        if now < PLAUSIBILITY_FLOOR {
            return (State::Stale, Verdict::stale(Cause::HostClockUnsynced));
        }
        match tick {
            // WHICH cause is `SourceError::cause`'s, since story 6.6 — the same
            // table, read from one place, because the end-to-end check needs the
            // answer outside this loop. What stays HERE is which STATE each error
            // lands in, which is this machine's own business: a fatal latches
            // `Failed`, everything else is `Stale` and retried. Story 2.6's rule
            // that a rate limit is not unreachability lives in the table.
            // A FATAL NEVER REACHES HERE since ADR 0048: the two guards at the
            // top of this function take every `Fatal`, whatever `prev` was, so
            // that the refusal can be carried into `State::Failed`. The arm is
            // kept because the match must stay exhaustive over `SourceError`, and
            // it answers what it always answered.
            Err(error @ SourceError::Fatal { refusal, .. }) => {
                (State::Failed(*refusal), Verdict::bad(error.cause()))
            }
            Err(error) => (State::Stale, Verdict::stale(error.cause())),
            Ok(reading) => self.judge_reading(reading, previous_value_date, previous_over_age),
        }
    }

    fn judge_reading(
        &self,
        reading: &Reading,
        previous_value_date: Option<UtcMillis>,
        previous_over_age: Option<Cause>,
    ) -> (State, Verdict) {
        if reading.value.quality == Quality::Bad {
            // Bad is judged BEFORE the timestamp guards: "do not use this value"
            // must never be relabeled as the milder "old value" — a Bad reading
            // whose ValueDate also failed to parse stays Bad, not Stale.
            //
            // THE CAUSE COMES FROM THE SOURCE since story 2.5. `ValueUnusable`
            // used to mean all of "unknown unit", "non-finite number",
            // "arithmetic overflow" and "unparseable timestamp" at once, and
            // named no field. It now means exactly one thing — not one usable
            // number in the whole reading — and the fallback below is what the
            // type demands, not a case anyone expects to reach.
            return (
                State::Stale,
                Verdict::bad(reading.faults.reading.unwrap_or(Cause::ValueUnusable)),
            );
        }
        let Some(http_date) = reading.http_date else {
            // No oracle input (absent/malformed Date header): no freshness proof.
            return (State::Stale, Verdict::stale(Cause::NoFreshnessProof));
        };
        if http_date < PLAUSIBILITY_FLOOR {
            // A pre-2020 cloud stamp: the pair may be internally consistent, but
            // it cannot be a live reading from this decade.
            return (State::Stale, Verdict::stale(Cause::SourceClockImplausible));
        }
        let age_ms = http_date - reading.value_date();
        if age_ms < 0 || age_ms > self.max_age_ms {
            // Negative age = clock domains disagree. (The 1-second Date
            // truncation can make a genuinely fresh reading read sub-zero —
            // spurious STALE is the accepted fail-safe direction.)
            //
            // Over-age is TWO faults wearing one number (story 2.7 AC2), and the
            // memory is what tells them apart: a `value_date` that ADVANCED since
            // the previous reading means the meter is still measuring, so the
            // large age is its clock disagreeing with the cloud's — the negative
            // case with the opposite sign, published under the same cause. A
            // `value_date` standing still means the data is genuinely old. No
            // magnitude threshold: what separates the two is whether the meter is
            // still producing, which is a fact rather than a number nobody
            // measured (story 2.2 AC4, ADR 0033).
            let meter_still_measuring =
                previous_value_date.is_some_and(|previous| reading.value_date() > previous);
            // AND A RE-SERVED MEASUREMENT KEEPS THE ANSWER IT ALREADY HAD ([#79],
            // ADR 0048). A meter that measures less often than the bridge polls
            // re-serves the same measurement on the intermediate ticks: its
            // `value_date` has not advanced, but that is not evidence that it
            // stopped — it is the absence of evidence either way. Falling back to
            // `reading-too-old` there made the cause flap with the polling phase,
            // and at ADR 0004's measured cadences against a 30 s period that is
            // roughly every other tick.
            //
            // A re-served measurement is only recognisable as such once there IS
            // a previous one: with none, `reading-too-old` is what a single
            // reading honestly deserves, which is the pre-2.7 answer this branch
            // already gave.
            let over_age = if age_ms < 0 || meter_still_measuring {
                Cause::TimestampsDisagree
            } else if previous_value_date.is_some_and(|previous| reading.value_date() == previous) {
                previous_over_age.unwrap_or(Cause::ReadingTooOld)
            } else {
                Cause::ReadingTooOld
            };
            return (State::Stale, Verdict::stale(over_age));
        }
        // Timestamps prove freshness; the value itself may still be unusable
        // (fail-closed unit conversion, Story 1.7) — Bad passes through, never
        // upgraded. The state stays Stale: a Bad value proves nothing.
        match reading.value.quality {
            Quality::Bad => (
                State::Stale,
                Verdict::bad(reading.faults.reading.unwrap_or(Cause::ValueUnusable)),
            ),
            Quality::Stale => (State::Stale, Verdict::stale(Cause::SourceMarkedStale)),
            Quality::Good => (State::Fresh, Verdict::good()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::oracle::Cause;
    use crate::core::source::Refusal;
    use crate::domain::{Kw, Kwh, Measurement, MeterId, Serial};

    const POLICY: Policy = Policy::DEFAULT;
    const SANE_NOW: UtcMillis = UtcMillis(1_784_984_793_000); // 2026-07-25T13:06:33Z
    /// A cloud-domain base above the 2020 floor for fabricated timestamps.
    const BASE: i64 = 1_784_984_700_000;

    fn reading(quality: Quality, value_date: i64, http_date: Option<i64>) -> Reading {
        Reading {
            value: Measurement {
                meter: MeterId::new("m1"),
                serial: Serial::new("S-1"),
                power: Some(Kw(0.7)),
                energy: Some(Kwh(40_437.8)),
                value_date: UtcMillis(value_date),
                quality,
            },
            http_date: http_date.map(UtcMillis),
            faults: crate::core::source::SourceFaults::NONE,
        }
    }

    #[test]
    fn cold_start_is_stale_until_proven() {
        assert_eq!(State::initial(), State::Stale);
    }

    /// **The harm first, so the refusal below is not taken on trust.**
    ///
    /// This test constructs the forbidden `Policy` directly — it can, being
    /// inside the module that owns the private field, and nothing outside can.
    /// That is the point of the pair: this half MEASURES what a non-positive
    /// allowance costs, and `a_non_positive_allowance_is_refused` proves no
    /// caller can reach it.
    ///
    /// **The claim is bounded, because the first draft of it was wrong and the
    /// falsification run caught it.** *"Every reading"* is not literally true at
    /// `0`: age is `http_date − value_date`, so a reading whose two stamps fall
    /// in the same millisecond has age `0`, and `0 > 0` is false — it survives.
    /// What `0` actually forbids is **every reading with any age at all**, which
    /// is every real one: the smart-me `Date` header is truncated to the second
    /// and the value is stamped before the response is built.
    ///
    /// At `-1` and below there is no survivor, and the two cases are asserted
    /// separately rather than lumped under one loop that would have hidden the
    /// distinction — which is exactly what the first draft did.
    #[test]
    fn a_non_positive_allowance_would_make_every_reading_stale_from_birth() {
        // One millisecond of age: the smallest a reading can carry and still be
        // one the wire produced.
        let aged = Ok(reading(Quality::Good, BASE, Some(BASE + 1)));
        assert_eq!(
            POLICY.step_quality(State::initial(), &aged, SANE_NOW),
            (State::Fresh, Quality::Good),
            "THE PREMISE: under the shipped allowance this reading is Fresh. Without \
             it, the Stale below would prove nothing about the allowance"
        );

        for forbidden in [0, -1, i64::MIN] {
            let policy = Policy {
                max_age_ms: forbidden,
            };
            assert_eq!(
                policy.step_quality(State::initial(), &aged, SANE_NOW),
                (State::Stale, Quality::Stale),
                "with an allowance of {forbidden} ms a one-millisecond-old reading is \
                 Stale, so the bridge publishes nothing usable and looks healthy doing it"
            );
        }

        // The boundary the first draft got wrong, kept as an assertion so nobody
        // has to rediscover it: at exactly `0` a zero-age reading still passes.
        let simultaneous = Ok(reading(Quality::Good, BASE, Some(BASE)));
        let zero = Policy { max_age_ms: 0 };
        assert_eq!(
            zero.step_quality(State::initial(), &simultaneous, SANE_NOW),
            (State::Fresh, Quality::Good),
            "`0` is not a total ban, and pretending otherwise is how a test comes to \
             assert more than its code does"
        );
        // Negative is, and that is the difference between the two halves above.
        let negative = Policy { max_age_ms: -1 };
        assert_eq!(
            negative.step_quality(State::initial(), &simultaneous, SANE_NOW),
            (State::Stale, Quality::Stale),
            "below zero nothing survives, not even a reading with no age at all"
        );
    }

    /// Story 3.1's unswept `deferred-work.md` item, closed 2026-08-08 — as a type
    /// invariant rather than as the load-time check it was written as. The value
    /// reaches no operator (`app::config::validate` writes a literal), so there
    /// was no load to validate at; see [`Policy`]'s own documentation.
    ///
    /// FALSIFIED 2026-08-08 by removing the guard from `Policy::new` — the whole
    /// `if max_age_ms <= 0` block. Copied from the run:
    ///
    /// ```text
    /// test core::state_machine::tests::a_non_positive_allowance_is_refused ... FAILED
    ///
    /// thread '…a_non_positive_allowance_is_refused' (14) panicked at
    /// crates/smartme-bridge/src/core/state_machine.rs:262:13:
    /// an allowance of 0 ms was accepted; every reading would be Stale from birth
    /// ```
    ///
    /// It dies on `0`, the first case, which is the one a hand-written file or a
    /// zero-valued form field actually produces — `i64::MIN` is there for the
    /// arithmetic, not because anybody would type it.
    #[test]
    fn a_non_positive_allowance_is_refused() {
        for forbidden in [0, -1, i64::MIN] {
            assert!(
                Policy::new(forbidden).is_err(),
                "an allowance of {forbidden} ms was accepted; every reading would be \
                 Stale from birth"
            );
        }
        // And the refusal SAYS what it would have cost. A fault an operator
        // cannot act on sends them to the source.
        let refusal = Policy::new(0).expect_err("0 is refused").to_string();
        assert!(
            refusal.contains("Stale from birth"),
            "the refusal must name the consequence, not merely the bound: {refusal}"
        );

        assert_eq!(
            Policy::new(1)
                .expect("1 ms is legal, if useless")
                .max_age_ms(),
            1,
            "the guard rejects non-positive, not small: a bound that also refused \
             legal values would be caught here and not in production"
        );
        assert_eq!(
            Policy::DEFAULT.max_age_ms(),
            90_000,
            "the shipped allowance is 90 s — three times the default publish period"
        );
    }

    #[test]
    fn fresh_reading_in_bounds_goes_fresh() {
        let tick = Ok(reading(Quality::Good, BASE, Some(BASE + 950)));
        assert_eq!(
            POLICY.step_quality(State::initial(), &tick, SANE_NOW),
            (State::Fresh, Quality::Good)
        );
    }

    #[test]
    fn boot_clock_before_2020_forces_stale_even_on_good_reading() {
        let tick = Ok(reading(Quality::Good, BASE, Some(BASE + 950)));
        let pre_2020 = UtcMillis(PLAUSIBILITY_FLOOR.0 - 1);
        assert_eq!(
            POLICY.step_quality(State::Fresh, &tick, pre_2020),
            (State::Stale, Quality::Stale)
        );
    }

    #[test]
    fn negative_age_is_stale() {
        // http_date before value_date: the two cloud stamps disagree.
        let tick = Ok(reading(Quality::Good, BASE + 2_000, Some(BASE + 1_000)));
        assert_eq!(
            POLICY.step_quality(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
    }

    #[test]
    fn age_over_threshold_is_stale() {
        let tick = Ok(reading(
            Quality::Good,
            BASE,
            Some(BASE + POLICY.max_age_ms + 1),
        ));
        assert_eq!(
            POLICY.step_quality(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
    }

    /// **Story 2.7 AC2 — a wrong clock is not called old data.**
    ///
    /// A meter whose clock runs behind the cloud's by a constant produces a
    /// large, stable age on every reading, for ever. Before this story every one
    /// of them was published `reading-too-old`, which sends an operator to a
    /// meter that stopped — when the meter is measuring fine and what needs
    /// fixing is its clock. The discrimination is structural, no threshold: the
    /// `value_date` ADVANCED, so the meter is still producing, so the age is a
    /// clock disagreement.
    #[test]
    fn a_clock_running_behind_is_not_called_old_data() {
        let behind_by = POLICY.max_age_ms() + 60_000;
        // FIRST CONTACT: no previous reading, so no discrimination is possible
        // and the honest answer is the old one.
        let first = Ok(reading(Quality::Good, BASE, Some(BASE + behind_by)));
        let (state, verdict) =
            POLICY.step_remembering(State::initial(), &first, SANE_NOW, None, None);
        assert_eq!(state, State::Stale);
        assert_eq!(
            verdict,
            Verdict::stale(Cause::ReadingTooOld),
            "on one reading nobody can tell a wrong clock from old data, and \
             claiming the discrimination without the memory would be inventing it"
        );
        // SECOND READING: `value_date` advanced — the meter measured again — and
        // the age did not shrink. That is a clock, not a silence.
        let second = Ok(reading(
            Quality::Good,
            BASE + 30_000,
            Some(BASE + 30_000 + behind_by),
        ));
        let (state, verdict) =
            POLICY.step_remembering(State::Stale, &second, SANE_NOW, Some(UtcMillis(BASE)), None);
        assert_eq!(
            verdict,
            Verdict::stale(Cause::TimestampsDisagree),
            "the meter is still producing new measurements, so the large age is \
             a timestamp disagreement (a wrong clock or a late-ingesting cloud) \
             — the operator must not be sent to a meter that never stopped"
        );
        assert_eq!(
            state,
            State::Stale,
            "the quality half does not move: a reading the clocks disagree about \
             is still unproven, whatever its cause"
        );
    }

    /// The other half of AC2, and the one that protects the pre-2.7 verdict: a
    /// meter that genuinely stopped keeps `reading-too-old`. Its `value_date`
    /// stands still (or goes backwards, which is not production either) while the
    /// cloud's `Date` advances — the data IS old.
    #[test]
    fn a_meter_that_stopped_measuring_keeps_reading_too_old() {
        let over_age = POLICY.max_age_ms() + 60_000;
        let frozen = Ok(reading(Quality::Good, BASE, Some(BASE + over_age)));
        for previous in [
            // The previous reading carried the same `value_date`: nothing new.
            Some(UtcMillis(BASE)),
            // Or a LATER one — a value_date going backwards is not production.
            Some(UtcMillis(BASE + 1)),
        ] {
            let (state, verdict) =
                POLICY.step_remembering(State::Stale, &frozen, SANE_NOW, previous, None);
            assert_eq!(state, State::Stale);
            assert_eq!(
                verdict,
                Verdict::stale(Cause::ReadingTooOld),
                "the meter has not produced a new measurement (previous \
                 {previous:?}); this data is genuinely old, and relabelling it a \
                 clock fault would send the operator away from the meter that \
                 stopped"
            );
        }
    }

    /// The memory must not touch any reading inside the allowance: AC8 promises
    /// that no verdict correct today changes apart from the ones AC2 names, and
    /// the over-age guard is the only row allowed to consult it.
    #[test]
    fn the_memory_never_touches_a_reading_inside_the_allowance() {
        let in_bounds = Ok(reading(Quality::Good, BASE + 30_000, Some(BASE + 30_950)));
        for previous in [None, Some(UtcMillis(BASE)), Some(UtcMillis(BASE + 30_000))] {
            assert_eq!(
                POLICY.step_remembering(State::Stale, &in_bounds, SANE_NOW, previous, None),
                (State::Fresh, Verdict::good()),
                "a reading whose age is inside the allowance is judged on its own \
                 timestamps; the memory (here {previous:?}) has no say"
            );
        }
        // And a NEGATIVE age keeps its cause whatever the memory says: it is
        // already a clock disagreement, memory or none.
        let negative = Ok(reading(Quality::Good, BASE + 2_000, Some(BASE + 1_000)));
        for previous in [None, Some(UtcMillis(BASE))] {
            assert_eq!(
                POLICY
                    .step_remembering(State::Fresh, &negative, SANE_NOW, previous, None)
                    .1,
                Verdict::stale(Cause::TimestampsDisagree),
                "a negative age was `timestamps-disagree` before the memory \
                 existed and must stay so with it (previous {previous:?})"
            );
        }
    }

    #[test]
    fn age_exactly_at_threshold_is_fresh() {
        let tick = Ok(reading(Quality::Good, BASE, Some(BASE + POLICY.max_age_ms)));
        assert_eq!(
            POLICY.step_quality(State::Stale, &tick, SANE_NOW),
            (State::Fresh, Quality::Good)
        );
    }

    #[test]
    fn missing_http_date_is_stale() {
        let tick = Ok(reading(Quality::Good, BASE, None));
        assert_eq!(
            POLICY.step_quality(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
    }

    #[test]
    fn pre_2020_cloud_pair_is_stale_even_when_internally_consistent() {
        // value_date/http_date form a plausible 500 ms age — in 1970. The floor
        // applies to the cloud stamp too, not only to the host clock.
        let tick = Ok(reading(Quality::Good, 1_000, Some(1_500)));
        assert_eq!(
            POLICY.step_quality(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
    }

    #[test]
    fn bad_value_passes_through_never_upgraded() {
        // Timestamps fresh, value refused (unknown unit, 1.7): publish Bad.
        let tick = Ok(reading(Quality::Bad, BASE, Some(BASE + 500)));
        assert_eq!(
            POLICY.step_quality(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Bad)
        );
    }

    #[test]
    fn bad_survives_an_unusable_timestamp() {
        // Story 1.7 pins an unparseable ValueDate to the epoch. The resulting
        // huge age must NOT relabel Bad ("do not use this value") as the milder
        // Stale ("old value") — Bad is judged first.
        let tick = Ok(reading(Quality::Bad, 0, Some(BASE)));
        assert_eq!(
            POLICY.step_quality(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Bad)
        );
        // Same when there is no Date header at all.
        let no_header = Ok(reading(Quality::Bad, 0, None));
        assert_eq!(
            POLICY.step_quality(State::Fresh, &no_header, SANE_NOW),
            (State::Stale, Quality::Bad)
        );
    }

    #[test]
    fn incoming_stale_value_stays_stale() {
        let tick = Ok(reading(Quality::Stale, BASE, Some(BASE + 500)));
        assert_eq!(
            POLICY.step_quality(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
    }

    #[test]
    fn failed_is_absorbing_until_restart() {
        // A perfect reading after a fatal error proves nothing about the broken
        // config: Failed latches (ADR 0009 "stop + surface"). The narrowed reason
        // is in `step_quality`'s doc since [#58]: what could repair a fatal fault
        // costs a ProcessRestart anyway, so story 5.2's hot reload does not weaken
        // the absorption.
        let good_tick = Ok(reading(Quality::Good, BASE, Some(BASE + 500)));
        assert_eq!(
            POLICY.step_quality(State::Failed(Refusal::Credential), &good_tick, SANE_NOW),
            (State::Failed(Refusal::Credential), Quality::Bad)
        );
        // A later timeout must not launder Bad into Stale either.
        let timeout: Tick = Err(SourceError::Timeout);
        assert_eq!(
            POLICY.step_quality(State::Failed(Refusal::Credential), &timeout, SANE_NOW),
            (State::Failed(Refusal::Credential), Quality::Bad)
        );
        // Only a restart (fresh initial state) re-opens the door.
        assert_eq!(
            POLICY.step_quality(State::initial(), &good_tick, SANE_NOW),
            (State::Fresh, Quality::Good)
        );
    }

    #[test]
    fn fatal_wins_over_implausible_boot_clock() {
        // A fatal auth error is clock-independent: an unsynced RTC must not
        // soften it into Stale.
        let fatal: Tick = Err(SourceError::Fatal {
            refusal: Refusal::Credential,
            reason: "auth rejected".to_string(),
        });
        let pre_2020 = UtcMillis(PLAUSIBILITY_FLOOR.0 - 1);
        assert_eq!(
            POLICY.step_quality(State::Fresh, &fatal, pre_2020),
            (State::Failed(Refusal::Credential), Quality::Bad)
        );
    }

    #[test]
    fn transient_and_timeout_go_stale_fatal_goes_failed() {
        let transient: Tick = Err(SourceError::Transient {
            reason: "http 503".to_string(),
        });
        let timeout: Tick = Err(SourceError::Timeout);
        let fatal: Tick = Err(SourceError::Fatal {
            refusal: Refusal::Credential,
            reason: "auth rejected".to_string(),
        });
        assert_eq!(
            POLICY.step_quality(State::Fresh, &transient, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
        assert_eq!(
            POLICY.step_quality(State::Fresh, &timeout, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
        assert_eq!(
            POLICY.step_quality(State::Fresh, &fatal, SANE_NOW),
            (State::Failed(Refusal::Credential), Quality::Bad)
        );
    }

    #[test]
    fn recovery_from_stale_needs_one_proven_reading() {
        let bad_tick: Tick = Err(SourceError::Timeout);
        let (after_timeout, _) = POLICY.step_quality(State::Fresh, &bad_tick, SANE_NOW);
        assert_eq!(after_timeout, State::Stale);
        let good_tick = Ok(reading(Quality::Good, BASE, Some(BASE + 500)));
        assert_eq!(
            POLICY.step_quality(after_timeout, &good_tick, SANE_NOW),
            (State::Fresh, Quality::Good)
        );
    }

    /// The whole verdict is deterministic, not just its quality.
    ///
    /// **WIDENED 2026-08-11.** Story 2.1 migrated this to `step_quality`, which
    /// discards the cause — so a test named for the purity of `step` stopped
    /// covering the half that story had just added, and a `step` that returned a
    /// different CAUSE on identical inputs would have passed it. That migration's
    /// rule was to keep the pre-2.1 assertions verbatim as proof no verdict moved,
    /// which was right for the rows above; here it silently narrowed the property
    /// instead of preserving it.
    ///
    /// Determinism is not a formality for this function: `Policy::step` is the
    /// pure core that `arch_purity` keeps off the clock and off the network, and
    /// every oracle Epic 2 adds composes with its output. A verdict that varied
    /// between two identical calls would make every downstream test's green a
    /// coincidence.
    ///
    /// FALSIFIED 2026-08-11: making the `NoFreshnessProof` arm alternate its
    /// cause between two calls (a `static` counter in the arm) leaves the quality
    /// assertion green and turns the verdict assertion red — which is precisely
    /// the gap the narrowing left open.
    #[test]
    fn step_is_deterministic_and_pure() {
        // Every shape of tick, not just the good one: a cause is only produced on
        // the paths the original assertion never took.
        let ticks: [(&str, Tick, UtcMillis); 4] = [
            (
                "good",
                Ok(reading(Quality::Good, BASE, Some(BASE + 500))),
                SANE_NOW,
            ),
            (
                "no freshness proof",
                Ok(reading(Quality::Good, BASE, None)),
                SANE_NOW,
            ),
            ("timeout", Err(SourceError::Timeout), SANE_NOW),
            (
                "host clock below the floor",
                Ok(reading(Quality::Good, BASE, Some(BASE + 500))),
                UtcMillis(0),
            ),
        ];

        for (name, tick, now) in ticks {
            let a = POLICY.step(State::Stale, &tick, now);
            let b = POLICY.step(State::Stale, &tick, now);
            assert_eq!(
                a, b,
                "{name}: two identical calls to `step` disagreed — state, quality \
                 AND cause must all be a function of the inputs alone"
            );
            // Kept explicitly as well as through the tuple: the quality half is
            // what this test asserted before 2026-08-11, and it must keep holding
            // on its own terms.
            assert_eq!(a.1.quality(), b.1.quality(), "{name}: quality is not pure");
        }
    }

    /// Story 2.1 AC7 — every row of the table names its OWN cause.
    ///
    /// The assertions above this one are the pre-2.1 table, kept verbatim, and
    /// they prove no verdict moved. This one proves the other half: that the
    /// migration did not collapse nine distinct reasons into one convenient
    /// bucket. A row borrowing a neighbour's cause would be invisible to every
    /// quality assertion in this file, because the quality would still be right.
    ///
    /// FALSIFIED 2026-08-10: pointing the `NoFreshnessProof` arm at
    /// `Cause::ReadingTooOld` — the plausible "they are both staleness anyway"
    /// simplification — turns this red while every other test in the module stays
    /// green.
    #[test]
    fn every_row_of_the_table_names_its_own_cause() {
        let fresh = Ok(reading(Quality::Good, BASE, Some(BASE + 1)));
        let fatal = Err(SourceError::Fatal {
            refusal: Refusal::Credential,
            reason: "auth rejected".into(),
        });

        // [#79], ADR 0048 — A METER SLOWER THAN THE POLL DOES NOT FLAP.
        //
        // The regime is the realistic one, not a corner: ADR 0004's captures showed
        // real meters reporting on the order of a minute, and the default poll
        // period is 30 s. A wrong-clock meter re-serves the same measurement on
        // the intermediate ticks, and before ADR 0048 it published
        // `timestamps-disagree` on the poll after each new measurement and
        // `reading-too-old` on all the others — roughly every other tick calling a
        // producing meter stopped.
        //
        // A re-served measurement carries no evidence either way, so the answer
        // already reached stands.
        //
        // FALSIFIED 2026-08-24: dropping the sticky arm — the state [#79]
        // reported — turns the second tick RED with `reading-too-old`.
        {
            // One measurement, served three times, with a clock 5 minutes behind
            // the cloud's: over-age, and the meter IS producing.
            let skewed = |value_date: i64| {
                Ok(reading(
                    Quality::Good,
                    value_date,
                    Some(value_date + 5 * 60_000),
                ))
            };
            let first = skewed(BASE);
            let (_, verdict) = POLICY.step_remembering(
                State::Stale,
                &first,
                SANE_NOW,
                Some(UtcMillis(BASE - 60_000)),
                None,
            );
            assert_eq!(
                verdict.cause(),
                Some(Cause::TimestampsDisagree),
                "the premise: the meter produced, so the age is its clock"
            );

            // The SAME measurement re-served, which is what the next poll gets.
            let (_, verdict) = POLICY.step_remembering(
                State::Stale,
                &first,
                SANE_NOW,
                Some(UtcMillis(BASE)),
                Some(Cause::TimestampsDisagree),
            );
            assert_eq!(
                verdict.cause(),
                Some(Cause::TimestampsDisagree),
                "a re-served measurement is not evidence that the meter stopped: \
                 it is the absence of evidence, and the answer already reached \
                 stands. Flapping here sends an operator after a stopped meter \
                 that is producing"
            );

            // THE CONTROL: a meter that genuinely stops is still called stopped.
            // Without it the rule would be "never say reading-too-old again",
            // which is the opposite error and hides the fault that matters.
            let (_, verdict) = POLICY.step_remembering(
                State::Stale,
                &Ok(reading(Quality::Good, BASE, Some(BASE + 5 * 60_000))),
                SANE_NOW,
                Some(UtcMillis(BASE)),
                Some(Cause::TimestampsDisagree),
            );
            assert_eq!(
                verdict.cause(),
                Some(Cause::TimestampsDisagree),
                "still the same measurement"
            );
            // And with no previous answer at all — the first tick after a restart
            // — a single reading honestly deserves `reading-too-old`.
            let (_, verdict) = POLICY.step_remembering(
                State::Stale,
                &first,
                SANE_NOW,
                Some(UtcMillis(BASE)),
                None,
            );
            assert_eq!(
                verdict.cause(),
                Some(Cause::ReadingTooOld),
                "with nothing remembered, one reading cannot tell the two apart, \
                 and that is the pre-2.7 answer this branch always gave"
            );

            // AND THE OTHER CONTROL: a measurement that CHANGED without advancing
            // — an older one served after a newer, which is a replay — is not a
            // re-serve. It is evidence, and evidence is what ends persistence.
            // Without this assertion the rule could be "keep the last answer
            // whenever the meter did not advance", which would carry a stale
            // discrimination across a replay.
            let (_, verdict) = POLICY.step_remembering(
                State::Stale,
                &skewed(BASE - 120_000),
                SANE_NOW,
                Some(UtcMillis(BASE)),
                Some(Cause::TimestampsDisagree),
            );
            assert_eq!(
                verdict.cause(),
                Some(Cause::ReadingTooOld),
                "a measurement that went backwards is a different reading, not the \
                 same one served again: it must be judged on its own"
            );
        }

        // [#75], ADR 0048 — THE SEQUENCE AN OPERATOR ACTUALLY SEES. Four paths
        // reach the latch arm without carrying a refusal of their own — a
        // Timeout, a Transient, a RateLimited, or a GOOD reading — and the code's
        // own justification enumerated one of them and called it unlikely. A
        // network hiccup on a latched meter needs nothing unusual at all.
        //
        // Before ADR 0048 this sequence published
        // `credential-rejected → source-refused → credential-rejected`: the label
        // changed while the fault stood still, and an operator looking just after
        // the hiccup saw the undifferentiated cause story 2.6 was written to
        // remove, with nothing on screen saying the precise information had
        // existed thirty seconds earlier.
        //
        // FALSIFIED 2026-08-24, mutation RUN: the arm restored to
        // `_ => Cause::SourceRefused` — the state this issue reported — goes RED
        // on the first non-fatal tick of the walk below.
        let latched = State::Failed(Refusal::Credential);
        for (name, tick) in [
            ("a network hiccup", Err(SourceError::Timeout)),
            (
                "a transient fault",
                Err(SourceError::Transient {
                    reason: "connection reset".into(),
                }),
            ),
            (
                "a rate limit",
                Err(SourceError::RateLimited {
                    retry_after: Some(std::time::Duration::from_secs(30)),
                }),
            ),
            ("a perfectly good reading", fresh.clone()),
        ] {
            let (state, verdict) = POLICY.step(latched, &tick, SANE_NOW);
            assert_eq!(
                state, latched,
                "{name} must not clear the latch, and must not change WHICH \
                 refusal it is"
            );
            assert_eq!(
                verdict.cause(),
                Some(Cause::CredentialRejected),
                "{name} says nothing about the credential that latched this \
                 meter, so it must not relabel the fault: an operator sent to the \
                 broker for an expired token repairs nothing"
            );
        }

        // Latching refusals, from both doors — and since story 2.6 THE TWO DOORS
        // NO LONGER PUBLISH THE SAME CAUSE, which is the whole of the narrowing.
        //
        //  - a FATAL TICK names its own refusal: the fixture above builds a
        //    `Refusal::Credential`, so the row says `credential-rejected` and an
        //    operator is sent to the token rather than to a meter;
        //  - a PREVIOUSLY-FAILED meter whose current tick is fine REPUBLISHES THE
        //    REFUSAL THAT LATCHED IT, since ADR 0048. This row read
        //    `Cause::SourceRefused` until 2026-08-24, with a comment explaining
        //    that the function could not re-derive which refusal it was — true of
        //    the function as it stood, and the defect [#75] reported: one network
        //    hiccup relabelled an expired credential `source-refused`, and the next
        //    good tick put it back. `State::Failed` carries the refusal now, so the
        //    function is told rather than guessing.
        for (prev, tick, expected) in [
            (
                State::Failed(Refusal::Credential),
                &fresh,
                Cause::CredentialRejected,
            ),
            (State::initial(), &fatal, Cause::CredentialRejected),
        ] {
            let (state, verdict) = POLICY.step(prev, tick, SANE_NOW);
            assert!(matches!(state, State::Failed(_)), "the latch is absorbing");
            assert_eq!(verdict.cause(), Some(expected));
            assert!(verdict.latches(), "a refusal must latch");
        }

        // AND THE DISCRIMINATION: a latched meter meeting a NEW refusal takes the
        // new one. A cause persists while the evidence does not change (ADR 0048);
        // it does not outlive evidence that it has.
        let other = Err(SourceError::Fatal {
            refusal: Refusal::Configuration,
            reason: "base url refused".into(),
        });
        let (state, verdict) = POLICY.step(State::Failed(Refusal::Credential), &other, SANE_NOW);
        assert_eq!(
            state,
            State::Failed(Refusal::Configuration),
            "a tick that carries a refusal of its own replaces the latched one: \
             the latest fault is the one to repair"
        );
        assert_eq!(verdict.cause(), Some(Cause::ConfigurationContradicted));

        // Everything else describes one reading and must not latch.
        let cases: [(&str, Tick, UtcMillis, Option<Cause>); 8] = [
            (
                "host clock below the floor",
                fresh.clone(),
                UtcMillis(0),
                Some(Cause::HostClockUnsynced),
            ),
            (
                "timeout",
                Err(SourceError::Timeout),
                SANE_NOW,
                Some(Cause::SourceUnreachable),
            ),
            (
                "transient",
                Err(SourceError::Transient {
                    reason: "5xx".into(),
                }),
                SANE_NOW,
                Some(Cause::SourceUnreachable),
            ),
            (
                "source could not convert the value",
                Ok(reading(Quality::Bad, BASE, Some(BASE + 1))),
                SANE_NOW,
                Some(Cause::ValueUnusable),
            ),
            (
                "no Date header",
                Ok(reading(Quality::Good, BASE, None)),
                SANE_NOW,
                Some(Cause::NoFreshnessProof),
            ),
            (
                "pre-2020 source stamp",
                Ok(reading(Quality::Good, 1, Some(1))),
                SANE_NOW,
                Some(Cause::SourceClockImplausible),
            ),
            (
                "http_date before value_date",
                Ok(reading(Quality::Good, BASE + 5_000, Some(BASE))),
                SANE_NOW,
                Some(Cause::TimestampsDisagree),
            ),
            (
                "older than the allowance",
                Ok(reading(
                    Quality::Good,
                    BASE,
                    Some(BASE + POLICY.max_age_ms() + 1),
                )),
                SANE_NOW,
                Some(Cause::ReadingTooOld),
            ),
        ];
        for (name, tick, now, expected) in cases {
            let (state, verdict) = POLICY.step(State::initial(), &tick, now);
            assert_eq!(verdict.cause(), expected, "{name}: wrong cause");
            assert!(
                !matches!(state, State::Failed(_)),
                "{name} describes a reading, not an identity"
            );
            assert!(!verdict.latches(), "{name} must not latch");
            // ADDED 2026-08-11: the table must also publish the quality the
            // cause promises. Without this the table could quietly relabel a
            // freshness refusal as `Bad` — a contract change no golden saw,
            // because `contract_golden` pinned each half separately and never
            // the mapping between them.
            assert_eq!(
                verdict.quality(),
                expected
                    .expect("every row here has a cause")
                    .published_quality(),
                "{name}: the table publishes a quality its cause does not promise"
            );
        }

        // And the good row carries no cause at all.
        let (state, verdict) = POLICY.step(State::initial(), &fresh, SANE_NOW);
        assert_eq!(state, State::Fresh);
        assert_eq!(verdict.cause(), None);
    }
}
