//! The `Source` seam (Story 1.4): the meter feed behind a trait, with a fake.
//!
//! `Source` is an I/O *port* — it reports raw facts and decides no truth; the
//! quality/staleness verdict stays in the pure state machine (Story 1.5). The trait
//! is async (native RPITIT, a language feature — no runtime import, so this module
//! stays PURE per `tests/arch_purity.rs`), because the real implementation
//! (`SmartMeCloudSource`, Story 1.7) awaits the network and the poll task's
//! per-fetch timeout wraps `fetch` itself — the fake must walk the same path
//! (party-mode decision D5).

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;

use crate::domain::{Measurement, MeterId, UtcMillis};

/// One per-meter reading as delivered by a source: the measured value and the two
/// cloud-domain timestamps the freshness formula needs (`age = http_date − value_date`,
/// ADR 0004).
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// The mapped measurement (units already canonical; a failed unit conversion
    /// arrives as `Quality::Bad` inside — fail-closed, Story 1.7 — never a guess).
    pub value: Measurement,
    /// HTTP response `Date` header, if present and parseable. `None` means the
    /// oracle input is missing — the source never invents a timestamp; the state
    /// machine draws the conservative conclusion.
    pub http_date: Option<UtcMillis>,
}

impl Reading {
    /// The meter's own timestamp (lives inside the measurement — not duplicated,
    /// two copies of one truth would be an invitation to diverge).
    pub fn value_date(&self) -> UtcMillis {
        self.value.value_date
    }
}

/// Skeleton error taxonomy — the transient/fatal split the bridge's classification
/// is built on (Epic 2 subdivides it without moving these).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// The per-fetch deadline elapsed. Minted by the poll task's timeout wrapper
    /// (the timer lives in the async shell; the variant lives here so `FakeSource`
    /// can script it and downstream has a single error path). → STALE.
    Timeout,
    /// Retryable trouble (network, 5xx, 429). → STALE, retry.
    Transient {
        /// Adapter-provided diagnostic, for tracing — never parsed for decisions.
        reason: String,
    },
    /// Non-retryable (auth rejected, config wrong). → FAILED; retrying would lie.
    Fatal {
        /// Adapter-provided diagnostic, for tracing — never parsed for decisions.
        reason: String,
    },
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("source fetch timed out"),
            Self::Transient { reason } => write!(f, "transient source error: {reason}"),
            Self::Fatal { reason } => write!(f, "fatal source error: {reason}"),
        }
    }
}

impl std::error::Error for SourceError {}

/// The meter feed. Implementations may write `async fn fetch(...)` against this
/// declaration; the explicit `+ Send` bound is what lets the Story 1.11 poll task
/// stay generic (`S: Source + Send`) and still cross `tokio::spawn`.
pub trait Source {
    /// Fetch one meter's current reading, or a typed failure.
    fn fetch(
        &mut self,
        meter: &MeterId,
    ) -> impl Future<Output = Result<Reading, SourceError>> + Send;
}

/// One scripted step of a [`FakeSource`].
#[derive(Debug, Clone, PartialEq)]
enum ScriptEntry {
    /// Resolve immediately with this result.
    Respond(Result<Reading, SourceError>),
    /// Never resolve — the cloud goes silent. Under a paused-time runtime the
    /// poll task's real timeout path fires (the Story 1.14 localization twin).
    Hang,
}

/// Scripted test double: pops one entry per `fetch`, in order. An exhausted
/// script fails CLOSED (`Fatal`, discriminable by its reason string) — a fake that
/// silently repeats its last answer would be a fake that lies, masking exactly the
/// missed-verdict bugs it exists to catch; tests assert full consumption via
/// [`FakeSource::is_exhausted`]. The queue is global fetch-order, not per-meter —
/// sufficient for the single-meter walking skeleton; per-meter scripts can be added
/// when multi-meter polling lands. Banned outside this module by
/// `tests/arch_purity.rs`.
#[derive(Debug)]
pub struct FakeSource {
    script: VecDeque<ScriptEntry>,
    /// Who was polled, in order — an assertion seam for the Story 1.11 tests.
    pub calls: Vec<MeterId>,
}

impl FakeSource {
    /// An empty script — every fetch fails closed until steps are added.
    pub fn new() -> Self {
        Self {
            script: VecDeque::new(),
            calls: Vec::new(),
        }
    }

    /// Appends a step that resolves immediately with `result`.
    #[must_use]
    pub fn then(mut self, result: Result<Reading, SourceError>) -> Self {
        self.script.push_back(ScriptEntry::Respond(result));
        self
    }

    /// Appends a step that never resolves — scripted silence, never accidental.
    #[must_use]
    pub fn then_hang(mut self) -> Self {
        self.script.push_back(ScriptEntry::Hang);
        self
    }

    /// Steps not yet consumed. A finished test asserts `is_exhausted()` so a
    /// too-short run cannot pass green.
    pub fn remaining(&self) -> usize {
        self.script.len()
    }

