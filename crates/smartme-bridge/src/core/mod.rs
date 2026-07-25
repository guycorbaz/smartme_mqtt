//! Pure core (Story 1.3 onward).
//!
//! The injected [`Clock`] and [`Source`] seams live here; the pure
//! `Fresh|Stale|Failed` state machine `(prev, tick, now) -> next` and the
//! inter-task channel message type follow in Stories 1.5–1.10. PURE: no
//! async/transport imports — enforced by `tests/arch_purity.rs`. This is where the
//! "no truth is ever decided inside an `async fn`" invariant lives (`Source` is
//! async but decides nothing — it is a port reporting raw facts).
//!
//! The test doubles are deliberately NOT re-exported here: import them as
//! `core::clock::FakeClock` / `core::source::FakeSource` (their names are banned
//! outside their home files by the purity scan, so production modules cannot
//! reach for them by accident).

pub mod clock;
pub mod source;
pub mod state_machine;

pub use clock::{Clock, MonotonicMs, SystemClock};
pub use source::{Reading, Source, SourceError, Tick};
pub use state_machine::{PLAUSIBILITY_FLOOR, Policy, State};
