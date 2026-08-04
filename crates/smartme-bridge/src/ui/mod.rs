//! The embedded web surface (Story 6.1, AR11, FR33, FR44).
//!
//! # It exists in the states where the bridge publishes nothing
//!
//! That is the whole reason this story came before any screen. Since
//! [ADR 0023] every setting but the credential arrives through a browser, so a
//! server that only ran alongside a healthy Sparkplug session would be absent
//! from exactly the situations it exists for: a first run with no configuration,
//! and a configuration whose mapping nobody has confirmed.
//!
//! # No authentication, and the bind is not a detail
//!
//! [ADR 0019]: the trust boundary is Traefik, not this process. The listener
//! binds `0.0.0.0` **inside the container**, which is unreachable from the LAN
//! because the container publishes no host port — Traefik reaches it over a
//! shared Docker network and is the only thing that can.
//!
//! **Do not "harden" this to loopback.** It would not reduce exposure by one
//! byte; it would make the container unreachable from the single thing that is
//! meant to reach it. [`the_bind_address_is_not_loopback`] exists to catch that
//! well-meant change.
//!
//! # Nothing here may take the bridge down
//!
//! The same rule file logging follows: a diagnostic aid that can stop the
//! meters has stopped being an aid. A port already in use degrades to "no UI"
//! and says so; it never propagates.
//!
//! [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md
//! [ADR 0019]: ../../../docs/adr/0019-no-auth-on-the-config-ui-secrets-are-write-only.md

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::get;

use crate::app::poll_publish::LastLoopTick;

/// The port the UI listens on when the configuration does not say.
///
/// A default is required rather than merely convenient: the first run has no
/// `config.toml` to read a port from, and that is precisely the run that needs
/// the UI most.
pub const DEFAULT_PORT: u16 = 8080;

/// Which of the four startup states the process is in.
///
/// **One value, decided once in `main.rs`.** FR29 asks for *"a single internal
/// source of truth"*, and the cheapest way to break that is to let a template
/// re-derive the state from whatever it can see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// No `config.toml`. Nothing has been configured yet.
    Unconfigured,
    /// A valid configuration whose mapping no human has confirmed (Story 5.3).
    Unconfirmed,
    /// Configured, confirmed, publishing.
    Running,
}

impl Lifecycle {
    /// Whether the bridge is deliberately silent.
    ///
    /// **Not the same as unhealthy** — see [`healthz`].
    pub const fn is_silent_on_purpose(self) -> bool {
        matches!(self, Lifecycle::Unconfigured | Lifecycle::Unconfirmed)
    }

    const fn headline(self) -> &'static str {
        match self {
            Lifecycle::Unconfigured => "Not configured yet",
            Lifecycle::Unconfirmed => "Waiting for you to confirm the meter mapping",
            Lifecycle::Running => "Running",
        }
    }

    const fn detail(self) -> &'static str {
        match self {
            Lifecycle::Unconfigured => {
                "Nothing has been published and nothing will be until this bridge \
                 is configured. No connection to the broker has been opened."
            }
            Lifecycle::Unconfirmed => {
                "The configuration is valid, but nobody has checked that each meter \
                 points at the right device. Nothing is published — not even a \
                 birth — until it is confirmed."
            }
            Lifecycle::Running => "The bridge is connected and publishing.",
        }
    }

    /// The string `/healthz` reports. Stable, lowercase, machine-facing.
    const fn slug(self) -> &'static str {
        match self {
            Lifecycle::Unconfigured => "unconfigured",
            Lifecycle::Unconfirmed => "unconfirmed",
            Lifecycle::Running => "running",
        }
    }
}

/// What every handler can see.
#[derive(Clone)]
pub struct UiState {
    lifecycle: Lifecycle,
    /// Present only when there is a poll loop to have a heartbeat.
    heartbeat: Option<LastLoopTick>,
}

impl UiState {
    pub fn new(lifecycle: Lifecycle, heartbeat: Option<LastLoopTick>) -> Self {
        Self {
            lifecycle,
            heartbeat,
        }
    }
}

/// The router. Split from serving so a test can exercise handlers without a
/// socket, and so the socket can be exercised without guessing at routes.
pub fn router(state: UiState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .with_state(Arc::new(state))
}

/// Bind and serve until the process stops.
///
/// **Never fatal.** Every failure degrades to "no UI" and says so loudly on the
/// console; the meters keep publishing. A bridge that stopped because a port was
/// taken would have turned a diagnostic aid into an outage.
pub async fn serve(port: u16, state: UiState) {
    // 0.0.0.0 INSIDE THE CONTAINER — see the module docs before changing this.
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(
                %error,
                %addr,
                "the web UI could NOT start; the bridge keeps publishing without it. \
                 The usual cause is another process on this port"
            );
            return;
        }
    };
    tracing::info!(%addr, "web UI ready");
    if let Err(error) = axum::serve(listener, router(state)).await {
        tracing::error!(%error, "the web UI stopped; the bridge keeps publishing without it");
    }
}

