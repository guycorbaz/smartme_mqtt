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
//! **Implemented (continued, 2026-08-29):** the `datatype` field is carried by
//! the message types that declare a tag set — NBIRTH, DBIRTH and both deaths —
//! and omitted from the four the specification names in
//! `tck-id-payloads-metric-datatype-not-req`. **A caller gets this from the
//! builder it chooses**, not from a flag: `birth`, `rebirth` and `device_birth`
//! declare, `data` and `device_data` do not. The consequence is worth stating
//! because it is a caller's problem and not this crate's: a consumer that missed
//! the BIRTH cannot learn a metric's type from the data stream, and a null metric
//! in a DATA message carries no type at all — so whoever owns the transport owes
//! that consumer a rebirth path.
//!
//! **Implemented (continued):** the NCMD topic form, via
//! [`MessageType::NCmd`] — construction only. Subscribing to it, and the QoS 1
//! that clause mandates, belong to whoever owns the transport.
//!
//! **Not implemented — a conformant node must supply these itself:** the
//! `Node Control/Rebirth` metric and the command (N/DCMD) path that acts on it,
//! host STATE handling, metric aliases, templates and datasets, and the QoS /
//! retain flags of each publication (a caller owns its transport). `DCMD` has
//! no [`MessageType`] variant at all: its subscribe clause is conditional on a
//! device supporting writable outputs, so the topic form is added when a caller
//! needs it rather than in advance.
//!
//! **Re-checked and CONFIRMED unchanged (2026-07-30), which is the interesting
//! part.** A downstream application has since implemented the
//! `Node Control/Rebirth` metric and the handler that answers a rebirth request
//! — and it needed no change here at all. The
//! list above is a statement about what THIS CRATE supplies, and the answer is
//! still "not those": the metric is an application-level declaration assembled
//! from [`Metric`] and [`MetricValue::Boolean`], and the handler that acts on it
//! belongs to whoever owns the transport. A caller that wants a conformant node
//! still has to supply both. Recorded rather than silently left alone, because
//! "this sentence is still true" and "nobody re-read this sentence" look
//! identical in a diff.
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
//! let session = NodeSession::start(Some(BdSeq::new(7)));
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
pub use encode::{DecodeError, LiveSession, NodeSession, decode, encode};
pub use model::{Metric, MetricValue, Quality};
pub use seq::{BD_SEQ_METRIC, BdSeq, SeqCounter};
pub use topic::{EdgeNode, MessageType, NAMESPACE, TopicError};

/// Generated Sparkplug B protobuf types (from `proto/sparkplug_b.proto`).
///
/// The module path mirrors the proto `package org.eclipse.tahu.protobuf`.
pub mod protobuf {
    include!(concat!(env!("OUT_DIR"), "/org.eclipse.tahu.protobuf.rs"));
}

/// The README's example, compiled and run with the crate's doctests.
///
/// **Story 8.2's review, 2026-08-21.** The README shipped an example that called a
/// constructor this crate does not have (`NodeSession::new`) and passed two of
/// `Metric::new`'s three arguments. It was the crate's front page on crates.io and
/// the one piece of code here that nothing compiled — the README is a file, not a
/// module, so `cargo test` never saw it.
///
/// `cfg(doctest)` means this item exists only while the doctests are being
/// collected, so the README's prose is not also duplicated into the rendered
/// documentation above.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct Readme;
