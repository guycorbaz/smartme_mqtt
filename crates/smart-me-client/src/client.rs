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
use crate::types::{Device, DeviceListing};

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
    /// something that is not there while publishing the fault as `Transient` —
    /// telling an operator to wait for something that will never pass.
    ///
    /// **Three origins, and the message names all three because they send an
    /// operator to different places**: the id is mistyped in the configuration, the
    /// device existed and has been removed from the account, or `api_base` is not
    /// the smart-me API at all. The second arrives on a configuration that worked
    /// yesterday, without anyone typing. The third was added by the 2026-08-13
    /// review, and it is the one with the widest blast radius: a wrong endpoint
    /// `404`s EVERY meter, so all of them latch at once on a message that sends the
    /// operator to a device id which is perfectly correct.
    ///
    /// Story 2.6's review found this reaching the wire as `source-unreachable`:
    /// `is_fatal` did not name it, so the single most likely configuration error
    /// was published as a network fault and never latched.
    #[error(
        "smart-me does not know device {device_id}. Either the device id is mistyped in the \
         configuration, the device has been removed from the smart-me account, or api_base is \
         not the smart-me API — check that one first if EVERY meter is refused. Correct the \
         configuration, then restart"
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
    ///
    /// # The field name, and the one case it does not exist (story 2.6 AC5, [#73])
    ///
    /// `reason` carries **serde's own message**, because `get_device` parses the
    /// body itself instead of using `resp.json()` — see the comment there. serde
    /// names the field when a field is **missing**:
    ///
    /// ```text
    /// missing field `ActivePower` at line 2 column 76
    /// ```
    ///
    /// **It names none when the field is present with the wrong type**, an explicit
    /// `null` included:
    ///
    /// ```text
    /// invalid type: null, expected f64 at line 3 column 31
    /// ```
    ///
    /// That gap is not ours to close from here, and it matters more than it looks:
    /// the API's own description declares SIX of the eight fields this client
    /// consumes as nullable (`docs/spec/smart-me-api/`), so the nameless case is the
    /// one the wire is most likely to produce. Closing it needs either
    /// `serde_path_to_error` or `Option` fields judged per metric — a design
    /// decision, recorded rather than taken.
    #[error("response decode failed: {reason}")]
    Decode {
        /// serde's message, verbatim. May quote an offending *value* as well as a
        /// field name; device payloads carry readings, never credentials.
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
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
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
        if let Some(e) = classify_token_status(status, retry_after.as_deref()) {
            return Err(e);
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
        // DECODED IN TWO STEPS, AND NOT WITH `resp.json()`, so that serde's message
        // survives (story 2.6 AC5, [#73]). `reqwest::Error`'s `Display` writes only
        // its kind and the URL — `error.rs:227-272` in 0.13.1 emits "error decoding
        // response body for url (…)" and nothing else; serde's text sits in the
        // `source()` chain, which `from_reqwest` does not walk. So `resp.json()`
        // throws away the one thing an operator needs: WHICH field the API changed.
        //
        // Taking the body as text first and parsing it here keeps serde's own error,
        // which names the field when it can — see `SmartMeError::Decode` for the case
        // it cannot.
        // BYTES, NOT `text()`. Found by the 2026-08-13 review: `text()` performs a
        // LOSSY UTF-8 conversion, so a body carrying invalid UTF-8 inside a JSON
        // string — a device `Name` typed in a non-UTF-8 locale, say — would be
        // silently accepted with U+FFFD substitutions where `resp.json()` used to
        // refuse it outright. `from_slice` keeps serde's message, which is the whole
        // point of parsing here, AND the byte-exact behaviour of what it replaced.
        let body = resp.bytes().await.map_err(SmartMeError::from_reqwest)?;
        let device = decode_device(&body)?;
        Ok(DeviceCapture {
            device,
            http_date_ms,
        })
    }

    /// `GET {base}/Devices` — the account listing (story 3.4, FR2).
    ///
    /// Same auth path as [`Self::get_device`]; no `Date` capture, because a
    /// listing feeds a configuration screen and no freshness oracle. The
    /// description declares only `200` for this path, as it does everywhere —
    /// failure behaviour is learned from the wire, never from it.
    ///
    /// # A `404` here is not [`SmartMeError::UnknownDevice`], decided
    ///
    /// [`classify_device_status`] maps `404` to a fatal unknown-DEVICE refusal
    /// naming the id an operator should fix — a diagnosis with no subject on
    /// the collection, which names no id and has never been observed to `404`.
    /// If it ever does, the API surface itself moved, and the honest report is
    /// the visible, transient [`SmartMeError::HttpStatus`] — the arm for a
    /// status that names no repair — not a latching instruction to fix a
    /// configuration row that does not exist.
    pub async fn get_devices(
        &self,
        token: Option<&TokenState>,
    ) -> Result<DeviceList, SmartMeError> {
        let req = self.http.get(format!("{}/Devices", self.base));
        let req = match (token, &self.credentials) {
            (Some(t), _) => req.bearer_auth(&t.access_token),
            (None, Credentials::Basic { user, password }) => req.basic_auth(user, Some(password)),
            (None, Credentials::ClientCredentials { .. }) => {
                return Err(SmartMeError::Misconfigured {
                    reason: "client-credentials mode needs a token (call fetch_token)".to_string(),
                });
            }
        };
        let resp = req.send().await.map_err(SmartMeError::from_reqwest)?;
        let status = resp.status().as_u16();
        if status == 404 {
            return Err(SmartMeError::HttpStatus { status });
        }
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        if let Some(e) = classify_device_status(status, retry_after.as_deref(), "the collection") {
            return Err(e);
        }
        let body = resp.bytes().await.map_err(SmartMeError::from_reqwest)?;
        decode_devices(&body)
    }
}

