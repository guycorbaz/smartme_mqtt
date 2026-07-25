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
    /// Any other non-success HTTP status (5xx, 429, 3xx — redirects are not
    /// followed). Transient.
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
#[derive(Clone)]
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
        let status = resp.status().as_u16();
        if status == 401 || status == 403 {
            return Err(SmartMeError::AuthRejected { status });
        }
        if !resp.status().is_success() {
            return Err(SmartMeError::HttpStatus { status });
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
        assert!(!SmartMeError::Timeout.is_fatal());
        assert!(!SmartMeError::HttpStatus { status: 429 }.is_fatal());
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

    #[test]
    fn error_display_never_embeds_credentials() {
        let e = SmartMeError::NotHttps {
            reason: "scheme is \"http\", require https".to_string(),
        };
        assert!(format!("{e}").contains("refusing endpoint"));
    }
}
