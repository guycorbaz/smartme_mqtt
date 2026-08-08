//! The async shell: the two tasks and the wiring that gives them life.
//!
//! Everything here is impure by design — timers, sockets, files. What it must
//! NEVER do is decide truth: the shell carries data to and from the pure core,
//! and `tests/arch_purity.rs` keeps the core clean of everything in here.

pub mod config;
pub mod mqtt_driver;
pub mod phase;
pub mod poll_publish;
pub mod reconfigure;
pub mod store;
pub mod supervisor;

pub use poll_publish::{FleetState, MeterPulse, MeterState, PollConfig};
pub use supervisor::{BridgeConfig, run};
