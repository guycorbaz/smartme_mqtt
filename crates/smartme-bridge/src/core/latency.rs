//! NFR10's latency window — what the bridge can measure about itself ([#102]).
//!
//! # Why this exists, and why it did not until 2026-08-30
//!
//! NFR10 reads *"read → accepted-for-transmission latency p95 ≤ 3 s, p99 ≤ 5 s
//! **over a 24 h window** under nominal load"*. Story 4.16 measures per-reading
//! latency end to end and asserts both thresholds — and recorded itself as
//! **unmet** on the window, because per-reading latency does not depend on how
//! long the bridge has been up, so a compressed run measures the same quantity
//! and cannot see what a day contains BESIDES readings: reconnections and their
//! backoff ladder, a broker restart, log rotation, the machine's own 03:00
//! habits.
//!
//! [#102] names the two ways out: a soak environment, or *"an operator-facing
//! latency figure on `/healthz` that makes production self-measuring"*. **The
//! second became possible on 2026-08-28**, when the bridge went into service —
//! the day-long window it lacked has existed ever since, and nothing was
//! recording it. This is the instrument.
//!
//! # What is measured, and what cannot be
//!
//! **From the response arriving to the message being accepted for transmission**,
//! on ONE monotonic clock. That is NFR10's own interval, and it is the largest
//! one a single process can measure honestly:
//!
//! - starting at the meter's own `ValueDate` would fold in the difference between
//!   its clock and ours — a quantity story 2.7 exists to distrust, and one that
//!   can make a latency negative;
//! - ending at a subscriber would need a subscriber, which is what story 4.16 has
//!   and production does not.
//!
//! So this is a LOWER bound on the interval story 4.16 measures, and the two are
//! comparable because both start where the reading enters this process.
//!
//! # Buckets, and why an approximation is the honest shape here
//!
//! A true percentile needs every sample; a day of a fleet is more samples than an
//! observability surface should hold. The counts are bucketed, and a reported
//! percentile is therefore **the upper bound of the bucket the percentile falls
//! in** — never a point. Against a 3 s budget with edges at 1, 2, 5, 10 ms and up,
//! that is precise where it matters and coarse only where nothing is at stake.
//!
//! Story 4.16 measured p95 at **0.1 % of its budget**, so the question a day-long
//! window answers is not *"is it close?"* but *"did anything happen at 03:00 that
//! a thirty-second run cannot see?"* — and a bucket boundary answers that.
//!
//! [#102]: https://github.com/guycorbaz/smartme_mqtt/issues/102

use crate::core::clock::MonotonicMs;

/// Bucket upper bounds, in milliseconds. A sample lands in the first bucket whose
/// edge is `>=` it; anything above the last edge lands in the overflow bucket.
///
/// **The last two edges are NFR10's own thresholds**, so a percentile sitting at
/// `3000` or `5000` is read against the requirement without arithmetic.
pub const EDGES_MS: [i64; 13] = [
    1, 2, 5, 10, 25, 50, 100, 250, 500, 1_000, 2_000, 3_000, 5_000,
];

/// One bucket per edge, plus the overflow.
const BUCKETS: usize = EDGES_MS.len() + 1;

/// How many hourly slots the window keeps. NFR10 says 24 h.
pub const HOURS: usize = 24;

const HOUR_MS: i64 = 3_600_000;

/// What a reported percentile means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// The percentile falls in a bucket whose upper edge is this.
    AtMost(i64),
    /// It falls in the overflow bucket: above the last edge, and this window
    /// cannot say by how much. **That is a fact worth reporting, not a gap** —
    /// against NFR10 it means the budget was exceeded.
    Above(i64),
}

/// A rolling 24-hour distribution of latencies, in hourly slots.
///
/// # Rolling by slot rather than by sample
///
/// Keeping a true sliding window needs every sample's instant. Twenty-four slots,
/// each cleared as it comes round again, give a window between 23 and 24 hours
/// wide — which is the honest cost of not keeping the samples, and is stated on
/// the surface that renders it.
#[derive(Debug, Clone)]
pub struct LatencyWindow {
    hours: [[u32; BUCKETS]; HOURS],
    current: usize,
    /// When the current slot began, on the monotonic clock.
    slot_started: MonotonicMs,
}

impl LatencyWindow {
    /// An empty window whose first slot begins now.
    pub const fn new(now: MonotonicMs) -> Self {
        Self {
            hours: [[0; BUCKETS]; HOURS],
            current: 0,
            slot_started: now,
        }
    }

