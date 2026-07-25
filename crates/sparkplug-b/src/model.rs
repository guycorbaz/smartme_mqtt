//! Hand-written Sparkplug model types, complementing the generated protobuf bindings.
//!
//! Holds [`Quality`] today (Story 1.2); the metric model and the seq/bdSeq lifecycle
//! extend this module as they land (Story 1.8 onward).

/// Quality of a metric value carried in a Sparkplug payload.
///
/// One canonical, three-state classification:
///
/// - [`Good`](Quality::Good) — the value is fresh and trusted.
/// - [`Stale`](Quality::Stale) — the value was valid once but is no longer current
///   (for example, the source stopped updating); consumers must not act on it as
///   live data.
/// - [`Bad`](Quality::Bad) — the value could not be read or failed validation and
///   must not be used.
///
/// Deliberately no `Default`: a quality is always an explicit decision, never a
/// silently substituted value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Fresh, trusted value.
    Good,
    /// Previously valid value that is no longer current.
    Stale,
    /// Unusable value: read failure or failed validation.
    Bad,
}
