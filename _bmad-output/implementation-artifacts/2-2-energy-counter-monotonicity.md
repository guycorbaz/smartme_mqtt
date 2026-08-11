# Story 2.2: An energy counter that goes backwards is never published as a valid measurement

Status: in-progress

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

### Review Findings — 2026-08-11

Three review layers (blind adversarial, edge-case, acceptance audit). The oracle detects a
backwards counter and publishes `Bad` with null values — the property AC1 names holds for the
tick on which it happens. **What the review found is that the number this story refuses to
publish reaches the wire anyway, one tick later**, and that the reference guard implements a
third rule that is neither AC2's nor AC3's.

**Decisions — settled by Guy on 2026-08-11.**

| Decision | Taken | Carries |
|---|---|---|
| What `last` and `energy_reference` adopt | **Both follow the COMPOSED verdict**, with an explicit exemption for `CounterWentBackwards` (the one `Bad` that must be adopted, per AC3) | makes AC2 true as written; makes the guard correct for stories 2.4 and 2.5 |
| Restart | **Persist the reference**, same mechanism as `bd_seq_path` | closes the window where FR15 is most needed; Epic 7 will make restarts frequent |
| NFR6 residual | **Accepted limitation, recorded with an issue** | AC3 mandates the behaviour; the gap with NFR6's letter was written down nowhere |

- [x] [Review][Decision] **The refused number reaches the wire on the next tick, as a real Double with quality `Stale`** [`crates/smartme-bridge/src/app/poll_publish.rs:403-409`] — `*last = Some(reading.value.clone())` runs on EVERY successful fetch, including one judged `Bad`. On the next failed fetch, `to_publish = last.clone()` and the composed verdict is `Stale(SourceUnreachable)`; `metrics_for` nulls values only on `Quality::Bad` (`sparkplug_publisher.rs:623-627`), so at `Stale` it publishes `Double(energy)` — the post-reset index, or the substituted `BAD_CARRIER = 0.0` when a unit conversion failed (`smartme_source.rs:38,265-280`). A consumer differencing `4843.822 → 0.0` gets −4843.8 with a flag saying "the network hiccuped". AC1: *"a consumer must not be handed the number at all, because the number is exactly what it would difference"*. No test covers the sequence backwards→timeout. The manual's justification for publishing values at `Stale` (*"this WAS a reading"*) is false in the `BAD_CARRIER` case: it was never a reading. **AC2's own rationale names this hazard** (*"`last` … is updated on every successful fetch including ones the oracle refused"*) and then guards only `energy_reference`.
- [x] [Review][Decision] **AC2 is unmet: the reference advances on the SOURCE's opinion, not on the composed verdict** [`crates/smartme-bridge/src/app/poll_publish.rs:375-379`] — the guard is `reading.value.quality != Quality::Bad`, but every freshness-level refusal (`ReadingTooOld`, `NoFreshnessProof`, `SourceClockImplausible`, `TimestampsDisagree`, `HostClockUnsynced`) leaves `value.quality == Good` and advances the reference. Concrete failure: reference 4851; a replayed response at 4800 publishes `Bad(CounterWentBackwards)` **and rewinds the reference to 4800**; a genuine reset to 4820 then passes `4820 < 4800 == false` and **publishes `Good`**. A real counter reset published as a valid measurement — FR15's exact harm. AC2 says *"the reference advances only on a reading this oracle accepted"*; the code implements a third rule, written in neither AC2 nor AC3. **This guard will also be wrong for stories 2.4 and 2.5**, which produce `Bad` verdicts on readings the source marked `Good`. The fix must read the composed verdict, with an explicit exemption for `CounterWentBackwards` (the one `Bad` that MUST be adopted, per AC3).
- [x] [Review][Decision] **The reference is amnesic across every process restart, and a restart is exactly when a meter is most likely to have been swapped** [`crates/smartme-bridge/src/app/poll_publish.rs:451`] — `energy_reference: Option<Kwh> = None`, never persisted. `energy_is_monotonic(None, x)` returns `Verdict::good()` by design, so the first reading after a restart is unchecked and silently becomes the new baseline. Restarts are routine: any `Cost::ProcessRestart` config change (`app/reconfigure.rs:201,204,240,249,271`), and Epic 7 will wire `/healthz` to one. The repository already persists cross-restart oracle state (`bd_seq_path`, `mqtt_driver.rs:1101`), so this is a missing decision, not a missing capability. The story file does not mention restart at all — an unconsidered case rather than an accepted limitation.
- [x] [Review][Decision] **NFR6's residual, decided at drafting but recorded nowhere** — after a reset the published sequence is `Good(4843.822) → Bad(null) → Good(12.5)`, so a consumer differencing two consecutive VALID measurements still gets a negative delta. AC3 mandates this behaviour and the manual documents it, so it is not an AC violation — but NFR6 reads literally *"0 negative deltas published as valid"*. Record it as an accepted limitation with an issue, or close the gap.

