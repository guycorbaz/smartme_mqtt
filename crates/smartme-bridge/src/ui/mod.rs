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

use crate::app::supervisor::Control;

mod origin;
mod screens;

/// How many publish periods the poll loop may miss before `/healthz` calls the
/// bridge unhealthy (AR12's `N`).
///
/// Three, not one: a single missed tick is a slow cloud call, which is exactly
/// the "honest STALE" AR12 says must never trigger a restart. Three consecutive
/// misses is a loop that is not looping.
pub const WEDGED_AFTER_PERIODS: u32 = 3;

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

/// Which phase the process is in, and what that phase makes observable.
///
/// **One value, swapped in place** rather than one server per phase. Story 6.2
/// AC7 requires that confirming a mapping starts the publishing bridge with no
/// human intervention, so the process now loops over its phases — and the web
/// surface has to outlive that loop, because it is the thing the operator is
/// looking at while it turns. Rebuilding the server per phase would close the
/// listener and re-bind it, and a `bind` that failed would degrade to "no UI"
/// at exactly the moment the operator had just used it.
///
/// It is still ONE source of truth: `main.rs` decides the phase and stores it
/// here; nothing re-derives it from what it can see.
#[derive(Clone)]
pub struct Phase {
    lifecycle: Lifecycle,
    /// Present only when there is a poll loop to have a heartbeat — it carries
    /// the heartbeat, the clock that reads it and the configuration that says
    /// how often it should tick, all three of which AR12 needs and none of which
    /// answers alone.
    running: Option<Control>,
}

/// The phase, shared between `main.rs`'s lifecycle loop and the running server.
pub type PhaseHandle = Arc<arc_swap::ArcSwap<Phase>>;

impl Phase {
    /// A bridge that is deliberately not publishing.
    pub fn silent(lifecycle: Lifecycle) -> Self {
        Self {
            lifecycle,
            running: None,
        }
    }

    /// A bridge that is publishing.
    ///
    /// Carries the whole [`Control`] rather than the three fields `/healthz`
    /// happens to read. Story 6.2 AC4 needs the screens to say what a change
    /// COSTS before it is made, and only the control surface can answer that —
    /// `Control::apply` is what returns the [`Plan`], and `Control::current` is
    /// what reports the configuration genuinely in force rather than the one
    /// just posted.
    pub fn running(control: Control) -> Self {
        Self {
            lifecycle: Lifecycle::Running,
            running: Some(control),
        }
    }

    /// A bridge that is configured, confirmed, and **building its runtime** —
    /// past every silence, not yet holding a control surface.
    ///
    /// # This exists because the UI told a lie, and CI caught it
    ///
    /// Story 6.2 made the web server outlive every phase, which is what lets a
    /// confirmation start the bridge without a restart. It also means the server
    /// answers **before** `run_with_control` hands back the [`Control`] — and in
    /// that window the phase was still the handle's initial value,
    /// `Unconfigured`. So a bridge with a valid, confirmed configuration, busy
    /// opening its MQTT session, served a page saying *"Not configured yet"*.
    ///
    /// The window is short. It never opened on a developer machine and opened on
    /// every CI run, which is the only reason it was found before a deployment
    /// met it — and an operator meeting it would reasonably have gone and
    /// rewritten a configuration that was already correct.
    ///
    /// `lifecycle: Running` with no control is not a fudge: it says *this bridge
    /// intends to publish*, which is true, while `loop_age()` stays `None` so
    /// `/healthz` reports no heartbeat rather than inventing a plausible instant.
    /// That is the same shape as a poll loop that has not completed its first
    /// iteration, which `healthz` already treats as starting rather than stuck.
    pub fn starting() -> Self {
        Self {
            lifecycle: Lifecycle::Running,
            running: None,
        }
    }

    /// The live control surface, when there is a running bridge to control.
    pub(crate) fn control(&self) -> Option<&Control> {
        self.running.as_ref()
    }

    /// A handle holding this phase, for a process that has not started looping.
    pub fn into_handle(self) -> PhaseHandle {
        Arc::new(arc_swap::ArcSwap::from_pointee(self))
    }

    /// How long since the poll loop last started an iteration, and how long it is
    /// allowed to be — `None` when there is no loop.
    fn loop_age(&self) -> Option<(i64, i64)> {
        let control = self.running.as_ref()?;
        let last = control.heartbeat().last()?;
        let age = control.clock().monotonic().0 - last.0;
        let allowed = i64::try_from(
            control.current().poll.interval.as_millis() * u128::from(WEDGED_AFTER_PERIODS),
        )
        .unwrap_or(i64::MAX);
        Some((age, allowed))
    }
}

