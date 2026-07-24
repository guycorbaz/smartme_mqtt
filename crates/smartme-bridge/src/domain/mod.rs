//! Pure domain model (Story 0.6 scaffold).
//!
//! The canonical `Measurement`, the physical-quantity newtypes (`Kw`, `Kwh`), `Serial`,
//! `MeterId`, `TopicPath`, and the `Quality` enum are implemented from Epic 1 onward.
//! This module is PURE: it must never import tokio/axum/reqwest/rumqttc — enforced by
//! `tests/arch_purity.rs`.
