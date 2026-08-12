# Story 2.7: A feed that stopped moving stops being called fresh, and a clock that is wrong is not called old

Status: ready-for-dev

## Story

As the operator,
I want a source that keeps handing back the same response to be refused rather than believed,
so that a frozen cloud cannot look exactly like a working one, and so that a meter whose clock is wrong is not reported as a meter whose data is stale.

## Why this exists, and two thirds of FR10 may already be met

FR10 asks three things. **Two of them look done, and this story's first job is to verify that
rather than build it again** — the discipline story 2.6 used for NFR1's backoff, which turned out
to exist already for the broker.

| FR10's clause | Where it stands, to be verified rather than assumed |
|---|---|
| *attach the meter's measurement timestamp to each value* | `Measurement::value_date` becomes the Sparkplug metric timestamp (`sparkplug_publisher::millis`) |
| *treat all timestamps as UTC end-to-end* | `UtcMillis` is the only time type in the domain; `parse_value_date` requires a trailing `Z`; `parse_imf_fixdate` requires a literal `GMT`; no `chrono`, no local timezone anywhere |
| *flag abnormal source clock skew* | **This is the gap.** `TimestampsDisagree` fires only on a NEGATIVE age. A meter whose clock runs behind by a constant produces a positive age that grows into `ReadingTooOld` — **a wrong clock reported as old data**, which sends an operator to the wrong place |

**And the parked oracle is here.** `Policy::step`'s own doc has carried this since Epic 1:

> *Known accepted limitation (deferred oracle): a byte-identical replayed response — `http_date`
> frozen WITH `value_date` — keeps a plausible age and stays Fresh; detecting it needs cross-tick
> state (`http_date` monotonicity), an additive Epic 2 oracle.*

**A frozen cloud is indistinguishable from a working one today**, because every freshness guard
compares two timestamps *inside one reading* and a replay keeps both consistent. This is the last
oracle Epic 2 owes, and it is the one the freshness formula cannot reach by construction.

## Acceptance Criteria

**AC1 — A feed that stopped moving is refused, and the refusal is about the FEED rather than the value.**

**Given** two consecutive successful fetches whose `http_date` has not advanced
**When** the second is judged
**Then** the verdict is non-good with a cause naming a stalled feed, applied to the reading as a
whole — both metrics, because a frozen response says nothing about either number in particular
**And** a feed whose `http_date` advances is untouched, whatever the values did
**And** it is falsified by removing the comparison: a test driving two identical responses must go
red naming what was believed.

*Decided at drafting: the reference is `http_date`, not `value_date`.* A meter that genuinely stops
reporting keeps its `value_date` frozen while the cloud's `Date` header advances — that is ordinary
staleness, already handled, and it must not be reported as a replay. Only the CLOUD's own clock
standing still means the response is not being regenerated.

**AC2 — Skew is distinguished from staleness structurally, not by a threshold.**

**Given** readings whose age is large but stable while `value_date` ADVANCES
**When** they are judged
**Then** the cause says the source's clock disagrees with the cloud's, not that the reading is old
**And** readings whose `value_date` does NOT advance keep `ReadingTooOld` — they are old
**And** the discrimination uses no magnitude threshold: what separates the two is whether the
meter is still producing new measurements, which is a fact rather than a number

*Decided at drafting, and it is the reason this criterion is expressible at all.* A skew threshold
would be a number nobody measured, which story 2.2 AC4 refused for the tolerance band and ADR 0033
refused for physical bounds. **The structural question — is the meter still measuring? — needs no
threshold**, and it is the one that tells an operator whether to fix a clock or a meter.

**AC3 — UTC end-to-end is VERIFIED and recorded, not rebuilt.**

**Given** FR10's *"treat all timestamps as UTC end-to-end"*
**When** the path is examined
**Then** a test asserts the two parsers refuse anything that is not explicitly UTC — a `ValueDate`
without its `Z`, an HTTP date without its `GMT` — and the assertion says why refusing is right
**And** the absence of any local-timezone or `chrono` dependency is asserted mechanically rather
than by inspection, in the spirit of `arch_purity`
**And** if a gap is found, it is fixed here; if none is, that is recorded as the finding.

**AC4 — Both new oracles carry cross-tick state, and the third one is the moment to stop threading parameters.**

