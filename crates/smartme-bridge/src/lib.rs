//! The smartme_mqtt bridge application library.
//!
//! Exposes the app for integration/chaos tests; `main.rs` is a thin shell over `run()`.
//! Pure modules (`domain`, `core`) never import tokio/axum/reqwest/rumqttc — enforced by
//! `tests/arch_purity.rs` (Story 0.6). `persist` is the shared atomic-write primitive
//! (Story 0.8), reused by `bdSeq` (Epic 1) and config (Epic 5).

pub mod core;
pub mod domain;
pub mod persist;

/// Application entry point. Wiring is implemented from Epic 1 onward; this is the
/// scaffold that proves the bin -> lib -> (sparkplug-b, smart-me-client) graph.
pub fn run() {
    // The 2-task runtime is born whole in Epic 1.
}