/// What every handler can see.
#[derive(Clone)]
pub struct UiState {
    phase: PhaseHandle,
    /// Where `config.toml` lives. The screens write through
    /// [`crate::app::store`], which needs the directory rather than the parsed
    /// configuration — and in the unconfigured phase there is no parsed
    /// configuration to carry it.
    state_dir: std::path::PathBuf,
    /// How a screen tells the lifecycle loop that the configuration became
    /// ready. **A nudge, never the configuration itself**: the loop re-reads the
    /// file, because the file is the configuration and a loop that trusted a
    /// message would be a second source (Story 6.2 Task 7).
    ready: Arc<tokio::sync::Notify>,
}

impl UiState {
    pub fn new(
        phase: PhaseHandle,
        state_dir: std::path::PathBuf,
        ready: Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            phase,
            state_dir,
            ready,
        }
    }

    /// The phase as it is right now.
    pub(crate) fn phase(&self) -> arc_swap::Guard<Arc<Phase>> {
        self.phase.load()
    }

    /// Where `config.toml` lives.
    pub(crate) fn state_dir(&self) -> &std::path::Path {
        &self.state_dir
    }

    /// Tell the lifecycle loop to look at the file again.
    ///
    /// **Carries nothing.** The loop re-reads and re-validates; a screen that
    /// handed it the values it had just posted would make the process a second
    /// source of the configuration, and the two would part company at the first
    /// write that did not come through a browser.
    pub(crate) fn notify_ready(&self) {
        // `notify_one`, NOT `notify_waiters`.
        //
        // `notify_waiters` stores no permit: a nudge issued while the loop is
        // between `decide()` and its next `notified()` — and `decide` re-reads
        // the file, re-validates, and builds a whole HTTP client — is dropped on
        // the floor. Two submissions back to back, which is exactly what a
        // scripted bring-up and `docker-smoke.sh` do, lose the second. The
        // operator confirms, is redirected, and the page still says the mapping
        // is unconfirmed — AC7 failing in the shape of a flake. `notify_one`
        // holds a permit for a waiter that has not arrived yet.
        self.ready.notify_one();
    }
}

/// The router. Split from serving so a test can exercise handlers without a
/// socket, and so the socket can be exercised without guessing at routes.
pub fn router(state: UiState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route(
            "/config",
            get(screens::config_form).post(screens::save_config),
        )
        // Confirming is its OWN route and its OWN submission (AC3). A
        // confirmation folded into the save would be a click the operator makes
        // for a different reason, which is how a guard becomes a formality.
        .route(
            "/confirm",
            get(screens::confirm_form).post(screens::confirm_mapping),
        )
        .with_state(Arc::new(state))
}

/// Bind and serve until the process stops.
///
/// **Never fatal.** Every failure degrades to "no UI" and says so loudly on the
/// console; the meters keep publishing. A bridge that stopped because a port was
/// taken would have turned a diagnostic aid into an outage.
/// Bind the UI's listener.
///
/// Split out of [`serve`] so a test can ask **production** which address it
/// chose. The version of that test written first created its own listener and
/// asserted a property of that, which is a tautology about a socket `serve`
/// never touched.
///
/// `0.0.0.0` INSIDE THE CONTAINER — see the module docs before changing this.
pub async fn bind(port: u16) -> std::io::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(std::net::SocketAddr::from(([0, 0, 0, 0], port))).await
}