**Given** `step_once` already carries `last`, `energy_reference` and `rate_limited_until`
**When** a fourth memory is added
**Then** they are bundled into one per-meter memory type rather than a fifth parameter
**And** the refactor changes no verdict: the assertions that exist before it must pass unchanged,
the proof stories 2.1, 2.3 and 2.5 all used.

*Decided at drafting: this is the story that pays that debt.* Three carried values were tolerable;
a fourth makes every call site a place to pass them in the wrong order, and `step_once` already has
six parameters.

**AC5 — [#69] closes here, or the reason it cannot is recorded.**

**Given** a replayed response, which the SOURCE marks `Good` and this story's oracle refuses
**When** the adoption rules run
**Then** neither `last` nor `energy_reference` adopts it, and the OLD guard
(`reading.value.quality != Quality::Bad`) would have adopted it — the two rules disagree
observably for the first time
**And** a test drives that disagreement and [#69] is closed in the same commit, story 2.3's AC3
marked met
**And** if the disagreement turns out not to be observable, that is recorded on [#69] instead of
being asserted.

**AC6 — Falsified before trusted, and RUN before recorded** — the criterion story 2.6 added.

**AC7 — `CONTRACT_VERSION` moves 8 → 9, additive**, with the golden written out, the manual, the
runbook, and the mechanical grep.

**AC8 — No verdict that is correct today changes**, apart from the cases AC1 and AC2 name.

## Tasks / Subtasks

- [ ] **Task 1 — Bundle the per-meter memory** (AC4)
  - [ ] One type carrying `last`, `energy_reference`, `rate_limited_until`
  - [ ] Every existing assertion passes unchanged
- [ ] **Task 2 — The stalled-feed oracle** (AC1)
  - [ ] `http_date` of the previous accepted fetch, carried in the memory
  - [ ] Reading-scoped judgement, composed like the others
- [ ] **Task 3 — Skew told apart from staleness** (AC2)
  - [ ] The structural discrimination; no threshold
- [ ] **Task 4 — Verify UTC end-to-end** (AC3)
- [ ] **Task 5 — Close [#69]** (AC5)
- [ ] **Task 6 — Contract 8 → 9** (AC7)
- [ ] **Task 7 — Falsify, running each mutation BEFORE writing its note** (AC6)
- [ ] **Task 8 — `./scripts/ci-local.sh` full run**, then `gh run list`

## Dev Notes

### The trap this story is most likely to fall into

**Two oracles in one story make the first failure ambiguous.** Story 2.1 refused to ship a layer
and an oracle together for exactly this reason. They are together here because both need the same
cross-tick state and both are FR10 — but **each must fail on its own assertion**, and a mutation to
one must leave the other's tests green. If that cannot be arranged, they are two stories.

**And the second trap is AC2's.** The temptation is to reach for a magnitude — *"more than N
seconds of stable age is skew"*. That number would be nobody's measurement, and this epic has
refused it twice: story 2.2 AC4 for the tolerance band, ADR 0033 for physical bounds. The
structural question — *is `value_date` advancing?* — is the one that needs no number.

### Where the code lives

- `crates/smartme-bridge/src/core/state_machine.rs:~150` — the parked-oracle note this story
  closes, and `judge_reading`'s guards
- `crates/smartme-bridge/src/app/poll_publish.rs` — `step_once`'s six parameters and the three
  carried memories AC4 bundles
- `crates/smart-me-client/src/types.rs` — `parse_value_date`, which requires the `Z`
- `crates/smart-me-client/src/http_date.rs:16` — `parse_imf_fixdate`, which requires the `GMT`
- `crates/smartme-bridge/src/core/clock.rs` — the only `SystemTime` use, behind the clock seam

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:280`] — FR10
- [Source: `_bmad-output/implementation-artifacts/2-2-energy-counter-monotonicity.md`] — AC4's
  refusal of a number nobody measured, which AC2 here follows
- [Source: `docs/adr/0033-fr14-is-withdrawn-physical-plausibility-is-not-the-bridge-s-to-judge.md`]
  — the same refusal, applied to an entire requirement
- [Source: `_bmad-output/implementation-artifacts/2-6-error-taxonomy-and-bounded-backoff.md`] —
  AC6's ordering rule, and the precedent of verifying a requirement instead of rebuilding it

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
