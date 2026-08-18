# Story 4.12: Anti-replay at the down→up instant

Status: ready-for-dev

> **READ THIS BEFORE PLANNING ANY CODE.** The property this story is named after is **already
> implemented**, in two places, with its reasoning written beside it. What does not exist is any
> proof — **not one test in this repository asserts a published Sparkplug timestamp**, and
> `chaos_sigterm_no_lie` says so in its own words: *"does not compare timestamps"*.
>
> So this is a **verification story**. Its deliverable is evidence, and the failure mode to
> avoid is writing code to make a green test greener. If a task below turns out to need
> production changes, that is a finding — record it, do not quietly widen the story.

## Story

As the SCADA,
I want nothing re-timestamped when the broker comes back,
so that an outage reads as a gap rather than as a burst of fresh data.

## Acceptance Criteria

**AC1 — the invariant, verified at the reconnection instant.**

**Given** a broker that returns after an outage
**When** the bridge reconnects and rebirths
**Then** every published Sparkplug timestamp equals its source `ValueDate` — verified **at the
reconnection instant**, not merely in steady state
**And** no reading acquired during the outage is published with a post-outage timestamp.

**AC2 — the rebirth re-declares history as history.**

**Given** the rebirth that follows reconnection
**When** it re-declares the last known reading
**Then** that reading is degraded to `Stale` and stamped with its own `ValueDate`.

**AC3 — the split between "a reading's time" and "now" is pinned, per message type.**

**Given** the seven message types this bridge publishes
**When** each is built
**Then** a test states which carry a **reading's** `ValueDate` and which legitimately carry
**now**, and fails if any message moves from one column to the other
**And** the table names, for each row, *why* — a node event is not a measurement.

*Decided at drafting, 2026-08-18. Without AC3 the invariant is a property of two call sites
that a third could silently break: `metrics_for` and the rebirth path both reach for
`value_date` today, and nothing would notice a new message type reaching for `now`. This is
story 4.17's delivery-table pattern applied to timestamps — the table that turned a QoS
violation from invisible into red.*

**AC4 — falsification.**

**Given** each new assertion
**When** the stamping it names is deliberately broken
**Then** the test goes red, and the run's output is copied next to the test.

## What is already true, and where

Read these before writing anything. **Both were delivered by earlier stories and neither is
this story's work to redo.**

| Property | Where | Since |
|---|---|---|
| A DDATA is stamped with the reading's `ValueDate` | `sparkplug_publisher.rs:521` — `millis(update.measurement.value_date)` | story 1.9 |
| Each metric inside it, likewise | `metrics_for`, `:655` | story 1.9 |
| A rebirth re-declares the last reading **degraded to `Stale`** | `:384` — `update.verdicts.map(degrade)`, and `degrade` maps `Good → Stale(NotRevalidated)` | story 4.7 |
| …stamped with **its own** `ValueDate`, not `now` | `:390` — `payload_ts` | story 4.7 |
| A device with no reading yet births `cold_start_metrics(now)` | `:385`, `:447` | story 1.9 |

The code already carries the argument in a comment: *"Claiming `Good` here would turn a
45-minute broker outage into a fresh-looking lie the moment the link came back."* That sentence
is AC2. **This story's job is to make it falsifiable.**

**AC1's second clause is now true by construction too, and for a reason that is only three
hours old.** Story 4.11 replaced the blocking hand-over with `try_send`, so a reading acquired
while the driver is not draining is **dropped and counted**, never queued for later. What can
still be in flight is the ≤64 already in the inbox; those are published — with **their own**
`ValueDate`, which is a pre-outage timestamp on a post-outage message, and that is exactly
right. Assert it; do not "fix" it.

## Tasks / Subtasks

- [ ] **Task 1 — the message-type timestamp table** (AC3)
  - [ ] In `sparkplug_publisher.rs`, add `timestamp_source_for(MessageType) -> TimestampSource`
        with two variants (`ReadingValueDate`, `PublicationInstant`) and a doc comment giving
        the reason per row. Pure, no session state — the same shape as `qos_for` in
        `mqtt_driver.rs`, which exists for exactly this class of drift.
  - [ ] Test `the_timestamp_table_says_which_clock_each_message_speaks`: NBIRTH, NDEATH,
        DDEATH and a **cold-start** DBIRTH carry the publication instant (they are node and
        session events, not measurements); DDATA and a **re-declaring** DBIRTH carry the
        reading's `ValueDate`.
  - [ ] **Cite the norm, do not paraphrase it.** `tck-id-payloads-*-timestamp` clauses govern
        the payload `timestamp` field; read them in `docs/spec/sparkplug-b-3.0.0/` and quote the
        `tck-id`. ADR 0013 already decided the payload-timestamp question — read it first and do
        not re-decide it.

- [ ] **Task 2 — the invariant, exhaustively, at the publisher level** (AC1, AC2)
  - [ ] Unit test over `SparkplugPublisher`: birth → DDATA → **new session** → rebirth, driven
        by a `FakeClock` whose wall time **advances by an hour** between the reading and the
        rebirth. Assert the re-declared DBIRTH's payload timestamp is still the reading's
        `ValueDate` and **not** the advanced `now`.
  - [ ] The clock advance is the whole test. With a clock that does not move, a publisher
        stamping `now` and one stamping `value_date` are indistinguishable — that is the fake
        clock that never advanced, one of the four Epic 1 tests this repository threw away.
  - [ ] Assert the re-declared metrics are `Stale` with cause `not-revalidated`, and that a
        reading already `Bad` keeps its own cause rather than being flattened to `Stale`
        (`degrade` only touches `Good`).

