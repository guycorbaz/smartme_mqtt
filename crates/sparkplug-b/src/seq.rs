//! Sequence numbering: the per-message `seq` and the per-session `bdSeq`.
//!
//! These two counters are what let a consumer detect that it missed something.
//! They are deliberately separate types with no arithmetic between them — mixing
//! them up is the kind of bug that produces a silently incomplete tag history.

/// The per-message sequence number: starts at 0, increments on every message of
/// a session, and wraps 255 → 0.
///
/// A BIRTH message resets it: the specification requires a BIRTH to carry
/// `seq = 0`, which is what tells a consumer that the numbering restarted rather
/// than that it lost 200 messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeqCounter {
    next: u8,
}

impl SeqCounter {
    /// A counter positioned at 0 (the value a BIRTH must carry).
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    /// The value the next message will carry, without consuming it.
    pub const fn peek(self) -> u64 {
        self.next as u64
    }

    /// Takes the next sequence number and advances, wrapping 255 → 0.
    pub fn take(&mut self) -> u64 {
        let current = self.next;
        self.next = self.next.wrapping_add(1);
        current as u64
    }

    /// Restarts the numbering at 0 (a new BIRTH).
    pub fn reset(&mut self) {
        self.next = 0;
    }
}

impl Default for SeqCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// The birth/death sequence number identifying one session.
///
/// It appears as a metric in both the BIRTH and the matching DEATH of a session,
/// so a consumer can pair them and ignore a DEATH belonging to a session that
/// has already been superseded. It increments on every new connection attempt
/// and wraps after 255, matching the specification's stated range.
///
/// It must SURVIVE a process restart (it is the only thing distinguishing "the
/// same node reconnecting" from "a stale death"), so it is plain `Copy` data
/// that the caller persists — this crate does no I/O.
///
/// **There is no "before the first session" value, and there was one until
/// 2026-08-22.** `BdSeq::before_first()` returned 0 and
/// [`NodeSession::start`](crate::NodeSession::start) advanced past it, so a node
/// that had never connected published **1** in its first BIRTH — while
/// `tck-id-topics-nbirth-bdseq-increment` requires the number to *start at
/// zero*. The absence of a previous session is not a number; it is now
/// [`Option::None`] at the one place that can know it ([#100], ADR 0042).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BdSeq(u8);

impl BdSeq {
    /// Restores a persisted value. Values outside 0–255 cannot be represented,
    /// so a corrupt persisted number is truncated by the caller's own parsing
    /// rather than silently accepted here.
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    /// The number to persist and to publish.
    pub const fn value(self) -> u8 {
        self.0
    }

    /// The value carried on the wire (the metric is a 64-bit integer).
    pub const fn wire_value(self) -> i64 {
        self.0 as i64
    }

    /// The next session's number, wrapping after 255.
    pub const fn next_session(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// The metric name under which [`BdSeq`] travels in BIRTH and DEATH payloads.
pub const BD_SEQ_METRIC: &str = "bdSeq";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_starts_at_zero_and_increments() {
        let mut s = SeqCounter::new();
        assert_eq!(s.peek(), 0);
        assert_eq!(s.take(), 0);
        assert_eq!(s.take(), 1);
        assert_eq!(s.take(), 2);
        assert_eq!(s.peek(), 3);
    }

    #[test]
    fn seq_wraps_255_to_0() {
        let mut s = SeqCounter::new();
        for _ in 0..255 {
            s.take();
        }
        assert_eq!(s.take(), 255);
        assert_eq!(s.take(), 0, "the wrap is 255 -> 0, never 256");
        assert_eq!(s.take(), 1);
    }

    #[test]
    fn reset_restarts_at_zero_for_a_birth() {
        let mut s = SeqCounter::new();
        for _ in 0..10 {
            s.take();
        }
        s.reset();
        assert_eq!(s.peek(), 0);
    }

    #[test]
    fn bdseq_increments_per_session_and_wraps() {
        let a = BdSeq::new(0);
        assert_eq!(a.value(), 0);
        let b = a.next_session();
        assert_eq!(b.value(), 1);
        let last = BdSeq::new(255);
        assert_eq!(last.next_session().value(), 0, "wraps after 255");
    }

    #[test]
    fn bdseq_survives_a_round_trip_through_persistence() {
        let persisted = BdSeq::new(42).value();
        assert_eq!(BdSeq::new(persisted), BdSeq::new(42));
        assert_eq!(BdSeq::new(42).wire_value(), 42_i64);
    }
}
