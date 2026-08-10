# Story 2.1: The oracle layer exists, and how verdicts compose is decided once

Status: in-progress

## Story

As the bridge,
I want every judgement about a reading to compose into one published verdict by a written rule,
so that adding an oracle cannot quietly change what an existing one means.

## Why this exists, and why it comes before any oracle

**Today exactly one thing produces a verdict.** `Policy::step(prev, tick, now) -> (State, Quality)`
is the whole judgement, and its doc comment carries a first-match-wins table. Epic 2 adds three
more producers — physical bounds, energy-counter monotonicity, payload domain — and a fourth is
already live but was never written down as an oracle at all (ADR 0029's serial-identity check,
delivered because a real fault demanded it).

**Four producers and no composition rule is how a guarantee rots.** Each new oracle would answer
"what quality do I publish?" on its own, and the answers would drift. `judge_reading` already
argues one case of the rule without generalising it — *"`Bad` is judged BEFORE the timestamp
guards: 'do not use this value' must never be relabeled as the milder 'old value'"*. That
sentence is the rule. It has never been stated as one.

**AR16 asks for a mechanism, and the mechanism does not exist.** It requires the oracle→quality
mapping to live in a versioned contract with `contract_golden.rs` failing if the mapping changes
without a version bump. Neither the document nor the test exists today; `CONTRACT_VERSION` is
served but nothing guards what it stands for. Building the mapping here without its guard would
be the "repair the instance, not the class" pattern the Epic 5 retrospective named (action B1).

**And the sequencing precondition is met.** `epics.md:245` deferred Epic 2 behind Epic 4 partly
because *"Epic 2 will define many oracle→quality mappings (AR16), which are cheaper to land on a
settled publishing machine than to revisit after rebirth and anti-replay change republication
semantics"*. The publishing machine settled with stories 4.1–4.10.

## Acceptance Criteria

**AC1 — The oracle layer is a named place in the pure core, and the purity guard covers it.**

**Given** the functional-core invariant enforced by `arch_purity`
**When** the oracle layer is added
**Then** it lives in `core/`, imports nothing async or transport-shaped, and `arch_purity` fails
if it ever does
**And** an oracle is a pure function of a `Reading` (plus, where an oracle needs it, per-meter
carried state) to a verdict — never of a clock it reads itself, never of a network.

**AC2 — Composition is worst-wins over a stated total order, and it is one pure function.**

**Given** several oracles judging one reading
**When** their verdicts are composed
**Then** the published quality is the worst of them under `Good < Stale < Bad`, computed by ONE
function that every producer passes through
**And** the composition is falsified: a mutation returning the first verdict instead of the worst
turns a test red, with a case where the order of evaluation and the severity order disagree.

**AC3 — The cause survives to the wire, and NOT inside the `Quality` property.**

**Given** a reading refused by a named oracle
**When** it is published
**Then** the metric carries the reason under a property key distinct from `Quality`
**And** `Quality` keeps exactly the values ADR 0012 chose, with no new code invented
**And** the reason names the oracle that refused, not a generic string.

**AC4 — The latch-versus-degrade rule is written, and ADR 0029 is reclassified as its first case.**

**Given** an oracle refusing a reading
**When** the refusal is classified
**Then** the rule is stated in one place: *a contradiction about WHICH METER this is latches
(`Failed`, restart-only); a contradiction about WHAT THE VALUE SAYS degrades that reading only*
**And** ADR 0029's serial-identity check is recorded as the first instance of the latching half —
it was decided before the rule existed and must not now read as an exception to it
**And** a test asserts that a degrading oracle does NOT latch: the next good reading publishes
`Good` again.

**AC5 — `contract_golden.rs` exists and fails when the mapping moves without a version bump.**

**Given** the oracle→quality mapping
**When** any mapping entry is changed, added or removed without `CONTRACT_VERSION` changing
**Then** `contract_golden.rs` fails, naming the entry that moved
**And** it is falsified in both directions: change a mapping without the bump → red; change it
with the bump → green. A guard only ever shown red proves it can fail, not that it can pass.

**AC6 — `CONTRACT_VERSION` moves and everything that states it follows.**

**Given** a new property on every metric
**When** the contract version is bumped 3 → 4
**Then** the manual, the Tier-3 runbook's run table and the conformance matrix are amended in the
same commit
**And** the amendment is verified mechanically (grep for the old number), not by memory — five
NBIRTH metric-count statements went stale this way during story 4.7.

**AC7 — This story implements NO oracle.**

**Given** the layer, the composition, the cause channel and the golden guard
**When** this story closes
**Then** physical bounds, monotonicity and payload domain are still absent, and stories 2.2–2.4
own them
**And** the existing judgements — freshness, the source's own `Bad`, ADR 0029's identity check —
are migrated onto the layer with **no verdict changing**, proven by asserting the current
`Policy::step` table row by row before and after.

