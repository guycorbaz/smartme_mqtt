//! The `Clock` seam (Story 1.3): time is data minted in exactly one place.
//!
//! No truth is ever computed from a hardcoded `now()` — logic receives time through
//! an injected [`Clock`], so every staleness decision is reproducible in tests.
//! `SystemClock` is the single sanctioned holder of `std::time` sources; everywhere
//! else in this crate's `src/`, raw `Instant::now()`/`SystemTime::now()` calls are
//! banned mechanically by `tests/arch_purity.rs` (integration tests and sibling
//! crates are held by review). This module is PURE (no async/transport imports).

use std::ops::Sub;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::domain::UtcMillis;

/// Monotonic instant as milliseconds since an arbitrary process-local epoch.
///
/// The staleness age (`now − last_loop_tick`, Story 1.11) is computed on this,
/// immune to wall-clock drift. Process-local: never persist it, never compare it
/// across processes — only values minted by the same [`Clock`] are comparable.
/// Mirror of [`UtcMillis`]: public inner value, saturating signed subtraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MonotonicMs(pub i64);

/// Signed difference in milliseconds. Saturates instead of wrapping: a fabricated
/// extreme in a test must not flip the sign a watchdog reads.
impl Sub for MonotonicMs {
    type Output = i64;

    fn sub(self, rhs: Self) -> i64 {
        self.0.saturating_sub(rhs.0)
    }
}

/// The injected time source: one monotonic instant, one wall-clock reading.
///
/// Object-safe — consume as `&dyn Clock` or `Arc<dyn Clock + Send + Sync>`. The
/// wall-clock is [`UtcMillis`] (the payload timestamp must be honest wall time);
/// the monotonic reading is [`MonotonicMs`] (ages and watchdogs, drift-immune).
pub trait Clock {
    /// Milliseconds elapsed on the process-local monotonic clock.
    fn monotonic(&self) -> MonotonicMs;
    /// Current UTC wall-clock time.
    fn wall(&self) -> UtcMillis;
}

/// Production clock — the ONLY place in the tree that reads `std::time` sources.
///
/// The monotonic epoch is captured at construction, so [`MonotonicMs`] values are
/// only comparable when minted by the SAME instance: construct exactly one
/// `SystemClock` in the composition root and share it (`Arc<dyn Clock + Send + Sync>`).
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    /// Captures the process-local monotonic epoch.
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn monotonic(&self) -> MonotonicMs {
        MonotonicMs(i64::try_from(self.start.elapsed().as_millis()).unwrap_or(i64::MAX))
    }

    /// Every failure path yields `UtcMillis(0)` — an honest sentinel the Story 1.5
    /// plausibility guard (`< 2020-01-01 → STALE`) catches; never a substituted
    /// "now". (Saturating to `i64::MAX` here would fail *fresh*, the wrong
    /// direction — both the pre-epoch and the beyond-i64 branch must fail STALE.)
    fn wall(&self) -> UtcMillis {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(0))
            .unwrap_or(0);
        UtcMillis(millis)
    }
}

/// Test double: time advances ONLY on explicit calls — two reads with no call in
/// between are identical. A `Mutex` (not atomics) keeps the mono/wall pair
/// consistent under concurrent access: no lost updates, no torn "mono advanced but
/// wall didn't" state — a test harness must not be cleverer than it is correct.
/// Banned outside this module by `tests/arch_purity.rs` (a fake that leaks into
/// the app would fabricate time in production).
pub struct FakeClock {
    inner: Mutex<FakeNow>,
}

struct FakeNow {
    mono: i64,
    wall: i64,
}

impl FakeClock {
    /// Starts at monotonic 0 with the given wall time.
    pub fn new(wall: UtcMillis) -> Self {
        Self {
            inner: Mutex::new(FakeNow {
                mono: 0,
                wall: wall.0,
            }),
        }
    }

    /// Recovers the state even if a test thread panicked while holding the lock —
    /// a poisoned fake must not cascade a second panic into unrelated tests.
    fn lock(&self) -> std::sync::MutexGuard<'_, FakeNow> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Advances BOTH clocks by `ms` — the realistic passage of time. `u64` by
    /// construction: a monotonic clock never rewinds, so a negative advance is
    /// unrepresentable rather than merely documented-against.
    pub fn advance_ms(&self, ms: u64) {
        let ms = i64::try_from(ms).unwrap_or(i64::MAX);
        let mut now = self.lock();
        now.mono = now.mono.saturating_add(ms);
        now.wall = now.wall.saturating_add(ms);
    }

    /// Moves ONLY the wall clock (an NTP step, forward or backward) — the
    /// monotonic clock is unaffected, exactly like the real thing.
    pub fn set_wall(&self, wall: UtcMillis) {
        self.lock().wall = wall.0;
    }
}

impl Clock for FakeClock {
    fn monotonic(&self) -> MonotonicMs {
        MonotonicMs(self.lock().mono)
    }

    fn wall(&self) -> UtcMillis {
        UtcMillis(self.lock().wall)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_never_advances_alone() {
        let c = FakeClock::new(UtcMillis(1_000));
        assert_eq!(c.monotonic(), c.monotonic());
        assert_eq!(c.wall(), c.wall());
        assert_eq!(c.monotonic(), MonotonicMs(0));
        assert_eq!(c.wall(), UtcMillis(1_000));
    }

    #[test]
    fn advance_moves_both_clocks() {
        let c = FakeClock::new(UtcMillis(1_000));
        c.advance_ms(250);
        assert_eq!(c.monotonic(), MonotonicMs(250));
        assert_eq!(c.wall(), UtcMillis(1_250));
    }

    #[test]
    fn set_wall_leaves_monotonic_untouched() {
        let c = FakeClock::new(UtcMillis(1_000));
        c.advance_ms(100);
        // NTP steps the wall clock back one hour; the monotonic clock must not move.
        c.set_wall(UtcMillis(1_000 - 3_600_000));
        assert_eq!(c.monotonic(), MonotonicMs(100));
        assert_eq!(c.wall(), UtcMillis(1_000 - 3_600_000));
    }

    #[test]
    fn monotonic_subtraction_is_signed_and_saturating() {
        assert_eq!(MonotonicMs(2_000) - MonotonicMs(500), 1_500);
        assert_eq!(MonotonicMs(500) - MonotonicMs(2_000), -1_500);
        assert_eq!(MonotonicMs(i64::MIN) - MonotonicMs(1), i64::MIN);
        assert_eq!(MonotonicMs(i64::MAX) - MonotonicMs(-1), i64::MAX);
    }

    #[test]
    fn fake_clock_is_shareable_across_threads() {
        let c = std::sync::Arc::new(FakeClock::new(UtcMillis(0)));
        let c2 = std::sync::Arc::clone(&c);
        let t = std::thread::spawn(move || c2.advance_ms(42));
        t.join().unwrap();
        assert_eq!(c.monotonic(), MonotonicMs(42));
    }

    #[test]
    fn system_clock_smoke() {
        let c = SystemClock::new();
        let a = c.monotonic();
        let b = c.monotonic();
        assert!(b >= a, "monotonic must never decrease");
        // Not `> 2020-01-01`: an unset RTC is an environment the production code
        // tolerates (0-sentinel → STALE); the test only asserts the honest floor.
        assert!(c.wall() >= UtcMillis(0));
    }
}
