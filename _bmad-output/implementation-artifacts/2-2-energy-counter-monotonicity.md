# Story 2.2: An energy counter that goes backwards is never published as a valid measurement

Status: review

## Story

As the operator,
I want a counter that resets, rolls over or is replaced to be marked rather than published as if
nothing happened,
so that a consumer computing a delta from my index cannot be handed a negative one and believe it.

## Why this one first, and why it has a date on it

**FR15 and NFR6 are the only requirements in Epic 2 with a live case waiting for them.** On
2026-08-10 `appart-est` (serial 9202685) froze at 09:34:50 and was still frozen three hours later
at 4926,766 kWh. When it returns, exactly one question decides whether the period is
reconstructible: did the index catch up, stand still, or **go backwards**? If the meter was reset
or replaced, nothing in the bridge today would notice — it would publish the new index as a valid
measurement, and a consumer differencing two readings would get a negative delta and no reason to
distrust it.

**This is the first oracle with memory.** Freshness compares two timestamps inside one reading;
monotonicity compares this reading to the last one. That difference is the whole design problem
here, and it is why the reference has to be decided rather than assumed (see AC2).

## Acceptance Criteria

**AC1 — A counter that goes backwards is published `Bad`, naming its cause.**

**Given** a meter whose last accepted energy index was `E`
**When** a reading arrives carrying an index strictly below `E`
**Then** the verdict is `Bad` with cause `CounterWentBackwards`, composed through
`core::oracle::compose` like every other verdict
**And** the metric values are published as null, as `Bad` already does — a consumer must not be
handed the number at all, because the number is exactly what it would difference.

**AC2 — The reference is the last ACCEPTED index, and it is not `last`.**

**Given** `poll_publish` already carries `last: Option<Measurement>` for the republish path
**When** the monotonicity reference is chosen
**Then** it is a separate piece of oracle state, because `last` records *what we would republish*
and is updated on every successful fetch including ones the oracle refused — using it would make a
rejected reading the reference for judging the next one
**And** the reference advances only on a reading this oracle accepted.

**AC3 — A backwards step is reported once, and the new index becomes the reference.**

**Given** a counter that went backwards and was published `Bad`
**When** the next reading arrives, consistent with the new lower index
**Then** it is judged normally against the NEW reference and can be `Good`
**And** a test asserts exactly that: the meter is not stuck `Bad` for ever.

*Rationale, decided at drafting: a replaced meter legitimately starts lower and every subsequent
reading would otherwise be "backwards" against a reference that no longer exists. Latching here
would take a working meter off the wire until somebody restarted the container — and the
latch/degrade rule (Story 2.1) puts this on the degrading side anyway: the counter's history is
broken, its identity is not.*

**AC4 — No tolerance band, and the absence is deliberate.**

**Given** a cumulative counter
**When** the comparison is made
**Then** it is a strict `<`, with no epsilon
**And** the reason is recorded: a cumulative counter does not go backwards by a little, so any
tolerance would be a number nobody measured, chosen to suppress a signal rather than to model one.
If real polling data ever shows benign jitter, that measurement is what justifies a band — not
comfort.

**AC5 — Reset, rollover and meter replacement share one cause, and the story says why.**

**Given** an index that dropped
**When** the cause is published
**Then** it is one cause, not three
**And** the reasoning is recorded: nothing available to the bridge distinguishes them. A rollover
is a reset with a particular arithmetic, and a replacement is a reset with a different serial —
and ADR 0029 already refuses a reading whose serial is not the configured one, so the replacement
case that would reach here is a meter re-serialised in the configuration by an operator who meant
to.

**AC6 — Falsified before trusted.**

**Given** the new oracle
**When** its tests are written
**Then** each is falsified: removing the comparison, flipping it to `>`, and letting the reference
advance on a refused reading each turn a distinct test red
**And** the falsification runs are recorded beside the tests.

**AC7 — The contract moves if the vocabulary does.**

**Given** `CounterWentBackwards` is a new cause string
**When** it is added
**Then** `tests/contract_golden.rs` fails until `CONTRACT_VERSION` and the golden move together —
which is the guard Story 2.1 built, doing its work for the first time on a real change.

## Tasks / Subtasks

- [x] **Task 1 — The oracle (AC1, AC4, AC5)**
  - [x] `Cause::CounterWentBackwards` with its wire string
  - [x] The pure judgement in `core/oracle.rs` or a sibling: `(reference, reading) -> Verdict`
  - [x] Strict comparison, no epsilon, with the reasoning in the doc comment