## Tasks / Subtasks

- [x] **Task 1 — Decide and record the composition (AC2, AC4)**
  - [x] Write the severity order and the worst-wins rule as the module doc of the oracle layer
  - [x] Write the latch-versus-degrade rule beside it, with ADR 0029 named as its first case
  - [x] Confirm `State` and `Quality` stay decoupled: `Ok, value Bad → (Stale, Bad)` today, and
        composition governs the published `Quality`, not the state
- [x] **Task 2 — Build the layer (AC1)**
  - [x] `core/oracle.rs`: the verdict type, the composition function, the registry an oracle joins
  - [x] Verify `arch_purity` covers the new module; break it deliberately and watch it fail
- [ ] **Task 3 — The cause channel (AC3)**
  - [ ] Choose the property key and record why `Quality` cannot carry it
        (`tck-id-payloads-propertyset-quality-value-value` restricts it to 0/192/500, and ADR 0012
        already deviates)
  - [ ] Publish it on DDATA; decide and record whether the DBIRTH declares it
  - [ ] Read the norm before deciding the DBIRTH half — cite the `tck-id`, not prose
- [x] **Task 4 — Migrate the three existing judgements (AC7)**
  - [x] Freshness, source `Bad`, ADR 0029's identity check
  - [x] Row-by-row equality of the `Policy::step` table before and after
- [ ] **Task 5 — `contract_golden.rs` (AC5)**
  - [ ] The golden mapping and the version it belongs to
  - [ ] Falsify both directions and record both runs
- [ ] **Task 6 — Consequences (AC6)**
  - [ ] `CONTRACT_VERSION` 3 → 4; manual, runbook run table, conformance matrix
  - [ ] Mechanical check for stale statements of the old number
- [ ] **Task 7 — `./scripts/ci-local.sh` green, all steps**

## Dev Notes

### Three decisions taken at drafting, so no later story re-chooses them

**1. Worst-wins over `Good < Stale < Bad`, not first-match-wins.** `Policy::step`'s table is
first-match-wins, and that is correct *for a single producer whose guards are ordered by
intent*. With several independent producers, evaluation order becomes an accident of
registration, and first-match would make the verdict depend on it. Worst-wins is
order-independent, which is the property that matters when the set of oracles grows.
`judge_reading` already behaves this way for the one case it has.

**2. The cause cannot travel in `Quality`, and this is settled by the norm, not by preference.**
`tck-id-payloads-propertyset-quality-value-value` (`Sparkplug_6_Payloads.adoc:634-636`): *"The
'value' of the Property Value MUST be an int_value and be one of the valid quality codes of 0,
192, or 500."* The bridge already deviates from this deliberately (ADR 0012 — the conformant
codes display as `Good` on Ignition, which is the exact lie this project exists to prevent).
Inventing a fourth value to encode a cause would deepen a deviation that was accepted only
because the alternative was a silent lie. A separate property key costs nothing in conformance:
`PropertySet` constrains only that keys and values have equal length
(`Sparkplug_6_Payloads.adoc:571,577`).

**3. Latch is about identity; degrade is about value.** ADR 0029 refused a mismatched serial as
`Fatal` — latching, restart-only — and it was right: if the reading is not from the meter we
declared, no later reading from the same misconfiguration is trustworthy either. A power value
outside physical bounds says nothing about the next one. The rule follows the distinction rather
than the severity, and it must be written down before 2.3 has to guess it.

### What this story does NOT do

**No oracle.** The layer, the rule, the channel and the guard. Stories 2.2, 2.3 and 2.4 bring the
judgements. A story that shipped both would make it impossible to tell a composition defect from
an oracle defect on the first failure.

**No change to any current verdict** (AC7). If a verdict moves, the migration is wrong — the
existing table is the specification of "no change", and it is dense enough to be asserted row by
row rather than sampled.

### The trap this story is most likely to fall into

**Asserting the composition against itself.** A test that builds two verdicts and checks the
composition returns the worse one proves the function computes what it computes. What must be
asserted is that a *reading* judged by two registered oracles publishes the worse quality — and
falsified by a mutation that returns the first. The repository has been caught by the
self-consistency shape twice: the quality codes that compared our encoder to our decoder, and the
`log.contains('3')` satisfied by a timestamp.

**And the golden test only ever shown red.** AC5 asks for both directions explicitly. Story
4.7's review found a proof cell that had inherited a mutation result from another chapter without
being run.

### Where the existing judgements live

- `core/state_machine.rs:138` — `Policy::step`, the first-match table, `Failed` absorbing
- `core/state_machine.rs` `judge_reading` — the source's own `Bad`, judged before the timestamp
  guards, with the reason stated
