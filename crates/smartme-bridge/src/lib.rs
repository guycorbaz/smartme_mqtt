//! The smartme_mqtt bridge application library.
//!
//! Exposes the app for integration/chaos tests; `main.rs` is a thin shell over `run()`.
//! Pure modules (`domain`, `core`) never import tokio/axum/reqwest/rumqttc — enforced by
//! `tests/arch_purity.rs` (Story 0.6). `persist` is the shared atomic-write primitive
//! (Story 0.8), reused by `bdSeq` (Epic 1) and config (Epic 5).

pub mod adapters;
pub mod app;
pub mod core;
pub mod domain;
pub mod persist;
#[cfg(test)]
mod test_capture;
pub mod ui;

/// Why a phase ended.
///
/// **The process loops over its phases** (Story 6.2 AC7), so ending is no longer
/// the same thing as stopping. An operator who saves a configuration and
/// confirms the mapping in the browser must be publishing without touching a
/// terminal — which means the silent phase has to be able to end for a reason
/// other than shutdown, and say which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The process was asked to stop. Nothing follows.
    Shutdown,
    /// The operator finished configuring. The caller must **re-read the file**
    /// and decide the next phase from it.
    ConfigurationReady,
}

/// Run the publishing bridge until the process is asked to stop.
///
/// Takes the shared phase rather than a port: the web server is started once by
/// `main.rs` and outlives every phase, so what this owes it is the observability
/// a running bridge has and a silent one does not — the heartbeat, the clock and
/// the live configuration.
///
/// The heartbeat comes FROM the supervisor rather than being made here, so
/// `/healthz` reports the instant the poll loop actually recorded and not a
/// second opinion about it.
pub async fn publish_until_shutdown(
    config: app::BridgeConfig,
    phase: ui::PhaseHandle,
) -> Result<Ended, app::supervisor::StartupError> {
    app::supervisor::run_with_control(config, app::supervisor::shutdown_signal(), move |control| {
        phase.store(std::sync::Arc::new(ui::Phase::running(control)));
    })
    .await?;
    Ok(Ended::Shutdown)
}

/// Come up, stay up, and put nothing on the wire (Story 5.2 AC2, [ADR 0023] §5;
/// Story 5.3 AC1).
///
/// **Two states share this function and the sharing is deliberate**, because what
/// they need from the process is identical: a first run with no `config.toml`,
/// and a configuration whose mapping no human has confirmed. Every setting but
/// the credential arrives through a browser, so a bridge that refused to start
/// in either state could never leave it. It waits for the same shutdown signal
/// [`run`] does, so a container here stops as promptly as one in any other.
///
/// **What is NOT shared is the reason.** The caller says which state it is in,
/// because an operator who cannot tell *"I have not configured it"* from *"I
/// have not confirmed it"* cannot act: the second is one click away and the
/// first is not.
///
/// **No MQTT session is opened here.** No CONNECT, no will registered, no
/// NBIRTH: an operator watching the broker sees nothing at all until a
/// configuration exists, rather than a node announcing itself with nothing to
/// say.
///
/// It said *"and that is the assertion"* until 2026-08-04. It was not an
/// assertion — it is a structural property, and nothing tested it. What tests it
/// now is `unconfirmed_publishes_nothing.rs`, against a real broker.
///
/// **The web UI is served across this** (Story 6.1) — which is the point: these
/// are the two states in which nothing is published, and therefore the two in
/// which an operator most needs a screen to tell them why. The server itself is
/// started once by `main.rs` and outlives every phase.
///
/// **It no longer waits only for shutdown** (Story 6.2 AC7). Until 2026-08-05 it
/// could do nothing but wait to be killed, so an operator who configured and
/// confirmed in the browser was met by a bridge that kept saying *"nothing is
/// published"* until somebody restarted the container — which turned *a first
/// run completed entirely in the browser* into a first run completed in the
/// browser and a terminal.
///
/// The nudge carries **no configuration**. It says only *"look again"*; the
/// caller re-reads the file, because the file is the configuration and a loop
/// that trusted the message would be a second source of it.
///
/// [ADR 0023]: ../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md
pub async fn wait_without_publishing(ready: &tokio::sync::Notify) -> Ended {
    // No heartbeat, because there is no poll loop to have one — and `/healthz`
    // says so rather than inventing a plausible instant.
    tokio::select! {
        () = app::supervisor::shutdown_signal() => Ended::Shutdown,
        () = ready.notified() => Ended::ConfigurationReady,
    }
}