/// The one page this story delivers: what the bridge believes about itself.
///
/// Deliberately plain. The screens are 6.2 and after; what 6.1 owes them is a
/// surface that exists in the states they will be used in, and an operator who
/// opens the address should never meet a blank page or a connection refusal.
async fn index(State(state): State<Arc<UiState>>) -> impl IntoResponse {
    Html(format!(
        "<!doctype html><meta charset=utf-8>\
         <title>smartme_mqtt</title>\
         <h1>smartme_mqtt</h1>\
         <p><strong>{}</strong></p>\
         <p>{}</p>\
         <hr><p>version {} · contract {}</p>",
        state.lifecycle.headline(),
        state.lifecycle.detail(),
        env!("CARGO_PKG_VERSION"),
        crate::adapters::sparkplug_publisher::CONTRACT_VERSION,
    ))
}

/// The endpoint Epic 7's Docker healthcheck consumes (FR33, AR12, FR44).
///
/// # A deliberate silence is NOT unhealthy, and this is the expensive one
///
/// Epic 7 wires this to a healthcheck that **restarts the container**. If an
/// unconfigured bridge answered "unhealthy", a fresh deployment would enter a
/// restart loop — and the loop would destroy, every few seconds, the very screen
/// needed to configure it. Epic 7's own rule is *"restart a wedged poller, never
/// an honest STALE"*; this extends it to never a deliberate silence either.
///
/// So the status code answers **"is this process worth keeping?"**, and the body
/// answers **"is it doing anything?"** — two different questions, and collapsing
/// them is what makes a healthcheck destructive.
///
/// FALSIFIED 2026-08-04 by returning `503` for a deliberate silence:
///
/// ```text
/// test healthz_does_not_call_a_deliberate_silence_unhealthy ... FAILED
/// an unconfirmed bridge must answer 200. […]
/// HTTP/1.1 503 Service Unavailable
/// ```
///
/// **The first attempt at that mutation MISSED ITS TARGET and the test stayed
/// green** — `rustfmt` had folded the return tuple onto one line, so the
/// multi-line pattern matched nothing. A mutation that does not apply proves
/// exactly as much as no mutation at all; the second attempt asserts that the
/// text actually changed before running anything.
async fn healthz(State(state): State<Arc<UiState>>) -> impl IntoResponse {
    let heartbeat = state
        .heartbeat
        .as_ref()
        .and_then(|h| h.last())
        .map(|t| t.0.to_string())
        .unwrap_or_else(|| "null".to_string());

    let body = format!(
        "{{\"status\":\"{}\",\"publishing\":{},\"last_loop_tick_ms\":{},\
          \"version\":\"{}\",\"contract\":{}}}",
        state.lifecycle.slug(),
        !state.lifecycle.is_silent_on_purpose(),
        heartbeat,
        // Compile-time, so it describes the BINARY and not the tag it wears —
        // the two can drift, which is why the publish workflow guards them.
        env!("CARGO_PKG_VERSION"),
        crate::adapters::sparkplug_publisher::CONTRACT_VERSION,
    );
    (StatusCode::OK, [("content-type", "application/json")], body)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC2 — and the test exists because the change it catches is one somebody
    /// makes ON PURPOSE, believing it an improvement.
    ///
    /// Binding to loopback would not reduce exposure: the container publishes no
    /// host port, so nothing on the LAN can reach it either way. It would only
    /// make the container unreachable from Traefik, which is the sole ingress.
    #[tokio::test]
    async fn the_bind_address_is_not_loopback() {
        // Port 0 asks the OS for a free one, so this asserts the ADDRESS without
        // colliding with anything.
        let listener = tokio::net::TcpListener::bind(std::net::SocketAddr::from(([0, 0, 0, 0], 0)))
            .await
            .expect("bind");
        let bound = listener.local_addr().expect("addr");
        assert!(
            !bound.ip().is_loopback(),
            "the UI must not bind loopback: the container publishes no host port, \
             so loopback protects nothing and makes it unreachable from Traefik, \
             which is the only thing meant to reach it"
        );
    }

    #[test]
    fn a_deliberate_silence_is_not_a_failure() {
        assert!(Lifecycle::Unconfigured.is_silent_on_purpose());
        assert!(Lifecycle::Unconfirmed.is_silent_on_purpose());
        assert!(
            !Lifecycle::Running.is_silent_on_purpose(),
            "a running bridge is not silent, and if this ever says otherwise the \
             healthcheck stops being able to notice a wedged one"
        );
    }

    /// The three states must not describe themselves in the same words — an
    /// operator who cannot tell them apart cannot act, and only one of them is a
    /// click away from being fixed.
    #[test]
    fn each_state_says_something_different() {
        let said: Vec<_> = [
            Lifecycle::Unconfigured,
            Lifecycle::Unconfirmed,
            Lifecycle::Running,
        ]
        .iter()
        .map(|s| (s.headline(), s.detail(), s.slug()))
        .collect();
        for (i, a) in said.iter().enumerate() {
            for b in said.iter().skip(i + 1) {
                assert_ne!(a.0, b.0, "two states share a headline");
                assert_ne!(a.1, b.1, "two states share a description");
                assert_ne!(a.2, b.2, "two states share a machine-facing slug");
            }
        }
    }
}
