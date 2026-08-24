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

    /// Gives back the number [`take`](Self::take) just handed out, because the
    /// message carrying it **never reached the wire**.
    ///
    /// # This is the sharpest operation in this crate
    ///
    /// A `seq` jump is not "one message missing" to a Sparkplug host: the
    /// specification makes it a lost-message condition, so the host issues a
    /// Rebirth Request or marks the node stale. Repairing a hole that a refused
    /// message would otherwise leave is therefore worth doing — and **replaying a
    /// number that DID reach the wire is worse than the hole**, because a
    /// duplicate leaves a consumer with no reading but corruption.
    ///
    /// So there is exactly one condition under which this is sound, and a caller
    /// that cannot state it must not call this:
    ///
    /// > **a single message was in flight, and the transport refused it.**
    ///
    /// Then the number never left the process, there is no hole to leave, and the
    /// continuity a consumer sees is the truth of what was sent.
    ///
    /// It does NOT hold for a partly-refused BIRTH sequence, where some messages
    /// went out and the counter has advanced for reasons a single refusal does
    /// not undo. See [ADR 0046] for the call site this exists for, and [#88] for
    /// what answers the partial case instead.
    ///
    /// Wraps 0 → 255, symmetrically with `take`.
    ///
    /// [ADR 0046]: ../../../docs/adr/0046-a-publication-is-confirmed-by-the-transport-or-taken-back.md
    pub fn give_back(&mut self) {
        self.next = self.next.wrapping_sub(1);
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

    /// A number given back is the number the next message takes, and the wrap is
    /// symmetric with `take` ([#92], [ADR 0046]).
    ///
    /// The asymmetric version is the bug worth guarding: `next -= 1` without
    /// `wrapping_sub` panics in debug at 0 and silently under-flows in release,
    /// and 0 is reached on every 256th message of a session rather than never.
    ///
    /// **FALSIFIED 2026-08-24**, both mutations RUN:
    /// - `give_back` as a no-op — the state before ADR 0046: RED, the counter
    ///   reads 8 where 7 is owed;
    /// - `self.next = self.next.saturating_sub(1)` — the plausible one-character
    ///   difference: RED on the wrap, which stays at 0 where 255 is owed.
    #[test]
    fn a_number_given_back_is_taken_again_and_wraps_the_other_way() {
        let mut seq = SeqCounter::new();
        for _ in 0..8 {
            seq.take();
        }
        assert_eq!(seq.peek(), 8, "eight taken, the ninth is next");
        seq.give_back();
        assert_eq!(
            seq.peek(),
            7,
            "the message carrying 7 never left, so 7 is what the next one owes: a \
             consumer must see continuity, because continuity is what was sent"
        );

        let mut wrapped = SeqCounter::new();
        wrapped.give_back();
        assert_eq!(
            wrapped.peek(),
            255,
            "0 - 1 is 255 here, symmetrically with `take`'s 255 + 1 = 0 — a \
             saturating subtraction would replay 0, and 0 is the value a BIRTH \
             claims"
        );
    }

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
