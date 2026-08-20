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

use crate::core::oracle::{Cause, Measured};
use crate::domain::{Measurement, MeterId, UtcMillis};

/// One per-meter reading as delivered by a source: the measured value and the two
/// cloud-domain timestamps the freshness formula needs (`age = http_date − value_date`,
/// ADR 0004).
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// The mapped measurement (units already canonical). A field the source could
    /// not give us usably arrives as `None`, never as a substituted number —
    /// story 2.5, FR16's *"never a substituted value"*.
    pub value: Measurement,
    /// HTTP response `Date` header, if present and parseable. `None` means the
    /// oracle input is missing — the source never invents a timestamp; the state
    /// machine draws the conservative conclusion.
    pub http_date: Option<UtcMillis>,
    /// What the source could not read, per field.
    pub faults: SourceFaults,
}

/// What a source could not read, said once per field rather than collapsed into
/// one verdict for the whole reading.
///
/// **Story 2.5, and it is ADR 0031 reaching the boundary.** A verdict belongs to a
/// metric; until this type existed, `map_device` set one `Quality::Bad` for the
/// whole reading, so an unrecognised unit on `ActivePower` degraded a cumulative
/// energy index that had been read and converted perfectly. The oracle layer had
/// been fixed for that in story 2.3 and the adapter had not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceFaults {
    /// A fault that makes the WHOLE reading unusable: no timestamp of its own, or
    /// not one usable number in it. Judged before the freshness guards, as it
    /// always was.
    pub reading: Option<Cause>,
    /// Why the power field is absent, when it is.
    pub power: Option<Cause>,
    /// Why the energy field is absent, when it is.
    pub energy: Option<Cause>,
}

impl SourceFaults {
    /// Nothing was wrong.
    pub const NONE: SourceFaults = SourceFaults {
        reading: None,
        power: None,
        energy: None,
    };

    /// The fault for one metric, if any.
    pub fn of(&self, metric: Measured) -> Option<Cause> {
        match metric {
            Measured::Power => self.power,
            Measured::Energy => self.energy,
        }
    }
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
    /// The source rate-limited us and, if it said so, for how long.
    ///
    /// **Story 2.6.** Separate from [`Self::Transient`] because it is the only
    /// source failure that carries an INSTRUCTION rather than a diagnosis: the
    /// other end told us when to come back. Everything else transient is retried
    /// on the poll interval, which ADR 0020 bounds and forbids turning off.
    RateLimited {
        /// How long the server asked us to wait, when it said. Already capped by
        /// the adapter — see `RETRY_AFTER_CAP`.
        retry_after: Option<std::time::Duration>,
    },
    /// Retryable trouble (network, 5xx). → STALE, retry.
    Transient {
        /// Adapter-provided diagnostic, for tracing — never parsed for decisions.
        reason: String,
    },
    /// Non-retryable (auth rejected, config wrong). → FAILED; retrying would lie.
    Fatal {
        /// WHICH refusal it is. Story 2.6: the taxonomy existed in this type and
        /// was invisible on the wire, because every fatal published one cause.
        refusal: Refusal,
        /// Adapter-provided diagnostic, for tracing — never parsed for decisions.
        reason: String,
    },
}

