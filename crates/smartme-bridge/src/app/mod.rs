//! The async shell: the two tasks and the wiring that gives them life.
//!
//! Everything here is impure by design — timers, sockets, files. What it must
//! NEVER do is decide truth: the shell carries data to and from the pure core,
//! and `tests/arch_purity.rs` keeps the core clean of everything in here.

pub mod mqtt_driver;
pub mod poll_publish;
pub mod supervisor;

pub use poll_publish::{LastLoopTick, PollConfig};
pub use supervisor::{BridgeConfig, run};
