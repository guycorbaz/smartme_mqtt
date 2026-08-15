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
use axum::routing::{get, post};

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
    /// The configuration on disk cannot be turned into settings.
    ///
    /// **A startup state since [ADR 0026]**, and that is the point of it. It used
    /// to be reachable only from a later turn — an operator in the browser whose
    /// hand-edit invalidated the file underneath them — because a configuration
    /// present and invalid *at startup* exited the process (Story 6.1 AC1) and
    /// served no screen at all. The commonest way to reach it is now the first
    /// thing that happens on a deployment whose state directory nobody `chown`ed.
    ///
    /// Two distinct repairs land here, and the faults say which: a file that was
    /// read and rejected is fixed in the form; one that could not be read at all
    /// is fixed on the host, because nothing here can write there either.
    ///
    /// [ADR 0026]: ../../../docs/adr/0026-a-configuration-it-cannot-use-stops-the-bridge-publishing-not-serving.md
    Misconfigured,
}

impl Lifecycle {
    /// Whether the bridge is deliberately silent.
    ///
    /// **Not the same as unhealthy** — see [`healthz`].
    pub const fn is_silent_on_purpose(self) -> bool {
        matches!(
            self,
            Lifecycle::Unconfigured | Lifecycle::Unconfirmed | Lifecycle::Misconfigured
        )
    }

