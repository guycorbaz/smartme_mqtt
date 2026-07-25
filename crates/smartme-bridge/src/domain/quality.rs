//! The single `Quality` definition, re-exported from `sparkplug-b` (Story 1.2).
//!
//! Quality is one enum, one definition: it lives in `sparkplug-b` and the bridge
//! re-exports it — never a second enum "kept aligned", which is drift by design.
//! The bridge's error taxonomy maps into it (Epic 2 onward); no ad-hoc quality
//! strings anywhere.

pub use sparkplug_b::Quality;

#[cfg(test)]
mod tests {
    use super::Quality;

    // Compiles only if `domain::Quality` IS `sparkplug_b::Quality` — the same type,
    // not a lookalike kept aligned by hand.
    fn id(q: sparkplug_b::Quality) -> sparkplug_b::Quality {
        q
    }

    #[test]
    fn quality_is_the_sparkplug_b_type() {
        assert_eq!(id(Quality::Good), sparkplug_b::Quality::Good);
        assert_ne!(Quality::Good, Quality::Stale);
        assert_ne!(Quality::Stale, Quality::Bad);
        assert_ne!(Quality::Good, Quality::Bad);
    }
}
