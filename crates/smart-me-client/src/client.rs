//! The smart-me HTTP client (Story 1.6): TLS-mandatory, token-aware, Date-capturing.
//!
//! Transport policy (NFR13): HTTPS only, enforced twice — full URL validation at
//! construction AND `reqwest`'s `https_only` (no silent plaintext fallback, ever).
//! Redirects are NOT followed: a 3xx surfaces as a status error — this client must
//! never replay credentials or a token form to a host the origin names (and a
//! "never lies" bridge must not silently accept data from a different origin).
//! Auth per ADR 0009: OAuth2 client-credentials primary (token endpoint
//! `POST {base}/oauth/token`, audited in Story 1.1), HTTP Basic fallback. The
//! client is CLOCK-FREE: token expiry is plain data (`TokenState`) judged by the
//! caller with its injected `Clock` — this crate never reads time.
//!
//! Secrets (NFR12): `Credentials`, `TokenState`, and `SmartMeClient` have MANUAL
//! `Debug` impls that redact secret material — a `{:?}` in any log line or panic
//! message must never print a secret.

use std::fmt;

use serde::Deserialize;

use crate::http_date::parse_imf_fixdate;
use crate::types::Device;

/// Errors of this crate only — no leaked transport types (the bridge classifies
/// transient vs fatal via [`SmartMeError::is_fatal`], and reqwest must not bleed
/// through).
#[derive(Debug, thiserror::Error)]
pub enum SmartMeError {
    /// The base URL is not a clean `https://host[:port]` — refused at
    /// construction (NFR13). Carries a SANITIZED description, never userinfo.
    #[error("refusing endpoint: {reason}")]
    NotHttps {
        /// Why the URL was refused (scheme/host class only — never credentials).
        reason: String,
    },
    /// A local usage/configuration error (no usable credentials for the call,
    /// invalid device id, zero timeout). Fatal: the operator must fix the
    /// config — the server was never asked.
    #[error("client misconfigured: {reason}")]
    Misconfigured {
        /// What is wrong locally.
        reason: String,
    },
    /// Authentication rejected by the server (401/403, or a token exchange the
    /// OAuth error body attributes to the client/credentials). Fatal: retrying
    /// with the same credentials would lie (ADR 0009).
    #[error("authentication rejected (http {status})")]
    AuthRejected {
        /// The HTTP status code returned.
        status: u16,
    },
    /// smart-me does not know the device id we asked for (`404`). **Fatal**, and
    /// the reasoning is ADR 0029's applied to the id rather than the serial: a
    /// device id does not come into existence on its own, so retrying is polling
    /// something that is not there while publishing a fault as weather.
    ///
    /// **Two origins, not one**, and the message names both because they send an
    /// operator to different places: the id is mistyped in the configuration, or
    /// the device existed and has been removed from the account. The second
    /// arrives on a configuration that worked yesterday, without anyone typing.
    ///
    /// Story 2.6's review found this reaching the wire as `source-unreachable`:
    /// `is_fatal` did not name it, so the single most likely configuration error
    /// was published as a network fault and never latched.
    #[error(
        "smart-me does not know device {device_id}. Either the device id is mistyped in the \
         configuration, or the device has been removed from the smart-me account. Check it \
         against the device list in the smart-me portal, correct the configuration, then restart"
    )]
    UnknownDevice {
        /// The id smart-me refused, quoted back so the operator can search for it.
        device_id: String,
    },
    /// The server rate-limited us (`429`), and how long it asked us to wait if it
    /// said so.
    ///
    /// **Story 2.6, and it is the ONE case a source-side wait is not redundant.**
    /// The poll interval already spaces retries (ADR 0020 bounds it and forbids
    /// turning it off), so a general backoff would be a second timer competing for
    /// the same loop. What the interval cannot know is the other end asking for
    /// longer than it.
    ///
    /// Transient: a rate limit passes.
    #[error("rate limited{}", match .retry_after_secs { Some(s) => format!(", retry after {s}s"), None => String::new() })]
    RateLimited {
        /// `Retry-After` in seconds, when the header was present and parseable as
        /// a delay. **The date form is not parsed**: it needs a trusted local
        /// clock, and this bridge's whole subject is that a local clock may be
        /// unsynced (`HostClockUnsynced` exists for that). Absent means "wait, but
        /// we were not told how long".
        retry_after_secs: Option<u64>,
    },
    /// Any other non-success HTTP status (5xx, 3xx — redirects are not followed).
    /// Transient.
    ///
    /// **The statuses this no longer covers are named above**: `401`/`403` are
    /// [`AuthRejected`](Self::AuthRejected), `404` is
    /// [`UnknownDevice`](Self::UnknownDevice), `429` is
    /// [`RateLimited`](Self::RateLimited). Each was carved out because it names a
    /// repair, and this variant names none.
    #[error("http status {status}")]
    HttpStatus {
        /// The HTTP status code returned.
        status: u16,
    },
    /// The request timed out at the transport level. Transient.
    #[error("request timed out")]
    Timeout,
    /// Connection/protocol trouble (DNS, TLS handshake, reset...). Transient.
    #[error("network error: {reason}")]
    Network {
        /// Diagnostic text, for tracing — never parsed for decisions.
        reason: String,
    },
    /// The response body did not match the audited contract. Transient
    /// (a payload anomaly is retried; persistence shows up in diagnostics).
    #[error("response decode failed: {reason}")]
    Decode {
        /// Diagnostic text, for tracing.
        reason: String,
    },
}