    /// Records one latency, advancing the window first.
    ///
    /// `latency_ms` below zero is recorded in the lowest bucket rather than
    /// refused: the monotonic clock cannot go backwards, so a negative here is a
    /// caller error, and swallowing the sample would hide it while distorting the
    /// count that makes the figure readable.
    pub fn record(&mut self, at: MonotonicMs, latency_ms: i64) {
        self.advance_to(at);
        let index = EDGES_MS
            .iter()
            .position(|edge| latency_ms <= *edge)
            .unwrap_or(BUCKETS - 1);
        let cell = &mut self.hours[self.current][index];
        *cell = cell.saturating_add(1);
    }

    /// Moves the window on, clearing every slot that has come round again.
    ///
    /// A bridge that published nothing for two days must not report the
    /// distribution it had before the silence, so the clearing is bounded by
    /// [`HOURS`] and a long enough gap empties the window entirely.
    fn advance_to(&mut self, at: MonotonicMs) {
        let elapsed = at.0.saturating_sub(self.slot_started.0);
        if elapsed < HOUR_MS {
            return;
        }
        let slots = (elapsed / HOUR_MS).min(HOURS as i64) as usize;
        for _ in 0..slots {
            self.current = (self.current + 1) % HOURS;
            self.hours[self.current] = [0; BUCKETS];
        }
        self.slot_started = MonotonicMs(self.slot_started.0 + (elapsed / HOUR_MS) * HOUR_MS);
    }

    /// How many samples the window holds.
    pub fn count(&self) -> u64 {
        self.hours
            .iter()
            .flat_map(|h| h.iter())
            .map(|c| u64::from(*c))
            .sum()
    }

    /// How many samples exceeded the last edge.
    pub fn over_top(&self) -> u64 {
        self.hours.iter().map(|h| u64::from(h[BUCKETS - 1])).sum()
    }

    /// The `p`-th percentile, or `None` when the window holds no samples.
    ///
    /// The rule is stated because *"p95" without one is three different numbers*:
    /// the smallest bucket whose cumulative count reaches `ceil(total × p / 100)`.
    pub fn percentile(&self, p: f64) -> Option<Bound> {
        let total = self.count();
        if total == 0 {
            return None;
        }
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a count of samples in a 24 h window is far below f64's exact range"
        )]
        let target = ((total as f64) * p / 100.0).ceil() as u64;
        let mut seen = 0_u64;
        for index in 0..BUCKETS {
            seen += self.hours.iter().map(|h| u64::from(h[index])).sum::<u64>();
            if seen >= target {
                return Some(match EDGES_MS.get(index) {
                    Some(edge) => Bound::AtMost(*edge),
                    None => Bound::Above(EDGES_MS[EDGES_MS.len() - 1]),
                });
            }
        }
        // Unreachable: `target <= total` and the loop sums to `total`. Spelled as
        // the last bucket rather than a panic — an observability surface must not
        // be the thing that stops the bridge.
        Some(Bound::Above(EDGES_MS[EDGES_MS.len() - 1]))
    }
}

#[cfg(test)]
mod tests {
    use super::{Bound, EDGES_MS, HOURS, LatencyWindow};
    use crate::core::clock::MonotonicMs;

    const HOUR: i64 = 3_600_000;

    /// **The percentile rule, pinned — because "p95" without one is three
    /// different numbers.**
    ///
    /// The smallest bucket whose cumulative count reaches `ceil(total × p / 100)`,
    /// reported as that bucket's upper edge. A hundred samples of 1 ms and one of
    /// 4 s put p95 in the 1 ms bucket and p99 there too; the outlier moves only
    /// the maximum, which is what a percentile is FOR.
    ///
    /// **FALSIFIED 2026-08-30, mutation RUN — and it went red somewhere else,
    /// which is recorded rather than tidied.** `floor` in place of `ceil` is the
    /// rounding anyone reaches for first, and I predicted it would fail THIS
    /// test's `p100`. It does not: for a hundred samples `floor` and `ceil` agree.
    /// It fails the other two, at `left: Some(AtMost(1)), right: Some(AtMost(10))`
    /// and on the overflow test — because with one sample `floor(0.5) = 0`, the
    /// target becomes zero, and `seen >= 0` is satisfied by the FIRST bucket
    /// whatever it holds. The rounding error shows up on small windows, not on
    /// round ones, which is the opposite of where it was expected.
    #[test]
    fn the_percentile_rule_is_the_one_written_down() {
        let mut w = LatencyWindow::new(MonotonicMs(0));
        for _ in 0..99 {
            w.record(MonotonicMs(0), 1);
        }
        w.record(MonotonicMs(0), 4_000);

        assert_eq!(w.count(), 100);
        assert_eq!(
            w.percentile(95.0),
            Some(Bound::AtMost(1)),
            "ninety-nine samples at 1 ms put the 95th there; one outlier moves the \
             maximum and not the percentile"
        );
        assert_eq!(
            w.percentile(100.0),
            Some(Bound::AtMost(5_000)),
            "and the outlier IS in the window — it lands in the 5 s bucket, which \
             is where a reading that blew NFR10's p99 budget belongs"
        );
        assert_eq!(
            w.over_top(),
            0,
            "4 s is under the last edge, so it is not overflow"
        );
    }

