//! Pure, generic Sparkplug B library.
//!
//! The checked-in `proto/sparkplug_b.proto` (the wire contract of record) is compiled by
//! `build.rs` via `prost` (Story 0.2). On top of it this crate provides a small hand-written
//! model ([`Metric`], [`MetricValue`], [`Quality`]) and the session lifecycle
//! ([`NodeSession`] / [`LiveSession`]: BIRTH / DATA / DEATH with `seq` and `bdSeq`). This
//! crate is self-contained — no application dependency — so it can be published to
//! crates.io independently (guarded by `tests/no_context_leak.rs`).
//!
//! # Conformance scope
//!
//! This crate builds and encodes payloads. It does NOT make a node conformant on
//! its own, and deliberately does not claim to:
//!
//! **Implemented:** node-level BIRTH/DATA/DEATH payload construction, protobuf
//! encoding, the wrapping per-message `seq`, the per-session `bdSeq` metric,
//! explicit null metrics, and metric properties for quality and engineering
//! units.
//!
//! **Implemented (continued):** device-level BIRTH/DATA/DEATH sharing the edge
//! node's sequence numbering, and validated topic construction ([`EdgeNode`]).
//!
//! **Not implemented — a conformant node must supply these itself:** the
//! `Node Control/Rebirth` metric and the command (N/DCMD) path that acts on it,
//! host STATE handling, metric aliases, templates and datasets, and the QoS /
//! retain flags of each publication (a caller owns its transport).
//!
//! # Public dependency
//!
//! The payload type returned by the builders is `prost`-generated and re-exported
//! from [`mod@protobuf`]: `prost` is a PUBLIC dependency, so a major `prost` bump
//! is a breaking change for downstream crates too.
//!
//! # Example
//!
//! ```
//! use sparkplug_b::{encode, BdSeq, Metric, MetricValue, NodeSession, Quality};
//!
//! // Continue the numbering from whatever was persisted before the restart.
//! let session = NodeSession::start(BdSeq::new(7));
//!
//! // The death is built FIRST: it is registered as the connection's last will.
//! let will = session.will(1_700_000_000_000);
//!
//! let energy = Metric::new("Energy", MetricValue::Double(4_843.822), 1_700_000_000_000)
//!     .with_quality(Quality::Good)
//!     .with_engineering_unit("kWh");
//! let (mut live, birth) = session.birth(1_700_000_000_000, vec![energy]);
//!
//! assert_eq!(birth.seq, Some(0));
//! assert_eq!(will.seq, None);
//! let bytes = encode(&birth);
//! # assert!(!bytes.is_empty());
//! ```
#![forbid(unsafe_code)]

pub mod datatype;
pub mod encode;
pub mod model;
pub mod seq;
pub mod topic;

pub use datatype::DataType;
pub use encode::{LiveSession, NodeSession, decode, encode};
pub use model::{Metric, MetricValue, Quality};
pub use seq::{BD_SEQ_METRIC, BdSeq, SeqCounter};
pub use topic::{EdgeNode, MessageType, NAMESPACE, TopicError};

/// Generated Sparkplug B protobuf types (from `proto/sparkplug_b.proto`).
///
/// The module path mirrors the proto `package org.eclipse.tahu.protobuf`.
pub mod protobuf {
    include!(concat!(env!("OUT_DIR"), "/org.eclipse.tahu.protobuf.rs"));
}
