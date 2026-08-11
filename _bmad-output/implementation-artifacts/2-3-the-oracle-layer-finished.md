# Story 2.3: The oracle layer finished — a verdict per metric, and ties that do not depend on order

Status: review

## Story

As the operator,
I want each published metric to carry the verdict of the oracles that actually judged it, and the
bridge's own screens to say what its wire says,
so that a fault in one number does not withhold another, and so that no surface calls a meter
healthy while the host is being told otherwise.

## Why this exists, and why it comes before story 2.4

**It was not planned. It is the output of the 2026-08-11 review of stories 2.1 and 2.2**, which
found four things that are decisions rather than defects — the code does exactly what it was
written to do, and what it was written to do is wrong for the oracles Epic 2 still owes. Guy took
all four the same day. This story is where they land, together, because they are one change seen
from four sides: **the oracle layer knows what it judged, and everything downstream reads that.**

**It comes before 2.4 because 2.4 cannot be written without it.** Physical bounds is a per-metric
oracle by nature — a power reading outside plausible bounds says nothing about the energy index
beside it — and today a verdict belongs to a *reading*. Writing 2.4 first means writing it against
a layer that cannot express what it judges, then rewriting it. The contract would move twice
instead of once, and the second move would be a correction of the first.

**One of the four is a live defect, not only a design flaw.** The number story 2.2 exists to
withhold reaches the wire one tick later: `last` is adopted on every successful fetch including a
refused one, so the next timeout republishes the post-reset index as a genuine `Double` marked
`Stale`. A consumer differencing it gets the negative delta FR15 exists to prevent, under a flag
that says the network hiccuped. Story 2.2's own AC2 rationale names that hazard — *"`last` … is
updated on every successful fetch including ones the oracle refused"* — and then guards only
`energy_reference`.

