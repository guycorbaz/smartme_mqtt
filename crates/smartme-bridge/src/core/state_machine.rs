//! The pure `Fresh|Stale|Failed` staleness oracle (Story 1.5).
//!
//! "Is this a lie?" is decided HERE — a deterministic function over plain data,
//! off the network, outside any `async fn`. The freshness formula is
//! `age = http_date − value_date` (both cloud-domain, ADR 0004): the host clock is
//! out of the equation, except for one boot-sanity guard. When in doubt, the
//! verdict is STALE — the cheap honest default.

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

/// The staleness policy — explicit data, no hidden defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// Maximum acceptable `http_date − value_date` age in milliseconds; beyond it
    /// the reading is STALE even though the fetch succeeded. Must be positive: a
    /// non-positive value makes EVERY reading Stale — fail-safe but useless; the
    /// Epic 3 config oracle rejects it at load time.
    pub max_age_ms: i64,
}

impl Policy {
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
    pub fn step(&self, prev: State, tick: &Tick, now: UtcMillis) -> (State, Quality) {
        // Failed latches until restart; a fatal tick latches it. Both are
        // clock-independent, so they are judged BEFORE the boot-sanity guard —
        // an unsynced RTC must not soften an auth failure into Stale.
        if prev == State::Failed || matches!(tick, Err(SourceError::Fatal { .. })) {
            return (State::Failed, Quality::Bad);
        }
        // Boot sanity: an unsynced host clock poisons every local stamp.
        if now < PLAUSIBILITY_FLOOR {
            return (State::Stale, Quality::Stale);
        }
        match tick {
            Err(SourceError::Fatal { .. }) => (State::Failed, Quality::Bad),
            Err(SourceError::Timeout) | Err(SourceError::Transient { .. }) => {
                (State::Stale, Quality::Stale)
            }
            Ok(reading) => self.judge_reading(reading),
        }
    }

