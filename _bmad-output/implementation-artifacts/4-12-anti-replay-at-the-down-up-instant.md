# Story 4.12: Anti-replay at the down→up instant

Status: review

> **READ THIS BEFORE PLANNING ANY CODE.** The property this story is named after is **already
> implemented**, in two places, with its reasoning written beside it.
>
> **CORRECTION, made during implementation and left here rather than tidied away.** This
> paragraph claimed *"not one test in this repository asserts a published Sparkplug timestamp"*.
> That is **false**, and reading the code rather than the prose is what showed it:
> `a_rebirth_redeclares_what_is_known_instead_of_blanking_it` has asserted the re-declared
> DBIRTH's timestamp since story 4.7, and the conformance matrix cites two more. What was
> actually missing is narrower and still worth the story: **no INTEGRATION test asserted one**
> (`chaos_sigterm_no_lie` says so in its own words — *"does not compare timestamps"*), the NBIRTH
> row was a **presence** check that [#30] said any constant would satisfy, and nothing exercised
> a clock that had genuinely moved.
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

- [x] **Task 1 — the message-type timestamp table** (AC3)
  - [x] In `sparkplug_publisher.rs`, add `timestamp_source_for(MessageType) -> TimestampSource`
        with two variants (`ReadingValueDate`, `PublicationInstant`) and a doc comment giving
        the reason per row. Pure, no session state — the same shape as `qos_for` in
        `mqtt_driver.rs`, which exists for exactly this class of drift.
  - [x] Test `the_timestamp_table_says_which_clock_each_message_speaks`: NBIRTH, NDEATH,
        DDEATH and a **cold-start** DBIRTH carry the publication instant (they are node and
        session events, not measurements); DDATA and a **re-declaring** DBIRTH carry the
        reading's `ValueDate`.
  - [x] **Cite the norm, do not paraphrase it.** `tck-id-payloads-*-timestamp` clauses govern
        the payload `timestamp` field; read them in `docs/spec/sparkplug-b-3.0.0/` and quote the
        `tck-id`. ADR 0013 already decided the payload-timestamp question — read it first and do
        not re-decide it.

- [x] **Task 2 — the invariant, exhaustively, at the publisher level** (AC1, AC2)
  - [x] Unit test over `SparkplugPublisher`: birth → DDATA → **new session** → rebirth, driven
        by a `FakeClock` whose wall time **advances by an hour** between the reading and the
        rebirth. Assert the re-declared DBIRTH's payload timestamp is still the reading's
        `ValueDate` and **not** the advanced `now`.
  - [x] The clock advance is the whole test. With a clock that does not move, a publisher
        stamping `now` and one stamping `value_date` are indistinguishable — that is the fake
        clock that never advanced, one of the four Epic 1 tests this repository threw away.
  - [x] Assert the re-declared metrics are `Stale` with cause `not-revalidated`, and that a
        reading already `Bad` keeps its own cause rather than being flattened to `Stale`
        (`degrade` only touches `Good`).

- [x] **Task 3 — the reconnection instant, from outside the process** (AC1)
  - [x] Extend `chaos_bdseq_per_connect.rs`, or add a sibling, using **the saboteur already
        there**: a second client stealing the client id forces the broker to disconnect the
        bridge, so a reconnect happens without stopping the broker.
  - [x] An independent subscriber records the whole sequence and asserts: **no published
        timestamp post-dates its own reading**, across the death, the reconnect and the rebirth.
  - [x] **Boundary with story 4.13, stated so it is not blurred:** 4.13 (`chaos_broker_recovery`)
        owns the proof with a broker **container stopped and restarted** — a transport loss.
        This task uses a **session takeover**, which is a different code path to the same
        reconnect. Neither replaces the other, and 4.13 must not be cited here as evidence.

- [x] **Task 4 — falsification** (AC4)
  - [x] Mutation: `payload_ts` → `timestamp` in the rebirth path. Task 2's test must redden.
  - [x] Mutation: `millis(update.measurement.value_date)` → `millis(now)` in `publish`.
  - [x] Mutation: remove `.map(degrade)` from the rebirth metrics.
  - [x] Mutation: move one row of Task 1's table to the other column.
  - [x] Run each, copy the output next to the test. A note written before its run is not a
        falsification — three of story 4.11's thirteen were wrong and had to be rewritten
        against the real output.

- [x] **Task 5 — the record**
  - [x] `docs/sparkplug-conformance.md`: the `-timestamp` rows may move from `gap (unproven)` to
        `conformant` **only** for clauses this story's tests actually witness. Count them; do
        not move a row on a neighbouring test's strength.
  - [x] `CONTRACT_VERSION` is **not** bumped — nothing about the payload changes.

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

Claude Opus 5 (1M context) — `claude-opus-5[1m]`

### Completion Notes List

**All four ACs met, and the story's own premise had to be corrected on the way** — see the
header. It claimed no test asserted a published timestamp; reading the code showed three did.
What was genuinely missing was the integration proof, a value check on the NBIRTH, and a clock
that had moved.

**Reading the norm changed the shape of AC3, and this is the rule earning its keep.** The
specification says the payload timestamp MUST denote *"the time at which the message was
published"* — in identical words for NBIRTH, DBIRTH, NDATA, DDATA and DDEATH — and puts
acquisition time in the **metric** timestamp. This bridge deviates on two of those, and **ADR
0013 decided it deliberately** in 2026-07: stamping `now` on a re-declared 45-minute-old reading
would be conformant and would be the exact silent lie contract v1 produced. So the table pins
**both columns**, deviations cited to the ADR and conformant rows to their `tck-id`.

**One asymmetry found by reading rather than assuming: there is no
`tck-id-payloads-ndeath-timestamp`.** DDEATH has one; NDEATH's clauses govern `seq`, `bdSeq` and
the will and none of them the payload timestamp. That row is ours by silence, and the test says
so separately rather than folding it in with the mandated ones.

**The table is a MECHANISM, not a comment.** The re-declaring DBIRTH and the cold-start DBIRTH
now resolve their clock through `timestamp_source_for`, so moving a row changes what is emitted —
mutation 1 shifted the published stamp by exactly one hour. **DDATA is deliberately NOT routed
through it**: `publish` is handed no clock at all, so `PublicationInstant` is unrepresentable
there. The signature is a stronger enforcer than a branch, and adding a clock parameter to make
the table reachable would be adding the very capability ADR 0013 refuses. (Epic 3's action D3:
an "unreachable" cites its enforcer.)

**A conformance row was earned, not claimed.** `payloads-nbirth-timestamp` was `gap (unproven)`
with [#30] saying *"replace `clock.wall()` with a small constant and every test stays green"*.
That mutation now goes red (`left: Some(42)`), so the row moves to `conformant` and the chapter-6
tally goes `38 · 4 · 8 · 59` → `39 · 4 · 7 · 59`. The two deviation rows gained evidence and
**stayed deviations** — more proof of a deliberate deviation is not a step toward conformance.

**The integration test's first draft ended in a sweep that swept ZERO messages** — an assertion
over an empty set, scored as coverage. It was caught because the count was printed. Replaced by
a second reading sent after the reconnect, which makes the post-reconnect path something the
test exercises rather than hopes to observe.

**No production behaviour changed.** `CONTRACT_VERSION` stays at 10, no conformance verdict
moved, and the only non-test code added is a pure table plus the two call sites that now read it.

**[#92] was not touched**, per the story's position: a rebirth re-declares the last reading the
bridge JUDGED, not the last it DELIVERED, and that is correct.

### Falsification record

| # | Mutation | Test | Went red with |
|---|---|---|---|
| 1 | `DeviceBirthRedeclaring` → `PublicationInstant` | `an_hour_of_outage…` | `left: Some(1784988392050), right: Some(1784984792050)` — one hour apart, and it is the EMITTED value that moved |
| 2 | `NodeBirth` → `ReadingValueDate` | `the_timestamp_table…` | `NodeBirth is fixed by the SPECIFICATION, not by us … left: ReadingValueDate, right: PublicationInstant` |
| 3 | drop `.map(degrade)` from the rebirth metrics | `an_hour_of_outage…` | `a reading not re-judged against now is published stale … left: 192, right: 2147484164` — 192 being Ignition's `Good`, which is the lie |
| 4 | DDATA stamped with a fixed instant | `an_hour_of_outage…` | `the DDATA payload timestamp IS the source ValueDate (ADR 0013) … left: Some(9000000000000)` |
| 5 | `degrade` flattens every quality to `Stale` | `a_rebirth_does_not_flatten…` | `left: 2147484164, right: 2147484160` |
| 6 | the birth stamp becomes `42` — **[#30]'s own prescription** | `an_hour_of_outage…` | `the node birth is an event and carries the instant it happened … left: Some(42)` |
| 7 | `DeviceBirthRedeclaring` → `PublicationInstant` | `chaos_no_replay_at_reconnect` | `THE RE-DECLARED READING FOLLOWED THE CLOCK … left: 1787078552364, right: 1786968000000` — observed on the wire, the real reconnection instant |
| 8 | DDATA always stamps the first reading's time | `chaos_no_replay_at_reconnect` | `a reading acquired AFTER the reconnection carries its own acquisition time too … left: Some(1786968000000), right: Some(1786968060000)` |

**Note 5's prediction was wrong and was rewritten against the run** — it said `left: 500, right:
0`. Third time in two stories that a note written before its run did not survive it, which is
the whole reason the rule says *run it first*.

### File List

- `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs` — modified (`TimestampSource`, `Emission`, `timestamp_source_for`, the two call sites now routed through it, a `cause_of` test helper, 3 tests)
- `crates/smartme-bridge/tests/chaos_no_replay_at_reconnect.rs` — new (the out-of-process proof)
- `docs/sparkplug-conformance.md` — modified (one row earned, two rows re-evidenced, the chapter-6 tally)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified
- `_bmad-output/implementation-artifacts/4-12-anti-replay-at-the-down-up-instant.md` — new

### Change Log

- **2026-08-18** — Story 4.12 implemented as the verification story it was drafted to be. Four
  tests, eight mutations, one conformance row earned and one story premise corrected. No
  production behaviour changed.