    /// **A sample beyond the last edge is reported as such, never as the edge.**
    ///
    /// Against NFR10 that is the difference between *"the budget was met"* and
    /// *"the budget was exceeded and this window cannot say by how much"*. A
    /// surface that rendered the top edge for both would report a breach as a
    /// pass, which is the one thing this bridge is built not to do.
    ///
    /// **FALSIFIED 2026-08-30:** returning `AtMost` for the overflow bucket — the
    /// shape that falls out of indexing `EDGES_MS` without checking — is RED here.
    #[test]
    fn a_sample_over_the_last_edge_is_not_reported_as_the_last_edge() {
        let mut w = LatencyWindow::new(MonotonicMs(0));
        w.record(MonotonicMs(0), 60_000);
        assert_eq!(w.percentile(50.0), Some(Bound::Above(5_000)));
        assert_eq!(w.over_top(), 1);
    }

    /// **The window rolls, and a long silence empties it.**
    ///
    /// A bridge that published nothing for two days must not answer with the
    /// distribution it had before the silence: that is a stale figure presented as
    /// current, which is the shape of every defect this project keeps finding.
    ///
    /// **FALSIFIED 2026-08-30, two mutations RUN:** dropping the `min(HOURS)` cap
    /// makes a two-day gap walk the ring many times over and is RED on the last
    /// assertion by timing out the loop; and not clearing the slot on advance
    /// leaves the old samples in place — RED with `count()` still 1.
    #[test]
    fn the_window_rolls_and_a_long_silence_empties_it() {
        let mut w = LatencyWindow::new(MonotonicMs(0));
        w.record(MonotonicMs(0), 1);
        assert_eq!(w.count(), 1);

        // Twenty-three hours later the sample is still inside the window.
        w.record(MonotonicMs(23 * HOUR), 2);
        assert_eq!(w.count(), 2, "23 h is inside a 24 h window");

        // Two days of silence, then one reading: nothing older survives.
        let mut quiet = LatencyWindow::new(MonotonicMs(0));
        quiet.record(MonotonicMs(0), 1);
        quiet.record(MonotonicMs(48 * HOUR), 7);
        assert_eq!(
            quiet.count(),
            1,
            "a distribution from two days ago is not this window's answer"
        );
        assert_eq!(quiet.percentile(50.0), Some(Bound::AtMost(10)));
    }

    /// An empty window says nothing rather than zero.
    ///
    /// `0 ms` and *"nobody has published yet"* are different states, and a
    /// surface that renders the first for the second tells an operator the bridge
    /// is fast when it has not run.
    #[test]
    fn an_empty_window_has_no_percentile() {
        let w = LatencyWindow::new(MonotonicMs(0));
        assert_eq!(w.percentile(95.0), None);
        assert_eq!(w.count(), 0);
    }

    /// The edges are ordered and end on NFR10's own two thresholds, so a reported
    /// percentile is read against the requirement without arithmetic.
    #[test]
    fn the_edges_are_ordered_and_carry_the_requirement_s_own_numbers() {
        assert!(
            EDGES_MS.windows(2).all(|w| w[0] < w[1]),
            "an unordered edge list makes `position` return the wrong bucket \
             silently: {EDGES_MS:?}"
        );
        assert_eq!(
            &EDGES_MS[EDGES_MS.len() - 2..],
            &[3_000, 5_000],
            "NFR10's p95 and p99 budgets are bucket boundaries on purpose"
        );
        assert_eq!(HOURS, 24, "NFR10 says a 24 h window");
    }
}
