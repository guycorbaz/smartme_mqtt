//! Adapters: where the pure core meets the outside world (Story 1.7 onward).
//!
//! Each adapter implements a core seam over a real dependency. Rules enforced by
//! `tests/arch_purity.rs`: the `Measurement` → Sparkplug metric mapping may live
//! ONLY in `sparkplug_publisher.rs` (Story 1.9); no truth (quality/staleness) is
//! decided here — adapters report facts, the core judges them.

pub mod smartme_source;
pub mod sparkplug_publisher;

pub use smartme_source::SmartMeCloudSource;
pub use sparkplug_publisher::{Outbound, Published, Sink, SparkplugPublisher};
