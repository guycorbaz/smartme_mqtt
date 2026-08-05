//! Refusing a submission that did not come from this UI (Story 6.2 AC5).
//!
//! # Why a no-auth UI still needs this
//!
//! [ADR 0019] put the trust boundary at Traefik and gave the bridge no login.
//! That decision is sound and this does not reopen it — but it has a consequence
//! that "no authentication" does not cover, and which had to be decided here
//! rather than discovered later.
//!
//! **Precisely because there is no login, any page open in the operator's
//! browser can post to the bridge if that browser can reach it.** A drive-by
//! reconfiguration needs no credential to steal, because there are none. The
//! blast radius is a Sparkplug namespace a SCADA host persists and an operator
//! deletes by hand.
//!
//! # What was chosen, and what was not
//!
//! Three options were on the table (Story 6.2 AC5): a same-origin header check,
//! a per-render token in a hidden field, or nothing with the reasons written
//! down. **The header check was chosen** — it costs one comparison, holds no
//! state, cannot desynchronise, and needs no cookie. A token would need
//! somewhere to remember it, which is the session ADR 0019 deliberately does not
//! have.
//!
//! # The host is compared and the scheme is NOT
//!
//! Traefik terminates TLS in front of a bridge that speaks plain HTTP on a
//! Docker network, so `Origin` says `https://…` while the listener only ever
//! sees `http`. Comparing schemes would refuse every legitimate submission on
//! the one deployment this bridge has. The host is what identifies the origin;
//! the scheme is the proxy's business.
//!
//! # A missing `Origin` is allowed, and that is not a hole
//!
//! Browsers send `Origin` on every `POST` — including same-origin ones, since
//! Fetch made that mandatory. What does *not* send it is `curl`, which is how
//! FR23's headless bring-up and the container smoke test drive this surface.
//! Refusing an absent `Origin` would therefore break the non-browser callers
//! while stopping no browser attack, since the attacker's browser sends one and
//! it is wrong. `Sec-Fetch-Site` is honoured first where present, because it is
//! the header that actually distinguishes the case.
//!
//! [ADR 0019]: ../../../docs/adr/0019-no-auth-on-the-config-ui-secrets-are-write-only.md

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

/// The host part of an `Origin` value, with the scheme and any port kept.
///
/// `Origin` is `scheme "://" host [ ":" port ]`. The port stays, because a UI
/// reached on `:8080` and one reached on `:9090` are different origins to a
/// browser and there is no reason to be more permissive than it is.
fn authority(origin: &str) -> Option<&str> {
    origin.split_once("://").map(|(_, rest)| rest)
}

/// The refusal for a mutating request that did not come from this UI, or `None`
/// to proceed.
///
/// Phrased as "the refusal, if there is one" rather than as a `Result`: there is
/// no success value to carry, and a `Result` whose `Ok` is `()` and whose `Err`
/// is a whole HTTP response is 128 bytes of error on every call that has none.
pub(super) fn refusal(headers: &HeaderMap) -> Option<Response> {
    // `Sec-Fetch-Site` is decisive where the browser sends it.
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok())
        && site != "same-origin"
        && site != "none"
    {
        return Some(refuse(&format!(
            "this submission came from another site (Sec-Fetch-Site: {site})"
        )));
    }

    // No Origin at all: not a browser. See the module docs.
    let origin = headers.get("origin").and_then(|v| v.to_str().ok())?;
    let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) else {
        return Some(refuse(
            "the request carries an Origin but no Host, so there is nothing to \
             compare it against",
        ));
    };

    match authority(origin) {
        Some(from) if from == host => None,
        _ => Some(refuse(&format!(
            "this submission was issued by {origin}, which is not this bridge"
        ))),
    }
}

fn refuse(why: &str) -> Response {
    tracing::warn!(
        reason = why,
        "a configuration change was REFUSED because it did not come from this UI"
    );
    (
        StatusCode::FORBIDDEN,
        [("content-type", "text/html; charset=utf-8")],
        format!(
            "<!doctype html><meta charset=utf-8><title>Refused</title>\
             <h1>Refused</h1><p>{why}.</p><p>Nothing was changed. This bridge has no \
             login, so it refuses submissions that did not come from its own pages — \
             otherwise any page open in your browser could reconfigure it.</p>",
        ),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        map
    }

    /// **Both halves, and the second is the one that matters.** A guard that
    /// refused everything would pass a test that only checked refusals, and it
    /// would make the UI unusable rather than safe.
    #[test]
    fn a_submission_from_this_ui_is_accepted() {
        assert!(
            refusal(&headers(&[
                ("origin", "http://smartme.home.arpa"),
                ("host", "smartme.home.arpa"),
            ]))
            .is_none()
        );
    }

    #[test]
    fn a_submission_from_another_site_is_refused() {
        assert!(
            refusal(&headers(&[
                ("origin", "http://evil.example"),
                ("host", "smartme.home.arpa"),
            ]))
            .is_some()
        );
    }

    /// Traefik terminates TLS; the bridge speaks plain HTTP behind it. Comparing
    /// schemes would refuse every real submission on the only deployment this
    /// bridge has.
    #[test]
    fn https_in_front_of_a_plain_http_bridge_is_still_the_same_origin() {
        assert!(
            refusal(&headers(&[
                ("origin", "https://smartme.home.arpa"),
                ("host", "smartme.home.arpa"),
            ]))
            .is_none(),
            "the reverse proxy's scheme is not the bridge's business; refusing \
             here would break the deployment this UI exists for"
        );
    }

    /// A port is part of an origin to a browser, so it is part of one here.
    #[test]
    fn a_different_port_is_a_different_origin() {
        assert!(
            refusal(&headers(&[
                ("origin", "http://smartme.home.arpa:9090"),
                ("host", "smartme.home.arpa:8080"),
            ]))
            .is_some()
        );
    }

    /// FR23 promises a headless bring-up, and the container smoke test drives
    /// this surface with `curl`. Neither sends `Origin`.
    #[test]
    fn a_non_browser_caller_with_no_origin_is_allowed() {
        assert!(refusal(&headers(&[("host", "localhost:8080")])).is_none());
    }

    /// The header that actually distinguishes the case, where it is sent.
    #[test]
    fn sec_fetch_site_is_honoured_where_the_browser_sends_it() {
        assert!(
            refusal(&headers(&[
                ("sec-fetch-site", "cross-site"),
                ("origin", "http://smartme.home.arpa"),
                ("host", "smartme.home.arpa"),
            ]))
            .is_some(),
            "a cross-site fetch must be refused even when Origin and Host agree — \
             they can agree while the request came from somewhere else entirely"
        );
        assert!(
            refusal(&headers(&[
                ("sec-fetch-site", "same-origin"),
                ("origin", "http://smartme.home.arpa"),
                ("host", "smartme.home.arpa"),
            ]))
            .is_none()
        );
    }
}
