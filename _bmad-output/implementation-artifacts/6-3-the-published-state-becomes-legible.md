# Story 6.3: The published state becomes legible — AR19's enriched state, before any screen consumes it

Status: done

> **AR19 settles the order of work before this story begins**: *"UI consumes this state, never
> recomputes it."* The screens FR28, FR30, FR34 and FR36 ask for cannot be written first and
> back-filled, because a screen that computes what the bridge should already know becomes the
> second place the truth lives. So this story enriches the state and ships **no new screen**.
>
> **What the fleet snapshot carries today**: the meter id, `last_tick`, the pacing period, the
> oracle's `State`, the published `Verdict`, and the six drop counters. **What it does not carry**:
> when the last publication happened, what changed and when, and whose fault a fault is. Those are
> AR19's, and they are what FR28/FR30/FR34 have been waiting for.

## Story

As the operator,
I want the bridge to know — and say — when it last published, what changed, and whose fault a
fault is,
so that a screen can show me that at three in the morning without deducing anything.

## The finding that shapes this story

**Culprit cannot be derived from the oracle's causes alone, and discovering that at implementation
time would have produced a wrong table.** Walking all twenty-one `Cause` variants against
world/you/bridge gives roughly fourteen *world*, five *you*, and — this is the point — **not one
that says *bridge***. The taxonomy has no cause meaning "this bridge has a defect", because the
oracle judges *readings*, and a reading is never the bridge's fault.

The bridge's own faults live in the other enum: `DropReason` — `OutboxFull`, `MqttTaskGone`,
`BeforeBirth`, `UndeclaredDevice`, `Unpublishable` are all *bridge*, and `TransportQueueFull` is
*world* (the broker is not keeping up).

**So `culprit` is a function of two inputs, exactly as AR19 says** — *"derived from the error
nature and source-vs-sink health"* — and a story that had read only `Cause` would have shipped a
label incapable of ever accusing the bridge, which is the one accusation an operator most needs to
see.

## Acceptance Criteria

**AC1 — the published state carries when, and against what.**

**Given** a meter that has published
**When** the fleet is read
**Then** its state carries `published_at` **and the staleness threshold that was in force when the
verdict was reached**
**And** the threshold travels with the instant, because a freshness judgement read against a
different threshold than the one used is a different judgement.

**AC2 — what changed is distinct from what was republished.**

**Given** ADR 0027's rule that every cycle publishes a verdict, so most publications repeat the
previous value
**When** the fleet is read
**Then** `last_changed_at` and `last_published_at` are separate fields
**And** a meter republishing an unchanged value for an hour shows an old `last_changed_at` and a
recent `last_published_at`, which is the distinction an operator needs to tell a frozen meter from
a quiet one.

**AC3 — culprit is a first-class value, derived from both enums.**

**Given** the twenty-one `Cause` variants and the six `DropReason` variants
**When** a fault is recorded
**Then** the state carries `Culprit::{World, You, Bridge}` as an enum, decided by a table with one
row per variant and a reason per row
**And** the table is pinned by a test that fails if a variant is added without being classified —
the `qos_for` and `timestamp_source_for` pattern, applied a third time
**And** `IdentityMismatch` is classified **You** with its ambiguity written down: the configuration
declares a serial the account does not confirm, which an operator repairs — but a physically
replaced meter is the world moving, and the configuration merely reporting it.

**AC4 — the repair gesture is derived, never stored.**

**Given** that `send_modify` holds a write lock every poll task waits on
**When** a repair gesture is needed
**Then** it is produced at render time from the `Culprit` and the variant, and **no formatted text
is written into the fleet state**
**And** the story records why: the state carries data, the screen carries words.

**AC5 — nothing on the hot path pays for this.**

**Given** that `snapshot()` clones, and that its only production caller is the UI
**When** the enriched state ships
**Then** the poll loop's cost is unchanged — no allocation added under `send_modify`
**And** the measurement is recorded rather than asserted.

*Measured at drafting, 2026-08-19, and it corrected the drafting assumption: `snapshot()` has
exactly one production caller, `ui/mod.rs:324`, so the clone is paid per HTTP request and not per
tick. The concern that shaped the first draft of this story — "enriching the watch channel taxes
the hot path" — was wrong. What remains true, and becomes AC4's rule, is that `send_modify` holds
a lock the poll tasks wait on, so nothing expensive may be built inside it.*

**AC6 — falsification.**

**Given** each new field and the culprit table
**When** the mechanism it names is broken
**Then** a test goes red, and the run's output is copied next to it.

## Out of scope, said rather than left to be inferred

AR19 lists five things. This story takes three. **Root-cause grouping** and **the persisted
expected mapping for Cold-Reopening reconciliation** are not here: the first needs the culprit
table to exist before it can group anything, and the second is a persistence question that touches
`store` and belongs with the reconciliation that consumes it. Neither is forgotten; both are named
here so the next story inherits a boundary rather than a surprise.

**No screen ships in this story.** FR28, FR30, FR34 and FR36 are the next one's, and they will
consume this state without recomputing it — which is the only reason to do these in this order.

## References

- [Source: `_bmad-output/planning-artifacts/epics.md:153`] — AR19, and *"UI consumes this state, never recomputes it"*
- [Source: `_bmad-output/planning-artifacts/epics.md:328`] — Epic 6's scope
- [Source: `crates/smartme-bridge/src/core/oracle.rs`] — the twenty-one causes and their slugs
- [Source: `crates/smartme-bridge/src/app/poll_publish.rs`] — `MeterState`, `DropReason`, `send_modify`
- [Source: `crates/smartme-bridge/src/app/mqtt_driver.rs`] — `qos_for`, the table pattern AC3 copies
- [Source: `CLAUDE.md`] — falsify before trusting; decide at drafting or measure first

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-19.