    const fn headline(self) -> &'static str {
        match self {
            Lifecycle::Unconfigured => "Not configured yet",
            Lifecycle::Unconfirmed => "Waiting for you to confirm the meter mapping",
            Lifecycle::Running => "Running",
            Lifecycle::Misconfigured => "The saved configuration is not usable",
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
            // NOT "connected and publishing".
            //
            // It said exactly that until 2026-08-05 — a compile-time constant
            // describing which branch of `main.rs` ran, presented to an operator
            // as an observation. That is word for word the claim removed from
            // `/healthz` on 2026-08-04 (`publishing` → `intends_to_publish`), and
            // it survived on the surface a human actually reads, in its stronger
            // form: *connected* AND *publishing*. `Phase::starting()` then made it
            // reachable before any socket had been opened at all.
            //
            // Broker connectivity is still not plumbed to the UI ([#53]). Until
            // it is, what this page can honestly say is what the bridge is
            // trying to do.
            Lifecycle::Running => {
                "The bridge is configured and confirmed, so it is polling the meters \
                 and publishing what it reads. Whether the broker is actually \
                 reachable is not reported here yet — the log says so."
            }
            // NOT "still running on the configuration it started with".
            //
            // It said that until 2026-08-06, and the control flow forbade it: this
            // state was reachable only from `Unconfigured` and `Unconfirmed`,
            // where nothing had ever been published, because the publishing arm
            // never returns to the top of the loop. Since [ADR 0026] it is also
            // the state a bridge STARTS in when `/data` cannot be read — the very
            // first thing an operator sees after a forgotten `chown`, described to
            // them as a bridge that is running.
            //
            // What is true in every way this state is reached: nothing is being
            // published, and the screen that says why is one link away.
            //
            // NOT "the faults below". They are not below — they are on
            // `/config`, and this page has never rendered one.
            //
            // Met on the panoramix deployment, 2026-08-09, by the first operator
            // to reach this state in the field: the page says "correct them and
            // save" while showing nothing to correct and offering no way to get
            // anywhere. ADR 0026 keeps the process alive for exactly one reason
            // — *killing it would destroy the screen that is the repair tool* —
            // and the entry page did not lead to that screen.
            //
            // Fifth instance of one shape, after `/healthz`'s `publishing`, the
            // `Running` detail below, the failed-source caveat and the panic
            // guard: a surface asserting something the code does not do.
            Lifecycle::Misconfigured => {
                "The configuration on disk cannot be used, so nothing is published — \
                 no connection, no birth. Open the configuration screen: it lists \
                 every fault beside the setting it belongs to. Some of them are \
                 repaired there; one — a state directory this process cannot write \
                 — has to be fixed on the host, and says so."
            }
        }
    }

    /// Where the operator goes next, as a link they can click.
    ///
    /// # This page had no links at all until 2026-08-09
    ///
    /// Every silent phase tells the operator to do something — configure the
    /// bridge, confirm the mapping, correct a fault — and none of them said
    /// where. The entry page was a dead end in exactly the three states whose
    /// whole purpose is to be left.
    ///
    /// It was met in the field rather than found by reading: the panoramix
    /// deployment came up `Misconfigured` (no meters yet, deliberately) and
    /// served *"correct them and save"* with nothing to correct and nowhere to
    /// go. [ADR 0026] keeps this process alive on a configuration it has refused
    /// for one stated reason — *killing it would destroy the screen that is the
    /// repair tool* — which this page then did not link to.
    ///
    /// `Running` gets one too, deliberately: a bridge that is publishing is
    /// exactly when an operator wants to add a meter, and making them guess the
    /// path is the same defect wearing a friendlier face.
    ///
    /// [ADR 0026]: ../../../docs/adr/0026-a-configuration-it-cannot-use-stops-the-bridge-publishing-not-serving.md
    const fn next_step(self) -> &'static str {
        match self {
            Lifecycle::Unconfigured => {
                "<p><a href=\"/config\"><strong>Configure this bridge</strong></a></p>"
            }
            Lifecycle::Unconfirmed => {
                "<p><a href=\"/confirm\"><strong>Review and confirm the meter \
                 mapping</strong></a> — nothing is published until you do.</p>\
                 <p><a href=\"/config\">Change the configuration</a></p>"
            }
            Lifecycle::Misconfigured => {
                "<p><a href=\"/config\"><strong>Open the configuration screen</strong></a> \
                 — every fault is listed there, beside the setting it belongs to.</p>"
            }
            Lifecycle::Running => "<p><a href=\"/config\">Change the configuration</a></p>",
        }
    }

    /// The string `/healthz` reports. Stable, lowercase, machine-facing.
    const fn slug(self) -> &'static str {
        match self {
            Lifecycle::Unconfigured => "unconfigured",
            Lifecycle::Unconfirmed => "unconfirmed",
            Lifecycle::Running => "running",
            Lifecycle::Misconfigured => "misconfigured",
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

    /// The fleet as it stood at ONE instant, or `None` in every silent phase
    /// because there is no poll loop to have an opinion (Story 3.3, AR6).
    ///
    /// **Taken once per request and passed down**, which is the point of the
    /// change: `failed_sources` and `loop_age` used to reach for the shared state
    /// separately, so a page could name a failed meter from one instant beside an
    /// age from another. Nothing was known to be wrong on the rendered page —
    /// neither figure was compared with the other — and the day one is, the
    /// mismatch would arrive silently.
    fn fleet(&self) -> Option<crate::app::poll_publish::FleetState> {
        Some(self.running.as_ref()?.heartbeats().snapshot())
    }

    /// The meters whose source has failed fatally, if any (Story 3.2 AC5).
    ///
    /// Empty in every silent phase, because there is no poll loop to have an
    /// opinion — and empty is then the truth rather than a default.
    fn failed_sources(fleet: Option<&crate::app::poll_publish::FleetState>) -> Vec<String> {
        match fleet {
            Some(fleet) => fleet.failed().into_iter().map(|m| m.to_string()).collect(),
            None => Vec::new(),
        }
    }

    /// How long since the poll loop last started an iteration, and how long it is
    /// allowed to be — `None` when there is no loop.
    ///
    /// The allowance comes from the cadence the loop RECORDED, not from the
    /// period the configuration currently asks for — see [`LastLoopTick::touch`]
    /// for the false 503 that produced.
    ///
    /// **The worst meter's, not the fleet's average and not the first one's**
    /// (Story 3.1). Each meter paces itself, so each carries its own allowance;
    /// the pair returned is the one most over it, and `None` only when no meter
    /// has ticked at all.
    ///
    /// A meter that has never ticked while its siblings have is skipped rather
    /// than counted as infinitely late: during startup that is every meter for a
    /// moment, and reporting a wedge there would restart a container that is
    /// merely young. It also means a task that dies before its first tick is
    /// invisible here — true before this change as well, and owed a guard of its
    /// own rather than a silent reinterpretation of this one.
    ///
    /// **This block documented `failed_sources` from 2026-08-07 to 2026-08-08.**
    /// `590c78d` inserted that function between this comment and the item it
    /// describes, so sixteen lines about per-meter allowances were attached to a
    /// function that has no opinion on them, and `loop_age` had none at all. A
    /// `///` block belongs to whatever follows it, which makes inserting an item
    /// above a comment a silent way to make documentation wrong rather than
    /// merely absent.
    fn loop_age(&self, fleet: Option<&crate::app::poll_publish::FleetState>) -> Option<(i64, i64)> {
        let control = self.running.as_ref()?;
        let now = control.clock().monotonic().0;
        fleet?
            .meters
            .iter()
            .filter_map(|meter| {
                let last = meter.last_tick?;
                let allowed = meter
                    .period_ms
                    .saturating_mul(i64::from(WEDGED_AFTER_PERIODS));
                Some((now - last.0, allowed))
            })
            // Most over its OWN allowance. Comparing raw ages would let a meter
            // polled every 300 s out-shout one polled every 5 s that is genuinely
            // wedged, which is the whole reason the allowance is per-meter.
            .max_by_key(|(age, allowed)| age.saturating_sub(*allowed))
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
    let routes = Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route(
            "/config",
            get(screens::config_form).post(screens::save_config),
        )
        // Discovery is its OWN route and its own submission (story 3.4): the
        // main form reaches it through `formaction`, so the operator's unsaved
        // edits ride along and nothing is saved by asking the account.
        .route("/config/discover", post(screens::discover))
        // Confirming is its OWN route and its OWN submission (AC3). A
        // confirmation folded into the save would be a click the operator makes
        // for a different reason, which is how a guard becomes a formality.
        .route(
            "/confirm",
            get(screens::confirm_form).post(screens::confirm_mapping),
        );

    // The probe, and it is NOT in the image.
    //
    // Story 6.1's AC5 has two halves and only the bind half could be asserted:
    // proving the panic half needs a route that panics, and shipping one was
    // rightly refused ([#51]). A Cargo feature that nothing enables by default
    // is the way out — `docker-publish.yml` builds without it, so the binary an
    // operator runs has no such route, while the test below exercises the REAL
    // router, the real middleware and the real `serve`. What is test-only is the
    // thing that panics; everything the assertion is about is production.
    #[cfg(feature = "panic-probe")]
    let routes = routes.route("/debug/panic", get(panic_probe));

    routes
        // AFTER every route, including the probe: `layer` wraps what is already
        // there, so a layer added first would not cover a route added second.
        .layer(axum::middleware::from_fn(catch_panic))
        .with_state(Arc::new(state))
}

/// A panicking handler must cost the page and nothing else (Story 6.1 AC5).
///
/// # Why this exists when a panic was already survivable
///
/// It was, and only in the weakest sense: `axum` serves each connection in its
/// own task, so a panic killed that connection and left the process alone. What
/// the operator got was a browser reporting a reset connection and **not one
/// line anywhere** — the panic hook writes to stderr, not through `tracing`, so
/// it missed the log file entirely and AC5's second clause ("traced, loudly, at
/// a level the default filter shows") was simply false for this half.
///
/// The unwind is caught around each `poll`, not around the whole future, because
/// a handler that panics after its first `await` panics inside a later poll —
/// catching only the first would cover the cheapest case and none of the real
/// ones.
async fn catch_panic(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use std::future::Future as _;
    use std::task::Poll;

    // Captured before the request is consumed, so the trace can name the page.
    let path = request.uri().path().to_owned();
    let mut handler = Box::pin(next.run(request));

    let outcome = std::future::poll_fn(|cx| {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler.as_mut().poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(response)) => Poll::Ready(Ok(response)),
            Err(panic) => Poll::Ready(Err(panic)),
        }
    })
    .await;

    match outcome {
        Ok(response) => response,
        Err(panic) => {
            let why = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic carrying no message".to_string());
            // WHAT THIS SAYS IS WHAT THIS LAYER CAN KNOW, and no more.
            //
            // It said "the bridge keeps polling and publishing" until 2026-08-09,
            // in the log line AND on the page. This middleware sees a request and
            // a panic; it has no view of the phase at all, and three of the four
            // phases publish nothing and never have. A panic in `/config` on a
            // bridge that has never been configured answered an operator that it
            // was publishing.
            //
            // Fourth instance of one shape: `/healthz`'s `publishing` became
            // `intends_to_publish` (2026-08-04), `Lifecycle::Running`'s detail
            // stopped claiming "connected and publishing" (2026-08-05), the
            // failed-source caveat stopped promising a republish that never
            // happened (2026-08-09) — and the guard written to make Story 6.1 AC5
            // honest introduced the same claim again.
            //
            // What IS true in every phase: a panicking page does not touch the
            // poll tasks or the driver, because nothing here can. That is a
            // statement about what was NOT harmed, which this layer can make.
            tracing::error!(
                path = %path,
                panic = %why,
                "a web UI handler PANICKED. This page is what was lost; the panic \
                 did not reach the poll tasks or the mqtt driver, which are \
                 unaffected by anything the web surface does"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(
                    "<!doctype html><meta charset=utf-8><title>smartme_mqtt</title>\
                     <h1>This page failed</h1><p>Only this page failed. Whatever the \
                     bridge was doing with the meters, it is still doing — the web \
                     surface cannot disturb it. The log carries what went wrong.</p>",
                ),
            )
                .into_response()
        }
    }
}