impl SmartMeError {
    /// The single transient/fatal classification the bridge maps into its
    /// `Failed`-latching taxonomy — defined HERE so callers cannot drift.
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            SmartMeError::NotHttps { .. }
                | SmartMeError::Misconfigured { .. }
                | SmartMeError::AuthRejected { .. }
                | SmartMeError::UnknownDevice { .. }
        )
    }

    fn from_reqwest(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            SmartMeError::Timeout
        } else if e.is_decode() {
            SmartMeError::Decode {
                reason: e.to_string(),
            }
        } else {
            SmartMeError::Network {
                reason: e.to_string(),
            }
        }
    }
}

/// Credentials, per ADR 0009. Client-credentials is primary; Basic is the
/// documented fallback when no OAuth app is configured.
///
/// `PartialEq` is derived and `Debug` deliberately is **not**: the bridge needs
/// to detect that a credential changed (`app::reconfigure`, Story 5.2 AC4) and
/// must never be able to render one. The comparison is not constant-time, and
/// does not need to be — it compares two configurations of this process against
/// each other, and authenticates nothing.
#[derive(Clone, PartialEq, Eq)]
pub enum Credentials {
    /// OAuth2 client-credentials (the smart-me "Device" app type).
    ClientCredentials {
        /// OAuth client id.
        client_id: String,
        /// OAuth client secret.
        client_secret: String,
    },
    /// HTTP Basic with account credentials.
    Basic {
        /// Account user name.
        user: String,
        /// Account password.
        password: String,
    },
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Credentials::ClientCredentials { client_id, .. } => f
                .debug_struct("ClientCredentials")
                .field("client_id", client_id)
                .field("client_secret", &"<redacted>")
                .finish(),
            Credentials::Basic { user, .. } => f
                .debug_struct("Basic")
                .field("user", user)
                .field("password", &"<redacted>")
                .finish(),
        }
    }
}

/// A bearer token plus its reported lifetime, as plain data. The CALLER anchors
/// expiry with its injected clock at the moment the exchange returns
/// (`expires_at = now + expires_in_s`, minus whatever safety margin it chooses);
/// this crate never reads time.
#[derive(Clone)]
pub struct TokenState {
    /// The bearer access token.
    pub access_token: String,
    /// Token lifetime as reported by the token endpoint, in seconds (validated
    /// positive).
    pub expires_in_s: i64,
}

impl fmt::Debug for TokenState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenState")
            .field("access_token", &"<redacted>")
            .field("expires_in_s", &self.expires_in_s)
            .finish()
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
}

/// The OAuth error body (RFC 6749 §5.2) — the discriminator between "your
/// credentials are bad" (fatal) and "the request upset something" (transient).
#[derive(Deserialize)]
struct OAuthErrorBody {
    error: String,
}