pub async fn serve(port: u16, state: UiState) {
    let listener = match bind(port).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(
                %error,
                port,
                "the web UI could NOT start; the bridge keeps publishing without it. \
                 The usual cause is another process on this port"
            );
            return;
        }
    };
    // The BOUND address, not the requested one: with `ui_port = 0` the OS picks
    // a port, and logging the request would name a port nothing is listening on.
    // This is the line an operator greps to find the UI.
    match listener.local_addr() {
        Ok(addr) => tracing::info!(%addr, "web UI ready"),
        Err(error) => tracing::info!(%error, port, "web UI ready (address unreadable)"),
    }
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
    let lifecycle = state.phase().lifecycle;
    Html(format!(
        "<!doctype html><meta charset=utf-8>\
         <title>smartme_mqtt</title>\
         <h1>smartme_mqtt</h1>\
         <p><strong>{}</strong></p>\
         <p>{}</p>\
         <hr><p>version {} · contract {}</p>",
        lifecycle.headline(),
        lifecycle.detail(),
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
    let phase = state.phase();
    let wedged = match phase.loop_age() {
        Some((age, allowed)) => age > allowed,
        // No loop, or a loop that has not ticked once yet. Neither is a wedge:
        // the silent states have no loop by design, and a bridge that has not
        // completed its first iteration is starting, not stuck.
        None => false,
    };
    let (age, allowed) = phase
        .loop_age()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .unwrap_or_else(|| ("null".to_string(), "null".to_string()));

    // `intends_to_publish`, NOT `publishing`.
    //
    // It said `publishing` until a review on 2026-08-04, and reported `true` for
    // any bridge that had reached the Running arm — including one whose broker
    // was unreachable and which had put nothing on the wire. The value was a
    // compile-time constant describing which branch of `main.rs` ran, presented
    // as an observation. This project's whole purpose is not to report something
    // as working when it is not, and the endpoint that exists to say so was
    // saying it.
    //
    // Broker connectivity is not plumbed to the UI yet ([#53]); until it is, the
    // honest report is what the bridge INTENDS plus the heartbeat, which a caller
    // can check for itself.
    let body = format!(
        "{{\"status\":\"{}\",\"intends_to_publish\":{},\"wedged\":{},\
          \"loop_age_ms\":{},\"loop_age_allowed_ms\":{},\
          \"version\":\"{}\",\"contract\":{}}}",
        phase.lifecycle.slug(),
        !phase.lifecycle.is_silent_on_purpose(),
        wedged,
        age,
        allowed,
        // Compile-time, so it describes the BINARY and not the tag it wears —
        // the two can drift, which is why the publish workflow guards them.
        env!("CARGO_PKG_VERSION"),
        crate::adapters::sparkplug_publisher::CONTRACT_VERSION,
    );

    // AR12, and the whole point of the status code.
    //
    // Unhealthy ONLY for a wedged poll loop. A deliberate silence answers 200
    // because Epic 7 restarts on this, and looping a fresh deployment would
    // destroy the screen needed to configure it. An honest STALE answers 200 too:
    // the loop is running, the cloud is not answering, and restarting the
    // container fixes nothing.
    //
    // Returned unconditionally 200 until a review on 2026-08-04 — so the
    // healthcheck degraded to "the process accepts TCP", which HEALTHCHECK gives
    // for free, and the restart AR12 exists to trigger could never fire.
    let code = if wedged {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    (code, [("content-type", "application/json")], body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LastLoopTick;
    use crate::app::supervisor::ConfigHandle;
    use crate::core::clock::Clock;

    /// A publishing phase built from the three things `/healthz` reads, wrapped
    /// in the detached control surface that carries them.
    fn publishing(
        heartbeat: LastLoopTick,
        clock: std::sync::Arc<dyn Clock + Send + Sync>,
        config: ConfigHandle,
    ) -> Phase {
        Phase::running(Control::detached(config, heartbeat, clock))
    }

    /// Wrap a phase in the state a handler sees. The state directory and the
    /// readiness nudge are irrelevant to `/healthz`, which is what these tests
    /// exercise.
    fn ui(phase: Phase) -> Arc<UiState> {
        Arc::new(UiState::new(
            phase.into_handle(),
            std::path::PathBuf::from("/nonexistent"),
            Arc::new(tokio::sync::Notify::new()),
        ))
    }

    /// A published bridge with a 30 s period — the allowance AR12 computes from.
    fn running_config() -> crate::app::supervisor::BridgeConfig {
        crate::app::supervisor::BridgeConfig {
            api_base: "https://api.smart-me.com".to_string(),
            credentials: smart_me_client::Credentials::Basic {
                user: "u".to_string(),
                password: "p".to_string(),
            },
            http_timeout: std::time::Duration::from_secs(10),
            meters: Vec::new(),
            group_id: "G".to_string(),
            node_id: "N".to_string(),
            broker_host: "b".to_string(),
            broker_port: 1883,
            bd_seq_path: std::path::PathBuf::from("/data/bdseq.toml"),
            poll: crate::app::PollConfig {
                interval: std::time::Duration::from_secs(30),
                fetch_timeout: std::time::Duration::from_secs(10),
            },
            policy: crate::core::state_machine::Policy { max_age_ms: 90_000 },
            log_dir: None,
            log_keep: None,
            ui_port: None,
        }
    }

    /// AC2 — and it now asks PRODUCTION where it bound.
    ///
    /// **The first version of this test asserted nothing.** It created its own
    /// listener on its own literal `0.0.0.0` and checked that *that* was not
    /// loopback — which is a tautology (`local_addr()` of `0.0.0.0` is
    /// unspecified, never loopback) about a socket `serve` never touched. Making
    /// `serve` bind `127.0.0.1` left it green; found by review 2026-08-04.
    ///
    /// `bind` was split out of `serve` for this: the test can now ask the code
    /// under review what address it chose.
    #[tokio::test]
    async fn the_bind_address_production_chooses_is_not_loopback() {
        let listener = bind(0).await.expect("the UI binds");
        let bound = listener.local_addr().expect("addr");
        assert!(
            bound.ip().is_unspecified(),
            "the UI must bind 0.0.0.0 INSIDE the container: it publishes no host \
             port, so loopback protects nothing and makes it unreachable from \
             Traefik, which is the only thing meant to reach it. It bound {bound}"
        );
    }

    /// AR12 — the half that was missing entirely until a review found it.
    ///
    /// `/healthz` returned 200 unconditionally, so the Docker healthcheck Epic 7
    /// will wire to it degraded to "the process accepts TCP" and the restart
    /// AR12 exists to trigger could never fire.
    #[test]
    fn a_wedged_poll_loop_is_unhealthy_and_a_slow_one_is_not() {
        use crate::core::clock::{FakeClock, MonotonicMs};
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let clock = Arc::new(FakeClock::new(UtcMillis(1_784_984_793_000)));
        let heartbeat = LastLoopTick::new();
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let phase = publishing(heartbeat.clone(), clock.clone(), config);

        // The loop ticks now, with a 30 s period, so 90 s is the allowance.
        heartbeat.touch(clock.monotonic());

        clock.advance_ms(60_000);
        let (age, allowed) = phase.loop_age().expect("a running bridge has an age");
        assert_eq!((age, allowed), (60_000, 90_000));
        assert!(
            age <= allowed,
            "two missed periods is a slow cloud — an HONEST STALE, which AR12 \
             says must never trigger a restart"
        );

        clock.advance_ms(30_001);
        let (age, _) = phase.loop_age().expect("age");
        assert!(
            age > allowed,
            "past three periods the loop is not looping, and that IS the case \
             the healthcheck exists to restart"
        );
        let _: MonotonicMs = clock.monotonic();
    }

    /// **The handler itself**, because the tests above exercise `loop_age` and a
    /// mutation that returned `StatusCode::OK` unconditionally left every one of
    /// them green — which is exactly how the unconditional 200 shipped in the
    /// first place.
    #[tokio::test]
    async fn the_status_code_follows_the_wedge_and_nothing_else() {
        use crate::core::clock::FakeClock;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let clock = Arc::new(FakeClock::new(UtcMillis(1_784_984_793_000)));
        let heartbeat = LastLoopTick::new();
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let state = ui(publishing(
            heartbeat.clone(),
            clock.clone(),
            Arc::clone(&config),
        ));
        heartbeat.touch(clock.monotonic());

        // Healthy: one period late.
        clock.advance_ms(30_000);
        let response = healthz(State(Arc::clone(&state))).await.into_response();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "one late tick is a slow cloud, and restarting the container fixes \
             nothing about a slow cloud"
        );

        // Wedged: past three periods.
        clock.advance_ms(61_000);
        let response = healthz(State(Arc::clone(&state))).await.into_response();
        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "AR12: a poll loop that has stopped looping is the ONE case the \
             healthcheck exists to restart, and it returned 200 for it until a \
             review on 2026-08-04"
        );

        // And a deliberate silence is still 200 — the case that would otherwise
        // loop a fresh deployment and destroy the screen needed to configure it.
        let silent = ui(Phase::silent(Lifecycle::Unconfigured));
        assert_eq!(
            healthz(State(silent)).await.into_response().status(),
            StatusCode::OK
        );
    }

    /// A bridge with no poll loop has no age, and that must not read as wedged —
    /// it is the state a fresh deployment sits in, and restarting it would
    /// destroy the screen needed to leave it.
    #[test]
    fn a_deliberately_silent_bridge_has_no_loop_age() {
        assert!(Phase::silent(Lifecycle::Unconfigured).loop_age().is_none());
        assert!(Phase::silent(Lifecycle::Unconfirmed).loop_age().is_none());
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
