# Story 4.16: Latency budget (NFR10)

Status: review

> **Unblocked on 2026-08-19 by Story 4.18**, which amended NFR10 from
> read→**broker-ACK** — unmeasurable, since the norm mandates QoS 0 for data and MQTT defines no
> acknowledgement there — to read→**accepted-for-transmission** ([ADR 0010]'s addendum, [#99]).
> The epic's note named two decisions its author must take at drafting rather than defer. Both are
> taken below.

## Story

As the operator,
I want a stated latency budget from reading to publication,
so that "a new reading reaches MQTT within one poll cycle" is measured rather than assumed.

## Acceptance Criteria

**AC1 — the latency is measured, from outside the process.**

**Given** a bridge publishing to a real broker
**When** readings are timed from acquisition to their arrival at an independent subscriber
**Then** p95 ≤ 3 s and p99 ≤ 5 s, over at least 300 readings.

*Decision 1, taken at drafting: **where acceptance is observed.** It is not — the test measures a
strictly LARGER interval. `try_publish` answers `Ok` on entering `rumqttc`'s request channel, a
point inside the driver that [#85] shows is not the same as leaving the socket, and exposing it
would mean adding a seam to production for a measurement. Arrival at a subscriber contains
acceptance, so `p95(read→subscriber) ≤ 3 s` implies `p95(read→accepted) ≤ 3 s`. The requirement is
discharged by a bound that cannot hold while the real one fails — and the measurement is immune to
[#85], because it does not depend on where acceptance is declared.*

**AC2 — the figures are reported, not merely asserted.**

**Given** the run
**When** it finishes
**Then** it prints p50, p95, p99, the worst sample, and each percentile as a share of its budget.

**AC3 — the 24-hour window is addressed explicitly, not silently.**

**Given** NFR10's *"over a 24 h window under nominal load"*
**When** the run takes about half a minute
**Then** the story records the window as **not covered**, with an issue, and the gate itself says
so.

*Decision 2, taken at drafting: **the thresholds are not tightened.** 4.18 left them unchanged
while making the requirement easier, and said 4.16 would decide on measurement. The measurement
is in: p95 is **0.1 % of its budget**. Tightening to fit today's figures would turn a
budget — how stale a value on a screen may be, an operator-facing promise — into a regression
detector for this machine's scheduler, which is not what NFR10 is for. Left at 3 s / 5 s,
deliberately, with the margin recorded so a future regression is visible against it.*

**AC4 — falsification, re-runnable by a reader.**

**Given** a gate that passes with three orders of magnitude to spare
**When** delay is injected
**Then** it goes red, and the injection is reachable from the environment.

## Tasks / Subtasks

- [x] **Task 1 — the harness** (AC1)
  - [x] `Stopwatch` source stamps the instant it answers, keyed by the `ValueDate` it answers
        with. The join key works because ADR 0013 puts the `ValueDate` in the published payload
        timestamp — the subscriber can name the reading it received without production carrying a
        correlation id for a test's benefit.
  - [x] Nearest-rank percentiles, with the rule stated: *"p95" without a rule is three different
        numbers*.
- [x] **Task 2 — report before asserting** (AC2)
- [x] **Task 3 — the window** (AC3) — [#102].
- [x] **Task 4 — falsification** (AC4) — `NFR10_INJECT_DELAY_MS`, with `NFR10_READINGS` so the
      demonstration does not take twenty minutes.

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-19.

### Completion Notes List

**AC1 — met, by three orders of magnitude.**

```
NFR10 — 300 readings timed from acquisition to an independent subscriber
NFR10 — p50 1.219ms, p95 1.978ms (budget 3s, 0.1 % of it), p99 2.141ms (budget 5s, 0.0 % of it),
        worst 2.539ms
```

The worst of 300 readings is 2.5 **milliseconds** against a 3 **second** p95 budget. NFR10 was
written as an operator-facing promise about staleness on a screen, and the bridge is nowhere near
it.

**AC2 — met.** Every percentile carries its share of its budget, printed before anything is
asserted.

**AC3 — met, and recorded as UNMET rather than papered over.** The 24 h window is not covered:
per-reading latency does not depend on uptime, but a day contains reconnections, a broker
restart and whatever the machine does at 03:00 — the tail a 24 h p99 exists to catch. [#102]
says so, and the gate's own header says it too, so a reader who never opens this file still
learns it. This is the same position [ADR 0038] took for the leak gate: production is the only
day-long window this project has.

**AC4 — met.** `NFR10_INJECT_DELAY_MS=4000 NFR10_READINGS=20` goes red with `p95 IS
4.003176894s, BUDGET 3s` — 133 % of budget — while p99 stays inside its own 5 s, so the failure
names the threshold it actually crossed rather than both.

**Both drafting decisions taken, neither deferred**, which is what the epic's unblocking note
required: acceptance is measured by a larger bound that contains it, and the thresholds stay
where 4.18 left them, with the margin on the record.

**No production code changed.** No conformance row moved — latency is not a Sparkplug clause.
`CONTRACT_VERSION` unchanged at 10.

### Falsification record

| # | Injection | Went red with |
|---|---|---|
| 1 | `NFR10_INJECT_DELAY_MS=4000`, 20 readings | `p95 IS 4.003176894s, BUDGET 3s` (133.4 % of it) |

### File List

- `crates/smartme-bridge/tests/nfr10_latency_budget.rs` — new
- `_bmad-output/implementation-artifacts/4-16-latency-budget.md` — new
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Change Log

- **2026-08-19** — Story 4.16, written and implemented the day 4.18 unblocked it. Latency measured
  end to end on a bound larger than the requirement; p95 at 0.1 % of budget; the 24 h window
  recorded unmet ([#102]); thresholds deliberately not tightened.

[ADR 0010]: ../../docs/adr/0010-fr20-delivery-claim-at-qos0.md
[#99]: https://github.com/guycorbaz/smartme_mqtt/issues/99
[#102]: https://github.com/guycorbaz/smartme_mqtt/issues/102
