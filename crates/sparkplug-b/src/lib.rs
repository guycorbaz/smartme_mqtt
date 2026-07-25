//! Pure, generic Sparkplug B library.
//!
//! The checked-in `proto/sparkplug_b.proto` (the wire contract of record) is compiled by
//! `build.rs` via `prost` (Story 0.2). The seq/bdSeq/rebirth state machine and the
//! BIRTH/DATA/DEATH lifecycle are implemented from Epic 1 / Epic 3 onward. This crate is
//! self-contained — no application or bridge dependency — so it can be published to
//! crates.io independently (guarded by `tests/no_context_leak.rs`).
#![forbid(unsafe_code)]

pub mod model;

pub use model::Quality;

/// Generated Sparkplug B protobuf types (from `proto/sparkplug_b.proto`).
///
/// The module path mirrors the proto `package org.eclipse.tahu.protobuf`.
pub mod protobuf {
    include!(concat!(env!("OUT_DIR"), "/org.eclipse.tahu.protobuf.rs"));
}
