//! Pure core (Story 1.3 onward).
//!
//! The injected [`Clock`] seam lives here; the `Source` seam, the pure
//! `Fresh|Stale|Failed` state machine `(prev, tick, now) -> next`, and the
//! inter-task channel message type follow in Stories 1.4–1.10. PURE: no
//! async/transport imports — enforced by `tests/arch_purity.rs`. This is where the
//! "no truth is ever decided inside an `async fn`" invariant lives.
//!
//! The `FakeClock` test double is deliberately NOT re-exported here: import it as
//! `core::clock::FakeClock` (its name is banned outside `core/clock.rs` by the
//! purity scan, so production modules cannot reach for it by accident).

pub mod clock;

pub use clock::{Clock, MonotonicMs, SystemClock};