- `adapters/smartme_source.rs:261` — where an unknown unit or non-finite value becomes
  `Quality::Bad`; the self-described "FR8 thin slice", and the place whose three failure modes
  collapse into one undifferentiated verdict (story 2.4's business, not this one's)
- `adapters/sparkplug_publisher.rs:108` — `ignition_quality_code`, the ADR 0012 mapping
- `domain/quality.rs` — the single `Quality`, re-exported from `sparkplug-b`, whose module doc has
  been waiting for this story since 1.2: *"the bridge's error taxonomy maps into it (Epic 2
  onward); no ad-hoc quality strings anywhere"*

### References

- [Source: `_bmad-output/planning-artifacts/epics.md:264`] — Epic 2's AR16/AR17 assignment
- [Source: `_bmad-output/planning-artifacts/epics.md:150`] — AR16, the versioned contract and
  `contract_golden.rs`
- [Source: `_bmad-output/planning-artifacts/epics.md:245`] — why Epic 2 followed Epic 4, and the
  precondition now met
- [Source: `docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_6_Payloads.adoc:617-636`] — the
  `Quality` property key and its permitted values
- [Source: `docs/adr/0012-quality-codes-spec-versus-host.md`] — the deviation this story must not
  deepen
- [Source: `docs/adr/0029-the-declared-serial-is-checked-against-the-one-smart-me-reports.md`] —
  the latching case, decided before the rule existed
- [Source: `docs/adr/0030-epics-run-in-numeric-order.md`] — why Epic 2 is open now

## Dev Agent Record

### Completion Notes List

**2026-08-10 — Tasks 1, 2 and 4 done. The layer exists and the core is migrated onto it;
Tasks 3, 5, 6 (the wire, the golden guard, the contract bump) remain.**

- **`core/oracle.rs`** carries `Cause`, `Verdict` and `compose`, and the three rules as module
  documentation rather than as folklore. `arch_purity` covers it — verified by adding a
  `tokio::sync::watch` import and watching the guard fail, then restoring.
- **Nine causes, one per row of `Policy::step`'s table.** The ninth,
  `Cause::SourceMarkedStale`, is produced by nothing today: `map_device` yields `Good` or `Bad`
  and never `Stale`. It exists because the arm exists, and naming it beats borrowing
  `ValueUnusable` — "the source said so" and "we could not convert it" are different diagnoses
  and would send an operator to different places.
- **AC7 is proven by assertions that did not change.** The 26 pre-2.1 assertions in
  `state_machine`'s tests are kept **verbatim**, routed through a `#[cfg(test)]`
  `step_quality` accessor that drops the cause. A table of assertions that still passes
  unchanged is the proof the migration moved no verdict; rewriting them would have destroyed
  the evidence in the act of collecting it. The same treatment was applied to
  `staleness_injected_clock`'s `verdict_for`.
- **Causes are covered separately** by `every_row_of_the_table_names_its_own_cause`, because a
  row borrowing a neighbour's cause would be invisible to every quality assertion in the file —
  the quality would still be right.

**Four falsifications, all red, each with its own message:**

| mutation | result |
|---|---|
| `compose` → first-non-good-wins | RED on the case where evaluation order and severity order disagree — `Stale` published where `Bad` was owed |
| `Cause::latches` widened to "anything worse than a timeout" | RED on every degrading cause: *"HostClockUnsynced describes a reading, not an identity"* |
| a `tokio` import in `core/oracle.rs` | RED in `arch_purity` |
| `NoFreshnessProof` pointed at `Cause::ReadingTooOld` | RED in the per-row cause test, while every other test in the module stayed green |

**Full suite green after the migration:** 190 unit tests plus every integration and chaos test.

**Not yet done:** Task 3 (the cause reaching the wire under its own property key), Task 5
(`contract_golden.rs`), Task 6 (`CONTRACT_VERSION` 3 → 4 and its consequences), Task 7
(`ci-local.sh`).

### File List

- `crates/smartme-bridge/src/core/oracle.rs` (new)
- `crates/smartme-bridge/src/core/mod.rs`
- `crates/smartme-bridge/src/core/state_machine.rs`
- `crates/smartme-bridge/src/core/channel.rs`
- `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs`
- `crates/smartme-bridge/src/adapters/smartme_source.rs`
- `crates/smartme-bridge/src/app/poll_publish.rs`
- `crates/smartme-bridge/tests/staleness_injected_clock.rs`
- `crates/smartme-bridge/tests/nfr2_staleness_latency.rs`
- `crates/smartme-bridge/tests/chaos_ncmd_rebirth.rs`
- `crates/smartme-bridge/tests/ignition_contract.rs`
