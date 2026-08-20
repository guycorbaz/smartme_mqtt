# Story 2.4: A number the physics forbids is refused, and it takes nothing else down with it

Status: **WITHDRAWN 2026-08-12, unimplemented** — [ADR 0033](../../docs/adr/0033-fr14-is-withdrawn-physical-plausibility-is-not-the-bridge-s-to-judge.md), [#72](https://github.com/guycorbaz/smartme_mqtt/issues/72)

> **This story is kept rather than deleted, because it is the artifact that killed its own
> requirement.** It was written on 2026-08-12 and withdrawn the same day, before a line of code:
> Guy read the draft and answered that judging a value is not this bridge's role — *« ton rôle est
> de collecter des données de compteur et de les afficher, pas de les juger. »*
>
> **What made the objection legible was the draft's own honesty.** AC2 could not be filled in: it
> needed the ceiling of each supply and whether a meter can see negative power, and refused to
> invent either, on story 2.2 AC4's rule about numbers nobody measured. AC7 then had to argue at
> length about where to store a constant describing a distribution board. A story that cannot
> state its own criterion without a fact about someone's electrical installation is a story whose
> component has taken on a responsibility that was never its own — and that reads much more
> clearly in a draft than in a merged implementation.
>
> **Had it picked a comfortable 40 kW instead**, the requirement would have shipped, the oracle
> would have passed its tests, and the first refusal of a real reading would have arrived on some
> sunny afternoon with nobody expecting it.
>
> **Two things below survive the withdrawal** and are the reason to keep reading: the FR14/FR16
> boundary drawn in the Dev Notes, which story 2.5 still needs; and AC6, which was the only
> identified subject of [#69] — its disappearance is what turns that issue from a wait into a
> question.
>
> Everything else stands as drafted and is not to be implemented.

## Story

As the operator,
I want a reading that no installation of mine could have produced to be published as refused rather than as a measurement,
so that a value which is not merely surprising but impossible never reaches a historian, and so that the metric beside it keeps its own good value.

## Why this exists, and why it is the story that closes [#69]

**FR14 is the last oracle Epic 2 owes that has no code at all.** Freshness judges the timestamps,
monotonicity judges the relation between two energy readings, identity judges the serial. Nothing
judges whether a *single number* is one the world could have produced.

**It is the first oracle that is per-metric by nature, which is why story 2.3 had to come first.**
A power reading outside plausible bounds says nothing whatever about the cumulative energy index
beside it, and before ADR 0031 a verdict belonged to a *reading* — so this oracle would have nulled
a perfectly sound energy index and labelled it with a power fault. That is exactly the defect
story 2.3 was written to remove, and building this story first would have re-created it.

**And it is the first oracle that refuses a reading the SOURCE called good** — which is the whole
of [#69]. Story 2.3 AC3 changed the adoption rule for `last` and `energy_reference` from *"the
source did not mark it `Bad`"* to *"the composed verdict did not refuse it"*, and recorded itself
UNMET because **no input existing at the time could tell the two rules apart**: every oracle then
in the tree either refused a reading the source had already marked `Bad`, or refused it for going
backwards, which the rule exempts by design. A bounds oracle breaks that symmetry on its first
reading. Closing [#69] is therefore not a courtesy this story pays to the previous one — it is the
first opportunity anyone has had to prove the rule at all, and it belongs here.

## Acceptance Criteria

**AC1 — The oracle is pure, per-metric, and joins the layer rather than sitting beside it.**

**Given** the oracle layer built by stories 2.1 and 2.3
**When** the bounds oracle is added
**Then** it is a pure function in `core/` of the value alone — never of a clock, a network, or a
previous reading — and it enters `step_once` as a `Judgement::about(Measured::Power, …)`, not as a
branch beside `compose`
**And** `arch_purity` covers the module it lives in, verified by breaking it deliberately.

**AC2 — A power reading outside the installation's physical envelope is refused, and the envelope
is a measured number rather than a comfortable one.**

**Given** a bound derived from what the installation can physically deliver or absorb
**When** a reading arrives whose instantaneous power falls outside it
**Then** the verdict is `Bad` with cause `PowerOutOfRange`, scoped to `Measured::Power`
**And** the two numbers — the ceiling, and the floor — are recorded in the module doc **with the
fact of the installation they come from**, not with a rationale for their convenience.

*Decided at drafting: this criterion refuses to invent its own numbers.* Story 2.2 AC4 rejected a
tolerance band because it would be *"a number nobody measured, chosen to suppress a signal rather
than to model one"*. A ceiling of "40 kW because that feels generous" is the same defect wearing a
different hat: it would refuse a real reading on a large installation and admit an absurd one on a
small one. See *The one input this story is missing*.

**AC3 — A negative cumulative index is refused, and this needs no site measurement at all.**

**Given** a cumulative energy counter
**When** a reading arrives whose index is strictly below zero
**Then** the verdict is `Bad` with cause `EnergyIndexImpossible`, scoped to `Measured::Energy`
**And** the criterion records why this is FR14 and not FR16: a negative cumulative counter is not a
value outside a *configured* domain, it is a value the quantity cannot take. No installation
detail is needed to know it, which is what makes it a different kind of bound from AC2's.

**AC4 — Two causes, not one, and the story says why it does not follow story 2.2's example.**

**Given** the two refusals above
**When** the cause is published
**Then** they are **two distinct causes**
**And** the reasoning is recorded, because it is the opposite of the reasoning story 2.2 AC5 used:
that story published one cause for reset, rollover and replacement *because nothing available to
the bridge distinguishes them*. Here the bridge distinguishes them perfectly — a power magnitude
the supply cannot carry and a cumulative index below zero are different faults, reached by
different failures, and they send an operator to different places. Publishing one cause would
claim a confusion we do not have.

**AC5 — A refused metric takes nothing else down with it.**

**Given** a reading whose power is out of range while its energy index is sound
**When** it is published
**Then** `Power` is null with quality `Bad` and cause `power-out-of-range`, **and `Energy` carries
its real value, quality `Good`, with no cause at all**
**And** the symmetric case is asserted too: an impossible energy index leaves `Power` good
**And** both are falsified by widening the judgement's scope to the reading: the untouched
metric's assertion must go red, naming the metric.

**AC6 — [#69] IS CLOSED, and this is the criterion the story exists to make provable.**

**Given** a reading the SOURCE marked `Good`, refused by this oracle
**When** the next tick arrives
**Then** neither `last` nor `energy_reference` has adopted it — the composed verdict refused it,
and the old rule (`reading.value.quality != Quality::Bad`) would have adopted it because the source
was content
**And** the test drives a sequence that distinguishes the two rules and could not have been written
before this oracle existed: an accepted reading, then a reading refused by bounds alone, then a
lower energy index which must publish `Bad(counter-went-backwards)` — proving the refused reading
never became the yardstick
**And** it is falsified by restoring the old guard: the third publication goes `Good` and the
assertion names it
**And** story 2.3's AC3 is marked met, [#69] closed, in the same commit.

**AC7 — The bound is a constant of the code, and the reason it is not configuration is recorded.**

**Given** the bound
**When** its home is chosen
**Then** it lives in the pure core as a constant, not in `config.toml`
**And** the reason is recorded: `MeterConfig` carries four fields and no numeric envelope, so
adding one reaches the validation table, the web UI form, `reconfigure::classify`'s hot-versus-
restart decision and the config round-trip — a surface several times the oracle's, in service of a
number that changes when the *installation* changes and not when an operator wants a different
display
**And** what would reopen it is named: a fleet whose meters have genuinely different envelopes —
a three-phase industrial feed beside a domestic one — at which point the constant becomes a lie for
one of them and configuration is the honest answer.

**AC8 — `CONTRACT_VERSION` moves 6 → 7, additive.**

**Given** two new cause strings
**When** they are added
**Then** `tests/contract_golden.rs` fails first — observed, not assumed — and then the version and
its golden move together, `GOLDEN_*_V7` written out rather than aliased
**And** the version is recorded **additive**, not breaking: the cause vocabulary grows, no metric
name, unit or nulling rule changes for a situation that already existed
**And** the manual's version table and prose, and the runbook's attestation block, follow in the
same commit, with the mechanical grep for the old number re-run — story 2.2 skipped it and the
runbook spent a day naming the wrong shipped contract.

**AC9 — Falsified before trusted, each mutation recorded beside its test.**

**Given** every assertion this story adds
**When** it is written
**Then** it is run against deliberately broken code and observed to fail, with the mutation named
in a `FALSIFIED` note next to the test
**And** the mutations include at minimum: the comparison removed; the bound widened past the test
value; the judgement's scope widened from the metric to the reading; and AC6's old adoption guard
restored.

**AC10 — No verdict that is correct today changes.**

**Given** the assertions that existed before this story
**When** it closes
**Then** they still pass unchanged — the same proof story 2.1 AC7 and story 2.3 AC10 used, for the
same reason.

## The one input this story is missing

**AC2 needs two numbers, and they are facts about Guy's installation rather than judgements about
software.** They are named here rather than deferred, because this repository's rule is that a
criterion may not say *"decided by the measurement that will exist later"* — AR13 sat unmade for a
whole epic that way.

1. **The ceiling.** What the supply behind each meter can physically deliver — in practice the main
   breaker's rating. `appart-est`, `appart-ouest` and `hangar` may not share one.
2. **The floor, and it is the one that can do harm.** Can any of these meters see **negative**
   active power — i.e. injection back into the grid, from photovoltaic panels present or planned?
   smart-me reports `ActivePower` signed, so the meter can. If injection is possible and the floor
   is set at zero, **this oracle will refuse perfectly real readings on the sunniest day of the
   year**, publish them as `Bad`, and do it precisely when someone is looking. If injection is
   impossible, a negative reading is a genuine fault and the floor belongs at zero.

Recorded observation, which sets the order of magnitude and nothing more: the captured smart-me
response in `crates/smart-me-client/fixtures/` carries `ActivePower: 0.018` kW and
`CounterReading: 4843.822` kWh — a standby load of 18 W. It says these are domestic meters. It does
not say what their ceiling is.

**Until both numbers exist, AC2 is not implementable and the story is not `ready-for-dev` for that
criterion.** AC3 and AC6 are, and they are the two that carry [#69].

## Tasks / Subtasks

- [ ] **Task 1 — Obtain AC2's two numbers, and record them with their source** (AC2)
  - [ ] The ceiling per meter, or one ceiling if they share it
  - [ ] Whether injection is possible; if it is, the floor is its magnitude, not zero
  - [ ] Write both into the module doc **with the installation fact**, not with a justification
- [ ] **Task 2 — The oracle** (AC1, AC3, AC4, AC7)
  - [ ] `Cause::PowerOutOfRange` and `Cause::EnergyIndexImpossible` with their wire strings
  - [ ] The pure judgement in `core/oracle.rs`, per metric, taking the value alone
  - [ ] The constants, and the note on why they are not configuration
  - [ ] Break `arch_purity` deliberately and watch it fail
- [ ] **Task 3 — Compose it** (AC1, AC5)
  - [ ] Two `Judgement::about(Measured::…, …)` entries in `step_once`'s array
  - [ ] **Respect the source's own refusal first**, exactly as the monotonicity call site does:
        a value the adapter marked `Bad` carries `BAD_CARRIER = 0.0` and is not a number to judge
        for plausibility — judging it would answer `power-out-of-range` about a documented
        non-value and send the operator hunting an electrical fault for a unit-contract failure
  - [ ] Assert the pair on one published update, on the wire, not on the in-process `MeterUpdate`
- [ ] **Task 4 — Close [#69]** (AC6)
  - [ ] The three-tick sequence, driven through `step_once`
  - [ ] Falsify by restoring `reading.value.quality != Quality::Bad`
  - [ ] Mark story 2.3 AC3 met; close [#69]; amend `epics.md`'s count of unmet criteria
- [ ] **Task 5 — Contract** (AC8)
  - [ ] Watch `contract_golden` fail FIRST, then bump 6 → 7 with `GOLDEN_*_V7` written out
  - [ ] Manual version table and prose, runbook attestation block, mechanical grep
- [ ] **Task 6 — Falsify everything** (AC9)
  - [ ] Each mutation run, observed red, recorded beside its test
- [ ] **Task 7 — `./scripts/ci-local.sh` full run**, then `gh run list` after pushing

## Dev Notes

### Decisions taken at drafting, so no later story re-chooses them

**1. Two causes, against story 2.2's precedent, and the difference is the discrimination we
actually have.** 2.2 collapsed reset, rollover and replacement into one cause because the bridge
cannot tell them apart. Here it can. The test of whether to split is never "are these conceptually
different?" but "would an operator go to a different place?" — and a power magnitude the supply
cannot carry sends them to the electrical installation, while an index below zero sends them to the
meter or to us.

**2. FR14 owns physical impossibility; FR16 (story 2.5) owns payload domain.** The PRD's wordings
overlap — FR16 mentions *"a value outside per-metric min/max bounds"* — so the boundary is drawn
here rather than left for whoever writes 2.5 to guess. **FR14 asks: could this quantity have this
value in the world?** **FR16 asks: is this payload well-formed and within its declared numeric
domain?** A negative cumulative counter fails the first. A missing field, a null, a NaN fail the
second. Story 2.5 inherits `map_device`'s debt — three failure modes collapsing into one
undifferentiated `Bad` naming no field — and this story does not touch it.

**3. Degrade, never latch.** ADR 0032's rule: a contradiction about WHICH METER this is latches; a
contradiction about WHAT THE VALUE SAYS degrades that reading only. A power spike says nothing
about the next reading. Both causes here are degrading, and `Cause::latches()` must return false
for both — which the golden pins.

### What this story does NOT do

**It does not make the bounds configurable** (AC7), it does not touch `map_device`'s undifferentiated
`Bad` (story 2.5), and it does not introduce a rate-of-change or plausibility-versus-history rule.
A reading judged against its predecessor is a different oracle with different failure modes, and
this one is a function of a single number by AC1's design.

### The trap this story is most likely to fall into

**Asserting the oracle against itself.** A test that feeds `(bound + 1)` and checks the verdict is
`Bad` proves the comparison compares. What must be asserted is that a *reading* judged in the
pipeline publishes `Bad` with a null value **on the wire**, and that its neighbour does not. Story
2.3's review found exactly this hole one layer up: every test reaching `metrics_for` handed it a
`Verdicts::uniform`, where the old and new code agree on every output, so reverting the whole of
ADR 0031 left the suite green. **The property lives on the published `Metric`s. Assert it there.**

**And the second trap is AC6's, which is subtler.** A test that drives a bounds refusal and checks
the reference did not move can pass for the wrong reason — because the refused reading's *energy*
was unchanged, so the reference would not have moved anyway. The sequence must make the two rules
disagree observably: the third reading's verdict is what discriminates them.

### Where the code lives

- `crates/smartme-bridge/src/core/oracle.rs` — `Cause` (an exhaustive `successor` chain: a new
  variant that misses it is a compile error, by design since 2026-08-11), `Verdict`, `Judgement`,
  `Measured`, `Scope`, `compose_for`, `compose_for_meter`, `energy_is_monotonic` as the model to
  follow
- `crates/smartme-bridge/src/app/poll_publish.rs:~413` — the `judgements` array in `step_once`,
  and at `:~474` the two adoption flags `reference_adoptable` / `last_adoptable` that AC6 is about
- `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs` — `metrics_for`, which nulls a
  metric on its OWN verdict since ADR 0031
- `crates/smartme-bridge/tests/contract_golden.rs` — the guard; it will fail before anything else
  notices the vocabulary grew
- `crates/smart-me-client/fixtures/` — the captured responses, and the only real values in the tree

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:284`] — FR14
- [Source: `_bmad-output/planning-artifacts/prd.md:286`] — FR16, whose boundary with FR14 is drawn above
- [Source: `_bmad-output/planning-artifacts/epics.md:265`] — Epic 2's AR16/AR17 assignment
- [Source: `docs/adr/0031-a-verdict-belongs-to-a-metric.md`] — why this oracle can exist per metric
- [Source: `docs/adr/0032-at-equal-severity-a-latching-cause-outranks-a-degrading-one.md`] — the latch/degrade rule
- [Source: `_bmad-output/implementation-artifacts/2-3-the-oracle-layer-finished.md`] — AC3, recorded UNMET, which this story closes
- [Source: `_bmad-output/implementation-artifacts/2-2-energy-counter-monotonicity.md`] — AC4's refusal to invent a number, and AC5's one-cause reasoning that this story deliberately departs from
- [#69]: https://github.com/guycorbaz/smartme_mqtt/issues/69

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