**And one is [#62] reappearing on a new code path.** `pulse.record` receives the freshness state,
which no oracle can influence. A meter whose counter went backwards publishes `Bad` with null
values to Ignition while `/healthz` and `/` report it `Fresh`. That is the ten-hour outage of
2026-08-10 in miniature: every operator surface green, the wire saying otherwise.

## Acceptance Criteria

1. **A verdict belongs to a metric, and `compose` composes per metric.** An oracle declares which
   metric it judges; an oracle that judges the reading as a whole (freshness, identity) applies to
   every metric. A meter whose energy index dropped while its instantaneous power is current
   publishes `Energy` = null, quality `Bad`, cause `counter-went-backwards`, **and `Power` with its
   real value and quality `Good`, carrying no cause at all**. A test asserts exactly that pair on
   one published update. Falsify by making the energy oracle apply to both metrics: the `Power`
   assertion must go red naming the metric.

2. **At equal severity, a latching cause outranks a degrading one; below that, composition stays
   worst-wins and order-independent.** `compose`'s result is a function of its inputs as a SET —
   feeding it any permutation yields the same verdict, including for ties. This gives
   [`Verdict::latches`] its first production caller: the composed verdict's latch, not
   `Policy::step`'s independent recomputation, is what decides `State::Failed`. The rule is stated
   in `core/oracle.rs` and nowhere else. Falsify by reversing the array at the call site — every
   assertion must stay green — and by restoring the first-wins tie: the identity-versus-value case
   must go red.

3. **`last` and `energy_reference` are adopted on the COMPOSED verdict, not on the source's
   opinion**, with one explicit exemption: `Cause::CounterWentBackwards`, whose new index MUST
   become the reference (story 2.2 AC3, so a reset meter is signalled once and not for ever). A
   test drives *accepted index → a reading the composed verdict refused → a lower reading* and
   asserts the third publishes `Bad(counter-went-backwards)`: the refused reading must not have
   become the yardstick. Falsify by making the adoption unconditional.

   **REWORDED 2026-08-11, and the first wording is left here because it is the finding.** It read:
   *"a test drives the sequence reference 4851 → **replayed** reading 4800 (refused) → genuine
   reset 4820"*. That test cannot pass, whatever the implementation: detecting a replayed feed
   needs an oracle that belongs to story 2.7, so 4800 is refused *for going backwards* and is
   therefore adopted by this criterion's own exemption. **I wrote a criterion whose proof depends
   on an artifact that does not exist, in a story that cites the rule forbidding exactly that**
   ([`CLAUDE.md`], "never defer a decision to an artifact that does not exist"; AR13 is the
   precedent it names).

   Two things survive the rewording, and they are the reason it is a rewording and not a deletion:

   - **What is provable today is proved.** The sequence above — with the refusal being a failed
     unit conversion rather than a replay — is `the_reference_does_not_advance_on_a_refused_reading`,
     falsified. It is also story 2.2 AC6's third mutation, which had been silently replaced.
   - **What is NOT provable is recorded as unmet, with [#69], per this repository's rule.** The
     rule this criterion states is *prospective*: no oracle today produces a `Bad` on a reading the
     SOURCE called `Good`, so on every input that exists the new rule and the old `value.quality
     != Bad` guard agree. Its first real subject is story 2.4's bounds oracle, and its proof
     belongs there.

   [`CLAUDE.md`]: ../../CLAUDE.md
   [#69]: https://github.com/guycorbaz/smartme_mqtt/issues/69

4. **A reading the bridge refused is never republished as a value.** `last` holds only measurements
   whose composed verdict was publishable, so the `BAD_CARRIER = 0.0` substituted on a failed unit
   conversion cannot resurface at `Stale` on the next timeout. A test drives *good reading → unit
   conversion failure → timeout* and asserts the third publication carries no `0.0`. Falsify by
   restoring the unconditional `*last = Some(...)` on `Ok`.

5. **The monotonicity reference survives a restart**, persisted by the same mechanism as `bdSeq`
   (`persist_atomic`, story 0.8) and restored per meter at startup. A meter at 900 000 kWh whose
   process restarts and comes back reading 12.0 publishes `Bad(counter-went-backwards)`, not
   `Good`. **Decided at drafting, not deferred:** a reference that fails to load is absent, not
   fatal — the meter starts unjudged exactly as a first-ever reading does, and says so in the log.
   Refusing to start would let a corrupt state file take a working fleet off the wire, which is a
   worse failure than the one being prevented. Falsify by deleting the persist call: the
   restart test must go red.

6. **Every operator surface reads the composed verdict.** `/healthz`, `/`, and the per-meter view
   report a meter whose composed verdict is not `Good` as not-good, naming the cause. A meter
   publishing `Bad(counter-went-backwards)` to the broker is never described as healthy by the
   bridge's own screens. A test asserts the two agree on one scenario where they diverge today.
   Falsify by feeding `pulse.record` the freshness state again.

7. **`CONTRACT_VERSION` moves 5 → 6, and the version is recorded as `breaking`.** Decided at
   drafting: a consumer that recorded `Power = null` whenever the energy index was refused will now
   record a real value for the same physical situation, so **values from either side of the
   boundary cannot be compared without knowing which side they are on** — which is precisely the
   manual's criterion for `breaking` rather than `additive`. `contract_golden.rs` gains its v6
   entry, written out rather than aliased to v5. The manual's version table and prose, and
   `docs/ignition-contract-runbook.md`'s attestation block, follow in the same commit — the
   mechanical grep for the stale number is re-run, because story 2.2 skipped it and the runbook
   spent a day naming the wrong shipped contract.

8. **Two ADRs, because two architectural positions change.** One for the per-metric verdict (AC1),
   one for the tie rule (AC2). Each names what it supersedes in `core/oracle.rs`'s module doc,
   which currently states the composition rules as if they were settled — they were, and this story
   is the amendment. GitHub issues per the repository's decision rule.

9. **The four patches deferred into this story by the review are applied**, each with its
   falsification recorded beside it:
   - the quality guard on the call to `energy_is_monotonic`, so `BAD_CARRIER` is never judged as an
     energy index and never produces `counter-went-backwards` for a unit-contract failure;
   - story 2.2 AC6's third mutation, which was replaced rather than played — *letting the reference
     advance on a refused reading* — now meaningful against AC3 above;
   - the missing `got.len()` assertion in `a_reading_that_is_both_stale_and_backwards_publishes_the_worse`,
     which today dies on an index panic rather than on the property it names;
   - `a_counter_that_goes_backwards_is_bad_once_and_then_recovers` stops discarding the states it
     should be checking (`let _ = step_once(...)`), which is what hid AC6's divergence.

10. **No verdict that is correct today changes**, apart from the four cases these criteria name.
    The pre-2.3 assertions are kept verbatim and must still pass unchanged — the same proof
    story 2.1 AC7 used for its own migration, and for the same reason.

## Tasks / Subtasks

- [ ] **1. Decide and record the shape of a per-metric verdict** (AC1, AC8)
  - [ ] Choose between an oracle declaring its metric and a verdict carrying a metric set; write the
        ADR before the code, not after.
  - [ ] Amend `core/oracle.rs`'s module doc, whose three numbered rules currently read as settled.
- [ ] **2. `compose` per metric, and the tie rule** (AC1, AC2, AC10)
  - [ ] Extend `compose`; keep worst-wins below the tie rule.
  - [ ] Give `latches()` its production caller: `State::Failed` follows the composed verdict.
  - [ ] Property test or exhaustive permutation over a small verdict set for order-independence.
- [ ] **3. `metrics_for` stamps each metric with its own verdict** (AC1)
  - [ ] Null only the metrics whose own verdict is `Bad`.
- [ ] **4. Adoption follows the composed verdict** (AC3, AC4, AC9)
  - [ ] One rule, one place, with the `CounterWentBackwards` exemption named in the code.
  - [ ] The two sequence tests (replayed-then-reset, and unit-failure-then-timeout).
- [ ] **5. Persist the reference** (AC5)
  - [ ] `persist_atomic`, per meter, restored at startup; a failed load is absent-and-logged.
- [ ] **6. Operator surfaces read the composed verdict** (AC6)
  - [ ] `pulse.record` and everything downstream of it; check `/healthz`, `/`, the per-meter view.
- [ ] **7. Contract, golden, manual, runbook** (AC7)
  - [ ] `CONTRACT_VERSION` 5 → 6, `GOLDEN_*_V6` written out, manual version table and prose,
        runbook attestation block, mechanical grep for the old number.
- [ ] **8. Falsify everything** (AC1–AC6, AC9)
  - [ ] Each mutation named in the ACs, run, observed red, recorded beside its test.
- [ ] **9. `./scripts/ci-local.sh` full run**, and `gh run list` after pushing.

## Dev Notes

### Where the code is

- `crates/smartme-bridge/src/core/oracle.rs` — `Cause`, `Verdict`, `compose`, `latches`,
  `published_quality`, `energy_is_monotonic`.
- `crates/smartme-bridge/src/core/state_machine.rs` — `Policy::step`, which recomputes the latch
  independently today (`prev == State::Failed || matches!(tick, Err(Fatal))`).
- `crates/smartme-bridge/src/app/poll_publish.rs` — `step_once`, `last`, `energy_reference`,
  `pulse.record`.
- `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs` — `metrics_for`, which stamps one
  verdict on every metric and nulls both on `Bad`.
- `crates/smartme-bridge/tests/contract_golden.rs` — the guard, extended on 2026-08-11 to pin names,
  units, the `Cause` property key, and the oracle→quality mapping.

### What the review already settled, so it is not re-litigated here

- **A property present in DATA but absent from BIRTH is legal.** Rebirth triggers concern metrics
  and aliases, not properties (`Sparkplug_5_Operational_Behavior.adoc:862-864`,
  `Sparkplug_6_Payloads.adoc:1448-1450`); the only property-level MUSTs are the two array-size
  clauses and `-metric-propertyvalue-type-req`, all satisfied unconditionally by `encode.rs`. What
  Ignition *does* with such a property is [#68] and needs a deployment, not code.
- **The strict `<` with no tolerance band stays** (story 2.2 AC4). Not reopened.
- **NFR6's residual** — a negative delta between two valid measurements either side of a refusal —
  is [#67], and is not this story's to close.

### The trap this story is most likely to fall into

Per-metric verdicts multiply the number of things that can be `Good` while something is wrong. The
guarantee is not "fewer nulls"; it is **that a metric's quality describes that metric**. If in
doubt about whether an oracle judges a metric, it judges the reading — degrading too much is the
honest failure, and it is the one the bridge has made from the start.

### The other trap, from the review that produced this story

Three assertions were written on 2026-08-11 and removed because no mutation could make them fail: a
NaN guard whose explicit arm returned what falling through returned, a `ptr::eq` check that rustc's
constant coalescing makes meaningless, and a claim about a poisoned reference that survives exactly
one tick. **A test that cannot fail is worse than no test**, because it is counted. Every assertion
added here gets its mutation run before it is trusted.
