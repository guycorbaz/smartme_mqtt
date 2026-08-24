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

mod check;
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
            // **Broker connectivity IS plumbed now** (story 6.5, [#53]), and this
            // sentence said it was not until the review of that story on
            // 2026-08-20. What the claim needed was never a hedge: it needed the
            // fact beside it, which the sink line above now carries.
            Lifecycle::Running => {
                "The bridge is configured and confirmed, so it is polling the meters \
                 and publishing what it reads. Whether what it reads is reaching the \
                 host is the broker's own line, beside this one."
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
    /// Every meter's last end-to-end check (story 6.6).
    ///
    /// **Not in the fleet state, deliberately**: `FleetState` records what the poll
    /// loop published, and a fact about a button does not belong inside it (6.6
    /// AC1). The UI writes this and the UI reads it; nothing the loop reads is
    /// touched by a check.
    checks: check::Checks,
    /// The clock this process started with, for the one thing a screen needs a
    /// time for: stamping a configuration write ([ADR 0039], story 6.7).
    ///
    /// **Held, never handed out.** [`UiState::wall_now`] is the whole interface. A
    /// caller holding the clock could read `monotonic()` from it, and a
    /// `MonotonicMs` counts from an origin held INSIDE the clock — so one taken
    /// here and one the poll loop recorded would be two different origins, which is
    /// the defect `Control::clock`'s own documentation warns about. Wall time has
    /// no such origin, which is why it is the half that may be exposed.
    ///
    /// [ADR 0039]: ../../../docs/adr/0039-the-configuration-remembers-when-it-was-written-and-which-meters-matter.md
    clock: Arc<dyn crate::core::Clock + Send + Sync>,
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
        clock: Arc<dyn crate::core::Clock + Send + Sync>,
        ready: Arc<tokio::sync::Notify>,
    ) -> Self {
        Self {
            phase,
            state_dir,
            checks: check::Checks::new(),
            clock,
            ready,
        }
    }

    /// Wall-clock now, for stamping a configuration write and nothing else.
    ///
    /// See the `clock` field for why this is a method rather than a getter handing
    /// the clock over.
    pub(crate) fn wall_now(&self) -> crate::domain::UtcMillis {
        self.clock.wall()
    }

    /// Every meter's last end-to-end check (story 6.6).
    ///
    /// Private to `ui`: the check registry is a screen's memory, and nothing
    /// outside these screens has any business reading it.
    fn checks(&self) -> &check::Checks {
        &self.checks
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
        // Story 6.4: the meter view. Its own route rather than a section of `/`,
        // because the two answer different questions — `/` says whether the bridge
        // is publishing at all, this says what each meter is doing.
        .route("/meters", get(screens::meter_view))
        // Story 6.6: the end-to-end check. GET reports, POST asks — so a reload
        // re-reads the answer instead of re-asking smart-me for it.
        .route("/check", get(check::check_view).post(check::run_check))
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
                    // AND WHAT TO DO ABOUT IT (FR31, story 6.8). The cause was
                    // already here; naming it without naming the gesture is the
                    // "actionable" half of FR31 left undone on the surface an
                    // operator opens first.
                    verdict.cause().map_or_else(
                        || format!("{meter} (no cause recorded)"),
                        |c| format!("{meter} ({}) — {}", c.as_str(), c.gesture()),
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
    // AND THE OTHER HALF OF FR29's PAIR: whether the broker is there.
    //
    // **Added by the review of story 6.5 (2026-08-20), which found AC3 met on
    // `/meters` and not on the screen the criterion names.** The two defects were
    // the same one seen twice: this page went on saying the broker's reachability
    // "is not reported here yet — the log says so" about a bridge that had just
    // been given the fact, and an operator reading `/` during an outage was sent
    // to the log for something the page could have told them. That is precisely
    // the shape of [#53], which story 6.5 closed on the other surface.
    //
    // Absent in every silent phase, because there is no driver to have observed
    // anything — and absent is then the truth rather than a default, which is the
    // rule `fleet()` and `failed_sources` already apply.
    let sink_line = phase.control().map_or_else(String::new, |control| {
        format!(
            "<p>{}</p>",
            screens::sink_health_line(control.sink(), control.clock().wall())
        )
    });
    // FR35's CONTEXT LINE — what this bridge knows about its own configuration
    // ([ADR 0039], story 6.7).
    //
    // **It describes the FILE, and says so.** The file is the configuration
    // (ADR 0023); a line built from the settings in force would answer a different
    // question and would disagree with the dates beside it the moment a cold change
    // was saved but not yet carried out.
    //
    // **Ages, not calendar dates.** The PRD asks for a human timestamp and gives
    // the example itself — "3 min ago" / "6 days ago" — and `ago` is what every
    // other screen here already speaks. A calendar date would need a calendar: this
    // workspace has no date library in its direct dependencies, and pulling one in
    // to render one line is not a trade this story is allowed to make quietly.
    let context = screens::configuration_context(state.state_dir(), state.wall_now());
    // AND THE WAY TO THE TWO PAGES THAT ANSWER "which meter, and which link".
    //
    // A screen nothing links to does not exist: story 6.6 shipped `/check`
    // reachable from `/meters` only, so an operator who opened the bridge at its
    // root had to know the path by heart. Present only where there is something to
    // look at — in the silent phases the way out is `next_step`'s, and it is the
    // configuration.
    let ways = if phase.control().is_some() {
        "<p><a href=/meters>What each meter is doing</a> · \
         <a href=/check>Check one meter end to end</a></p>"
    } else {
        ""
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
         {}\
         {}\
         {}\
         <hr><p>version {} · contract {}</p>",
        lifecycle.headline(),
        lifecycle.detail(),
        context,
        sink_line,
        caveat,
        degraded_caveat,
        ways,
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
    // It keeps that name now that the sink IS plumbed ([#53], story 6.5): the two
    // are different questions, and `sink_connected` below answers the second one.
    // "Has the bridge reached its publishing arm" stays worth reporting on its own
    // — a bridge that never got there and one whose broker went away are not the
    // same incident.
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
    // AND A LOST READING IS NEITHER (story 4.11 AC4, FR22). A meter can be
    // healthy, fresh and publishing, and still have had readings thrown away
    // because the broker was unreachable — `failed_sources` does not see it,
    // `degraded_meters` does not see it, and the wire cannot say it, because what
    // is being reported is precisely what never reached the wire.
    //
    // From the SAME snapshot as everything above: `fleet` is read once at the top
    // of this handler, for the reason its comment gives.
    //
    // The status code does NOT move for this. Epic 7 wires it to a container
    // restart, and a restart provably cannot reach a broker that is down — it
    // would loop, destroying the surface that names the fault (ADR 0027 §2).
    let dropped_json = format!(
        "[{}]",
        fleet
            .as_ref()
            .map(|f| f.dropped())
            .unwrap_or_default()
            .iter()
            // `retired` travels WITH the count ([#90]). A machine reading this
            // list has the same question an operator has — is this number a
            // live fault or the history of a meter someone switched off — and
            // the count alone cannot answer it: a disabled meter's figure is
            // frozen, not falling, so it looks exactly like a fault that has
            // stopped getting worse.
            .map(|lost| format!(
                "{{\"meter\":{},\"reason\":{},\"count\":{},\"retired\":{},\
                 \"republications\":{}}}",
                json_string(&lost.meter.to_string()),
                json_string(lost.reason.as_str()),
                lost.count,
                lost.retired,
                // HOW MANY OF THIS METER'S LOSSES WERE COPIES ([#92]). `count`
                // stays what the manual defines — messages the bridge could not
                // hand over — and the historian's question, *how many distinct
                // measurements am I missing*, is `count` minus this. Per meter,
                // so it repeats across that meter's rows: it counts what was
                // lost, not why.
                lost.republications,
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    // THE SINK'S OWN HEALTH, and it stays OUT of the status code ([#53], story
    // 6.5). An unreachable broker is an honest STALE: the bridge is working
    // correctly and saying so, and the container restart Epic 7 wires to a non-200
    // would fix nothing while killing every meter's session. The rule is unchanged
    // — unhealthy only for a wedged poll loop — and this is a fact for the body.
    //
    // `null` is not `false`: a bridge that has never connected has not lost
    // anything, and telling an operator "disconnected" about one that never tried
    // sends them after an outage that did not happen.
    let (sink_connected, sink_since) = phase.control().and_then(Control::sink).map_or_else(
        || ("null".to_string(), "null".to_string()),
        |s| (s.connected.to_string(), s.since.0.to_string()),
    );
    let body = format!(
        "{{\"status\":\"{}\",\"intends_to_publish\":{},\"wedged\":{},\
          \"sink_connected\":{sink_connected},\"sink_since_ms\":{sink_since},\
          \"failed_sources\":{},\"degraded_meters\":{},\"dropped_readings\":{},\
          \"loop_age_ms\":{},\"loop_age_allowed_ms\":{},\
          \"version\":\"{}\",\"contract\":{}}}",
        phase.lifecycle.slug(),
        !phase.lifecycle.is_silent_on_purpose(),
        wedged,
        failed_json,
        degraded_json,
        dropped_json,
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

    /// **A screen nothing links to does not exist** — the review of story 6.6,
    /// 2026-08-20.
    ///
    /// `/check` shipped reachable from `/meters` alone, so an operator opening the
    /// bridge at its root had to know the path by heart. The same omission would
    /// have hidden `/meters` itself if `/` had not linked it.
    ///
    /// **The silent half is asserted too**, and it is the half that makes the first
    /// one mean something: in a phase with no poll loop there is nothing to look at,
    /// the way out is the configuration, and offering "check one meter end to end"
    /// to a bridge that has no meters would be a link to a page that can only say
    /// so.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN: `let ways = "";` unconditionally goes
    /// red with `the page an operator opens first must offer the two screens that
    /// answer "which meter, and which link"`.
    #[tokio::test]
    async fn the_state_screen_offers_the_way_to_the_other_screens() {
        use std::sync::Arc;

        let clock = Arc::new(crate::core::clock::FakeClock::new(
            crate::domain::UtcMillis(1_784_984_793_000),
        ));
        let beats = Heartbeats::for_meters([crate::domain::MeterId::new("appart-est")]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(config_with_a_meter()));
        let running = ui(publishing(beats, clock, Arc::clone(&config)));

        let html = body(index(State(Arc::clone(&running))).await.into_response()).await;
        assert!(
            html.contains("href=/meters") && html.contains("href=/check"),
            "the page an operator opens first must offer the two screens that answer \
             \"which meter, and which link\" — a page reachable only by typing its \
             path is a page nobody opens at three in the morning:\n{html}"
        );

        let silent = ui(Phase::silent(Lifecycle::Unconfigured));
        let html = body(index(State(Arc::clone(&silent))).await.into_response()).await;
        assert!(
            !html.contains("href=/check"),
            "and a bridge with no poll loop must not offer an end-to-end check: there \
             is nothing to check, and the way out is the configuration:\n{html}"
        );
    }

    /// **Story 6.8 AC4 — the gesture on the page is the CAUSE's, not the culprit's
    /// three-way one.**
    ///
    /// [#103]'s finding, asserted where an operator would meet it. Two meters with
    /// two different `You` causes must read differently: under the old table both
    /// said *"open the configuration: a credential, a serial or a device id is
    /// wrong"*, which names three repairs and points at a screen that — for the
    /// credential — has no field to make one ([ADR 0023]).
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN: rendering `repair(culprit)` again (the
    /// state before this story) goes red with `two different causes must not read
    /// the same`.
    #[tokio::test]
    async fn the_meter_page_names_the_gesture_this_cause_asks_for() {
        use crate::core::oracle::{Cause, Verdict};
        use crate::core::state_machine::State as OracleState;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let now = UtcMillis(1_784_984_793_000);
        let clock = Arc::new(crate::core::clock::FakeClock::new(now));
        let a = crate::domain::MeterId::new("appart-est");
        let b = crate::domain::MeterId::new("atelier");
        let beats = Heartbeats::for_meters([a.clone(), b.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(config_with_a_meter()));
        let state = ui(publishing(beats.clone(), clock, Arc::clone(&config)));

        for (meter, cause) in [
            (&a, Cause::CredentialRejected),
            (&b, Cause::IdentityMismatch),
        ] {
            beats.record_at(
                meter,
                OracleState::Failed,
                Verdict::bad(cause),
                Some(crate::app::poll_publish::Publication {
                    at: UtcMillis(now.0 - 1_000),
                    threshold_ms: 90_000,
                    value_date: None,
                    power_kw: None,
                    energy_kwh: None,
                }),
            );
        }

        let html = body(
            crate::ui::screens::meter_view(State(Arc::clone(&state)))
                .await
                .into_response(),
        )
        .await;

        assert!(
            html.contains("SMARTME_CLIENT_SECRET"),
            "a rejected credential must name the two variables that repair it — and \
             NOT the configuration screen, which has no field for it ([ADR \
             0023]):\n{html}"
        );
        assert!(
            html.contains("compare that row against the account"),
            "and a serial that does not match must name the row to look at: two \
             different causes must not read the same, which is [#103]'s whole \
             finding:\n{html}"
        );
    }

    /// **Story 6.7 AC4 — FR35's context line, including the half that says it does
    /// not know.**
    ///
    /// The unknown case is asserted FIRST and it is the one that matters: a
    /// configuration written before [ADR 0039] has no creation date, and the
    /// alternative the ADR refused — the file's mtime — would have rendered a
    /// plausible date that no change of this bridge's ever produced. A test that
    /// only checked the happy path would pass against that implementation too.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN, output copied: falling back to the
    /// file's modification time goes red with
    ///
    /// ```text
    /// a configuration whose creation nobody recorded must SAY so … found a date
    /// where "unknown" belongs
    /// ```
    #[tokio::test]
    async fn the_state_screen_says_what_it_knows_about_its_own_configuration() {
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!("smartme_6_7_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");

        let now = crate::domain::UtcMillis(1_784_984_793_000);
        let clock: Arc<dyn crate::core::Clock + Send + Sync> =
            Arc::new(crate::core::clock::FakeClock::new(now));

        // A file from before ADR 0039: two meters, no dates, nothing marked.
        let mut stored = crate::app::store::StoredConfig {
            schema_version: crate::app::store::SCHEMA_VERSION,
            created_ms: None,
            last_change_ms: None,
            group_id: "G".into(),
            node_id: "N".into(),
            broker_host: "b".into(),
            broker_port: 1883,
            publish_period_secs: 30,
            api_base: None,
            log_dir: None,
            log_keep: None,
            mapping_confirmed: true,
            ui_port: None,
            meters: vec![
                crate::app::store::StoredMeter {
                    meter_id: "appart-est".into(),
                    device_id: "dev-0".into(),
                    serial: "1112222".into(),
                    enabled: true,
                    priority: false,
                },
                crate::app::store::StoredMeter {
                    meter_id: "atelier".into(),
                    device_id: "dev-1".into(),
                    serial: "3334444".into(),
                    enabled: true,
                    priority: false,
                },
            ],
        };
        crate::persist::persist_atomic(&crate::app::store::config_path(&dir), &stored)
            .expect("plant the old file");

        let state = Arc::new(UiState::new(
            Phase::silent(Lifecycle::Unconfirmed).into_handle(),
            dir.clone(),
            Arc::clone(&clock),
            Arc::new(tokio::sync::Notify::new()),
        ));
        let html = body(index(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            html.contains("created before this bridge recorded creation dates"),
            "a configuration whose creation nobody recorded must SAY so: the file's \
             mtime moves on a docker cp, a restore or a touch, so it would be a \
             plausible date that is nobody's change ([ADR 0039]):\n{html}"
        );
        assert!(
            html.contains("2 meters, 0 of them marked as mattering"),
            "and it must count what it does know:\n{html}"
        );

        // AND A CONFIGURATION THIS BUILD CREATED, in a directory with no file: the
        // only situation in which a creation date can be known. Writing over the
        // old file above would NOT have produced one, which is the rule and not an
        // oversight — this second directory exists because the first cannot answer.
        let fresh = std::env::temp_dir().join(format!("smartme_6_7b_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&fresh);
        stored.meters[1].priority = true;
        crate::app::store::save(&fresh, &stored, crate::domain::UtcMillis(now.0 - 7_200_000))
            .expect("write");
        let state = Arc::new(UiState::new(
            Phase::silent(Lifecycle::Unconfirmed).into_handle(),
            fresh.clone(),
            Arc::clone(&clock),
            Arc::new(tokio::sync::Notify::new()),
        ));
        let html = body(index(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            html.contains("created 2 hours ago") && html.contains("last changed 2 hours ago"),
            "a configuration this build created carries both dates, as ages — the \
             human timestamp the PRD asks for:\n{html}"
        );
        assert!(
            html.contains("1 of them marked as mattering"),
            "and the priority count is the operator's own mark, which is the only \
             place that fact exists:\n{html}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&fresh);
    }

    /// A configuration carrying one meter, which the check needs and
    /// `running_config` deliberately does not have.
    ///
    /// **`api_base` is `http://` on purpose, and it is what keeps these tests off
    /// the network.** `SmartMeClient::new` refuses any scheme but `https` before it
    /// opens a socket, so a check run here fails locally whatever the environment
    /// holds. Without it the test would pass for two different reasons: with no
    /// credential in the environment it never builds a client, and WITH one — other
    /// tests in this binary set `SMARTME_CLIENT_*`, and environment variables are
    /// per-process — it would have sent a real request to smart-me and hung on the
    /// timeout. The one it must not have is the second.
    fn config_with_a_meter() -> crate::app::supervisor::BridgeConfig {
        let mut config = running_config();
        config.api_base = "http://127.0.0.1:1".to_string();
        config.meters.push(crate::app::config::MeterConfig {
            priority: false,
            meter: crate::domain::MeterId::new("appart-est"),
            device_id: "dev-0".to_string(),
            serial: crate::domain::Serial::new("1112222"),
            enabled: true,
        });
        config
    }

    /// **Story 6.6 AC1 and AC3 — a check writes NOTHING the poll loop reads.**
    ///
    /// This is the criterion the whole story is shaped around. `step_once` judges
    /// against a per-meter memory — `energy_reference`, `last_http_date`,
    /// `last_value_date` — and the fleet state is what `/healthz` reports; a check
    /// that touched either would make a button change what the host is told.
    ///
    /// The proof is the whole snapshot, `generation` included: that counter moves on
    /// every `send_modify`, so any write at all is visible here even if the field it
    /// wrote happened to hold the same value.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN, output copied: clearing the meter's
    /// opinion before asking (`control.heartbeats().retire(&meter)` in `run_check`,
    /// the plausible "start from a clean slate" edit) goes red with
    ///
    /// ```text
    /// a check must not move the fleet state: generation 1 became 2, and the verdict
    /// the host is being told changed because somebody pressed a button
    /// ```
    #[tokio::test]
    async fn a_check_writes_nothing_the_poll_loop_reads() {
        use crate::core::oracle::Verdict;
        use crate::core::state_machine::State as OracleState;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let now = UtcMillis(1_784_984_793_000);
        let clock = Arc::new(crate::core::clock::FakeClock::new(now));
        let meter = crate::domain::MeterId::new("appart-est");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(config_with_a_meter()));
        let state = ui(publishing(
            beats.clone(),
            clock.clone(),
            Arc::clone(&config),
        ));

        beats.record_at(
            &meter,
            OracleState::Fresh,
            Verdict::good(),
            Some(crate::app::poll_publish::Publication {
                at: UtcMillis(now.0 - 1_000),
                threshold_ms: 90_000,
                value_date: Some(UtcMillis(now.0 - 2_000)),
                power_kw: Some(0.018),
                energy_kwh: Some(4_843.822),
            }),
        );
        let before = beats.snapshot();

        // No credential in the environment is the fastest honest path: the check
        // refuses to send an unauthenticated request, so this exercises everything
        // up to and including the write of the result — which is the part under test.
        let response = check::run_check(
            State(Arc::clone(&state)),
            axum::http::HeaderMap::new(),
            axum::extract::Form(vec![("meter".to_string(), "appart-est".to_string())]),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::SEE_OTHER,
            "the POST redirects to the page that reports it, so a reload re-reads \
             the answer instead of re-asking smart-me"
        );
        for _ in 0..1_000 {
            if !matches!(
                state.checks().get(&meter),
                Some(check::Check::Running { .. })
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }

        let after = beats.snapshot();
        assert_eq!(
            before.generation, after.generation,
            "a check must not move the fleet state: generation {} became {}, and the \
             verdict the host is being told would change because somebody pressed a \
             button",
            before.generation, after.generation
        );
        assert_eq!(
            before.meters[0].published, after.meters[0].published,
            "and the published verdict in particular is the poll loop's alone"
        );
    }

    /// **Story 6.6 AC5 — the button cannot become a way to hammer smart-me.**
    ///
    /// [#77] found that a 429 on the token endpoint arms no wait, so this bridge's
    /// own restraint is the only restraint there is. The rule is walked as a pure
    /// function, then seen in the page's words — a refusal that were silent, or that
    /// re-rendered the previous result without saying it was old, would pass a test
    /// that only checked no request went out.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN: deleting the `TooSoon` arm (letting a
    /// finished check always re-run) goes red with `a second check inside the poll
    /// period must be refused: the button would out-poll the poll loop`.
    #[test]
    fn a_second_check_inside_the_poll_period_is_refused() {
        use crate::domain::UtcMillis;
        let now = UtcMillis(1_784_984_793_000);
        let period = 30_000;

        assert!(
            check::refusal_for(None, now, period).is_none(),
            "a meter never checked is checkable"
        );
        assert!(
            check::refusal_for(Some(&check::Check::Running { started: now }), now, period)
                .is_some(),
            "one in flight is one in flight"
        );
        assert!(
            check::refusal_for(
                Some(&check::Check::Done {
                    at: UtcMillis(now.0 - 5_000),
                    source: check::SourceLink::NoCredential,
                }),
                now,
                period
            )
            .is_some(),
            "a second check inside the poll period must be refused: the button would \
             out-poll the poll loop, against an API that answers a 429 this bridge \
             does not yet wait out ([#77])"
        );
        assert!(
            check::refusal_for(
                Some(&check::Check::Done {
                    at: UtcMillis(now.0 - 31_000),
                    source: check::SourceLink::NoCredential,
                }),
                now,
                period
            )
            .is_none(),
            "and one poll period later it is checkable again, or the feature is a \
             button that works once"
        );
    }

    /// **Story 6.6 AC2 — the middle link is the PUBLISHED verdict, and the page says
    /// so.**
    ///
    /// The state is `Bad (credential-rejected)`, latched. A check that re-judged
    /// would light this link from whatever the source just answered and tell an
    /// operator the meter was fine while the host is being told it is not.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN: rendering link 2 from the source answer
    /// instead of `FleetState` goes red with `the page must report the verdict IN
    /// FORCE … "credential-rejected" absent`.
    #[tokio::test]
    async fn the_middle_link_is_what_the_host_is_being_told() {
        use crate::core::oracle::{Cause, Verdict};
        use crate::core::state_machine::State as OracleState;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let now = UtcMillis(1_784_984_793_000);
        let clock = Arc::new(crate::core::clock::FakeClock::new(now));
        let meter = crate::domain::MeterId::new("appart-est");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(config_with_a_meter()));
        let state = ui(publishing(beats.clone(), clock, Arc::clone(&config)));

        beats.record_at(
            &meter,
            OracleState::Failed,
            Verdict::bad(Cause::CredentialRejected),
            Some(crate::app::poll_publish::Publication {
                at: UtcMillis(now.0 - 4_000),
                threshold_ms: 90_000,
                value_date: None,
                power_kw: None,
                energy_kwh: None,
            }),
        );

        let html = body(
            check::check_view(
                State(Arc::clone(&state)),
                "/check?meter=appart-est".parse().expect("a uri"),
            )
            .await,
        )
        .await;

        assert!(
            html.contains("credential-rejected"),
            "the page must report the verdict IN FORCE, cause included — it is what \
             the host is being told while the operator reads this:\n{html}"
        );
        // **AMENDED by story 6.8**, and the amendment is a repair rather than an
        // accommodation: this asserted "open the configuration" for a rejected
        // credential, and [ADR 0023] put the credential in the ENVIRONMENT — there
        // is no field for it on that screen, deliberately. The old three-way gesture
        // sent the operator to a form that could not hold the repair; the cause's
        // own gesture names the two variables.
        assert!(
            html.contains("you") && html.contains("SMARTME_CLIENT_SECRET"),
            "with the culprit and the gesture THIS cause asks for, derived at render \
             time (story 6.3 AC4, story 6.8):\n{html}"
        );
        assert!(
            html.contains("did not re-judge"),
            "and it must SAY that the check did not re-judge, or the two links read \
             as one measurement seen twice — which is the confusion this story was \
             shaped to avoid:\n{html}"
        );
    }

    /// **Story 6.6 AC6 — never run, running, and answered are three states the page
    /// RENDERS.**
    ///
    /// FR32's distinction, on this page. The running state carries the refresh that
    /// makes it resolve; without it the page would be honest and useless — an
    /// operator staring at "asking" forever.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN: dropping the `meta refresh` from the
    /// running branch goes red with `a page that says "asking" must come back for
    /// the answer`.
    #[tokio::test]
    async fn the_three_states_of_a_check_are_told_apart() {
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let now = UtcMillis(1_784_984_793_000);
        let clock = Arc::new(crate::core::clock::FakeClock::new(now));
        let meter = crate::domain::MeterId::new("appart-est");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(config_with_a_meter()));
        let state = ui(publishing(beats, clock, Arc::clone(&config)));
        let uri: axum::http::Uri = "/check?meter=appart-est".parse().expect("a uri");

        let html = body(check::check_view(State(Arc::clone(&state)), uri.clone()).await).await;
        assert!(
            html.contains("Not checked since this bridge started"),
            "never run is its own state, and an empty result must not read as a \
             passing one:\n{html}"
        );

        state
            .checks()
            .set(&meter, check::Check::Running { started: now });
        let html = body(check::check_view(State(Arc::clone(&state)), uri.clone()).await).await;
        assert!(
            html.contains("Asking smart-me"),
            "running says it is running:\n{html}"
        );
        assert!(
            html.contains("http-equiv=refresh"),
            "a page that says \"asking\" must come back for the answer, or the \
             operator waits on a page that will never change:\n{html}"
        );

        state.checks().set(
            &meter,
            check::Check::Done {
                at: now,
                source: check::SourceLink::NoCredential,
            },
        );
        let html = body(check::check_view(State(Arc::clone(&state)), uri).await).await;
        assert!(
            html.contains("no credential in the environment")
                && !html.contains("http-equiv=refresh"),
            "answered says what it found and stops refreshing:\n{html}"
        );
    }

    /// **Story 6.6 AC4 — a refused source is classified by the table the poll loop
    /// reads, not by a second one.**
    ///
    /// `SourceError::cause` was extracted from `Policy::step_remembering` for exactly
    /// this: the check needed the mapping outside the loop, and a copy would have
    /// been a second place the truth lives.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN: classifying `AuthRejected` as transient
    /// (the wildcard's answer) goes red with `a refused credential is the operator's
    /// to repair … left: source-unreachable, right: credential-rejected`.
    #[test]
    fn a_refused_source_carries_the_cause_the_loop_would_publish() {
        use crate::core::oracle::{Cause, Culprit};
        use smart_me_client::SmartMeError;

        let cases = [
            (
                SmartMeError::AuthRejected { status: 401 },
                Cause::CredentialRejected,
                Culprit::You,
            ),
            (
                SmartMeError::UnknownDevice {
                    device_id: "dev-0".to_string(),
                },
                Cause::DeviceNotInAccount,
                Culprit::You,
            ),
            (
                SmartMeError::Timeout,
                Cause::SourceUnreachable,
                Culprit::World,
            ),
            (
                SmartMeError::HttpStatus { status: 503 },
                Cause::SourceUnreachable,
                Culprit::World,
            ),
        ];
        for (error, expected, culprit) in cases {
            let named = check::cause_of(&error);
            assert_eq!(
                named,
                expected,
                "a refused credential is the operator's to repair and an unreachable \
                 host is not: {error} must be published as {}, and this page must \
                 name what the loop would name",
                expected.as_str()
            );
            assert_eq!(
                named.culprit(),
                culprit,
                "and the culprit follows the cause"
            );
        }
    }

    fn ui(phase: Phase) -> Arc<UiState> {
        Arc::new(UiState::new(
            phase.into_handle(),
            std::path::PathBuf::from("/nonexistent"),
            std::sync::Arc::new(crate::core::clock::FakeClock::new(
                crate::domain::UtcMillis(1_784_984_793_000),
            )),
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

    /// **Story 4.14 AC1 — a slow server must not be able to restart this bridge.**
    ///
    /// # What holds AR12 up, and it is not a test elsewhere
    ///
    /// AR12's rule is *"an honest STALE never triggers a restart; a wedged poller
    /// does"*, and today the first half is true because of three numbers that
    /// have never been in the same sentence. The poll loop writes its heartbeat
    /// BEFORE the fetch and wraps the fetch in
    /// [`FETCH_TIMEOUT`](crate::app::config::FETCH_TIMEOUT), so a source that
    /// hangs holds the loop for at most 10 s. `/healthz` calls it wedged past
    /// [`WEDGED_AFTER_PERIODS`] × the period, and the shortest period an operator
    /// may set is [`PERIOD_MIN`](crate::app::config::PERIOD_MIN), 5 s — an
    /// allowance of 15 s.
    ///
    /// **The margin is 5 s and nothing was watching it.** Raise the fetch
    /// deadline for a good local reason, or shorten `PERIOD_MIN`, or drop `N` to
    /// 2, and a smart-me server having a slow minute becomes a container restart
    /// that kills the Sparkplug session of every meter — a fault outside this
    /// process, answered by destroying state inside it.
    ///
    /// This test is deliberately arithmetic rather than scenario-based: the
    /// scenario is `chaos_poller_wedge`, and a scenario test would go red for a
    /// dozen reasons that are not this one.
    ///
    /// FALSIFIED 2026-08-19 — mutation RUN, output copied: `FETCH_TIMEOUT` raised
    /// to 20 s goes red with `A BLOCKED FETCH CAN NOW OUTLIVE THE WEDGE
    /// ALLOWANCE: a fetch may hold the loop for 20s while /healthz calls it
    /// wedged after 3 x 5s = 15s`.
    #[test]
    fn the_wedge_allowance_outlives_a_blocked_fetch() {
        let allowance = crate::app::config::PERIOD_MIN * WEDGED_AFTER_PERIODS;
        assert!(
            crate::app::config::FETCH_TIMEOUT < allowance,
            "A BLOCKED FETCH CAN NOW OUTLIVE THE WEDGE ALLOWANCE: a fetch may \
             hold the loop for {fetch:?} while /healthz calls it wedged after \
             {n} x {period:?} = {allowance:?}. Under Epic 7 that verdict restarts \
             the container, so a smart-me server having a slow minute would end \
             the Sparkplug session of every meter — AR12's \"an honest STALE \
             never triggers a restart\" would be false. Whichever of the three \
             numbers moved, move it back or re-decide AR12 in an ADR",
            fetch = crate::app::config::FETCH_TIMEOUT,
            n = WEDGED_AFTER_PERIODS,
            period = crate::app::config::PERIOD_MIN,
            allowance = allowance,
        );
    }

    /// **Story 6.4 AC2, AC5 — a frozen meter reads differently from a quiet one.**
    ///
    /// This is what `last_changed_at` was built for in story 6.3, and the test that
    /// makes that field earn its place: without the pair, a meter that stopped
    /// measuring an hour ago and one measuring every second look identical, because
    /// ADR 0027 makes both publish every cycle.
    ///
    /// FALSIFIED 2026-08-19 — mutation RUN, output copied: rendering
    /// `last_changed_at` from `last_published_at` (the "obvious simplification")
    /// goes red with `a FROZEN meter must not read like a quiet one … both columns
    /// say "just now"`.
    #[tokio::test]
    async fn a_frozen_meter_and_a_quiet_one_do_not_read_the_same() {
        use crate::core::oracle::Verdict;
        use crate::core::state_machine::State as OracleState;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let now = UtcMillis(1_784_984_793_000);
        let clock = Arc::new(crate::core::clock::FakeClock::new(now));
        let meter = crate::domain::MeterId::new("appart-est");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let state = ui(publishing(
            beats.clone(),
            clock.clone(),
            Arc::clone(&config),
        ));

        // Measured an hour ago, republished a second ago — ADR 0027's normal case
        // for a meter that has stopped moving.
        let acquired = UtcMillis(now.0 - 3_600_000);
        beats.record_at(
            &meter,
            OracleState::Stale,
            Verdict::stale(crate::core::oracle::Cause::NotRevalidated),
            Some(crate::app::poll_publish::Publication {
                at: UtcMillis(now.0 - 3_600_000),
                threshold_ms: 90_000,
                value_date: Some(acquired),
                power_kw: Some(0.018),
                energy_kwh: Some(4_843.822),
            }),
        );
        beats.record_at(
            &meter,
            OracleState::Stale,
            Verdict::stale(crate::core::oracle::Cause::NotRevalidated),
            Some(crate::app::poll_publish::Publication {
                at: UtcMillis(now.0 - 1_000),
                threshold_ms: 90_000,
                value_date: Some(acquired),
                power_kw: Some(0.018),
                energy_kwh: Some(4_843.822),
            }),
        );

        let html = body(
            crate::ui::screens::meter_view(State(Arc::clone(&state)))
                .await
                .into_response(),
        )
        .await;

        assert!(
            html.contains("1 second ago"),
            "the publication follows every cycle and the page must say so:\n{html}"
        );
        assert!(
            html.contains("1 hour ago"),
            "a FROZEN meter must not read like a quiet one: it last MEASURED an hour \
             ago and the page has to say it, or the operator reads a recent \
             publication as a recent reading — which is the exact lie this bridge \
             exists to refuse:\n{html}"
        );
    }

    /// **Story 6.4 AC2 — FR28's FRESHNESS AGE**, added by the review of that
    /// story, 2026-08-20.
    ///
    /// The page shipped with eight of AC2's nine columns and the completion note
    /// listed the eight as though they were the nine. What was missing is the one
    /// FR28 names by that name: how old the MEASUREMENT is. Story 6.3 had stored
    /// `source_value_date` and `staleness_threshold_ms` for it — both fields were
    /// written every tick and read by nobody.
    ///
    /// **The two questions are genuinely different**, which is what this test
    /// pins: a bridge republishing every ten seconds has a fresh publication
    /// instant while carrying a reading four minutes old. Reading the second as the
    /// first is the age lie this project exists to refuse, one surface further out
    /// than the wire.
    ///
    /// FALSIFIED 2026-08-20 — two mutations RUN, output copied:
    ///
    /// ```text
    /// // 1. freshness rendered from `last_published_at`, the obvious simplification:
    /// the page must say how old the READING is, not only when we last republished it:
    /// …<td>0.018 kW</td><td>4843.822 kWh</td><td>1 second ago (stale past 90 s)</td>…
    ///
    /// // 2. the threshold dropped from the cell:
    /// and the threshold that judged it must travel with it (story 6.3 AC1)…
    /// …<td>4 minutes ago</td><td>Good</td><td>1 second ago</td>…
    /// ```
    ///
    /// Mutation 1 is the whole point of the column: the row still read plausibly —
    /// a fresh age, a threshold, a `Good` — and every number in it was about the
    /// publication rather than the measurement.
    #[tokio::test]
    async fn the_page_says_how_old_the_reading_is_and_under_which_threshold() {
        use crate::core::oracle::Verdict;
        use crate::core::state_machine::State as OracleState;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let now = UtcMillis(1_784_984_793_000);
        let clock = Arc::new(crate::core::clock::FakeClock::new(now));
        let meter = crate::domain::MeterId::new("appart-est");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let state = ui(publishing(
            beats.clone(),
            clock.clone(),
            Arc::clone(&config),
        ));

        // Measured four minutes ago, published one second ago — the normal case,
        // not a pathological one: ADR 0027 publishes a verdict every cycle and the
        // meter's own cadence is slower than the poll period.
        beats.record_at(
            &meter,
            OracleState::Fresh,
            Verdict::good(),
            Some(crate::app::poll_publish::Publication {
                at: UtcMillis(now.0 - 1_000),
                threshold_ms: 90_000,
                value_date: Some(UtcMillis(now.0 - 240_000)),
                power_kw: Some(0.018),
                energy_kwh: Some(4_843.822),
            }),
        );

        let html = body(
            crate::ui::screens::meter_view(State(Arc::clone(&state)))
                .await
                .into_response(),
        )
        .await;

        assert!(
            html.contains("4 minutes ago"),
            "the page must say how old the READING is, not only when we last \
             republished it: a fresh publication instant beside a four-minute-old \
             measurement is exactly the pair an operator misreads:\n{html}"
        );
        assert!(
            html.contains("stale past 90 s"),
            "and the threshold that judged it must travel with it (story 6.3 AC1) \
             — an age compared against a bound the operator has to remember is the \
             oracle's work done again, by hand, at three in the morning:\n{html}"
        );
    }

    /// **Story 6.4 AC2 — a value nobody has published is a dash, never a zero.**
    ///
    /// FR16's rule — *never a substituted value* — reaching the screen. A `0.000 kW`
    /// where nothing has been read is a number an operator would act on.
    ///
    /// FALSIFIED 2026-08-19 — mutation RUN: `unwrap_or(0.0)` in place of the dash
    /// goes red with `a value nobody published must not render as a number … found
    /// "0.000 kW"`.
    #[tokio::test]
    async fn a_meter_that_has_published_nothing_shows_no_number() {
        use std::sync::Arc;

        let clock = Arc::new(crate::core::clock::FakeClock::new(
            crate::domain::UtcMillis(1_784_984_793_000),
        ));
        let meter = crate::domain::MeterId::new("appart-est");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let state = ui(publishing(beats, clock, Arc::clone(&config)));

        let html = body(
            crate::ui::screens::meter_view(State(Arc::clone(&state)))
                .await
                .into_response(),
        )
        .await;

        assert!(
            !html.contains("0.000 kW"),
            "a value nobody published must not render as a number: `None` means \
             nothing was read, and a zero is a measurement. FR16 forbids the \
             substitution at the source; this is the same rule at the screen:\n{html}"
        );
        assert!(
            html.contains("Meter"),
            "and the table itself must be there, or the assertion above passes over \
             an empty page:\n{html}"
        );
    }

    /// **Story 6.4 AC3 — the three states FR32 asks to be told apart.**
    ///
    /// An empty table reads as "all quiet", which is the misreading FR32 exists to
    /// prevent: *unconfigured*, *starting*, and *running with a fault* are three
    /// different situations and an operator acts differently on each. The page must
    /// say which in words, not by the absence of rows.
    ///
    /// FALSIFIED 2026-08-19 — mutation RUN: returning the table shell for the
    /// unconfigured phase goes red with `an unconfigured bridge must SAY so … found
    /// a table instead`.
    #[tokio::test]
    async fn the_three_states_are_told_apart_in_words() {
        use std::sync::Arc;

        // 1. Nothing configured: no control at all.
        let silent = ui(Phase::silent(Lifecycle::Unconfigured));
        let html = body(
            crate::ui::screens::meter_view(State(Arc::clone(&silent)))
                .await
                .into_response(),
        )
        .await;
        assert!(
            html.contains("no configuration has been confirmed"),
            "an unconfigured bridge must SAY so: an empty table would read as \
             'all quiet', which is the whole of FR32:\n{html}"
        );
        assert!(
            !html.contains("<th>Meter</th>"),
            "and it must not show the table shell either — a header with no rows is \
             the same misreading wearing a border:\n{html}"
        );

        // 2. Running, but no meter has completed a tick: the fleet is empty rather
        //    than absent, and that is NOT the same message.
        let clock = Arc::new(crate::core::clock::FakeClock::new(
            crate::domain::UtcMillis(1_784_984_793_000),
        ));
        let beats = Heartbeats::for_meters([]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let starting = ui(publishing(beats, clock, Arc::clone(&config)));
        let html = body(
            crate::ui::screens::meter_view(State(Arc::clone(&starting)))
                .await
                .into_response(),
        )
        .await;
        assert!(
            html.contains("<th>Meter</th>"),
            "a running bridge shows its table, even with nothing in it yet — the \
             operator needs to see that the page works and the fleet is empty, not \
             wonder which:\n{html}"
        );
    }

    /// **Story 6.5, [#53] — the sink's state is reported, and it never touches the
    /// status code.**
    ///
    /// The trap this test exists against is the one [#53] describes: a field that
    /// compiles, renders, and reports `null` for ever teaches nobody anything. So
    /// the sink is DRIVEN here — connected, then lost — and the assertions are on
    /// what changed.
    ///
    /// FALSIFIED 2026-08-19 — two mutations RUN, output copied. Letting a
    /// disconnected sink drive the code (`if !connected { 503 }`) goes red with
    /// `AN UNREACHABLE BROKER IS AN HONEST STALE … status 503`. Reporting `false`
    /// instead of `null` before the first connect goes red with `a bridge that has
    /// never connected has not lost anything`.
    #[tokio::test]
    async fn the_sink_is_reported_in_the_body_and_never_in_the_status_code() {
        use crate::app::mqtt_driver::SinkHealth;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let clock = Arc::new(crate::core::clock::FakeClock::new(UtcMillis(
            1_784_984_793_000,
        )));
        let meter = crate::domain::MeterId::new("appart-est");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let sink = SinkHealth::new();
        let control =
            Control::detached_with_sink(Arc::clone(&config), beats.clone(), clock, sink.clone());
        let state = ui(Phase::running(control));

        // 1. NEVER CONNECTED. `null`, not `false`.
        let answer = healthz(State(Arc::clone(&state))).await.into_response();
        assert_eq!(answer.status(), 200);
        let html = body(answer).await;
        assert!(
            html.contains("\"sink_connected\":null"),
            "a bridge that has never connected has not lost anything, and `false` \
             would send an operator after an outage that did not happen:\n{html}"
        );

        // 2. CONNECTED.
        sink.observed(true, UtcMillis(1_784_984_700_000));
        let html = body(healthz(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            html.contains("\"sink_connected\":true")
                && html.contains("\"sink_since_ms\":1784984700000"),
            "the fact and its instant, both:\n{html}"
        );

        // 3. LOST — and the status code MUST NOT MOVE.
        sink.observed(false, UtcMillis(1_784_984_790_000));
        let answer = healthz(State(Arc::clone(&state))).await.into_response();
        let status = answer.status();
        let html = body(answer).await;
        assert!(
            html.contains("\"sink_connected\":false"),
            "the loss is reported:\n{html}"
        );
        assert_eq!(
            status, 200,
            "AN UNREACHABLE BROKER IS AN HONEST STALE. The bridge is working \
             correctly and saying so; Epic 7 wires non-200 to a container restart, \
             which would kill every meter's Sparkplug session over somebody else's \
             outage. The rule is unchanged: unhealthy ONLY for a wedged poll loop"
        );
    }

    /// **Story 6.5 AC3, FR29 — the page says WHICH END to look at.**
    ///
    /// A source that answers nothing and a broker that is gone produce the same
    /// silence on the wire and need opposite gestures. This is the assertion that
    /// makes the two healths independent in practice rather than in the struct.
    ///
    /// FALSIFIED 2026-08-19 — mutation RUN: rendering the unreachable branch as
    /// "connected" goes red with `an unreachable broker must be named as such`.
    #[tokio::test]
    async fn the_page_says_which_end_is_at_fault() {
        use crate::app::mqtt_driver::SinkHealth;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let now = UtcMillis(1_784_984_793_000);
        let clock = Arc::new(crate::core::clock::FakeClock::new(now));
        let beats = Heartbeats::for_meters([crate::domain::MeterId::new("appart-est")]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let sink = SinkHealth::new();
        let state = ui(Phase::running(Control::detached_with_sink(
            Arc::clone(&config),
            beats,
            clock,
            sink.clone(),
        )));

        // Never connected is its own message, not a disconnection.
        let html = body(
            crate::ui::screens::meter_view(State(Arc::clone(&state)))
                .await
                .into_response(),
        )
        .await;
        assert!(
            html.contains("never connected"),
            "a bridge that never reached the broker must say so, not report a \
             loss:\n{html}"
        );

        sink.observed(false, UtcMillis(now.0 - 600_000));
        let html = body(
            crate::ui::screens::meter_view(State(Arc::clone(&state)))
                .await
                .into_response(),
        )
        .await;
        assert!(
            html.contains("unreachable") && html.contains("10 minutes ago"),
            "an unreachable broker must be named as such, with since when — \
             otherwise 'nothing is published' sends the operator to the meters, \
             which are fine:\n{html}"
        );
        assert!(
            html.contains("restarting it repairs nothing"),
            "and the page must say the gesture is NOT to restart the bridge: that \
             is the same judgement `/healthz` makes by staying at 200:\n{html}"
        );
    }

    /// **Story 6.5 AC3, on the screen the criterion actually names** — added by
    /// the review of that story, 2026-08-20.
    ///
    /// The story shipped the sink line on `/meters` and declared AC3 met. `/` —
    /// the page an operator opens first, and the only one a link from a bookmark
    /// reaches — went on saying the broker's reachability *"is not reported here
    /// yet — the log says so"*, about a bridge that had just been handed the fact.
    /// During an outage that is the surface that could not say which end had gone,
    /// which is [#53] reappearing one page over from where story 6.5 closed it.
    ///
    /// **Both halves are asserted**, on the pattern the failed-source test set: a
    /// page that shouted about the broker whatever the state would pass the second
    /// half and be useless, so the never-connected wording is checked first.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN, output copied: dropping the sink line
    /// from `index` (`let sink_line = String::new();`, the state this page was in
    /// before this repair) goes red on the FIRST assertion, which is the one that
    /// matters — the page said nothing whatever about the broker:
    ///
    /// ```text
    /// thread 'ui::tests::the_state_screen_names_the_broker_too' panicked at
    /// crates/smartme-bridge/src/ui/mod.rs:1655:9:
    /// a bridge that never reached the broker must say so on this page too, and must
    /// not report a loss that did not happen:
    /// <!doctype html>…<p><strong>Running</strong></p><p>The bridge is configured and
    /// confirmed, so it is polling the meters and publishing what it reads. Whether
    /// what it reads is reaching the host is the broker's own line, beside this
    /// one.</p><p><a href="/config">Change the configuration</a></p>…
    /// ```
    ///
    /// The dump is the page itself, and it shows the sentence promising a line that
    /// is not there — the mutation made the detail text lie a second way.
    #[tokio::test]
    async fn the_state_screen_names_the_broker_too() {
        use crate::app::mqtt_driver::SinkHealth;
        use crate::domain::UtcMillis;
        use std::sync::Arc;

        let now = UtcMillis(1_784_984_793_000);
        let clock = Arc::new(crate::core::clock::FakeClock::new(now));
        let beats = Heartbeats::for_meters([crate::domain::MeterId::new("appart-est")]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let sink = SinkHealth::new();
        let state = ui(Phase::running(Control::detached_with_sink(
            Arc::clone(&config),
            beats,
            clock,
            sink.clone(),
        )));

        let html = body(index(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            html.contains("never connected"),
            "a bridge that never reached the broker must say so on this page too, \
             and must not report a loss that did not happen:\n{html}"
        );
        assert!(
            !html.contains("not reported here yet"),
            "and it must no longer send the operator to the log for a fact it \
             holds — that sentence was true until story 6.5 and false the moment \
             it landed:\n{html}"
        );

        sink.observed(false, UtcMillis(now.0 - 600_000));
        let html = body(index(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            html.contains("unreachable") && html.contains("10 minutes ago"),
            "the state screen must name an unreachable broker, with since when: it \
             is the page an operator opens first, and \"the bridge is publishing \
             what it reads\" is only half the truth while nothing says whether it \
             arrives:\n{html}"
        );

        sink.observed(true, UtcMillis(now.0 - 5_000));
        let html = body(index(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            html.contains("connected") && !html.contains("unreachable"),
            "and a reconnect must clear it, or the page keeps an outage alive after \
             it healed:\n{html}"
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

    /// **Story 4.11 AC4 — a reading the broker never received is on the screen too.**
    ///
    /// The third kind of fault this endpoint has had to learn to say. A meter can
    /// be polling (`wedged: false`), unrefused (`failed_sources: []`) and
    /// publishing something the oracles are happy with (`degraded_meters: []`),
    /// and still have had readings thrown away because the broker was
    /// unreachable. The wire cannot report it — what is being reported is
    /// precisely what never reached the wire — so this endpoint is the only place
    /// it can appear.
    ///
    /// **The clean case is asserted first**, because every assertion below would
    /// also hold for an endpoint that reported losses unconditionally.
    ///
    /// **And the status code must not move.** Epic 7 wires it to a container
    /// restart, and a restart provably cannot reach a broker that is down: it
    /// would loop, destroying the surface that names the fault (ADR 0027 §2).
    ///
    /// FALSIFIED 2026-08-18 — mutation RUN, output copied: making the endpoint
    /// report no losses (`.map(|f| f.dropped().into_iter().take(0).collect::<Vec<
    /// _>>())`) goes red with the whole body printed, and it is worth reading —
    /// `{"status":"running","intends_to_publish":true,"wedged":false,
    /// "failed_sources":[],"degraded_meters":[],"dropped_readings":[],…}`. That
    /// is a bridge losing readings while every field on this endpoint says it is
    /// [#90] — a disabled meter keeps its losses AND says they are history.
    ///
    /// # The two errors this sits between
    ///
    /// Clearing the counters on `retire` would erase a fact that did happen, and
    /// `dropped`'s own rule forbids exactly that erasure for exactly that reason.
    /// Leaving them bare reports an operator's deliberate gesture as an
    /// unexplained loss that merely stopped getting worse — indistinguishable, on
    /// a screen, from a fault nobody has looked at. So the number stays and the
    /// sentence is added.
    ///
    /// **Falsification, 2026-08-23:**
    ///
    /// 1. `retire` not setting `retired` — the state before this repair: RED
    ///    (`"retired":false` on a meter that was switched off).
    /// 2. `retire` clearing `dropped` instead — the other candidate the issue
    ///    named: RED, the row disappears entirely and the count is lost.
    /// 3. `record_at` not clearing `retired`: RED on the last assertion, where a
    ///    meter that publishes again is still labelled switched-off.
    #[tokio::test]
    async fn a_disabled_meter_keeps_its_losses_and_healthz_says_they_are_history() {
        use crate::app::poll_publish::DropReason;
        use crate::core::State as OracleState;
        use crate::core::clock::FakeClock;
        use crate::core::oracle::Verdict;
        use crate::domain::{MeterId, UtcMillis};
        use std::sync::Arc;

        let meter = MeterId::new("appart-est");
        let clock = Arc::new(FakeClock::new(UtcMillis(1_784_984_793_000)));
        let beats = Heartbeats::for_meters([meter.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let state = ui(publishing(
            beats.clone(),
            clock.clone(),
            Arc::clone(&config),
        ));

        beats.dropped(&meter, DropReason::OutboxFull);
        beats.retire(&meter);

        let health = body(healthz(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            health.contains(
                "{\"meter\":\"appart-est\",\"reason\":\"outbox-full\",\"count\":1,\"retired\":true,\"republications\":0}"
            ),
            "the reading WAS lost, so the count stays; what is added is that it \
             cannot rise, because the operator switched this meter off:\n{health}"
        );

        // And it comes back: a meter that publishes again is not retired,
        // whatever it was a moment ago. Without this the label latches, and a
        // re-enabled meter's live losses read as history.
        beats.record(&meter, OracleState::initial(), Verdict::good());
        let health = body(healthz(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            health.contains("\"count\":1,\"retired\":false"),
            "a meter that publishes is not switched off:\n{health}"
        );
    }

    /// well, which is the state this test exists to make impossible.
    #[tokio::test]
    async fn a_lost_reading_is_named_in_healthz_and_moves_no_status_code() {
        use crate::app::poll_publish::DropReason;
        use crate::core::clock::FakeClock;
        use crate::domain::{MeterId, UtcMillis};
        use std::sync::Arc;

        let meter = MeterId::new("appart-est");
        let clock = Arc::new(FakeClock::new(UtcMillis(1_784_984_793_000)));
        let beats = Heartbeats::for_meters([meter.clone()]);
        let config: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(running_config()));
        let state = ui(publishing(
            beats.clone(),
            clock.clone(),
            Arc::clone(&config),
        ));

        // CLEAN FIRST, or the assertions below prove nothing.
        let health = body(healthz(State(Arc::clone(&state))).await.into_response()).await;
        assert!(
            health.contains("\"dropped_readings\":[]"),
            "a fleet that has lost nothing must SAY so with an empty list, not by \
             omission — and not with six zero rows per meter:\n{health}"
        );

        // Two readings lost to a full outbox, one to a DATA that would have
        // preceded its BIRTH. Two reasons, one meter: the count is per PAIR.
        beats.dropped(&meter, DropReason::OutboxFull);
        beats.dropped(&meter, DropReason::OutboxFull);
        beats.dropped(&meter, DropReason::BeforeBirth);

        let response = healthz(State(Arc::clone(&state))).await.into_response();
        let status = response.status();
        let health = body(response).await;

        assert!(
            health.contains(
                "{\"meter\":\"appart-est\",\"reason\":\"outbox-full\",\"count\":2,\"retired\":false,\"republications\":0}"
            ),
            "the loss must name the meter, the reason and how many — an operator \
             told only that something was dropped cannot tell a full queue from an \
             unpublishable payload:\n{health}"
        );
        assert!(
            health.contains("\"reason\":\"before-birth\",\"count\":1"),
            "two reasons on one meter are two rows; collapsing them loses the \
             distinction the count exists for:\n{health}"
        );
        assert!(
            health.contains("\"failed_sources\":[]") && health.contains("\"degraded_meters\":[]"),
            "a lost reading is NEITHER a failed source nor a degraded meter, and \
             an endpoint that confused them would report a fault about a meter \
             reading perfectly well:\n{health}"
        );
        assert_eq!(
            status,
            StatusCode::OK,
            "a broker that is down must not answer 503: Epic 7 restarts the \
             container on that, and a restart cannot reach a broker — it would \
             loop, destroying the surface that names the fault"
        );
    }
}