/// Parses a response body into a [`Device`], keeping serde's message when it fails.
///
/// Separate from `get_device` for the same reason [`classify_device_status`] is: the
/// property lives in the parse, not in the HTTP round-trip, and this crate has no way
/// to reach code that sits behind a live request. Story 2.6 AC5 could not be tested
/// while this was one line inside an `async fn`.
fn decode_device(body: &[u8]) -> Result<Device, SmartMeError> {
    serde_json::from_slice(body).map_err(|e| SmartMeError::Decode {
        reason: e.to_string(),
    })
}

/// The account listing as decoded: what parsed, and what was DROPPED with its
/// reason — never silently (story 3.4 AC1).
///
/// A listing takes the opposite trade from a measurement. Measurements are
/// fail-closed — a payload the bridge cannot read yields no reading at all —
/// because a substituted number is a lie on the wire. A LISTING that failed as
/// a whole over one malformed element would be the silent failure instead: an
/// empty screen, and every pickable meter gone for a reason nobody is told. So
/// one bad element costs that element, counted and named, and the caller is
/// obliged by this type to know about it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceList {
    /// The devices that parsed, in the order the API sent them.
    pub devices: Vec<DeviceListing>,
    /// serde's reason for each element that did not, kept verbatim — the
    /// field-naming discipline of story 2.6 AC5 applies to a device an
    /// operator cannot see just as it does to a reading they cannot use.
    pub dropped: Vec<String>,
}