/// One captured fetch: the device AND the response `Date` header (the freshness
/// oracle's clock input), parsed strictly — `None` when absent/malformed.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceCapture {
    /// The deserialized device.
    pub device: Device,
    /// The response `Date` header as UTC epoch-ms, if present and well-formed.
    pub http_date_ms: Option<i64>,
}

/// smart-me REST client. Construction fails on any non-HTTPS or non-clean base URL.
#[derive(Clone)]
pub struct SmartMeClient {
    http: reqwest::Client,
    base: String,
    credentials: Credentials,
}

impl fmt::Debug for SmartMeClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmartMeClient")
            .field("base", &self.base)
            .field("credentials", &self.credentials)
            .finish_non_exhaustive()
    }
}

impl SmartMeClient {
    /// Production endpoint.
    pub const DEFAULT_BASE: &'static str = "https://api.smart-me.com";

    /// True in client-credentials mode (callers must obtain a token first);
    /// false in Basic mode (`get_device` with `token: None`).
    pub fn uses_client_credentials(&self) -> bool {
        matches!(self.credentials, Credentials::ClientCredentials { .. })
    }

    /// Builds a client for `base` (e.g. [`Self::DEFAULT_BASE`]). Refuses anything
    /// but a clean `https://host[:port]` — no userinfo, no query, no fragment —
    /// so there is no way to construct a plaintext (or credential-smuggling)
    /// client, and refuses a sub-second timeout (an instant-timeout client would
    /// spin the retry path forever).
    pub fn new(
        base: impl Into<String>,
        credentials: Credentials,
        timeout: std::time::Duration,
    ) -> Result<Self, SmartMeError> {
        let base = base.into();
        let url = reqwest::Url::parse(base.trim()).map_err(|_| SmartMeError::NotHttps {
            reason: "base URL does not parse".to_string(),
        })?;
        if url.scheme() != "https" {
            return Err(SmartMeError::NotHttps {
                reason: format!("scheme is {:?}, require https", url.scheme()),
            });
        }
        let Some(host) = url.host_str().filter(|h| !h.is_empty()) else {
            return Err(SmartMeError::NotHttps {
                reason: "base URL has no host".to_string(),
            });
        };
        if !url.username().is_empty() || url.password().is_some() {
            return Err(SmartMeError::NotHttps {
                reason: "base URL must not carry userinfo".to_string(),
            });
        }
        if url.query().is_some() || url.fragment().is_some() || url.path() != "/" {
            return Err(SmartMeError::NotHttps {
                reason: "base URL must be a bare https origin".to_string(),
            });
        }
        if timeout < std::time::Duration::from_secs(1) {
            return Err(SmartMeError::Misconfigured {
                reason: "timeout under 1s would instant-fail every request".to_string(),
            });
        }
        let base = match url.port() {
            Some(p) => format!("https://{host}:{p}"),
            None => format!("https://{host}"),
        };
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json"),
        );
        let http = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .default_headers(headers)
            .build()
            .map_err(SmartMeError::from_reqwest)?;
        Ok(Self {
            http,
            base,
            credentials,
        })
    }

    /// Exchanges client credentials for a bearer token
    /// (`POST {base}/oauth/token`, audited in Story 1.1 / ADR 0009). A 401/403,
    /// or a 400 whose OAuth error body names the client/grant, is fatal; a bare
    /// 400 without that attribution stays transient (a WAF/proxy artifact must
    /// not latch the bridge's absorbing `Failed` state). With
    /// [`Credentials::Basic`] this is [`SmartMeError::Misconfigured`] — Basic
    /// sends no token; callers use [`SmartMeClient::get_device`] with
    /// `token: None` instead.
    pub async fn fetch_token(&self) -> Result<TokenState, SmartMeError> {
        let Credentials::ClientCredentials {
            client_id,
            client_secret,
        } = &self.credentials
        else {
            return Err(SmartMeError::Misconfigured {
                reason: "token exchange requires client credentials".to_string(),
            });
        };
        let resp = self
            .http
            .post(format!("{}/oauth/token", self.base))
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", client_id.as_str()),
                ("client_secret", client_secret.as_str()),
                ("scope", "device.read"),
            ])
            .send()
            .await
            .map_err(SmartMeError::from_reqwest)?;
        let status = resp.status().as_u16();
        if status == 401 || status == 403 {
            return Err(SmartMeError::AuthRejected { status });
        }
        if status == 400 {
            // RFC 6749 §5.2: only a body blaming the client/credentials is fatal.
            let fatal = resp
                .json::<OAuthErrorBody>()
                .await
                .map(|b| {
                    matches!(
                        b.error.as_str(),
                        "invalid_client"
                            | "unauthorized_client"
                            | "invalid_grant"
                            | "access_denied"
                    )
                })
                .unwrap_or(false);
            return Err(if fatal {
                SmartMeError::AuthRejected { status }
            } else {
                SmartMeError::HttpStatus { status }
            });
        }
        if !resp.status().is_success() {
            return Err(SmartMeError::HttpStatus { status });
        }
        let token: TokenResponse = resp.json().await.map_err(SmartMeError::from_reqwest)?;
        if token.expires_in <= 0 {
            return Err(SmartMeError::Decode {
                reason: format!("non-positive expires_in {}", token.expires_in),
            });
        }
        Ok(TokenState {
            access_token: token.access_token,
            expires_in_s: token.expires_in,
        })
    }

    /// `GET {base}/Devices/{id}` — returns the device AND the captured `Date`
    /// header. `token` is `Some` for bearer auth (client-credentials), `None`
    /// falls back to HTTP Basic per ADR 0009. The id must be a bare device id
    /// (hex/dash UUID shape) — anything else is refused before a request exists.
    pub async fn get_device(
        &self,
        device_id: &str,
        token: Option<&TokenState>,
    ) -> Result<DeviceCapture, SmartMeError> {
        if device_id.is_empty()
            || !device_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(SmartMeError::Misconfigured {
                reason: "device id must be non-empty [0-9A-Za-z-]".to_string(),
            });
        }
        let req = self
            .http
            .get(format!("{}/Devices/{}", self.base, device_id));
        let req = match (token, &self.credentials) {
            (Some(t), _) => req.bearer_auth(&t.access_token),
            (None, Credentials::Basic { user, password }) => req.basic_auth(user, Some(password)),
            (None, Credentials::ClientCredentials { .. }) => {
                // No token and nothing to fall back on: refuse locally rather
                // than sending an unauthenticated request.
                return Err(SmartMeError::Misconfigured {
                    reason: "client-credentials mode needs a token (call fetch_token)".to_string(),
                });
            }
        };
        let resp = req.send().await.map_err(SmartMeError::from_reqwest)?;
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        if let Some(e) =
            classify_device_status(resp.status().as_u16(), retry_after.as_deref(), device_id)
        {
            return Err(e);
        }
        let http_date_ms = resp
            .headers()
            .get(reqwest::header::DATE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_imf_fixdate);
        let device: Device = resp.json().await.map_err(SmartMeError::from_reqwest)?;
        Ok(DeviceCapture {
            device,
            http_date_ms,
        })
    }
}

