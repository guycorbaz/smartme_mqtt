# ADR 0059 — Production is the 24-hour window, so production measures itself

- **Status:** accepted
- **Date:** 2026-08-30
- **Decides:** how NFR10's 24-hour window is observed, and what `/healthz` says about latency.
- **Issue:** [#102](https://github.com/guycorbaz/smartme_mqtt/issues/102)
- **Related:** story 4.16, ADR 0038 (the leak gate's slope, and the same absence), ADR 0027 §2,
  [#85].

## Context

NFR10 reads *"read → accepted-for-transmission latency p95 ≤ 3 s, p99 ≤ 5 s **over a 24 h window**
under nominal load"*. Story 4.16 measures per-reading latency end to end, asserts both thresholds —
and recorded itself **unmet** on the window, per the repository's rule on unmet criteria.

The reasoning it recorded is right: per-reading latency does not depend on how long the bridge has
been up, so a compressed run measures the same quantity. What it cannot see is what a day contains
*besides* readings — reconnections and their backoff ladder, a broker restart, log rotation, the
machine's own 03:00 habits.

[#102] named two ways out: a soak environment, or *"an operator-facing latency figure on `/healthz`
that makes production self-measuring — which is Epic 6/7 territory and a different decision"*.

**That decision is taken here, and one thing changed to make it worth taking: the bridge went into
service on 2026-08-28.** The day-long window it lacked has existed ever since, and nothing was
recording it. The issue's own words were already the answer — *"production is the 24 h window"* —
and what was missing was not an occasion but an instrument.

## Decision 1 — the bridge measures the interval NFR10 names, on one clock

**Read → accepted-for-transmission**, from the response arriving to the transport accepting the
message, on the monotonic clock. It is the largest interval a single process can measure honestly:

- **starting at the meter's `ValueDate`** would fold in the difference between its clock and ours —
  a quantity story 2.7 exists to distrust, and one that can make a latency negative;
- **ending at a subscriber** needs a subscriber, which is what story 4.16 has and production
  does not.

So it is a **lower bound** on what story 4.16 measures, and the two are comparable because both start
where the reading enters this process.

**Monotonic, never wall**: an NTP step mid-flight would otherwise produce a latency that never
happened, in a figure an operator reads against a requirement.

**Only readings, and only accepted ones.** A republication of a last known value is not a read —
timing it would report the age of an old measurement as this bridge's latency — so `fetched_at` is
`Option`, and `None` is a statement rather than a default nobody set. A refusal is already counted by
`dropped_readings`; timing it would answer *"how fast do we fail"* under a heading that says *"how
fast do we deliver"*.

## Decision 2 — bucketed counts, and a percentile is an upper bound

A true percentile needs every sample; a day of a fleet is more samples than an observability surface
should hold. The window is 24 hourly slots of a 14-bucket histogram, and **a reported percentile is
the upper bound of the bucket it falls in, never a point**.

The last two edges are **3000 and 5000 ms** — NFR10's own thresholds — so a figure is read against
the requirement without arithmetic. Story 4.16 measured p95 at 0.1 % of budget, so the question a
day-long window answers is not *"is it close?"* but *"did anything happen at 03:00 that a
thirty-second run cannot see?"*, and a bucket boundary answers that.

**Rolling by slot, not by sample**, which gives a window between 23 and 24 hours wide. That is the
honest cost of not keeping the samples, and it is stated on the surface that renders it. A long
enough silence empties the window: a distribution from two days ago is not an answer about now.

## Decision 3 — three states, and the surface must not confuse them

`/healthz` gains a `latency` object, and the reason it is not a bare number is that **three
situations must not read alike**:

| what happened | rendered |
|---|---|
| nothing published yet | `count: 0`, percentiles `null` |
| within budget | `p95_ms_at_most: 2` |
| **above the last bucket** | percentile `null`, `over_5000_ms` non-zero |

The third is the dangerous one. The window cannot say by how much a breach exceeded the budget, and
printing the top edge there would **report a budget exceeded as a budget met** — the one thing this
bridge is built not to do. It renders `null`, and the overflow count is what distinguishes that
`null` from the empty one. The guard is falsified against exactly that rendering.

## Consequences

- **NFR10's window becomes observable where it actually exists.** [#102] closes on the second of the
  two routes it named, not on a soak environment.
- **ADR 0038's absence is unchanged.** The leak gate's 24-hour slope needs a place to run for a day
  under observation; this measures a distribution the running bridge produces, which is a different
  thing. That ADR's *"what would reopen this"* still applies and is not discharged here.
- **The figure is a floor on the interval**, for the same reason the drop counts are a floor on the
  loss: `try_publish` answers on entering the client's queue, not on leaving the socket ([#85]). The
  manual says so in the same words, beside both.
- **`/healthz` gains a field**, additively: a reader that does not know it is unaffected.
- **`MeterUpdate` carries an optional instant**, and `SinkHealth` carries the window beside the
  transport's state — because a latency figure with no idea whether the transport is up invites the
  reading it must not get, *slow* where the answer is *disconnected*.