- [x] **Task 2 — The reference (AC2, AC3)**
  - [x] A per-meter reference carried alongside `State`, distinct from `last`
  - [x] Advance it only on an accepted reading; adopt the new index after a backwards step
- [x] **Task 3 — Compose it (AC1)**
  - [x] The verdict joins the freshness verdict through `compose`, not beside it
  - [x] Assert the composition on a reading that is BOTH stale and backwards — worst wins
- [x] **Task 4 — Falsification (AC6)**
  - [x] Three mutations, each red, each recorded
- [x] **Task 5 — Contract (AC7)**
  - [x] Watch `contract_golden` fail first, then bump `CONTRACT_VERSION` 4 → 5 with its golden
  - [x] Manual: the cause list and the version table
- [x] **Task 6 — `./scripts/ci-local.sh` green, all steps**

## Dev Notes

### The trap this story is most likely to fall into

**Asserting the oracle against itself.** A test that feeds `(100.0, 99.0)` and checks the verdict
is `Bad` proves the comparison compares. What must be asserted is that a *reading* judged in the
pipeline publishes `Bad` and null values, and — AC3 — that the meter recovers. The repository has
been caught by the self-consistency shape twice.

**And the composition case is the one that would be skipped:** a reading that is both too old and
backwards. Publishing `Stale` there would be Story 2.1's rule broken on its first real user.

### Where things live

- `core/oracle.rs` — `Cause`, `Verdict`, `compose`, and `Cause::ALL` (which the golden reads)
- `core/state_machine.rs` — `Policy::step`, the freshness half, already returning a `Verdict`
- `app/poll_publish.rs:322` — `step_once`, which carries `previous: State` and
  `last: &mut Option<Measurement>`; the new reference belongs beside `previous`, not inside `last`
- `tests/contract_golden.rs` — will fail until the version and golden move

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:285`] — FR15
- [Source: `_bmad-output/planning-artifacts/prd.md:345`] — NFR6
- [Source: `_bmad-output/implementation-artifacts/2-1-the-oracle-layer-and-how-verdicts-compose.md`]
  — the composition and latch/degrade rules this story is the first to use

## Dev Agent Record

### Completion Notes List

**2026-08-10 — implemented, all seven ACs met.**

- **AC1/AC4/AC5.** `core::oracle::energy_is_monotonic` — a strict `<`, no epsilon, one cause
  for reset/rollover/replacement. `Bad` rather than `Stale`, and the doc comment argues it: the
  value may be perfectly current (a replacement meter really does read 12 kWh) and it is the
  *relation* that is broken. `Bad` publishes null values, withholding exactly the number a
  consumer would difference.
- **AC2.** The reference is a `&mut Option<Kwh>` carried beside `State`, and it advances only on
  a reading whose source quality is not `Bad` — which is what makes it genuinely distinct from
  `last`, which moves on every successful fetch because its job is the republish.
- **AC3.** The new index is adopted after a drop, so a replaced meter recovers instead of being
  marked `Bad` for ever against an index that no longer exists.
- **AC7 — the guard built in Story 2.1 caught a real change on its first outing.**
  `contract_golden` failed before anything else noticed: *"the cause vocabulary changed size
  (11 live, 10 in the v4 golden) without CONTRACT_VERSION moving"*. Bumped 4 → 5 with its
  golden; the quality codes did not move, so v5 reuses `GOLDEN_QUALITY_V4`.

**The test that no other assertion in the tree would have made:** a reading that is BOTH too old
and backwards. Publishing `Stale` there — freshness is consulted first — would be Story 2.1's
composition rule broken by its own first real user, and every "is it degraded?" assertion would
still pass, both verdicts being non-good.

**Three falsifications, all red on the assertion that names them:**

| mutation | result |
|---|---|
| the comparison removed | RED — *"a consumer differencing these two indices would get a negative delta"* |
| the comparison flipped to `>` | RED — the rising reading is marked bad and the falling one good |
| the new index not adopted | RED on the recovery assertion — the stuck meter AC3 forbids |

**`./scripts/ci-local.sh` green, all ten steps.** Manual rebuilt: 70 pages, overfull boxes
exactly the five in the committed baseline.

**Waiting on a real case.** `appart-est` froze on 2026-08-10 at 4926,766 kWh. If it returns with
a reset index, this oracle is what stands between that and a consumer reading a negative delta
as a valid measurement — and it will be the first time FR15 is exercised outside a test.

### File List

- `crates/smartme-bridge/src/core/oracle.rs`
- `crates/smartme-bridge/src/app/poll_publish.rs`
- `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs`
- `crates/smartme-bridge/tests/contract_golden.rs`
- `docs/manual/chapters/05-mqtt-sparkplug-contract.tex`
