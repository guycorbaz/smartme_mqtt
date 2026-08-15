//! smart-me REST client (Story 1.6): auth, `GET /Devices/{id}`, and the
//! `Date`-header capture the freshness oracle feeds on.
//!
//! This crate carries zero bridge dependency; its heavy deps (reqwest/serde) are
//! isolated here. Transport is TLS-only (NFR13, enforced at construction AND via
//! `https_only`). The crate is clock-free: token expiry travels as plain data and
//! is judged by the caller's injected `Clock`. Errors are this crate's own types —
//! no leaked reqwest/serde types (the bridge maps them to its transient/fatal
//! taxonomy). Deserialization is pinned to the REAL captured payload
//! (`fixtures/smartme_sample.json`, the contract-of-record) by `tests/fixtures_shape.rs`
//! and `tests/contract_of_record.rs`.

pub mod client;
pub mod http_date;
pub mod types;

pub use client::{Credentials, DeviceCapture, DeviceList, SmartMeClient, SmartMeError, TokenState};
pub use http_date::parse_imf_fixdate;
pub use types::{Device, DeviceListing, parse_value_date};
