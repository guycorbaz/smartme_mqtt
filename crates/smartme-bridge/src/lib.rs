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

/// Application entry point: build the runtime and run until the process is
/// asked to stop. `main.rs` is a thin shell over this.
pub fn run(config: app::BridgeConfig) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(app::run(config, app::supervisor::shutdown_signal()))?;
    Ok(())
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
/// **No MQTT session is opened here, and that is the assertion.** No CONNECT, no
/// will registered, no NBIRTH: an operator watching the broker sees nothing at
/// all until a configuration exists, rather than a node announcing itself with
/// nothing to say.
///
/// The web UI will be served from this function when Epic 6 lands. Until then it
/// waits and does nothing, which is the same behaviour minus the listener.
///
/// [ADR 0023]: ../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md
pub fn run_without_publishing() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(app::supervisor::shutdown_signal());
    Ok(())
}