/// Parses a `GET /Devices` body: tolerant PER ELEMENT, fail-closed as a whole.
///
/// Pure, beside [`decode_device`], for the same reason (story 3.4 AC1, and
/// Epic 2's action C1): the property lives in the parse, and it must be
/// assertable without an HTTP harness. A body that is not a JSON array at all
/// is a response we cannot account for and fails whole, serde's reason kept.
fn decode_devices(body: &[u8]) -> Result<DeviceList, SmartMeError> {
    let elements: Vec<serde_json::Value> =
        serde_json::from_slice(body).map_err(|e| SmartMeError::Decode {
            reason: e.to_string(),
        })?;
    let mut devices = Vec::new();
    let mut dropped = Vec::new();
    for element in elements {
        match serde_json::from_value::<DeviceListing>(element) {
            Ok(device) => devices.push(device),
            Err(reason) => dropped.push(reason.to_string()),
        }
    }
    Ok(DeviceList { devices, dropped })
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
/// **Two callers since story 3.4, and only one has a device id.** `get_devices`
/// pre-empts `404` and passes a placeholder for `device_id` — so any FUTURE
/// id-bearing arm added here (the doc below contemplates a `400`) must first
/// ask what the message reads like from the collection caller, or a latching
/// instruction naming "device the collection" reaches an operator. The
/// story 3.4 review flagged the trap; this sentence is the tripwire.
///
/// # Only `404`, deliberately
///
/// A `400` on a device id that passed the local shape check would plausibly also
/// mean "the id is wrong", but this API has never been observed returning one and
/// guessing its meaning would be a fact about smart-me that nobody measured — the
/// refusal story 2.2 AC4 and ADR 0033 both made. If a `400` appears in the field
/// it arrives as `HttpStatus`, visibly, and gets classified then.
/// What a token-exchange status means, for every status but `400` ([#77]).
///
/// # Why a sibling of [`classify_device_status`] rather than a caller of it
///
/// The two endpoints do not answer the same questions. `404` means *this device
/// is not in the account* on one and nothing at all on the other; `400` carries
/// an OAuth error body that only this one has, and reading it is `async`, which
/// is why that status is settled before this function is reached.
///
/// What they DO share is the one that matters here: **the rate limit is applied
/// to the account, so a `429` reaches both** — and this is the endpoint that runs
/// first, since `ensure_token` precedes every fetch whose token has expired.
/// Story 2.6's AC3 built the wait for the device fetch alone, so the mechanism
/// failed precisely on the day it was needed: unclassified, a `429` here fell to
/// `HttpStatus` → `Transient` → `source-unreachable`, and no wait was armed.
///
/// `Retry-After` is read exactly as it is on the device path, in its seconds form
/// only — the date form yields "wait, but we were not told how long" rather than
/// a delay silently read as zero.
fn classify_token_status(status: u16, retry_after: Option<&str>) -> Option<SmartMeError> {
    match status {
        200..=299 => None,
        401 | 403 => Some(SmartMeError::AuthRejected { status }),
        429 => Some(SmartMeError::RateLimited {
            retry_after_secs: retry_after.and_then(|v| v.trim().parse::<u64>().ok()),
        }),
        _ => Some(SmartMeError::HttpStatus { status }),
    }
}

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

    /// **Story 3.4 AC1 — one bad element costs that element, counted and
    /// named, and the rest of the account stays pickable.**
    ///
    /// The trade is the OPPOSITE of a measurement's fail-closed rule, on
    /// purpose: a listing that failed whole over one malformed element would be
    /// an empty screen with every meter gone for a reason nobody is told —
    /// which is the silent failure, in a story whose whole point is that
    /// absence gets said.
    #[test]
    fn a_malformed_element_costs_itself_and_not_the_list() {
        let body = br#"[
            {"Id":"aaa-1","Name":"appart-est","Serial":9202685,"ActivePower":0.7},
            {"Id":"bbb-2","Name":"broken"},
            {"Id":"ccc-3","Name":null,"Serial":6387488},
            {"Id":"ddd-4","Serial":6387987}
        ]"#;
        let list = decode_devices(body).expect("three of four elements are well-formed");
        assert_eq!(
            list.devices.len(),
            3,
            "the well-formed devices survive their neighbour"
        );
        assert_eq!(list.devices[0].serial, 9202685);
        assert_eq!(
            list.devices[0].name.as_deref(),
            Some("appart-est"),
            "unknown fields are ignored, as everywhere (the API may widen)"
        );
        assert_eq!(
            list.devices[1].name, None,
            "a NULL name stays absent — nothing invents a name for a meter"
        );
        assert_eq!(
            list.devices[2].name, None,
            "and an OMITTED name reads the same as a null one: a listing is not \
             a measurement, so absence of a display field is not a refusal"
        );
        assert_eq!(list.dropped.len(), 1, "the drop is COUNTED");
        assert!(
            list.dropped[0].contains("Serial"),
            "and NAMED, with serde's own reason — the story 2.6 AC5 discipline; \
             a silently vanished meter is a drop-down lying by omission: {}",
            list.dropped[0]
        );
    }

    /// The two boundary shapes of AC1: an empty account is a LISTING (a state,
    /// not a fault), and a body that is not an array at all is a response we
    /// cannot account for and fails whole, serde's reason kept.
    #[test]
    fn an_empty_account_lists_and_a_non_array_refuses_whole() {
        let empty = decode_devices(b"[]").expect("an empty account is not an error");
        assert!(empty.devices.is_empty() && empty.dropped.is_empty());

        let refused = decode_devices(br#"{"Error":"maintenance"}"#)
            .expect_err("an object where an array was owed is not a listing");
        assert!(
            matches!(&refused, SmartMeError::Decode { reason } if reason.contains("sequence")),
            "the refusal keeps serde's reason rather than summarising it: {refused:?}"
        );
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
    ///   know does not come into existence on its own, so a Transient verdict would
    ///   poll nothing for ever and ask an operator to wait for it: …"* — re-run on
    ///   2026-08-13 after the assertion's wording changed, because a note that
    ///   quotes a message the test no longer emits is a prediction again;
    /// - an origin dropped from the `#[error]` string: RED, *"the operator is sent
    ///   to one place only; \"not the smart-me API\" missing from …"* — re-run on
    ///   2026-08-13 when the review added the third origin.
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
             so a Transient verdict would poll nothing for ever and ask an operator \
             to wait for it: {e}"
        );
        let shown = e.to_string();
        for origin in [
            "mistyped",
            "removed from the smart-me account",
            "not the smart-me API",
        ] {
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

    /// [#77] — the wait is armed on the token endpoint too, and that is the one
    /// that runs FIRST.
    ///
    /// # The gap this closes, in the words the issue used
    ///
    /// Story 2.6's AC3 promised *"a 429 is honoured, and it is the only
    /// source-side wait this story builds"* — and built it for the device fetch.
    /// The account-wide limit that produces a `429` on `/Devices/{id}` produces
    /// one on `/oauth/token` as well, and `ensure_token` precedes every fetch
    /// whose token has expired. **The endpoint that was not covered is the one
    /// that runs first**, so the mechanism failed on exactly the day it was
    /// needed: a `429` fell to `HttpStatus` → `Transient` → `source-unreachable`,
    /// and nothing waited.
    ///
    /// The control is the pair below it: `401` must stay the credential and `500`
    /// must stay transient. Without them, a classifier answering `RateLimited` to
    /// everything would pass the first assertion — and arming a wait on a rejected
    /// credential is worse than not arming one at all, because it hides a fault a
    /// wait can never clear.
    ///
    /// **FALSIFIED 2026-08-24**, two mutations RUN:
    ///
    /// - the `429` arm deleted, which is the state this issue reported: RED with
    ///   `HttpStatus { status: 429 }` — the exact shape that armed no wait;
    /// - `retry_after` ignored (`retry_after_secs: None` always): RED — and on the
    ///   FIRST assertion, which pins `Some(120)`, not on the one written for the
    ///   absent header. Caught, and the reason recorded is the measured one.
    ///
    /// # What this does NOT cover, measured and stated
    ///
    /// That `fetch_token` calls this at all. Restoring the old
    /// `if !resp.status().is_success()` — the state [#77] reported — leaves the
    /// whole workspace green, measured 2026-08-24.
    ///
    /// It is not coverable the way the same gap was closed three times elsewhere
    /// today, and the reason is a deliberate one: [`SmartMeClient::new`] refuses
    /// any base URL that is not `https` (NFR13), so a stub answering `429` over
    /// plain HTTP cannot be reached, and standing up TLS inside a unit test would
    /// cost more than the branch is worth.
    ///
    /// **The same is true of [`classify_device_status`]'s call sites**, and has
    /// been since they were extracted — so this is the shape of the client's
    /// testing, not a regression introduced here. Worth its own issue rather than
    /// a silence: what is pinned everywhere in this file is the mapping, never the
    /// dispatch.
    #[test]
    fn a_rate_limit_on_the_token_endpoint_arms_the_same_wait() {
        assert!(
            matches!(
                classify_token_status(429, Some("120")),
                Some(SmartMeError::RateLimited {
                    retry_after_secs: Some(120)
                })
            ),
            "the account-wide limit reaches this endpoint too, and it is the one \
             `ensure_token` runs before every expired fetch"
        );
        assert_eq!(
            match classify_token_status(429, None) {
                Some(SmartMeError::RateLimited { retry_after_secs }) => retry_after_secs,
                other => panic!("429 must classify as RateLimited, got {other:?}"),
            },
            None,
            "absent means we were not told how long, never zero"
        );

        // THE CONTROLS, and they are what stop this passing for the wrong reason.
        for status in [401, 403] {
            assert!(
                matches!(
                    classify_token_status(status, None),
                    Some(SmartMeError::AuthRejected { status: s }) if s == status
                ),
                "{status} is the credential: arming a wait on it would hide a fault \
                 no wait can clear"
            );
        }
        assert!(
            matches!(
                classify_token_status(500, None),
                Some(SmartMeError::HttpStatus { status: 500 })
            ),
            "a server fault names no repair and must stay transient"
        );
        assert!(
            classify_token_status(200, None).is_none(),
            "and a token that was issued is not an error"
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
        // Added with [#76]: the sign and the decimal point are the two near-miss
        // shapes a `u64` parse could conceivably be loosened to accept later.
        for near_miss in ["-5", "12.5", ""] {
            assert_eq!(
                secs(Some(near_miss)),
                None,
                "{near_miss:?} is not a delay in whole seconds and must not become one"
            );
        }
    }

    /// **Story 2.6 AC5 — the field the API changed reaches the operator.**
    ///
    /// The residual story 2.5 left: a payload the deserializer refused arrived as a
    /// failure naming nothing. It was `resp.json()` that lost it — `reqwest::Error`'s
    /// `Display` writes only its kind and the URL (`error.rs:227-272`, v0.13.1), and
    /// serde's text sits in a `source()` chain nobody walked.
    ///
    /// The assertion is on the OPERATOR-FACING string, not on the variant: the whole
    /// criterion is what someone reads, and `Decode`'s `Display` is what they get.
    ///
    /// FALSIFIED — mutation RUN, message copied: `decode_device` made to return what
    /// `reqwest` would have given (`reason: "error decoding response body"`) goes RED
    /// here and in the null test — *"the refusal must at least say what arrived:
    /// \"response decode failed: error decoding response body\""*. That string is
    /// exactly what shipped before this change.
    #[test]
    fn a_refused_payload_names_the_field_the_api_changed() {
        // A NON-NULLABLE field, since [#74]. `Serial` and `Id` are the two the
        // API's description does not allow to be null, so they are the two whose
        // absence still means *the shape of the answer changed* rather than
        // *this meter had nothing to report* — and they are what this guard is
        // about. The nullable six are carried as absences now, each degrading its
        // own metric.
        let without_serial = r#"{
            "Id": "1", "Name": "n",
            "ActivePower": 0.7, "ActivePowerUnit": "kW",
            "CounterReading": 4843.822, "CounterReadingUnit": "kWh",
            "ValueDate": "2026-07-25T13:06:32.0500519Z"
        }"#;
        let e = decode_device(without_serial.as_bytes())
            .expect_err("a payload we cannot read must not parse");
        assert!(matches!(e, SmartMeError::Decode { .. }));
        let shown = e.to_string();
        assert!(
            shown.contains("Serial"),
            "an operator learns nothing from a decode failure that names no field: {shown:?}"
        );
        assert!(
            !e.is_fatal(),
            "a payload anomaly is retried; only the shape of the answer changed"
        );
    }

    /// **[#74] — a `null` is no longer refused at all, so there is no name to
    /// miss.**
    ///
    /// This slot held `a_null_is_refused_and_serde_names_no_field_for_it`, which
    /// pinned that serde names a field it did not find and names none when the
    /// field is there with the wrong type — `invalid type: null, expected f64 at
    /// line 3 column 31`, a line and a column into a payload no operator ever
    /// sees. It carried its own condition for removal: *"if serde has started
    /// naming the field here, this limitation is over"*.
    ///
    /// **Serde did not start naming it. The question stopped being asked.** Six of
    /// the eight fields are nullable per the API's description, so a `null` is now
    /// carried as the absence it is and degrades its own metric. There is no
    /// decode failure left to name anything, which closes the naming half of [#74]
    /// by removing the case rather than by improving the message.
    ///
    /// FALSIFIED 2026-08-24: making `active_power` a bare `f64` again brings the
    /// refusal back and turns this red.
    #[test]
    fn a_null_is_carried_as_an_absence_rather_than_refused() {
        let null_power = r#"{
            "Id": "1", "Name": "n", "Serial": 30000001,
            "ActivePower": null, "ActivePowerUnit": "kW",
            "CounterReading": 4843.822, "CounterReadingUnit": "kWh",
            "ValueDate": "2026-07-25T13:06:32.0500519Z"
        }"#;
        let device = decode_device(null_power.as_bytes())
            .expect("a null in one metric must not cost the reading");
        assert_eq!(device.active_power, None);
        assert_eq!(
            device.counter_reading,
            Some(4843.822),
            "the energy index was readable throughout, and losing it to the power's \
             null is what [#74] reported"
        );
    }

    /// **A body that is not valid UTF-8 is refused, not silently repaired.**
    ///
    /// Found by the 2026-08-13 review. The first version of this change took the
    /// body through `resp.text()`, which performs a LOSSY conversion: invalid bytes
    /// become U+FFFD and the payload parses. `resp.json()`, which it replaced, ran
    /// `from_slice` on the raw bytes and refused. A device `Name` typed in a
    /// non-UTF-8 locale would therefore have been accepted with substituted
    /// characters — a value nobody sent, reaching the wire under a `Good` quality.
    ///
    /// FALSIFIED — mutation RUN, message copied: restoring the lossy path
    /// (`from_str(&String::from_utf8_lossy(body))`) goes RED, and the panic prints
    /// the defect itself — *"a byte sequence we cannot read is not a name we can
    /// publish: Device { id: \"1\", name: \"\u{fffd}\", … }"*. The substituted
    /// character reaching the domain type is the whole finding.
    #[test]
    fn a_body_that_is_not_utf8_is_refused_rather_than_repaired() {
        let mut body = br#"{"Id":"1","Name":""#.to_vec();
        body.push(0xFF); // not valid UTF-8, inside a JSON string
        body.extend_from_slice(
            br#"","Serial":30000001,"ActivePower":0.018,"ActivePowerUnit":"kW",
                "CounterReading":4843.822,"CounterReadingUnit":"kWh",
                "ValueDate":"2026-07-25T13:06:32.0500519Z"}"#,
        );
        let e = decode_device(&body)
            .expect_err("a byte sequence we cannot read is not a name we can publish");
        assert!(
            matches!(e, SmartMeError::Decode { .. }),
            "and it is a decode failure, retried, not a fatal one: {e:?}"
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