/// The three ways a source refuses us for good, kept apart because **an operator
/// repairs each somewhere else**.
///
/// **Story 2.6.** They were one cause on the wire, `source-refused`, until
/// 2026-08-12 — so a rejected token, a device id smart-me does not know and a
/// meter answering under the wrong serial all read alike, and the 2026-08-11
/// review of story 2.1 recorded that an operator *"cannot tell NFR7 from an
/// expired credential"*.
///
/// All three latch. That is not the identity-versus-value rule of [ADR 0032] —
/// a credential is neither — but its own reason: **retrying against a refusal the
/// other end has already given is how a bridge hammers an API**, and no reading
/// obtained that way would be more trustworthy than the refusal.
///
/// [ADR 0032]: ../../../docs/adr/0032-at-equal-severity-a-latching-cause-outranks-a-degrading-one.md
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The source rejected our credentials: 401/403, or a token exchange whose
    /// OAuth error body attributes the failure to the client. **Go and look at
    /// the credential.**
    Credential,
    /// The source contradicts the configuration, or the configuration contradicts
    /// itself: a base URL refused at construction, a source wired for one meter
    /// and asked for another. **Go and look at the configuration.**
    ///
    /// A device id the account does not have moved OUT of here in story 3.5 —
    /// see [`Refusal::DeviceNotInAccount`]: the repair site differs (the row or
    /// the account, not the file's plumbing), and the fleet topology responds
    /// differently (that device gets a certificate; this refusal's subjects
    /// keep their devices, because nothing here is evidence about a device).
    Configuration,
    /// The account itself says it has no such device: story 2.6's `404`,
    /// pronounced by smart-me about the id the configuration names. **Go and
    /// look at the meter row — the id is mistyped, or the device was removed
    /// from the account**; the bridge cannot tell those two apart and does not
    /// pretend to (one `404` covers both, and inventing a discrimination would
    /// claim a diagnosis nobody has). **Latches**, like every refusal — and it
    /// is the one refusal that is EVIDENCE ABOUT THE DEVICE, which is why story
    /// 3.5 ends it with a DDEATH where its siblings end with nothing.
    DeviceNotInAccount,
    /// The device answered and it is not the one declared ([ADR 0029]). **Go and
    /// look at which physical meter is which** — the credential is fine and the
    /// configuration is internally consistent; it simply names the wrong device.
    ///
    /// [ADR 0029]: ../../../docs/adr/0029-the-declared-serial-is-checked-against-the-one-smart-me-reports.md
    Identity,
}

impl Refusal {
    /// The cause published for this refusal.
    pub fn cause(self) -> crate::core::oracle::Cause {
        use crate::core::oracle::Cause;
        match self {
            Refusal::Credential => Cause::CredentialRejected,
            Refusal::Configuration => Cause::ConfigurationContradicted,
            Refusal::Identity => Cause::IdentityMismatch,
            Refusal::DeviceNotInAccount => Cause::DeviceNotInAccount,
        }
    }
}

impl SourceError {
    /// The cause a failed fetch is published under.
    ///
    /// **Extracted from `Policy::step_remembering` by story 6.6**, which needed the
    /// same answer outside the poll loop and must not have a second copy of it. The
    /// mapping is unchanged, and the state machine now reads it here: a fatal names
    /// its refusal, a rate limit is NOT unreachability (story 2.6 — the source
    /// answered and said come back later, and an operator sent to look at the
    /// network would find nothing wrong with it), and a timeout is the same fact as
    /// a transient failure seen from the near end.
    ///
    /// **This is a fact about the FETCH, not a judgement of a reading.** Nothing
    /// here looks at a value, a timestamp or a counter — those are the oracles', and
    /// they need the per-meter memory this function does not take. That distinction
    /// is what lets story 6.6's check use this and stop.
    pub fn cause(&self) -> crate::core::oracle::Cause {
        use crate::core::oracle::Cause;
        match self {
            Self::Fatal { refusal, .. } => refusal.cause(),
            Self::RateLimited { .. } => Cause::SourceRateLimited,
            Self::Timeout | Self::Transient { .. } => Cause::SourceUnreachable,
        }
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("source fetch timed out"),
            Self::RateLimited { retry_after } => match retry_after {
                Some(d) => write!(f, "source rate-limited, asked for {}s", d.as_secs()),
                None => f.write_str("source rate-limited, no delay given"),
            },
            Self::Transient { reason } => write!(f, "transient source error: {reason}"),
            Self::Fatal { refusal, reason } => {
                write!(f, "fatal source error ({refusal:?}): {reason}")
            }
        }
    }
}

impl std::error::Error for SourceError {}

/// One fetch outcome — what the poll task feeds the pure state machine. The
/// epic's tick fields `{value, value_date, http_date}` live in [`Reading`]; the
/// error side carries the typed failure the oracle turns into `Stale`/`Failed`.
pub type Tick = Result<Reading, SourceError>;

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
                    refusal: Refusal::Configuration,
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
                power: Some(Kw(power)),
                energy: Some(Kwh(1.0)),
                value_date: UtcMillis(value_date),
                quality: Quality::Good,
            },
            http_date: http_date.map(UtcMillis),
            faults: SourceFaults::NONE,
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