- [ ] **Task 3 — the reconnection instant, from outside the process** (AC1)
  - [ ] Extend `chaos_bdseq_per_connect.rs`, or add a sibling, using **the saboteur already
        there**: a second client stealing the client id forces the broker to disconnect the
        bridge, so a reconnect happens without stopping the broker.
  - [ ] An independent subscriber records the whole sequence and asserts: **no published
        timestamp post-dates its own reading**, across the death, the reconnect and the rebirth.
  - [ ] **Boundary with story 4.13, stated so it is not blurred:** 4.13 (`chaos_broker_recovery`)
        owns the proof with a broker **container stopped and restarted** — a transport loss.
        This task uses a **session takeover**, which is a different code path to the same
        reconnect. Neither replaces the other, and 4.13 must not be cited here as evidence.

- [ ] **Task 4 — falsification** (AC4)
  - [ ] Mutation: `payload_ts` → `timestamp` in the rebirth path. Task 2's test must redden.
  - [ ] Mutation: `millis(update.measurement.value_date)` → `millis(now)` in `publish`.
  - [ ] Mutation: remove `.map(degrade)` from the rebirth metrics.
  - [ ] Mutation: move one row of Task 1's table to the other column.
  - [ ] Run each, copy the output next to the test. A note written before its run is not a
        falsification — three of story 4.11's thirteen were wrong and had to be rewritten
        against the real output.

- [ ] **Task 5 — the record**
  - [ ] `docs/sparkplug-conformance.md`: the `-timestamp` rows may move from `gap (unproven)` to
        `conformant` **only** for clauses this story's tests actually witness. Count them; do
        not move a row on a neighbouring test's strength.
  - [ ] `CONTRACT_VERSION` is **not** bumped — nothing about the payload changes.

## Dev Notes

### The one thing that might not be verification

**[#92] sits inside AC2's subject.** `SparkplugPublisher::publish` writes
`self.declared.insert(serial, Some(update))` **before** the sink is drained, so a reading the
transport then refuses is nonetheless what a later rebirth re-declares as last-published.

**Position taken at drafting, so the story does not stall on it:** the rebirth re-declares the
last reading the bridge **judged**, not the last it **delivered**, and that is correct. A
rebirth exists to tell the host what the bridge currently believes; the value is stamped with
its own `ValueDate` and degraded to `Stale`, so nothing is presented as fresh, and re-declaring
a reading the host missed is the *repair*, not the defect. **What is wrong is only the
disagreement between two surfaces** — `/healthz` calls that reading lost while the publisher
calls it last-published. That is [#92]'s to resolve, and it is a wording-and-record question
rather than a stamping one. **Do not change `publish`'s ordering in this story.**

### What must not break

- **ADR 0013** decided the payload-timestamp question. Read it before Task 1 and do not
  re-decide it under a new name.
- **`node_metrics(timestamp)` at `:555`** stamps the NBIRTH's own metrics with the publication
  instant, and that is right: `bdSeq`, the contract version and the Rebirth command are
  properties of the *session*, not measurements. Task 1's table must say so rather than treating
  it as an exception.
- **`millis()` clamps a pre-epoch instant to 0** (`:731`). A test feeding a negative
  `ValueDate` proves the clamp, not the invariant — keep the two apart.
- **The fake clock must advance.** See Task 2.

### Previous story intelligence (4.11, closed 2026-08-18 after a three-layer review)

- **A test that cannot fail is worse than no test**, because it is scored as coverage. 4.11
  shipped `assert_eq!(dropped.len(), dropped.len())` on a fixed-size array — `6 == 6` for every
  value of every program — as the discharge of an acceptance criterion. Before writing any
  assertion here, ask what value of the code would make it red.
- **A positive control is half the test.** 4.11's saturation test could not tell saturation from
  a no-op. Task 2's clock advance is this story's positive control; without it the test proves
  nothing.
- **Claims in doc comments get audited.** 4.11 shipped *"is asserted at the call site's own
  arm"* about an assertion that did not exist. If a comment here says something is proven, the
  proof must be nameable.
- **Count what you claim.** 4.11's record said "eight tests" when there were nine, and "seven
  crates" when there were six.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md:1150`] — Story 4.12, both original ACs
- [Source: `_bmad-output/planning-artifacts/epics.md:141`] — AR7's anti-replay invariant (amended by ADR 0036)
- [Source: `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs:521`] — the DDATA stamp
- [Source: `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs:378`] — the rebirth re-declaration, `degrade` and `payload_ts`
- [Source: `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs:731`] — `millis`, and the pre-epoch clamp
- [Source: `crates/smartme-bridge/src/app/mqtt_driver.rs`] — `qos_for` and its clause table, the shape Task 1 copies
- [Source: `crates/smartme-bridge/tests/chaos_bdseq_per_connect.rs:59`] — the saboteur, Task 3's harness
- [Source: `docs/adr/0013-*.md`] — the payload-timestamp decision
- [Source: `docs/spec/sparkplug-b-3.0.0/`] — the pinned norm; cite `tck-id-…`, never prose
- [Source: `CLAUDE.md`] — falsify before trusting; read the norm first

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
