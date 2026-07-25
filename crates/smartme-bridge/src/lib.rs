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
