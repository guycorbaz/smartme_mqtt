//! Pure core (Story 0.6 scaffold).
//!
//! The injected `Clock` and `Source` seams, the pure `Fresh|Stale|Failed` state machine
//! `(prev, tick, now) -> next`, and the inter-task channel message type are implemented
//! from Epic 1 onward. PURE: no async/transport imports — enforced by `tests/arch_purity.rs`.
//! This is where the "no truth is ever decided inside an `async fn`" invariant lives.
