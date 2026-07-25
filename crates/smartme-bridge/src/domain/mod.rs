//! Pure domain model (Story 1.2).
//!
//! The canonical [`Measurement`], the physical-quantity newtypes ([`Kw`], [`Kwh`]),
//! the identifier newtypes ([`MeterId`], [`Serial`], [`TopicPath`]), the timestamp
//! newtype [`UtcMillis`], and the single [`Quality`] definition (re-exported from
//! `sparkplug-b`). This module is PURE: it must never import
//! tokio/axum/reqwest/rumqttc — enforced by `tests/arch_purity.rs`.

pub mod measurement;
pub mod quality;

pub use measurement::{Kw, Kwh, Measurement, MeterId, Serial, TopicPath, UtcMillis};
pub use quality::Quality;