    fn judge_reading(&self, reading: &Reading) -> (State, Quality) {
        let Some(http_date) = reading.http_date else {
            // No oracle input (absent/malformed Date header): no freshness proof.
            return (State::Stale, Quality::Stale);
        };
        if http_date < PLAUSIBILITY_FLOOR {
            // A pre-2020 cloud stamp: the pair may be internally consistent, but
            // it cannot be a live reading from this decade.
            return (State::Stale, Quality::Stale);
        }
        let age_ms = http_date - reading.value_date();
        if age_ms < 0 || age_ms > self.max_age_ms {
            // Negative age = clock domains disagree; huge age = old data. (The
            // 1-second Date truncation can make a genuinely fresh reading read
            // sub-zero — spurious STALE is the accepted fail-safe direction;
            // tolerance tuning is deferred to Epic 2.)
            return (State::Stale, Quality::Stale);
        }
        // Timestamps prove freshness; the value itself may still be unusable
        // (fail-closed unit conversion, Story 1.7) — Bad passes through, never
        // upgraded. The state stays Stale: a Bad value proves nothing.
        match reading.value.quality {
            Quality::Bad => (State::Stale, Quality::Bad),
            Quality::Stale => (State::Stale, Quality::Stale),
            Quality::Good => (State::Fresh, Quality::Good),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Kw, Kwh, Measurement, MeterId, Serial};

    const POLICY: Policy = Policy { max_age_ms: 90_000 };
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

    #[test]
    fn fresh_reading_in_bounds_goes_fresh() {
        let tick = Ok(reading(Quality::Good, BASE, Some(BASE + 950)));
        assert_eq!(
            POLICY.step(State::initial(), &tick, SANE_NOW),
            (State::Fresh, Quality::Good)
        );
    }

    #[test]
    fn boot_clock_before_2020_forces_stale_even_on_good_reading() {
        let tick = Ok(reading(Quality::Good, BASE, Some(BASE + 950)));
        let pre_2020 = UtcMillis(PLAUSIBILITY_FLOOR.0 - 1);
        assert_eq!(
            POLICY.step(State::Fresh, &tick, pre_2020),
            (State::Stale, Quality::Stale)
        );
    }

    #[test]
    fn negative_age_is_stale() {
        // http_date before value_date: the two cloud stamps disagree.
        let tick = Ok(reading(Quality::Good, BASE + 2_000, Some(BASE + 1_000)));
        assert_eq!(
            POLICY.step(State::Fresh, &tick, SANE_NOW),
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
            POLICY.step(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
    }

    #[test]
    fn age_exactly_at_threshold_is_fresh() {
        let tick = Ok(reading(Quality::Good, BASE, Some(BASE + POLICY.max_age_ms)));
        assert_eq!(
            POLICY.step(State::Stale, &tick, SANE_NOW),
            (State::Fresh, Quality::Good)
        );
    }

    #[test]
    fn missing_http_date_is_stale() {
        let tick = Ok(reading(Quality::Good, BASE, None));
        assert_eq!(
            POLICY.step(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
    }

    #[test]
    fn pre_2020_cloud_pair_is_stale_even_when_internally_consistent() {
        // value_date/http_date form a plausible 500 ms age — in 1970. The floor
        // applies to the cloud stamp too, not only to the host clock.
        let tick = Ok(reading(Quality::Good, 1_000, Some(1_500)));
        assert_eq!(
            POLICY.step(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
    }

    #[test]
    fn bad_value_passes_through_never_upgraded() {
        // Timestamps fresh, value refused (unknown unit, 1.7): publish Bad.
        let tick = Ok(reading(Quality::Bad, BASE, Some(BASE + 500)));
        assert_eq!(
            POLICY.step(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Bad)
        );
    }

    #[test]
    fn incoming_stale_value_stays_stale() {
        let tick = Ok(reading(Quality::Stale, BASE, Some(BASE + 500)));
        assert_eq!(
            POLICY.step(State::Fresh, &tick, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
    }

    #[test]
    fn failed_is_absorbing_until_restart() {
        // A perfect reading after a fatal error proves nothing about the broken
        // config: Failed latches (ADR 0009 "stop + surface"; config restart-only).
        let good_tick = Ok(reading(Quality::Good, BASE, Some(BASE + 500)));
        assert_eq!(
            POLICY.step(State::Failed, &good_tick, SANE_NOW),
            (State::Failed, Quality::Bad)
        );
        // A later timeout must not launder Bad into Stale either.
        let timeout: Tick = Err(SourceError::Timeout);
        assert_eq!(
            POLICY.step(State::Failed, &timeout, SANE_NOW),
            (State::Failed, Quality::Bad)
        );
        // Only a restart (fresh initial state) re-opens the door.
        assert_eq!(
            POLICY.step(State::initial(), &good_tick, SANE_NOW),
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
            POLICY.step(State::Fresh, &fatal, pre_2020),
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
            POLICY.step(State::Fresh, &transient, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
        assert_eq!(
            POLICY.step(State::Fresh, &timeout, SANE_NOW),
            (State::Stale, Quality::Stale)
        );
        assert_eq!(
            POLICY.step(State::Fresh, &fatal, SANE_NOW),
            (State::Failed, Quality::Bad)
        );
    }

    #[test]
    fn recovery_from_stale_needs_one_proven_reading() {
        let bad_tick: Tick = Err(SourceError::Timeout);
        let (after_timeout, _) = POLICY.step(State::Fresh, &bad_tick, SANE_NOW);
        assert_eq!(after_timeout, State::Stale);
        let good_tick = Ok(reading(Quality::Good, BASE, Some(BASE + 500)));
        assert_eq!(
            POLICY.step(after_timeout, &good_tick, SANE_NOW),
            (State::Fresh, Quality::Good)
        );
    }

    #[test]
    fn step_is_deterministic_and_pure() {
        let tick = Ok(reading(Quality::Good, BASE, Some(BASE + 500)));
        let a = POLICY.step(State::Stale, &tick, SANE_NOW);
        let b = POLICY.step(State::Stale, &tick, SANE_NOW);
        assert_eq!(a, b);
    }
}