/// The panic the test needs, compiled only when `panic-probe` is on.
///
/// Never present in a released image: `docker-publish.yml` and every default
/// build leave the feature off.
#[cfg(feature = "panic-probe")]
async fn panic_probe() {
    panic!("the panic probe was called deliberately");
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
    let phase = state.phase();
    let lifecycle = phase.lifecycle;
    // THE CLAIM IS QUALIFIED, or it is not made (Story 3.2 AC5, ADR 0027 §1).
    //
    // `Lifecycle::Running`'s detail says the bridge "is polling the meters and
    // publishing what it reads". That is a claim about the SOURCE, and until now
    // the page had no way to see one: a rejected credential put every meter into
    // an absorbing `Failed`, nothing reached the wire, and this page went on
    // saying it was publishing. Naming the meters is the whole point — "something
    // is wrong" sends an operator to the logs, a meter's name sends them to the
    // meter.
    let failed = Phase::failed_sources(phase.fleet().as_ref());
    // WHAT THIS SAYS HAS TO BE TRUE IN EVERY CASE THAT REACHES IT, and it was
    // not until 2026-08-09. Two defects, both found by reading the code the
    // sentence describes rather than the sentence:
    //
    //  - it named ONE cause — "the smart-me cloud refused or could not answer".
    //    A meter whose reported serial is not its declared one reaches `Failed`
    //    with the cloud having answered perfectly, so the page would have sent an
    //    operator to look at their credentials for a typo in their own form.
    //  - it promised that "the last known values are still published, marked
    //    not-good". They are not. `Failed` publishes `Quality::Bad`, and
    //    `metrics_for` deliberately publishes NULL for `Bad` — "this number is
    //    not a reading", so no number goes out at all. A meter that never
    //    answered publishes nothing whatever, having no last value to carry.
    //    The conclusion (nothing shows as current) was true; the reason given
    //    for it was false, and an operator would have gone looking in their
    //    historian for a value that is not there.
    //
    // So: no cause is asserted, the two an operator can actually cause are
    // offered, and the log is named for the rest.
    let caveat = if failed.is_empty() {
        String::new()
    } else {
        let (subject, pronoun) = if failed.len() == 1 {
            ("One meter is", "it")
        } else {
            ("Meters are", "them")
        };
        format!(
            "<p><strong>{subject} not being read: {}.</strong> The bridge cannot get a \
             reading it can vouch for — usually a refused smart-me credential, or a \
             device that is not the one the configuration declares for that meter. \
             <strong>No value is published for {pronoun}</strong>, so nothing downstream \
             shows one as current. This is a fault a restart is needed to clear, and the \
             log names the reason.</p>",
            screens::escape(&failed.join(", ")),
        )
    };
    // AND A METER THAT IS BEING READ CAN STILL BE PUBLISHING SOMETHING THE HOST
    // MUST NOT TRUST (Story 2.3 AC6, [#62]).
    //
    // **Added 2026-08-11 by this story's own review, which found AC6 implemented
    // on `/healthz` only.** The criterion names three surfaces; one had it. This
    // is the surface a human opens, and it is the one that spent the ten hours of
    // 2026-08-10 saying the bridge "is polling the meters and publishing what it
    // reads" about a meter frozen since 09:34.
    //
    // Distinct from `failed` above, and the wording keeps them apart: that block
    // is about meters producing NOTHING, this one about meters producing
    // something marked. A degraded meter needs no restart and the log is not
    // where its reason lives — the cause is on the wire, and it is here.
    let degraded: Vec<String> = phase
        .fleet()
        .as_ref()
        .map(|fleet| {
            fleet
                .degraded()
                .into_iter()
                .map(|(meter, verdict)| {
                    format!(
                        "{} ({})",
                        meter,
                        verdict.cause().map_or("no cause recorded", |c| c.as_str())
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let degraded_caveat = if degraded.is_empty() {
        String::new()
    } else {
        let subject = if degraded.len() == 1 {
            "One meter is"
        } else {
            "Meters are"
        };
        format!(
            "<p><strong>{subject} being read, but what is published must not be \
             trusted: {}.</strong> The bridge is polling normally and every reading \
             reaches the host — carrying a quality that says it is not good, and the \
             reason beside it. Nothing here is cleared by a restart: the named cause \
             is what to act on.</p>",
            screens::escape(&degraded.join(", ")),
        )
    };
    Html(format!(
        "<!doctype html><meta charset=utf-8>\
         <title>smartme_mqtt</title>\
         <h1>smartme_mqtt</h1>\
         <p><strong>{}</strong></p>\
         <p>{}</p>\
         {}\
         {}\
         {}\
         <hr><p>version {} · contract {}</p>",
        lifecycle.headline(),
        lifecycle.detail(),
        caveat,
        degraded_caveat,
        lifecycle.next_step(),
        env!("CARGO_PKG_VERSION"),
        crate::adapters::sparkplug_publisher::CONTRACT_VERSION,
    ))
}

/// One JSON string literal, quotes included, escaped per RFC 8259.
///
/// **Added 2026-08-11 by the review of story 2.3.** The two lists in [`healthz`]
/// escaped `\` and `"` and passed U+0000–U+001F through raw. A meter id
/// containing a newline or a tab — `config.rs` applies no charset rule, and TOML
/// basic strings accept `\n` — put a literal control byte inside a JSON string,
/// which RFC 8259 §7 forbids. A strict parser rejects the whole document.
///
/// The consumer is Epic 7's healthcheck: the field added to make a fault visible
/// would be what makes the body undecodable, and the endpoint an operator
/// consults during exactly the incident this story exists to surface would go
/// dark. The pre-existing `failed_sources` had the same hole; it is fixed here
/// too rather than left as the older half of a matched pair.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),

            // The three RFC 8259 gives a short form, which keeps a meter id
            // legible in a body an operator may well be reading by eye.
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Every other control character, by code point. `\u007F` is legal
            // unescaped and is left alone.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
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
    // ONE reading, used for both the verdict and the body.
    //
    // `loop_age()` was called twice — once to decide the status code and once to
    // print the numbers — each re-reading the clock and the atomic. A body
    // reporting `age < allowed` beside a 503 was reachable at the boundary, on
    // the one endpoint whose whole job is that the number and the verdict agree.
    // ONE SNAPSHOT, used for the age AND for the failed list (Story 3.3, AR6).
    //
    // The same argument as the one below, one level up: these two figures were
    // read from the shared state separately, so a body could carry an age from
    // one instant beside a fault list from another.
    let fleet = phase.fleet();
    let reading = phase.loop_age(fleet.as_ref());
    // No loop, or a loop that has not ticked once yet. Neither is a wedge: the
    // silent states have no loop by design, and a bridge that has not completed
    // its first iteration is starting, not stuck.
    let wedged = matches!(reading, Some((age, allowed)) if age > allowed);
    let (age, allowed) = reading
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
    // A FAULT IS NOT A DELIBERATE SILENCE, and the body has to tell them apart
    // (Story 3.2 AC5, ADR 0027 §2). The status code stays 200: Epic 7 wires this
    // to a container restart, and a restart provably cannot clear a rejected
    // credential — it would loop, destroying the screen that names the fault.
    let failed = Phase::failed_sources(fleet.as_ref());
    let degraded: Vec<(crate::domain::MeterId, crate::core::oracle::Verdict)> = fleet
        .as_ref()
        .map(|f| {
            f.degraded()
                .into_iter()
                .map(|(m, v)| (m.clone(), v))
                .collect()
        })
        .unwrap_or_default();
    let failed_json = format!(
        "[{}]",
        failed
            .iter()
            .map(|m| json_string(m))
            .collect::<Vec<_>>()
            .join(",")
    );
    // AND A DEGRADED METER IS NEITHER (Story 2.3 AC6, [#62]). A meter whose
    // composed verdict is not `Good` is still polling and still publishing — it
    // is not `failed` and the loop is not `wedged` — but what it puts on the wire
    // must not be trusted. Until this field, no operator surface said so: on
    // 2026-08-10 a meter froze for ten hours, was published `Bad_Stale`
    // throughout, and this endpoint reported the fleet healthy the whole time.
    //
    // The status code stays 200 for the same reason `failed_sources` does not
    // move it: Epic 7 wires this to a container restart, and a restart cannot
    // clear a backwards counter — it would loop, destroying the surface that
    // names the fault (ADR 0027 §2).
    let degraded_json = format!(
        "[{}]",
        degraded
            .iter()
            .map(|(meter, verdict)| format!(
                "{{\"meter\":{},\"quality\":{},\"cause\":{}}}",
                json_string(&meter.to_string()),
                json_string(match verdict.quality() {
                    crate::domain::Quality::Good => "good",
                    crate::domain::Quality::Stale => "stale",
                    crate::domain::Quality::Bad => "bad",
                }),
                // `null`, not `""`. A non-good verdict without a cause is
                // unreachable today — every constructor takes one — but encoding
                // "unknown" and "empty" identically is how a field stops being
                // able to say it does not know.
                verdict
                    .cause()
                    .map_or_else(|| "null".to_string(), |c| json_string(c.as_str())),
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    let body = format!(
        "{{\"status\":\"{}\",\"intends_to_publish\":{},\"wedged\":{},\
          \"failed_sources\":{},\"degraded_meters\":{},\
          \"loop_age_ms\":{},\"loop_age_allowed_ms\":{},\
          \"version\":\"{}\",\"contract\":{}}}",
        phase.lifecycle.slug(),
        !phase.lifecycle.is_silent_on_purpose(),
        wedged,
        failed_json,
        degraded_json,
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

/// The rendered bytes of a response.
///
/// **Not `format!("{response:?}")`.** `http::Response`'s `Debug` prints the
/// status, version, headers and `body: Body(UnsyncBoxBody)` — never the content.
/// A test written that way on 2026-08-05 asserted that a refusal page contained
/// no `<script>` and could not fail for any mutation of the function that renders
/// it; it shipped inside the commit whose subject was *"the checks that could not
/// see what they searched for"*.
///
/// **It lives here, beside the module rather than inside `mod tests`, since
/// 2026-08-09.** When this helper was built, the test its own documentation named
/// was in `ui::origin::tests`, which could not reach a private item of a sibling's
/// test module — so the test that guards **the only unescaped sink on the whole
/// web surface** kept the shape this exists to replace, and was measured still
/// green with `escape()` deleted outright. A helper a caller cannot reach is a
/// correction that does not travel.
#[cfg(test)]
pub(super) async fn rendered_body(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body reads");
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::rendered_body as body;
    use super::*;
    use crate::app::MeterPulse;
    use crate::app::poll_publish::Heartbeats;
    use crate::app::supervisor::ConfigHandle;
    use crate::core::clock::Clock;

    /// A publishing phase built from the three things `/healthz` reads, wrapped
    /// in the detached control surface that carries them.
    ///
    /// Takes the whole [`Heartbeats`] rather than one tick, because since Story
    /// 3.1 the wedge verdict is the WORST meter's and a helper that could only
    /// build a fleet of one would make that untestable.
    fn publishing(
        heartbeats: Heartbeats,
        clock: std::sync::Arc<dyn Clock + Send + Sync>,
        config: ConfigHandle,
    ) -> Phase {
        Phase::running(Control::detached(config, heartbeats, clock))
    }

    /// A one-meter fleet, which is what most of these tests want.
    fn one_meter() -> (Heartbeats, MeterPulse) {
        let beats = Heartbeats::for_meters([crate::domain::MeterId::new("meter-a")]);
        let tick = beats
            .of(&crate::domain::MeterId::new("meter-a"))
            .expect("just created");
        (beats, tick)
    }

    /// Wrap a phase in the state a handler sees. The state directory and the
    /// readiness nudge are irrelevant to `/healthz`, which is what these tests
    /// exercise.
    /// **Every phase that asks the operator to act must say where to go.**
    ///
    /// Met in the field rather than found by reading. The panoramix deployment
    /// came up `Misconfigured` on 2026-08-09 — deliberately, with no meters
    /// configured yet — and served *"The faults below say what is wrong with it;
    /// correct them and save"* over a page that rendered no fault and contained
    /// **no link of any kind**. [ADR 0026] keeps this process alive on a
    /// configuration it has refused for one stated reason, *killing it would
    /// destroy the screen that is the repair tool*, and the entry page did not
    /// lead there.
    ///
    /// Nothing caught it because nothing asserted anything about this page's
    /// content at all: 182 unit tests stayed green across the repair.
    ///
    /// FALSIFIED 2026-08-09 by returning `""` from `Lifecycle::next_step` — the
    /// state of the code the deployment met. Copied from the run:
    ///
    /// ```text
    /// ---- ui::tests::every_phase_offers_a_way_out stdout ----
    /// Misconfigured tells the operator to act and offers no way to: <!doctype html>…
    /// <p><strong>The saved configuration is not usable</strong></p>…<hr><p>version …
    ///
    /// test result: FAILED. 1 failed
    /// ```
    ///
    /// The `Misconfigured` arm additionally asserts the old sentence is gone: a
    /// link would satisfy the first assertion while the prose still promised a
    /// list that is on another page.
    ///
    /// [ADR 0026]: ../../../docs/adr/0026-a-configuration-it-cannot-use-stops-the-bridge-publishing-not-serving.md
    #[tokio::test]
    async fn every_phase_offers_a_way_out() {
        for lifecycle in [
            Lifecycle::Unconfigured,
            Lifecycle::Unconfirmed,
            Lifecycle::Misconfigured,
            Lifecycle::Running,
        ] {
            let state = ui(Phase::silent(lifecycle));
            let page = body(index(State(state)).await.into_response()).await;
            assert!(
                page.contains("href=\"/config\""),
                "{lifecycle:?} tells the operator to act and offers no way to: {page}"
            );
        }

        // The one that has to reach a DIFFERENT screen: confirming is its own
        // submission, deliberately separate from saving (Story 6.2 AC3).
        let page = body(
            index(State(ui(Phase::silent(Lifecycle::Unconfirmed))))
                .await
                .into_response(),
        )
        .await;
        assert!(
            page.contains("href=\"/confirm\""),
            "the phase whose whole exit is one click must link to it: {page}"
        );

        // And the sentence that sent an operator looking below for nothing.
        let page = body(
            index(State(ui(Phase::silent(Lifecycle::Misconfigured))))
                .await
                .into_response(),
        )
        .await;
        assert!(
            !page.contains("faults below"),
            "the faults are on /config; promising them here sends the reader \
             scrolling past a horizontal rule: {page}"
        );
    }

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
            policy: crate::core::state_machine::Policy::DEFAULT,
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
        let (beats, heartbeat) = one_meter();
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let phase = publishing(beats.clone(), clock.clone(), config);

        // The loop ticks now, with a 30 s period, so 90 s is the allowance.
        heartbeat.touch(clock.monotonic(), 30_000);

        clock.advance_ms(60_000);
        let (age, allowed) = phase
            .loop_age(phase.fleet().as_ref())
            .expect("a running bridge has an age");
        assert_eq!((age, allowed), (60_000, 90_000));
        assert!(
            age <= allowed,
            "two missed periods is a slow cloud — an HONEST STALE, which AR12 \
             says must never trigger a restart"
        );

        clock.advance_ms(30_001);
        let (age, _) = phase.loop_age(phase.fleet().as_ref()).expect("age");
        assert!(
            age > allowed,
            "past three periods the loop is not looping, and that IS the case \
             the healthcheck exists to restart"
        );
        let _: MonotonicMs = clock.monotonic();
    }

    /// A meter id carrying a control character does not make `/healthz` emit
    /// invalid JSON (review of story 2.3, 2026-08-11).
    ///
    /// `config.rs` applies no charset rule to a meter id and TOML basic strings
    /// accept `\n`, so an operator can configure `appart\nest`. The previous
    /// escaping handled `\` and `"` and passed U+0000–U+001F through raw, which
    /// RFC 8259 §7 forbids inside a string: a strict parser rejects the whole
    /// body. The consumer is Epic 7's healthcheck, and the field added to make a
    /// fault visible would be the thing that made the body undecodable — during
    /// exactly the incident it exists to surface.
    ///
    /// FALSIFIED 2026-08-11 by restoring the old escaping
    /// (`.replace('\\', …).replace('"', …)` only): the parse assertion goes red
    /// with a literal newline inside the string.
    #[tokio::test]
    async fn a_control_character_in_a_meter_id_cannot_break_the_health_body() {
        use crate::core::clock::FakeClock;
        use crate::core::oracle::{Cause, Verdict};
        use crate::core::state_machine::State as OracleState;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let awkward = crate::domain::MeterId::new("appart\nest\t\"quoted\"\\slash");
        let clock = Arc::new(FakeClock::new(UtcMillis(1_784_984_793_000)));
        let beats = Heartbeats::for_meters([awkward.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let state = ui(publishing(
            beats.clone(),
            clock.clone(),
            Arc::clone(&config),
        ));

        beats.record(
            &awkward,
            OracleState::Fresh,
            Verdict::bad(Cause::CounterWentBackwards),
        );
        let health = body(healthz(State(Arc::clone(&state))).await.into_response()).await;

        // The body must PARSE. Nothing else is asserted about the id's rendering:
        // what matters is that a diagnostic surface survives its own inputs.
        let parsed = serde_json::from_str::<serde_json::Value>(&health);
        assert!(
            parsed.is_ok(),
            "/healthz must emit valid JSON whatever a meter is called — an \
             operator's healthcheck cannot parse its way past a raw control \
             character: {:?}\n{health}",
            parsed.err()
        );
    }

    /// **Story 2.3 AC6** — a meter publishing a non-good verdict is reported as
    /// such, and it is neither `failed` nor `wedged`.
    ///
    /// This is [#62] closed on the surface where it was invisible. On 2026-08-10
    /// `appart-est` froze at 09:34:50, was published `Bad_Stale` for ten hours,
    /// and `/healthz` reported the fleet healthy throughout: the poll loop was
    /// ticking (so not `wedged`), the source had not refused us (so not
    /// `failed`), and no field existed for *"publishing, but not to be trusted"*.
    /// The bridge's own screens were the last place the fault could be seen.
    ///
    /// **The healthy case is asserted first**, because every assertion below
    /// would also hold for an endpoint that reported a degraded meter
    /// unconditionally — the shape that made three of this file's earlier
    /// assertions hollow.
    ///
    /// FALSIFIED 2026-08-11 by making `FleetState::degraded` return `Vec::new()`
    /// unconditionally — the state the code was in before this story: the
    /// `degraded_meters` assertion goes red while `failed_sources` and `wedged`
    /// stay exactly as they are, which is the whole point of the finding.
    #[tokio::test]
    async fn a_degraded_meter_is_named_in_healthz_and_is_neither_failed_nor_wedged() {
        use crate::core::clock::FakeClock;
        use crate::core::oracle::{Cause, Verdict};
        use crate::core::state_machine::State as OracleState;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let clock = Arc::new(FakeClock::new(UtcMillis(1_784_984_793_000)));
        let beats = Heartbeats::for_meters([crate::domain::MeterId::new("appart-est")]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let state = ui(publishing(
            beats.clone(),
            clock.clone(),
            Arc::clone(&config),
        ));

        // HEALTHY FIRST, or the assertions below prove nothing.
        beats.record(
            &crate::domain::MeterId::new("appart-est"),
            OracleState::Fresh,
            Verdict::good(),
        );
        let health = body(healthz(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            health.contains("\"degraded_meters\":[]"),
            "a healthy fleet must say so with an empty list, not by omission:\n{health}"
        );

        // The counter goes backwards. The meter keeps polling and keeps
        // publishing — what it publishes is `Bad`, with nulls.
        beats.record(
            &crate::domain::MeterId::new("appart-est"),
            OracleState::Fresh,
            Verdict::bad(Cause::CounterWentBackwards),
        );
        let health = body(healthz(State(Arc::clone(&state))).await.into_response()).await;

        assert!(
            health.contains("\"meter\":\"appart-est\""),
            "the degraded meter must be NAMED — an operator who cannot tell which \
             of four meters is lying has to check all four:\n{health}"
        );
        assert!(
            health.contains("\"cause\":\"counter-went-backwards\""),
            "and the cause must travel with it, or the operator is told something \
             is wrong and not what:\n{health}"
        );

        // THE TWO FIELDS THAT REPORTED HEALTHY THROUGH THE TEN-HOUR OUTAGE, and
        // which are still telling the truth: this is why a new field was needed
        // rather than a wider reading of an existing one.
        assert!(
            health.contains("\"failed_sources\":[]"),
            "a degraded meter has not FAILED: the source answered, and a restart \
             would not fix a counter that went backwards:\n{health}"
        );
        assert!(
            health.contains("\"wedged\":false"),
            "and the poll loop is not wedged: it ticked, judged, and published a \
             refusal, which is the loop working:\n{health}"
        );

        // AND THE PAGE A HUMAN OPENS, which is where AC6 was left unmet until its
        // own review found it: `/healthz` had the field and `/` said the bridge
        // "is polling the meters and publishing what it reads", unqualified,
        // about a meter it was publishing `Bad` for.
        let page = body(index(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            page.contains("appart-est"),
            "the page must name the degraded meter — it is the surface an operator \
             opens at 3am, and the one that reported healthy through the ten-hour \
             outage of 2026-08-10:\n{page}"
        );
        assert!(
            page.contains("counter-went-backwards"),
            "and it must carry the cause, or the operator is told to distrust a \
             value without being told why:\n{page}"
        );
        assert!(
            !page.contains("not being read"),
            "a degraded meter IS being read: saying otherwise sends the operator \
             to look for a credential or a device mapping, when the fault is in \
             the number:\n{page}"
        );
    }

    /// **A meter is reported ONCE, and `/` never says both things about it**
    /// (story 2.3 AC6, found by the 2026-08-12 review of the story's own fix).
    ///
    /// `degraded()` filtered on the published quality alone and excluded nothing,
    /// while `failed()` filters on `State::Failed` — and `pulse.record` writes
    /// both on every tick. A meter with a refused credential was therefore in both
    /// lists, and `/` printed, one paragraph after the other:
    ///
    /// ```text
    /// One meter is not being read: cellar. … No value is published for it …
    /// This is a fault a restart is needed to clear …
    ///
    /// One meter is being read, but what is published must not be trusted:
    /// cellar (source-refused). … every reading reaches the host …
    /// Nothing here is cleared by a restart …
    /// ```
    ///
    /// Not lied to, but told two contradictory things and left to pick — on the
    /// surface an operator opens during the incident, which is the whole subject
    /// of AC6. The code even carried the distinction in prose: *"that block is
    /// about meters producing NOTHING, this one about meters producing something
    /// marked."* It was written and not implemented.
    ///
    /// Fixed in `FleetState::degraded` rather than in this page, because
    /// `/healthz` counted the same meter in `failed_sources` and `degraded` too —
    /// defensible for two machine-read fields until a consumer adds them up.
    ///
    /// FALSIFIED 2026-08-12 against the code as `90a7437` left it: restoring
    /// `degraded()` without its `Failed` filter turns the first assertion below
    /// red, with the two paragraphs in the dump.
    #[tokio::test]
    async fn a_failed_meter_is_not_also_reported_as_being_read() {
        use crate::core::clock::FakeClock;
        use crate::core::state_machine::State as OracleState;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let clock = Arc::new(FakeClock::new(UtcMillis(1_784_984_793_000)));
        let cellar = crate::domain::MeterId::new("cellar");
        let beats = Heartbeats::for_meters([cellar.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let state = ui(publishing(
            beats.clone(),
            clock.clone(),
            Arc::clone(&config),
        ));

        // A refused credential: the state latches `Failed` AND the published
        // verdict is `Bad(source-refused)`. Both are recorded, which is what put
        // the meter in two lists.
        beats.record(
            &cellar,
            OracleState::Failed,
            crate::core::oracle::Verdict::bad(crate::core::oracle::Cause::SourceRefused),
        );

        let page = body(index(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            page.contains("not being read"),
            "the premise: a failed source is still named:\n{page}"
        );
        assert!(
            !page.contains("is being read, but what is published must not be trusted"),
            "a meter reported as NOT being read must not also be reported as being \
             read and publishing — the two paragraphs disagree about whether it is \
             polled and about whether a restart clears it, and an operator reading \
             at 3am has to guess which:\n{page}"
        );

        let health = body(healthz(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            health.contains("\"failed_sources\":[\"cellar\"]"),
            "the machine-read surface still names it as failed:\n{health}"
        );
        assert!(
            health.contains("\"degraded_meters\":[]"),
            "and does not ALSO count it as degraded: a consumer adding the two \
             fields would report two faulty meters where there is one:\n{health}"
        );
    }

    /// **Story 3.2 AC5, [ADR 0027] §1 and §2** — the page and `/healthz` must
    /// agree with the wire about a source that has failed.
    ///
    /// The defect: a rejected smart-me credential puts every meter into an
    /// absorbing `Failed`, nothing reaches the wire, and `/` went on saying the
    /// bridge *"is polling the meters and publishing what it reads"* — a claim
    /// about the source, made by a page that had no way to see one. `/healthz`
    /// reported `intends_to_publish: true` and `wedged: false`, both true and
    /// both beside the point.
    ///
    /// **Both halves are asserted.** A page that shouted about a fault whatever
    /// the state would pass the first half and be useless; the healthy case is
    /// checked first, and its silence is what gives the second case meaning.
    ///
    /// The status code stays 200 deliberately: Epic 7 restarts on it, and a
    /// restart cannot clear a rejected credential — it would loop, eating the
    /// screen that names the fault.
    ///
    /// FALSIFIED 2026-08-07 by making `Phase::failed_sources` return `Vec::new()`
    /// unconditionally — the state the code was in before this story. Copied:
    ///
    /// ```text
    /// test ui::tests::a_failed_source_is_named_on_the_page_and_in_healthz ... FAILED
    ///
    /// thread '…a_failed_source_is_named_on_the_page_and_in_healthz' (57) panicked at
    /// crates/smartme-bridge/src/ui/mod.rs:797:9:
    /// the page claims the bridge is publishing what it reads; a meter whose source has
    /// FAILED must be named, or the operator is sent to the logs to discover it:
    /// <!doctype html>…<p><strong>Running</strong></p><p>The bridge is configured and
    /// confirmed, so it is polling the meters and publishing what it reads.…</p>…
    /// ```
    ///
    /// The dump is the page itself, which is the point: the helper reads the
    /// rendered bytes, so a mutation of what is rendered can reach the assertion.
    ///
    /// **REUNITED WITH ITS TEST 2026-08-12.** This block, and the two beside it,
    /// had drifted onto a pile above `a_control_character_…`: two of them were
    /// already orphaned before the story 2.3 fix, which then added a third rather
    /// than noticing the pile. So a recorded falsification sat next to a test that
    /// could not produce it, which is the repository rule
    /// (*"record the falsification next to the test"*) failing in the form that is
    /// hardest to see — the note exists, it is just attached to the wrong thing.
    /// The same file records the previous occurrence, on `Phase::loop_age`
    /// (`590c78d`, 2026-08-07).
    ///
    /// [ADR 0027]: ../../../docs/adr/0027-a-failed-source-is-a-fault-the-screen-must-name.md
    #[tokio::test]
    async fn a_failed_source_is_named_on_the_page_and_in_healthz() {
        use crate::core::clock::FakeClock;
        use crate::core::state_machine::State as OracleState;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let clock = Arc::new(FakeClock::new(UtcMillis(1_784_984_793_000)));
        let beats = Heartbeats::for_meters([
            crate::domain::MeterId::new("garage"),
            crate::domain::MeterId::new("cellar"),
        ]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let state = ui(publishing(
            beats.clone(),
            clock.clone(),
            Arc::clone(&config),
        ));

        // HEALTHY FIRST. Without this the assertions below would also hold for a
        // page that named a fault unconditionally.
        beats.record(
            &crate::domain::MeterId::new("garage"),
            OracleState::Fresh,
            crate::core::oracle::Verdict::good(),
        );
        beats.record(
            &crate::domain::MeterId::new("cellar"),
            OracleState::Fresh,
            crate::core::oracle::Verdict::good(),
        );
        let page = body(index(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            !page.contains("not being read"),
            "a fleet that is being read must not be reported as faulty:\n{page}"
        );
        let health = body(healthz(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            health.contains("\"failed_sources\":[]"),
            "the healthy body must say so with an empty list, not by omission:\n{health}"
        );

        // Now one meter's source fails fatally — a refused credential.
        beats.record(
            &crate::domain::MeterId::new("cellar"),
            OracleState::Failed,
            crate::core::oracle::Verdict::bad(crate::core::oracle::Cause::SourceRefused),
        );

        let page = body(index(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            page.contains("not being read") && page.contains("cellar"),
            "the page claims the bridge is publishing what it reads; a meter whose \
             source has FAILED must be named, or the operator is sent to the logs \
             to discover it:\n{page}"
        );
        assert!(
            !page.contains("garage"),
            "only the failed meter is named; naming the healthy one too would make \
             the list noise an operator learns to skip:\n{page}"
        );

        let response = healthz(State(Arc::clone(&state))).await.into_response();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "ADR 0027 §2: a restart cannot clear a rejected credential, so a 503 \
             here would loop the container and destroy the screen that names it"
        );
        let health = body(response).await;
        assert!(
            health.contains("\"failed_sources\":[\"cellar\"]"),
            "the body must distinguish a FAULT from a deliberate silence, and name \
             which meter:\n{health}"
        );
    }

    /// **The handler itself**, because the tests above exercise `loop_age` and a
    /// mutation that returned `StatusCode::OK` unconditionally left every one of
    /// them green — which is exactly how the unconditional 200 shipped in the
    /// first place.
    ///
    /// **Reunited with its test 2026-08-12**, from the same pile as the block on
    /// `a_failed_source_is_named_on_the_page_and_in_healthz`; the reason is
    /// recorded there.
    #[tokio::test]
    async fn the_status_code_follows_the_wedge_and_nothing_else() {
        use crate::core::clock::FakeClock;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let clock = Arc::new(FakeClock::new(UtcMillis(1_784_984_793_000)));
        let (beats, heartbeat) = one_meter();
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let state = ui(publishing(
            beats.clone(),
            clock.clone(),
            Arc::clone(&config),
        ));
        heartbeat.touch(clock.monotonic(), 30_000);

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
    /// A supported hot change must not make a healthy loop look wedged.
    ///
    /// The allowance came from the configuration's CURRENT period, which moves
    /// the instant `Control::apply` stores it — while the loop is still parked in
    /// `tick().await` for the old one. Dropping 300 s to 5 s therefore reported
    /// `wedged: true` for up to five minutes about a loop doing exactly what it
    /// was told, and Epic 7 wires that to a container restart.
    #[test]
    fn a_hot_period_change_does_not_make_a_healthy_loop_look_wedged() {
        use crate::core::clock::FakeClock;
        use crate::domain::UtcMillis;

        let clock = Arc::new(FakeClock::new(UtcMillis(1_784_984_793_000)));
        let (beats, heartbeat) = one_meter();
        let mut slow = running_config();
        slow.poll.interval = std::time::Duration::from_secs(300);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(slow));
        let phase = publishing(beats.clone(), clock.clone(), Arc::clone(&config));

        // The loop ticks at 300 s and then sleeps.
        heartbeat.touch(clock.monotonic(), 300_000);

        // The operator drops the period to the minimum. In force in the model
        // immediately; the loop cannot notice until its next tick.
        let mut fast = (*config.load_full()).clone();
        fast.poll.interval = std::time::Duration::from_secs(5);
        config.store(Arc::new(fast));

        clock.advance_ms(60_000);
        let (age, allowed) = phase.loop_age(phase.fleet().as_ref()).expect("age");
        assert!(
            age <= allowed,
            "a loop sleeping out the period it was told to use is not wedged; \
             restarting the container would kill a Sparkplug session over a \
             supported reconfiguration. age {age} vs allowed {allowed}"
        );
    }

    #[test]
    fn a_deliberately_silent_bridge_has_no_loop_age() {
        assert!(
            Phase::silent(Lifecycle::Unconfigured)
                .loop_age(None)
                .is_none()
        );
        assert!(
            Phase::silent(Lifecycle::Unconfirmed)
                .loop_age(None)
                .is_none()
        );
    }

    /// The page must never claim a connection it cannot observe.
    ///
    /// `Running`'s text said *"The bridge is connected and publishing."* until
    /// 2026-08-05 — a compile-time constant describing which branch of `main.rs`
    /// ran, handed to an operator as an observation. It is word for word the
    /// claim removed from `/healthz` on 2026-08-04, left standing on the surface
    /// a human reads, and `Phase::starting()` then made it reachable before any
    /// socket existed. Broker connectivity is not plumbed to the UI ([#53]), so
    /// no page may assert it.
    #[test]
    fn no_page_claims_a_connection_the_bridge_cannot_observe() {
        for state in [
            Lifecycle::Unconfigured,
            Lifecycle::Unconfirmed,
            Lifecycle::Running,
            Lifecycle::Misconfigured,
        ] {
            let said = format!("{} {}", state.headline(), state.detail());
            assert!(
                !said.contains("is connected"),
                "{state:?} claims a connection nothing here can see: {said}"
            );
        }
        // And the positive half, so a page that said nothing at all would not
        // pass: the running state must still describe what it is doing.
        assert!(Lifecycle::Running.detail().contains("publishing"));
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
            Lifecycle::Misconfigured,
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