### Completion Notes List

**AC1, AC2, AC3, AC4 — met.** `MeterState` gains five fields: `last_published_at`,
`staleness_threshold_ms`, `last_changed_at`, `source_value_date` and `culprit`. `record_at`
writes them from the poll loop, where every one of those values already existed; `record` stays
as the thin call the UI's own tests use.

**The culprit table is in two halves, and that was the story's finding rather than its plan.**
`Cause::culprit` classifies twenty-one variants — sixteen `World`, five `You` — and
`DropReason::culprit` supplies the six on the publishing side, five of them `Bridge`. **A table
built from causes alone would have been structurally incapable of accusing this process**, which
is the accusation an operator most needs. `no_cause_accuses_the_bridge_because_the_oracle_judges_readings`
turns that from an observation into a property: a future cause meaning "the bridge is broken"
makes it red, and its author has to decide whether the oracle is its home.

**`NotRevalidated` is classified `World`, and it is the row most likely to be argued with.** It is
not a fault at all — ADR 0027 requires a verdict every cycle, so a value not re-judged is
republished degraded. `World` is chosen because what the operator is seeing is the source not
having produced anything new; the screen should read "waiting", not "blame". `IdentityMismatch`
carries its ambiguity in the table itself.

**Change is the source's, not the bridge's.** `last_changed_at` moves only when the reading's own
`ValueDate` differs from the last published one. Falsified: dropping that guard makes every
republication count as a change, and `Some(UtcMillis(3000))` stands where `Some(UtcMillis(1000))`
belongs.

**AC5 — met, and the drafting assumption behind it was wrong.** The story was first shaped by
"enriching the watch channel taxes the hot path". Measured: `snapshot()` has exactly one
production caller, `ui/mod.rs:324`, so the clone is paid per HTTP request, never per tick. Writes
go through `send_modify`, which mutates in place. What survives is the rule AC4 states — that lock
is held by every poll task, so nothing formatted is built inside it.

**No screen ships, deliberately.** FR28, FR30, FR34 and FR36 are the next story's, and they can
now consume this state without recomputing it — the only reason to do these in this order.

**An incident worth recording, because it is the second in one day.** Undoing a mutation with
`git checkout <file>` wiped the uncommitted work in that file — the whole `Culprit` table and its
tests — exactly as it had that morning on `ui/mod.rs`. Rewritten, and every falsification since
was undone by **inverse edit**, never by checkout. The lesson had been written down that morning
and was not yet a habit; the repository's own record of story 4.11's repeated defect says the same
thing about written lessons.

### Falsification record

| # | Mutation | Went red with |
|---|---|---|
| 1 | `CredentialRejected` moved to the `World` arm | `a rejected credential is repaired by the operator, not waited out … left: World, right: You` |
| 2 | `ValueUnusable` classified `Bridge` | `NO CAUSE MAY ACCUSE THE BRIDGE, and value-unusable does` |
| 3 | the `value_date != source_value_date` guard dropped | `a REPUBLISHED reading must not move last_changed_at … left: Some(UtcMillis(3000)), right: Some(UtcMillis(1000))` |
| 4 | `entry.culprit` write removed from `dropped` | `a reading the BRIDGE lost must say so … left: None, right: Some(Bridge)` |

### File List

- `crates/smartme-bridge/src/core/oracle.rs` — modified (`Culprit`, `Cause::culprit`, two tests)
- `crates/smartme-bridge/src/app/poll_publish.rs` — modified (five fields, `record_at`, `DropReason::culprit`, the drop-site write, two tests)
- `_bmad-output/implementation-artifacts/6-3-the-published-state-becomes-legible.md` — new
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Change Log

- **2026-08-19** — Story 6.3. AR19's enriched state, three of its five parts, with the culprit as
  a first-class value in two halves. No screen. Four mutations run. `CONTRACT_VERSION` unchanged
  at 10 — nothing here reaches the wire.

### Review — 2026-08-20

**Every acceptance criterion holds, and the two halves of the culprit table are the right
shape.** AC3's pinning was checked against the mechanism rather than the name: adding a
`Cause` variant stops the build at `Cause::culprit`'s exhaustive `match` *and* reddens
`every_cause_names_whose_fault_it_is`, whose final assertion compares the two columns'
combined length against `Cause::ALL`. Both were exercised.

**One residue, recorded rather than repaired.** That length comparison would also pass if a
future edit listed one variant twice and dropped another — sixteen plus five is sixteen plus
five either way. The consequence is bounded: the missing variant is still *classified* (the
`match` refuses to compile otherwise), it is merely no longer *pinned*, so no behaviour can
drift silently, only an assertion can go quiet. Left as it stands; a `HashSet` of the
twenty-one would close it if a third table ever joins these two.

**AC5's measurement was re-taken and still holds.** `snapshot()` has exactly one production
caller — `ui/mod.rs:324` — every other call site sits below `poll_publish.rs:1731`, inside
`#[cfg(test)]`. The clone is paid per HTTP request. `record_at` adds no allocation under
`send_modify`: five `Option` fields of `Copy` types, and `Culprit` is a fieldless enum.

**Its consumers arrived in 6.4, and two of them arrived one story late.** `source_value_date`
and `staleness_threshold_ms` — the pair AC1 exists for — were written every tick and read by
nobody until the review of story 6.4 added the freshness column. The state was right; the
screen had not caught up. Recorded here because AC1's *"the threshold travels with the
instant"* is only true end to end since 2026-08-20.

**Citation corrected mechanically (action E4):** `ui/mod.rs:323` → `:324`. The line named the
function signature; the call is the line below.