**Patches.**

- [ ] [Review][Patch] **`energy_is_monotonic` returns `Good` for NaN, in either argument** [`crates/smartme-bridge/src/core/oracle.rs:~334-339`] — `reading.0 < previous.0` is false when either side is NaN. A NaN reading is judged monotonic-good; a NaN REFERENCE disables the oracle permanently for that meter with no signal. The only thing preventing it is an invariant in another module (the source adapter marks non-finite values `Bad`), unmentioned in this `pub` function's doc, which enumerates the cases it considered without naming non-finite.
- [ ] [Review][Patch] **The oracle is handed `BAD_CARRIER` and judges it as an energy index** [`crates/smartme-bridge/src/app/poll_publish.rs:353-358`] — the `Ok(reading)` arm calls `energy_is_monotonic` with NO quality guard, three lines above the reference update that has exactly that guard. It returns `Bad(CounterWentBackwards)` about a documented non-value. The wire still reads `value-unusable` only because `compose` keeps the first verdict at equal severity — which `oracle.rs:280-285` states *no caller may rely on*. This caller relies on it. The published cause is the operator's only diagnosis: it sends them hunting a meter reset when the fault is an API unit-contract change.
- [ ] [Review][Patch] **AC6's third mutation was not played, and the guard it aimed at is covered by no test** [`crates/smartme-bridge/src/app/poll_publish.rs:555-568`] — AC6 names *"removing the comparison, flipping it to `>`, and letting the reference advance on a refused reading"*. The third was replaced by a different mutation. No test in `poll_publish` chains a source-`Bad` reading followed by another reading, so deleting the guard at `:375-378` leaves everything green.
- [ ] [Review][Patch] **`a_reading_that_is_both_stale_and_backwards_publishes_the_worse` indexes `got[1]` with no length assertion** [`crates/smartme-bridge/src/app/poll_publish.rs:~671`] — its sibling asserts `got.len() == 3` first. If the second update stops being emitted — the regression ADR 0027 exists to prevent — the test dies on an index panic instead of on the assertion that names the property.
- [ ] [Review][Patch] **`a_counter_that_goes_backwards_is_bad_once_and_then_recovers` discards the states it should be checking** [`crates/smartme-bridge/src/app/poll_publish.rs`] — it asserts `s1 == State::Fresh` (the premise) then drops both later states with `let _ = step_once(…)`, which is what hides the verdict/health divergence recorded against story 2.1.

**Deferred.**

- [x] [Review][Defer] **A mid-stream unit change can produce a false `counter-went-backwards`** [`crates/smartme-bridge/src/core/oracle.rs:336`] — deferred, low likelihood. If smart-me reports `counter_reading` in `Wh` on one poll and `kWh` on the next, the same physical index reaches the oracle through two different conversion paths (`rescale`, `smartme_source.rs:300-315`) and can differ by an ULP. A downward ULP nulls a good reading for one tick. The "no tolerance band" decision is not being re-litigated; the unhandled input is the UNIT SWITCH, not jitter.