    /// True when every scripted step has been consumed.
    pub fn is_exhausted(&self) -> bool {
        self.script.is_empty()
    }
}

impl Default for FakeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for FakeSource {
    fn fetch(
        &mut self,
        meter: &MeterId,
    ) -> impl Future<Output = Result<Reading, SourceError>> + Send {
        let meter = meter.clone();
        // All side effects live INSIDE the future: like the real async source, a
        // fetch that is built but dropped before its first poll (a lost `select!`
        // race, a shutdown branch) makes no request — it must consume no script
        // entry and log no call, or every later assertion desynchronizes.
        async move {
            self.calls.push(meter);
            let entry = self.script.pop_front().unwrap_or_else(|| {
                ScriptEntry::Respond(Err(SourceError::Fatal {
                    reason: "fake source: script exhausted".to_string(),
                }))
            });
            match entry {
                ScriptEntry::Respond(result) => result,
                ScriptEntry::Hang => std::future::pending().await,
            }
        }
    }
}

/// Polls a future exactly once with a no-op waker — enough to drive any
/// [`FakeSource`] future to completion without a runtime (`Respond` entries never
/// await). Returns `None` if the future is pending — the future is then DROPPED,
/// not resumable, and a noop waker can never wake it: this is a test-harness tool,
/// confined to test use by `tests/arch_purity.rs` (production code polls inside
/// the runtime).
#[must_use]
pub fn poll_now<F: Future>(fut: F) -> Option<F::Output> {
    let mut fut = std::pin::pin!(fut);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match fut.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => Some(v),
        std::task::Poll::Pending => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Kw, Kwh, Quality, Serial};

    fn reading(power: f64, value_date: i64, http_date: Option<i64>) -> Reading {
        Reading {
            value: Measurement {
                meter: MeterId::new("m1"),
                serial: Serial::new("S-1"),
                power: Kw(power),
                energy: Kwh(1.0),
                value_date: UtcMillis(value_date),
                quality: Quality::Good,
            },
            http_date: http_date.map(UtcMillis),
        }
    }

    #[test]
    fn script_pops_in_order_success_transient_timeout() {
        let m = MeterId::new("m1");
        let mut src = FakeSource::new()
            .then(Ok(reading(1.0, 1_000, Some(1_500))))
            .then(Err(SourceError::Transient {
                reason: "http 503".to_string(),
            }))
            .then(Err(SourceError::Timeout));

        let first = poll_now(src.fetch(&m)).expect("ready");
        assert_eq!(
            first.as_ref().map(Reading::value_date),
            Ok(UtcMillis(1_000))
        );
        assert!(matches!(
            poll_now(src.fetch(&m)),
            Some(Err(SourceError::Transient { .. }))
        ));
        assert_eq!(poll_now(src.fetch(&m)), Some(Err(SourceError::Timeout)));
    }

    #[test]
    fn exhausted_script_fails_closed_not_silent() {
        let m = MeterId::new("m1");
        let mut src = FakeSource::new().then(Ok(reading(1.0, 1_000, None)));
        assert!(matches!(poll_now(src.fetch(&m)), Some(Ok(_))));
        // One fetch past the script: a typed Fatal, never a repeated Ok.
        assert!(matches!(
            poll_now(src.fetch(&m)),
            Some(Err(SourceError::Fatal { .. }))
        ));
    }

    #[test]
    fn hang_entry_stays_pending_without_a_runtime() {
        let m = MeterId::new("m1");
        let mut src = FakeSource::new().then_hang();
        assert_eq!(poll_now(src.fetch(&m)).map(|_| ()), None);
    }

    #[test]
    fn calls_log_records_polled_meters_in_order() {
        let a = MeterId::new("a");
        let b = MeterId::new("b");
        let mut src = FakeSource::new()
            .then(Ok(reading(1.0, 1, None)))
            .then(Ok(reading(2.0, 2, None)));
        let _ = poll_now(src.fetch(&a));
        let _ = poll_now(src.fetch(&b));
        assert_eq!(src.calls, vec![a, b]);
    }

    #[test]
    fn unpolled_fetch_consumes_nothing() {
        let m = MeterId::new("m1");
        let mut src = FakeSource::new().then(Ok(reading(1.0, 1_000, None)));
        // Build the future and drop it unpolled (a lost select! race): like the
        // real source, no request happened — no call logged, no entry consumed.
        drop(src.fetch(&m));
        assert!(src.calls.is_empty());
        assert_eq!(src.remaining(), 1);
        assert!(!src.is_exhausted());
        assert!(matches!(poll_now(src.fetch(&m)), Some(Ok(_))));
        assert!(src.is_exhausted());
    }

    #[test]
    fn absent_http_date_is_expressible_as_none() {
        // The `absent`/`malformed` header fixtures collapse to None: same verdict
        // (no oracle input), diagnostic distinction stays in the adapter's logs.
        let r = reading(1.0, 1_000, None);
        assert_eq!(r.http_date, None);
        assert_eq!(r.value_date(), UtcMillis(1_000));
    }
}
