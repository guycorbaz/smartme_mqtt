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
    Failed,
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
    /// | `Ok`, `age > max_age_ms`          | Stale   | Stale   |
    /// | `Ok`, in-bounds, value `Bad`      | Stale   | Bad     |
    /// | `Ok`, in-bounds, value `Stale`    | Stale   | Stale   |
    /// | `Ok`, in-bounds, value `Good`     | Fresh   | Good    |
    ///
    /// `Failed` is ABSORBING: a fatal error (auth rejected, config wrong) means
    /// retrying with the same config would lie, and config is restart-only — so
    /// only a process restart (fresh `State::initial()`) can leave `Failed`, per
    /// ADR 0009's "stop + surface". A later Timeout must not launder `Bad` into
    /// `Stale`, and a later Ok proves nothing about the broken config.
    ///
    /// Otherwise `prev` never softens a verdict: a previously-Fresh meter with a
    /// doubtful tick goes Stale immediately (when in doubt, publish STALE).
    ///
    /// Known accepted limitation (deferred oracle, see deferred-work.md): a
    /// byte-identical replayed response — `http_date` frozen WITH `value_date` —
    /// keeps a plausible age and stays Fresh; detecting it needs cross-tick state
    /// (`http_date` monotonicity), an additive Epic 2 oracle.
    pub fn step(&self, prev: State, tick: &Tick, now: UtcMillis) -> (State, Verdict) {
        // Failed latches until restart; a fatal tick latches it. Both are
        // clock-independent, so they are judged BEFORE the boot-sanity guard —
        // an unsynced RTC must not soften an auth failure into Stale.
        if prev == State::Failed || matches!(tick, Err(SourceError::Fatal { .. })) {
            return (State::Failed, Verdict::bad(Cause::SourceRefused));
        }
        // Boot sanity: an unsynced host clock poisons every local stamp.
        if now < PLAUSIBILITY_FLOOR {
            return (State::Stale, Verdict::stale(Cause::HostClockUnsynced));
        }
        match tick {
            Err(SourceError::Fatal { .. }) => (State::Failed, Verdict::bad(Cause::SourceRefused)),
            Err(SourceError::Timeout) | Err(SourceError::Transient { .. }) => {
                (State::Stale, Verdict::stale(Cause::SourceUnreachable))
            }
            Ok(reading) => self.judge_reading(reading),
        }
    }

    fn judge_reading(&self, reading: &Reading) -> (State, Verdict) {
        if reading.value.quality == Quality::Bad {
            // Bad is judged BEFORE the timestamp guards: "do not use this value"
            // must never be relabeled as the milder "old value" — a Bad reading
            // whose ValueDate also failed to parse stays Bad, not Stale.
            return (State::Stale, Verdict::bad(Cause::ValueUnusable));
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
            // Negative age = clock domains disagree; huge age = old data. (The
            // 1-second Date truncation can make a genuinely fresh reading read
            // sub-zero — spurious STALE is the accepted fail-safe direction;
            // tolerance tuning is deferred to Epic 2.)
            return (
                State::Stale,
                if age_ms < 0 {
                    Verdict::stale(Cause::TimestampsDisagree)
                } else {
                    Verdict::stale(Cause::ReadingTooOld)
                },
            );
        }
        // Timestamps prove freshness; the value itself may still be unusable
        // (fail-closed unit conversion, Story 1.7) — Bad passes through, never
        // upgraded. The state stays Stale: a Bad value proves nothing.
        match reading.value.quality {
            Quality::Bad => (State::Stale, Verdict::bad(Cause::ValueUnusable)),
            Quality::Stale => (State::Stale, Verdict::stale(Cause::SourceMarkedStale)),
            Quality::Good => (State::Fresh, Verdict::good()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::oracle::Cause;
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
                power: Kw(0.7),
                energy: Kwh(40_437.8),
                value_date: UtcMillis(value_date),
                quality,
            },
            http_date: http_date.map(UtcMillis),
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
        // config: Failed latches (ADR 0009 "stop + surface"; config restart-only).
        let good_tick = Ok(reading(Quality::Good, BASE, Some(BASE + 500)));
        assert_eq!(
            POLICY.step_quality(State::Failed, &good_tick, SANE_NOW),
            (State::Failed, Quality::Bad)
        );
        // A later timeout must not launder Bad into Stale either.
        let timeout: Tick = Err(SourceError::Timeout);
        assert_eq!(
            POLICY.step_quality(State::Failed, &timeout, SANE_NOW),
            (State::Failed, Quality::Bad)
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
            reason: "auth rejected".to_string(),
        });
        let pre_2020 = UtcMillis(PLAUSIBILITY_FLOOR.0 - 1);
        assert_eq!(
            POLICY.step_quality(State::Fresh, &fatal, pre_2020),
            (State::Failed, Quality::Bad)
        );
    }

    #[test]
    fn transient_and_timeout_go_stale_fatal_goes_failed() {
        let transient: Tick = Err(SourceError::Transient {
            reason: "http 503".to_string(),
        });
        let timeout: Tick = Err(SourceError::Timeout);
        let fatal: Tick = Err(SourceError::Fatal {
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
            (State::Failed, Quality::Bad)
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
            reason: "auth rejected".into(),
        });

        // Latching identity trouble, from both doors: a previously-Failed meter,
        // and a fatal tick.
        for (prev, tick) in [(State::Failed, &fresh), (State::initial(), &fatal)] {
            let (state, verdict) = POLICY.step(prev, tick, SANE_NOW);
            assert_eq!(state, State::Failed);
            assert_eq!(verdict.cause(), Some(Cause::SourceRefused));
            assert!(verdict.latches(), "identity trouble must latch");
        }

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
            assert_ne!(
                state,
                State::Failed,
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