/// What a `GET /Devices/{id}` response status means, or `None` when it succeeded.
///
/// # Why this is a function and not four `if`s inside `get_device`
///
/// **The status is what decides these, not the round-trip** — and this crate has
/// no HTTP test harness, so while the classification lived inside `get_device`
/// nothing could reach it. Story 2.6 shipped the `429` branch and the
/// `Retry-After` parse with no test of either; story 2.6's review found the `404`
/// branch missing entirely and the suite green. Both are the same failure the
/// Epic 2 retrospective is about: a property tested one layer above where it
/// lives, or not at all because that layer is out of reach.
///
/// `retry_after` is the raw header value; parsing it here keeps the whole
/// status-to-error decision in one testable place.
///
/// # Only `404`, deliberately
///
/// A `400` on a device id that passed the local shape check would plausibly also
/// mean "the id is wrong", but this API has never been observed returning one and
/// guessing its meaning would be a fact about smart-me that nobody measured — the
/// refusal story 2.2 AC4 and ADR 0033 both made. If a `400` appears in the field
/// it arrives as `HttpStatus`, visibly, and gets classified then.
fn classify_device_status(
    status: u16,
    retry_after: Option<&str>,
    device_id: &str,
) -> Option<SmartMeError> {
    match status {
        200..=299 => None,
        401 | 403 => Some(SmartMeError::AuthRejected { status }),
        404 => Some(SmartMeError::UnknownDevice {
            device_id: device_id.to_string(),
        }),
        429 => Some(SmartMeError::RateLimited {
            retry_after_secs: retry_after.and_then(|v| v.trim().parse::<u64>().ok()),
        }),
        _ => Some(SmartMeError::HttpStatus { status }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

    fn creds() -> Credentials {
        Credentials::ClientCredentials {
            client_id: "id".to_string(),
            client_secret: "sekret-material".to_string(),
        }
    }

    #[test]
    fn https_base_is_accepted() {
        assert!(SmartMeClient::new("https://api.smart-me.com/", creds(), TIMEOUT).is_ok());
        assert!(SmartMeClient::new("https://api.smart-me.com:8443", creds(), TIMEOUT).is_ok());
    }

    #[test]
    fn non_tls_or_dirty_bases_are_refused_at_construction() {
        for bad in [
            "http://api.smart-me.com",
            "ftp://x",
            "api.smart-me.com",
            "https://",
            "https://user:pw@api.smart-me.com",
            "https://api.smart-me.com/?q=1",
            "https://api.smart-me.com/#frag",
            "https://api.smart-me.com/some/path",
        ] {
            let c = SmartMeClient::new(bad, creds(), TIMEOUT);
            assert!(
                matches!(c, Err(SmartMeError::NotHttps { .. })),
                "must refuse {bad:?}"
            );
        }
    }

    #[test]
    fn sub_second_timeout_is_refused() {
        let c = SmartMeClient::new(
            "https://api.smart-me.com",
            creds(),
            std::time::Duration::ZERO,
        );
        assert!(matches!(c, Err(SmartMeError::Misconfigured { .. })));
    }

    #[test]
    fn debug_output_never_contains_secret_material() {
        let client = SmartMeClient::new("https://api.smart-me.com", creds(), TIMEOUT).unwrap();
        let token = TokenState {
            access_token: "sekret-material".to_string(),
            expires_in_s: 3600,
        };
        let basic = Credentials::Basic {
            user: "gc".to_string(),
            password: "sekret-material".to_string(),
        };
        for dump in [
            format!("{client:?}"),
            format!("{token:?}"),
            format!("{basic:?}"),
            format!("{:?}", creds()),
        ] {
            assert!(!dump.contains("sekret-material"), "leak in {dump}");
            assert!(dump.contains("<redacted>") || dump.contains("base"));
        }
    }

    #[test]
    fn error_classification_is_centralized() {
        assert!(SmartMeError::AuthRejected { status: 401 }.is_fatal());
        assert!(
            SmartMeError::Misconfigured {
                reason: String::new()
            }
            .is_fatal()
        );
        assert!(
            SmartMeError::NotHttps {
                reason: String::new()
            }
            .is_fatal()
        );
        assert!(
            SmartMeError::UnknownDevice {
                device_id: String::new()
            }
            .is_fatal()
        );
        assert!(!SmartMeError::Timeout.is_fatal());
        // 500 rather than 429: since story 2.6 a 429 is `RateLimited` and never
        // reaches this variant, so asserting on it here would document a path
        // that no longer exists.
        assert!(!SmartMeError::HttpStatus { status: 500 }.is_fatal());
        assert!(
            !SmartMeError::Network {
                reason: String::new()
            }
            .is_fatal()
        );
        assert!(
            !SmartMeError::Decode {
                reason: String::new()
            }
            .is_fatal()
        );
    }

    /// **A device id smart-me does not know is a configuration fault, and it must
    /// latch.** Before this, a `404` fell through to `HttpStatus`, which
    /// `is_fatal` does not name, so the most likely configuration error there is —
    /// a mistyped device id — was published as `source-unreachable` and the bridge
    /// polled a device that does not exist for ever.
    ///
    /// The message is asserted, not merely the variant, because the whole point of
    /// splitting the refusal is where it sends the operator. A `404` has two
    /// origins and only one of them is a typo.
    ///
    /// FALSIFIED — three mutations, each RUN and its output copied here:
    /// - the `404` arm deleted from `classify_device_status`: RED,
    ///   *"a 404 must name the device smart-me refused, got HttpStatus { status:
    ///   404 }"* — the pre-fix behaviour exactly;
    /// - `UnknownDevice` removed from `is_fatal`: RED, *"an id smart-me does not
    ///   know does not come into existence on its own, so retrying is polling
    ///   nothing while reporting weather: …"*;
    /// - the second origin dropped from the `#[error]` string: RED, *"the operator
    ///   is sent to one place only; \"removed from the smart-me account\" missing
    ///   from …"*.
    #[test]
    fn an_unknown_device_id_is_fatal_and_names_both_origins() {
        let Some(e) = classify_device_status(404, None, "9202685") else {
            panic!("a 404 must not be read as a successful response");
        };
        assert!(
            matches!(&e, SmartMeError::UnknownDevice { device_id } if device_id == "9202685"),
            "a 404 must name the device smart-me refused, got {e:?}"
        );
        assert!(
            e.is_fatal(),
            "an id smart-me does not know does not come into existence on its own, \
             so retrying is polling nothing while reporting weather: {e}"
        );
        let shown = e.to_string();
        for origin in ["mistyped", "removed from the smart-me account"] {
            assert!(
                shown.contains(origin),
                "the operator is sent to one place only; {origin:?} missing from {shown:?}"
            );
        }
    }

    /// Each status that names a repair is its own error, and everything else is
    /// the one that names none. Success must classify as `None` — a mapping that
    /// invented an error on `200` would take every meter off the wire.
    #[test]
    fn each_status_that_names_a_repair_is_its_own_error() {
        assert!(classify_device_status(200, None, "d").is_none());
        assert!(classify_device_status(204, None, "d").is_none());
        for status in [401, 403] {
            assert!(
                matches!(
                    classify_device_status(status, None, "d"),
                    Some(SmartMeError::AuthRejected { status: s }) if s == status
                ),
                "{status} is the credential, not the configuration"
            );
        }
        assert!(
            matches!(
                classify_device_status(429, None, "d"),
                Some(SmartMeError::RateLimited { .. })
            ),
            "429 is the one wait this bridge honours (story 2.6), not a generic status"
        );
        assert!(
            matches!(
                classify_device_status(500, None, "d"),
                Some(SmartMeError::HttpStatus { status: 500 })
            ),
            "a server fault names no repair and must stay transient"
        );
    }

    /// `Retry-After` is honoured only in its seconds form. The date form is not
    /// parsed here — see [`SmartMeError::RateLimited`] — and the assertion pins
    /// that it yields "wait, but we were not told how long" rather than a delay
    /// silently read as zero.
    #[test]
    fn retry_after_is_read_only_as_a_delay_in_seconds() {
        let secs = |h: Option<&str>| match classify_device_status(429, h, "d") {
            Some(SmartMeError::RateLimited { retry_after_secs }) => retry_after_secs,
            other => panic!("429 must classify as RateLimited, got {other:?}"),
        };
        assert_eq!(secs(Some("120")), Some(120));
        assert_eq!(secs(Some("  120 ")), Some(120), "the header is trimmed");
        assert_eq!(secs(None), None, "absent means we were not told how long");
        assert_eq!(
            secs(Some("Wed, 12 Aug 2026 14:05:00 GMT")),
            None,
            "the date form must not be read as a number of seconds"
        );
    }

    #[test]
    fn error_display_never_embeds_credentials() {
        let e = SmartMeError::NotHttps {
            reason: "scheme is \"http\", require https".to_string(),
        };
        assert!(format!("{e}").contains("refusing endpoint"));
    }
}
