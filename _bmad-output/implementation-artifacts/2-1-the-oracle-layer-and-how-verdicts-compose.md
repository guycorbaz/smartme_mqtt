# Story 2.1: The oracle layer exists, and how verdicts compose is decided once

Status: done (2026-08-12) — **Task 3 recorded UNMET**, [#68](https://github.com/guycorbaz/smartme_mqtt/issues/68)

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
**Then** physical bounds, monotonicity and payload domain are still absent, and stories 2.2 and 2.4–2.5
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
- [ ] **Task 3 — The cause channel (AC3) — UNMET, [#68](https://github.com/guycorbaz/smartme_mqtt/issues/68)**
  - [x] Choose the property key and record why `Quality` cannot carry it
        (`tck-id-payloads-propertyset-quality-value-value` restricts it to 0/192/500, and ADR 0012
        already deviates)
  - [x] Publish it on DDATA
  - [ ] **Decide and record whether the DBIRTH declares it — NOT DECIDED.** Ticked on 2026-08-10
        while the completion notes deferred it to a tag-browser observation, which is the rule
        `CLAUDE.md` opens the "specifications" section with. Untocked 2026-08-12 at closure.
  - [x] Read the norm before deciding the DBIRTH half — cite the `tck-id`, not prose. Done during
        the 2026-08-11 review, and it settles the legality half: a property present in DATA and
        absent from BIRTH is legal (`Sparkplug_5_Operational_Behavior.adoc:862-864`,
        `Sparkplug_6_Payloads.adoc:1448-1450`; the only property-level MUSTs are the two array-size
        clauses and `tck-id-payloads-metric-propertyvalue-type-req`, satisfied unconditionally by
        `encode.rs:273-310`). What Ignition *does* with such a property is the half that remains.
- [x] **Task 4 — Migrate the three existing judgements (AC7)**
  - [x] Freshness, source `Bad`, ADR 0029's identity check
  - [x] Row-by-row equality of the `Policy::step` table before and after
- [x] **Task 5 — `contract_golden.rs` (AC5)**
  - [x] The golden mapping and the version it belongs to
  - [x] Falsify both directions and record both runs
- [x] **Task 6 — Consequences (AC6)**
  - [x] `CONTRACT_VERSION` 3 → 4; manual, runbook run table, conformance matrix
  - [x] Mechanical check for stale statements of the old number
- [x] **Task 7 — `./scripts/ci-local.sh` green, all steps**

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
than the severity, and it must be written down before 2.4 has to guess it.

### What this story does NOT do

**No oracle.** The layer, the rule, the channel and the guard. Stories 2.2, 2.4 and 2.5 bring the
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

**2026-08-10, later — Tasks 3, 5, 6 and 7 done. All seven ACs met.**

- **AC3.** The cause travels under `Cause`, a `String` property, and never inside `Quality` —
  `tck-id-payloads-propertyset-quality-value-value` admits only `0`/`192`/`500` there and the
  bridge already deviates (ADR 0012); a fourth value would deepen a deviation accepted only
  because the alternative was a silent lie. The `sparkplug-b` crate gained a generic
  `with_property`, since an open `PropertySet` is the norm's concept and not this bridge's.
  A `Good` metric carries no cause: falsified by attaching one unconditionally, which turns
  the good-metric assertion red while the two degraded ones stay green.
- **AC5.** `tests/contract_golden.rs` binds `CONTRACT_VERSION` to what it means — the quality
  codes and the whole cause vocabulary, each cause with its side of the latch/degrade rule.
  `Cause::ALL` is defended by an exhaustive `match`, so a cause cannot join the enum and miss
  the golden. **Falsified in BOTH directions**, which AC5 required: three reds (a cause string
  moved, a quality code moved, a bump with no golden) and one deliberate GREEN — the same
  change carried with its bump and its golden passes, proving the guard discriminates the bump
  rather than the change.
- **AC6.** `CONTRACT_VERSION` 3 → 4, on the rule the constant's own doc states: a new property
  on a metric IS a change to the tag set, unlike story 5.2's DDEATH which was not. Manual
  amended (version table, metric table, a new *"Why a value is not good"* subsection) and
  rebuilt: 69 pages, overfull boxes **exactly the five in the committed baseline**. The Tier-3
  runbook now warns that its rows attest to v3 while the contract is v4. The matrix moves no
  verdict — the `PropertySet` clauses constrain array lengths and the `type` field, satisfied
  for the new key as for the other two — and records the third property so it stops reading as
  describing a two-property set.
- **AC7.** No oracle was implemented. Physical bounds, monotonicity and payload domain remain
  absent and belong to stories 2.2 and 2.4–2.5.

**Nine falsifications in all, each red with its own message, plus one deliberate green.**

**`./scripts/ci-local.sh` green, all ten steps including the image**, verified by reading the
log rather than the exit code. Three of its steps caught defects of mine that review had not:
`fmt`, then `clippy -D warnings` on a production-dead `discriminant` (now `#[cfg(test)]`, with
the reason written rather than silenced by an `allow`), then clippy again on an over-complex
return type in the golden.

**LEFT OPEN, AND IT NEEDS A MEASUREMENT RATHER THAN A DECISION.** The norm does not require a
property to be declared in a BIRTH before it may appear in DATA — only that its `type` field is
present in BIRTH messages (`tck-id-payloads-metric-propertyvalue-type-req`), which the encoder
satisfies. But **what Ignition does with a property it did not see at BIRTH is not measured**,
and since a DBIRTH verdict is usually `Good` the property will be absent there. If the host
ignores it, the fix is to declare it at BIRTH with a neutral value — which contradicts the
"no cause on a good value" rule and deserves an arbitration rather than a workaround. Worth
checking in the tag browser on the next deployment.

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
- `crates/smartme-bridge/tests/contract_golden.rs` (new)
- `crates/sparkplug-b/src/model.rs`
- `crates/sparkplug-b/src/encode.rs`
- `docs/manual/chapters/05-mqtt-sparkplug-contract.tex`
- `docs/ignition-contract-runbook.md`
- `docs/sparkplug-conformance.md`

### Review Findings — 2026-08-11

Three review layers (blind adversarial, edge-case, acceptance audit). The layer exists and the
core migrated onto it with no verdict changed (AC7 holds, 26 assertions preserved verbatim). What
the review found is that **three of the four mechanisms this story promised are not wired to
anything**: the latch rule has no production caller, the golden does not pin what AC5 named, and
the cause's journey to the wire is untested at its last hop.

**Decisions — settled by Guy on 2026-08-11, before story 2.3 (the oracle-layer story; physical bounds is now 2.4).**

All three were taken the same day the review raised them. Two of them change an architectural
position and are owed an ADR each; one of those also moves the wire contract.

| Decision | Taken | Carries |
|---|---|---|
| Equal-severity ties | **A latching cause outranks a degrading one** | ADR; gives `latches()` its first production caller |
| Verdict scope | **Per-metric, now — not after 2.3** | ADR + `CONTRACT_VERSION` bump; `compose` composes per metric |
| Health surfaces | **They read the composed verdict** | closes the `[#62]` family on this path |

- [x] [Review][Decision] **Equal-severity ties are resolved by array position, and the module doc says no caller may rely on that — one already does** — `compose` replaces only on STRICTLY greater severity, so `compose([freshness, monotonicity])` silently means "freshness wins ties". Reachable today: a unit-conversion failure yields `bad(ValueUnusable)` from freshness while monotonicity independently yields `bad(CounterWentBackwards)` on the same reading. The operator sees one cause and never learns the other applied. Nothing logs or counts the collision, so the documented mitigation ("a decision to take then, in the open") has no trigger. Story 2.4's bounds oracle lands in this same tie space. A rule that a LATCHING cause outranks a degrading one at equal severity would be order-independent, which is the property `compose` exists to provide.
- [x] [Review][Decision] **The published verdict and the bridge's own health surface have diverged** [`crates/smartme-bridge/src/app/poll_publish.rs:484-500,534`] — `pulse.record(&meter, state)` receives the FRESHNESS state; no oracle can influence it. A meter whose counter goes backwards publishes `Bad` with null values to Ignition while `/healthz` and `/` report `Fresh`. This is the family of `[#62]`, on a code path added yesterday. Decide whether the operator surfaces read the composed verdict.
- [x] [Review][Decision] **One verdict per READING is stamped on every METRIC** [`crates/smartme-bridge/src/adapters/sparkplug_publisher.rs:110-146`] — an energy-only oracle nulls Power and labels it `counter-went-backwards`. The composition layer has no notion of which metric an oracle judges, so a per-metric oracle degrades metrics it says nothing about. Story 2.4 (physical bounds) is per-metric by nature, so this decision cannot wait.

**Patches.**

- [ ] [Review][Patch] **`Cause::ALL` can be bypassed by APPENDING a variant — the natural case, and the only way a cause has ever been added here** [`crates/smartme-bridge/src/core/oracle.rs:~1007-1052`, `crates/smartme-bridge/tests/contract_golden.rs`] — `as_str()` and `discriminant()` are exhaustive matches; `ALL` is a hand-written slice that nothing forces. Append a variant, add its two match arms, forget `ALL`: positions still align, `ALL.len()` is unchanged, the golden compares 11 to 11, everything is green, and the new string reaches the wire. The module doc claims *"a cause cannot reach the wire without passing the golden test"*. `every_cause_has_its_own_wire_string` and `identity_latches_and_value_does_not` each carry a THIRD hand-copied duplicate of the list. `latches()` is `matches!(…)`, not an exhaustive match, so a new cause silently defaults to non-latching with no build error.
- [ ] [Review][Patch] **AC5 is unmet as written: the golden does not pin the oracle→quality mapping** [`crates/smartme-bridge/tests/contract_golden.rs`] — it pins `Quality → integer code` and `cause → (wire string, latches)`, never WHICH QUALITY A CAUSE PRODUCES. Turning `Verdict::stale(ReadingTooOld)` into `Verdict::bad(…)` leaves it green. Also unpinned: `METRIC_PROPERTY_CAUSE = "Cause"` (the very change v4 was struck for — rename it to `"Reason"` and every consumer's tag binding breaks with the guard green), metric names, and units — while the manual promises a bump *"on ANY change to a metric name or unit"*. Conversely the golden pins `latches`, which is never published.
- [ ] [Review][Patch] **Nothing proves the cause reaches the wire — the only assertion is on the in-memory struct** [`crates/sparkplug-b/src/encode.rs:281-288`] — `a_non_good_metric_names_its_cause_and_a_good_one_does_not` compares `metric.properties`, the model's `Vec<(String,String)>`. The new loop that pours those pairs into the protobuf `PropertySet` is traversed by NO test: `a_birth_is_self_describing` and `quality_travels_as_a_property_on_every_message` cover only `Quality`/`engUnit`, and `builders_attach_self_describing_properties` never calls `with_property`. Delete the encoding loop and the whole suite stays green — the cause would never reach a consumer and nothing would say so. This is the exact defect shape the story was written to prevent.
- [ ] [Review][Patch] **AC4 is unmet: `Verdict::latches()` governs nothing, and its doc claims the opposite** [`crates/smartme-bridge/src/core/oracle.rs:424-425`] — the only callers outside `oracle.rs` are three test assertions. The real latch is computed independently by `Policy::step` (`prev == State::Failed || matches!(tick, Err(Fatal))`). So `assert!(Cause::SourceRefused.latches())` restates `matches!(SourceRefused, SourceRefused)` in a second file — the "bdSeq compared against itself" shape. The doc says the `Failed` latch *"is reached through `Verdict::latches` alone"*, which is false. The rule lives in two places, one of them inert.
- [ ] [Review][Patch] **AC4's non-latching test asserts that a stateless fold is stateless** [`crates/smartme-bridge/src/core/oracle.rs:429-435`] — `a_degrading_cause_does_not_poison_the_next_reading` calls `compose([bad(…)])` then `compose([good()])`. No reading is judged, no "next reading" is published. AC4 asked that *"the next good reading publishes `Good` again"*. The property was only really attested afterwards, by story 2.2's test. The story file names this trap itself.
- [ ] [Review][Patch] **`step_is_deterministic_and_pure` silently narrowed in the migration** [`crates/smartme-bridge/src/core/state_machine.rs:~1698`] — migrated to `step_quality`, which discards the cause. It now proves the QUALITY is deterministic and says nothing about the half this story added, in a migration whose rule was to keep assertions verbatim.
- [ ] [Review][Patch] **Falsification is not recorded next to the tests** [`crates/smartme-bridge/tests/contract_golden.rs`] — no `FALSIFIED` note anywhere in the file, though AC5 demands both directions. The three reds and the deliberate green live only in this story file. Repository rule: *"record the falsification next to the test"*.
- [ ] [Review][Patch] **`a_version_without_a_golden_is_refused…` is near-tautological, and `GOLDEN_QUALITY_V4` is shared by reference between the v4 and v5 arms** [`crates/smartme-bridge/tests/contract_golden.rs`] — the first restates that the `match` has two listed arms; the load-bearing half (that the panic arm fires) is unexercised. The second means editing v4's golden retroactively rewrites what v4 attested to — a golden is a historical record.
- [ ] [Review][Patch] **The runbook still says "the contract is now v4" while `CONTRACT_VERSION = 5`** [`docs/ignition-contract-runbook.md:312`] — verified. AC6 instituted a mechanical grep for the old number; story 2.2 did not re-run it. The manual's prose (`05-…tex:179-182`) states the nature of v4, v3 and v2 and skips v5, the current one.
- [ ] [Review][Patch] **ADR 0029 is not amended** [`docs/adr/0029-*.md`] — AC4 asked that the serial-identity check be recorded as the first instance of the latching half. The link exists only inside `oracle.rs`; ADR 0029's reader still sees an isolated decision. Repository rule: an architectural position gets an ADR.
- [ ] [Review][Patch] **A closed task carries an undecided decision** [Task 3] — *"decide and record whether the DBIRTH declares it"* is ticked `[x]` while the completion notes defer it to a future tag-browser observation. Repository rule: never defer a decision to an artifact that does not exist. Record it as UNMET with an issue, or decide it.

**Deferred.**

- [x] [Review][Defer] **`source-refused` is a generic string shared by a rejected credential and a wrong meter** [`crates/smartme-bridge/src/adapters/smartme_source.rs:191`] — deferred, belongs to story 2.6 (error taxonomy). An operator cannot tell NFR7 (wrong meter) from an expired credential, which is the reproach this story levels at `smartme_source.rs:261`.
- [x] [Review][Defer] **A cold-start or newly-announced DBIRTH publishes a non-good quality with NO `Cause`** [`crates/smartme-bridge/src/adapters/sparkplug_publisher.rs:590-607`] — deferred, pre-existing path plus an owed measurement. `cold_start_metrics` bypasses `metrics_for`, so the invariant "a non-good metric names its cause" does not hold there, and no cause in the vocabulary means "never read yet". **Settled against the norm during review: adding a property in DATA that was absent from BIRTH is LEGAL** — the rebirth triggers at `Sparkplug_5_Operational_Behavior.adoc:862-864` concern metrics and aliases, not properties, and the only property-level MUSTs (`tck-id-payloads-propertyset-keys-array-size`, `-values-array-size`, `tck-id-payloads-metric-propertyvalue-type-req`) are satisfied unconditionally by `encode.rs:273-310`. What Ignition DOES with such a property is the Tier-3 measurement already owed in `sprint-status.yaml`.
- [x] [Review][Defer] **"No opinion" and "I checked and it is fine" are the same value** [`crates/smartme-bridge/src/app/poll_publish.rs:357`] — deferred, harmless under worst-wins. There is no `Verdict::abstain()`, so any future rule needing "every oracle affirmed" (a coverage assertion, an operator page listing which oracles ran) cannot tell them apart after composition.

## Closure — 2026-08-12

The review sent this story back to `in-progress` on 2026-08-11 with AC4 and AC5 recorded UNMET and
three of its four promised mechanisms wired to nothing. All three are now wired, and the closure is
verified against the code rather than against the completion notes that made the claim the first
time.

| AC | state at closure | what closed it |
|---|---|---|
| AC1 | met | `core/oracle.rs` under `arch_purity`, falsified by a `tokio` import |
| AC2 | met | the equal-severity tie the review found is closed by story 2.3 AC2 and [ADR 0032]: `compose` is a function of its inputs as a SET, permutation-tested, ties included |
| AC3 | met **on the wire**, not only in the struct | `7b78928` added the encoding-loop coverage whose absence meant deleting the loop left the suite green |
| AC4 | met on its letter | [ADR 0029] gained *"Identity latches; value degrades"*, naming itself the first latching case; the degrading half is attested by story 2.2's recovery test rather than by the stateless-fold tautology this file's own trap section predicted |
| AC5 | met | the golden now pins WHICH QUALITY A CAUSE PRODUCES, the `Cause` property key, metric names and units, each version written out; `Cause::ALL` is a `successor` chain whose single `None` makes an appended variant a compile error |
| AC6 | met | v3 → v4, and the mechanical grep instituted here caught its own first regression one story later |
| AC7 | met | no oracle implemented; the 26 pre-2.1 assertions still pass verbatim |

**AC4 closes on its letter and not on the stronger claim, and the difference is written where the
claim was made.** The rule is *stated* in one place; it is *enforced* in two, and the second is
inert. `latches()` is true only for `Cause::SourceRefused`, which `Policy::step` produces at exactly
the two sites already returning `State::Failed` — so the composed-verdict latch branch cannot change
an answer today. Story 2.3's review proved that by running it, and closed it by making
`poll_publish.rs`'s comment and [ADR 0032] say so rather than by refactoring, which would have moved
the table AC10 required preserved. The branch is a net for the first cause that latches without
`Policy::step` knowing, not a mechanism doing work now.

**Task 3 is recorded UNMET, and it is the only thing this story leaves open.** [#68]. The legality
half is settled by the norm; the arbitration half cannot be taken before a measurement, and the
measurement cannot be taken before the deployment moves — it is still `v0.4.0-rc2`, contract v3,
while the code is at v6. Closing the story does not close the issue, and the issue is what carries
it. This is the practice that let Epic 1 close honestly with two criteria in the open.

[ADR 0029]: ../../docs/adr/0029-the-declared-serial-is-checked-against-the-one-smart-me-reports.md
[ADR 0032]: ../../docs/adr/0032-at-equal-severity-a-latching-cause-outranks-a-degrading-one.md
[#68]: https://github.com/guycorbaz/smartme_mqtt/issues/68
