# ADR 0025 — The execution order actually followed, and the one to follow from here

- **Status:** accepted
- **Date:** 2026-08-05
- **Supersedes:** nothing. **Amends:** the execution order recorded at the Epic 1
  retrospective (2026-07-26) and restated at `epics.md:217`.
- **Issue:** [#55](https://github.com/guycorbaz/smartme_mqtt/issues/55)

## Context

The plan of record says the epics run **0 → 1 → 4 → 2 → 3 → 5 → 6 → 7 → 8**. That is not what
happened, and the discrepancy went unrecorded for a month.

What actually happened:

| Epic | Declared | Built |
|---|---|---|
| 0 | 8 stories | all 8, `done` |
| 1 | 15 stories | all 15, `done` |
| 4 | 19 stories | **4.1–4.10 only**; 4.11–4.19 are `backlog` |
| 2 | *no stories written* | nothing |
| 3 | *no stories written* | nothing |
| 5 | 3 stories (written just-in-time) | all 3, in `review` |
| 6 | 2 stories so far | both, in `review` |

So the order was departed from **twice**: Epic 4 was abandoned at 4.10, and Epics 2 and 3 were
skipped entirely.

This matters because of the project's own rule — *"anything that changes a requirement, the
wire contract, or an architectural position gets an ADR and a GitHub issue"* — which
[ADR 0016](0016-rebirth-before-primary-host-wait.md) honoured for a reordering **inside** Epic 4
(4.6/4.7 before 4.5), while a far larger reordering **between** epics went unwritten. A plan
that contradicts the repository is worse than no plan: it is a document that will be believed.

### Why it happened — the reasons are good, which is why they deserved recording

1. **The pre-production gap was not in Epic 4.** By 2026-08-01 there was no Dockerfile, no
   `/healthz`, and configuration was twelve raw environment variables. Nothing in Epics 2, 3 or
   4's tail addressed any of that; Epics 5, 6 and 7 did. Continuing down the declared order
   would have deepened a bridge that could not be deployed.
2. **Guy's decision of 2026-08-03: the web interface first, the multi-meter runtime after.**
   The configuration model went plural in Story 5.1 while the runtime stayed singular, precisely
   so that the screens could be built against the model's final shape and the fan-out cost only
   a DBIRTH later.
3. **ADR 0023 made it urgent.** Once `config.toml` became the configuration and the credential
   left the file, a browser became the *only* way to configure a fresh deployment — and the
   browser did not exist. Every day spent elsewhere was a day the product could not be brought
   up by its intended path.

None of these is a reason to hide the departure. They are the reason it was correct, and they
belong in the record rather than in three people's memory.

## Decision

**1. The order actually followed is recorded as fact**, and `epics.md` is amended to state it
rather than the order that was planned:

> `0 → 1 → 4.1–4.10 → 5 → 6 → …`

**2. From here the order is `6 → 3 → 2 → 7 → 8`**, with two insertions:

- **The five stories in `review` (5.1, 5.2, 5.3, 6.1, 6.2) are reviewed before anything new
  is built.** Nothing in Epics 5 or 6 is `done`. Four fresh-context reviews on 2026-08-04 found
  roughly thirty real defects in work that had been green in CI throughout, and Story 6.2's own
  falsification found two more — including an assertion that passed while the property it named
  was not implemented at all. Green is not a review.
- **Story 4.17 is pulled out of the Epic 4 backlog and runs before Epic 7.** It fixes a
  confirmed violation of the specification, read in the vendored norm rather than remembered:
  `tck-id-message-flow-edge-node-birth-publish-will-message-qos` (`Sparkplug_5:184`) —
  *"The Edge Node's MQTT Will Message's MQTT QoS MUST be 1"* — and the bridge publishes it at
  QoS 0, with a unit test that locks the violation in ([#26]). It runs before Epic 7 because
  Epic 7 is deployment, and **the wire is only cheap to break while nothing is in production**.
  That window closes without announcing itself. It is small and may be taken sooner.

**3. The remaining eight Epic 4 stories stay deferred, explicitly**: 4.11 (broker-outage traced
drop), 4.12 (anti-replay), 4.13/4.14 (two chaos tests), 4.15 (AC-LEAK-01), 4.18 (ADR 0010's
wording) and 4.19 (chapter 4's 29 clauses). **4.16 is not merely deferred, it is BLOCKED** and
has been since 2026-08-01: NFR10 asks for a read-to-broker-ACK latency, and ADR 0010 established
there is no ACK at QoS 0. Its measurable analogue must be agreed, not substituted quietly.

**4. The epics are still not renumbered.** `epics.md:227` rejected renumbering because seventeen
Rust doc comments, the coverage map, the manual and the issue tracker all reference epic numbers.
That reasoning is unchanged, and this ADR is the cheaper answer to the same confusion.

## Consequences

### What is thinner than the prose implies, and must not be forgotten

Skipping Epic 2 and Epic 3 is not free, and the cost is easy to lose because it is a cost in
*absence*:

- **Epic 2 owns the four runtime oracles** — unit rejection, serial-identity binding and
  verification, physical bounds, energy-counter monotonicity — plus payload completeness and
  UTC-skew handling. None exist. The bridge's founding claim is that it *never lies*, and today
  that claim rests on the freshness state machine alone. It is a narrower guarantee than the PRD
  reads as promising.
- `deferred-work.md` parks **at least fifteen items** on "Epic 2" or "Epic 3", including
  `Serial::new("")` key collisions, `TopicPath` accepting strings invalid as MQTT topics, and
  `Policy::max_age_ms` validation. Each was deferred to an epic that has since been deferred
  again. Deferring to a deferred epic is how an item stops being tracked.
- **Epic 3 owns per-meter isolation.** The runtime serves one meter
  (`config::RUNTIME_METER_LIMIT`), and a configuration enabling more is refused rather than
  truncated — which is the honest behaviour, and is not the same as the fleet working.
  *(**Superseded in part, 2026-08-06.** Epic 3 was opened and story 3.1 removed the limit:
  the runtime now serves every enabled meter. Per-meter staleness isolation, which is the
  rest of what this bullet names, remains owed — stories 3.2 and 3.3.)*

### What this changes today

Nothing in the code. This ADR is a correction to the record, which is where the defect was.

## What would reopen this

A decision to bring the fleet or the oracles forward, or evidence that an Epic 2 oracle would
have caught a defect that reached the wire. The second would be the strongest argument available
and should be treated as one.

[#26]: https://github.com/guycorbaz/smartme_mqtt/issues/26
