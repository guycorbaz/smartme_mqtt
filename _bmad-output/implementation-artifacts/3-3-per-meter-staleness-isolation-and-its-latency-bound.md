# Story 3.3: One meter's silence is bounded in time, and the fleet is read at one instant

Status: review

## Story

As the SCADA host,
I want a meter that goes silent to be marked stale within a stated time, while the others stay
fresh,
so that "this value is old" arrives soon enough to act on, and nobody has to guess how soon.

## Why this exists, and what is already true

**FR12 is largely delivered, and saying otherwise would make this story redo it.** Story 3.1 gave
every meter its own poll task and its own heartbeat, with `a_hanging_meter_does_not_cost_the_others_their_cadence`
asserting `[3, 3, 3]` against a serialised walk's `[2, 2, 2]`. Story 3.2 published each meter's own
verdict to its own device, with `each_meters_verdict_reaches_its_own_device` asserting the whole
by-topic map rather than a count. Between them, *one silent meter does not affect the others* is
implemented and falsified.

What is missing is not the isolation. It is **the bound and the instant**:

- **NFR2 has never been measurable**, because one of its three terms has never had a value. The
  requirement reads `last_success + 2×poll_interval + publish_margin`, and `publish_margin` appears
  four times in this repository — in the PRD twice, in `epics.md`, and in story 3.1 — always inside
  that formula and **never with a number, a derivation or a definition**. A bound with a free
  variable cannot be met or missed; it can only be quoted. Deciding it is this story's first act,
  and it is decided below rather than deferred to the test that would measure it — the failure
  AR13 cost the whole of Epic 1.
- **AR6's coherent snapshot does not exist, and story 3.1 ticked it.** `tokio::sync::watch` appears
  nowhere in the crate. What shipped is `poll_publish::Heartbeats` — an
  `Arc<Vec<(MeterId, LastLoopTick, Arc<AtomicU8>)>>` read one meter at a time under
  `Ordering::Relaxed` — which is per-meter, the half that mattered for the fleet, and is not a
  snapshot: a reader walking four meters can observe four different instants. Nothing is known to
  be wrong on a screen today, because `/` and `/healthz` render per request and no assertion yet
  depends on two meters agreeing about *when*. **A latency bound is the assertion that does.**
  Measuring "how long after its last success was this meter marked stale" against values sampled at
  four different instants measures the sampler as much as the bridge.

## Decisions taken at drafting

**1. `publish_margin` := `fetch_timeout`. Derived, not chosen.**

The arithmetic, for a meter that succeeds and then goes silent. `P` is the publish period, `T` the
per-fetch timeout (10 s, `config.rs:740`), `ε` the encode-and-hand-to-the-driver cost:

| step | at the latest |
| --- | --- |
| last success, end of tick *k* | `s` |
| tick *k+1* starts (`MissedTickBehavior::Delay`) | `s + P` |
| its fetch fails by timeout | `s + P + T` |
| the verdict is published (Story 3.2's republish, `try_publish`, QoS 0) | `s + P + T + ε` |

NFR2 demands `s + 2P + publish_margin`, so the requirement is met when `margin ≥ T + ε − P`. The
binding case is the **minimum** legal period, not the default: at `P = PERIOD_MIN = 5 s` that is
`margin ≥ 5 s + ε`, while at the shipped `P = 30 s` any margin at all — including zero — would do.
**A margin picked at the default period would be violated by a legal configuration**, which is
precisely how a bound comes to be quoted rather than met.

Setting `publish_margin = T` satisfies every period in `[PERIOD_MIN, PERIOD_MAX]` and stays derived
from the mechanism instead of being a number somebody liked. It needs [ADR 0028](../../docs/adr/0028-publish-margin-is-the-fetch-timeout.md)
and an issue, because defining a term of a stated NFR is an amendment to it.

**2. The bound this story MEASURES is tighter than the one NFR2 states, and both are recorded.**

The `2×` in NFR2 predates Story 3.2. It was right when a failed fetch published nothing and the
host had to wait for a later cycle to learn anything; since ADR 0027 **one missed tick is enough**,
because the failed tick republishes the last value with the oracle's verdict. So the measured
worst case is `s + P + T + ε` — one period, not two.

NFR2 is not narrowed to match. It is a *ceiling*, the ceiling still holds, and tightening a
published requirement to whatever the implementation currently achieves is how a requirement stops
being able to catch a regression. The test asserts the NFR2 ceiling **and** records the observed
figure next to it, so a change that doubles the real latency fails nothing but is visible.

**3. AR6 is met with `watch<Vec<MeterState>>` and `send_modify`, not with an `ArcSwap`.**

Each poll task owns one entry and must not see the others' — but a reader must see all of them as
they stood at one instant. `tokio::sync::watch`'s `send_modify` takes `&mut` on the shared value
under the channel's own lock, so N writers serialise their own field updates while `borrow()` hands
a reader the whole vector as it stood. `ArcSwap` would need a read-modify-write to rebuild the
vector, which is a race between tasks — the defect being repaired, reintroduced in the repair.

`MeterState` is what `Heartbeats` already carries, made one value: the meter id, its last loop tick,
the period it was pacing at, and its oracle verdict. This story **replaces** `Heartbeats` rather
than adding a second source of truth beside it; two collections answering "how is this meter" is
how `same_mapping` and `mapping_fingerprint` disagreed for a month.

**4. `Heartbeats::meters()` keeps its meaning, and `reconfigure` keeps its guarantee.**

The set of served meters must still come from the running tasks — the 2026-08-08 repair that
stopped `classify` inferring it. The replacement type must expose the same thing, and the existing
`app::reconfigure` tests are the check that it still does.

**5. NOT in this story:** discovery (3.4), the disappearing meter (3.5), the orphan purge (3.6), and
`NFR10`'s read→broker-ACK percentiles, which are a different measurement (per reading, not per
outage) and belong to story 4.16's latency budget.

## Acceptance Criteria

**AC1 — the staleness latency is measured, per meter, at the period where it binds**

**Given** four enabled meters at `PERIOD_MIN`, one of which stops answering after a `Good` reading
**When** the wire is read with an injected clock
**Then** that meter's first non-good quality appears no later than
`last_success + 2×PERIOD_MIN + publish_margin`
**And** the assertion names the measured figure as well as the ceiling, so a regression that stays
inside the ceiling is still visible in the failure output.

> **The trap, and it has already been paid for here.** Four Epic 1 tests passed for the wrong
> reason, one of them because *a fake clock that never advanced*. A latency assertion over a clock
> that does not move is the purest form of that: every measured duration is zero and every bound
> holds. The test must prove the clock advanced by the amount it thinks it did **before** asserting
> anything about lateness.
>
> **And the period matters more than the meter.** At the shipped 30 s period this AC passes with a
> `publish_margin` of zero, so a test written at the default would assert nothing about the decision
> taken above. `PERIOD_MIN` is where the bound is tight.

**AC2 — the three healthy meters stay fresh across the fourth's outage**

**Given** the same run
**When** the failing meter's verdict goes non-good
**Then** each of the other three publishes `Good` on every cycle throughout
**And** their qualities are asserted **individually and by serial**, never as "no other meter went
stale".

> An aggregate assertion — *"exactly one meter is stale"* — is satisfied by the wrong meter being
> the stale one. Story 3.1's first cadence test counted `[9,0,0]` because a shared fixture hard-codes
> one meter id, so "labelled with the right device" is never free here.

**AC3 — the fleet is read at one instant**

**Given** N poll tasks writing their own state concurrently
**When** a reader takes a snapshot
**Then** it observes a single, self-consistent set of per-meter states — never a mixture of instants
**And** the test would fail against the `Vec`-of-atomics this replaces.

> **The vacuity to build against**: a snapshot test over a quiet system passes whatever the
> implementation is. The writers have to be *writing* while the reader reads, and the assertion has
> to be about a relationship the old code could break — a generation counter written by every task
> in the same modification is the cheapest such invariant.

**AC4 — `/healthz` and `/` read the snapshot, and say the same thing as each other**

**Given** the replacement in AC3
**When** both surfaces are rendered from the same state
**Then** neither samples per meter, and the wedge verdict remains the worst meter against its own
allowance (Story 3.1)
**And** `reconfigure::classify` still receives the served set from the running tasks, with its
existing tests unchanged as the proof.

**AC5 — the consequences are swept, and `epics.md` is one of them**

**Given** the standing rule that a decision changes the PRD, the epics and the manual together
**When** `publish_margin` is defined
**Then** ADR 0028 exists with an issue, the PRD's NFR2 line and `epics.md`'s carry the definition,
the manual states the latency an operator can expect
**And** story 3.1's `watch<[MeterState; N]>` box is corrected where it was ticked — the tick is
what let the gap live, and leaving it ticked while fixing the code repeats it.

## Tasks / Subtasks

- [x] **Task 1 — decide and record** (AC: 5)
  - [x] ADR 0028: `publish_margin = fetch_timeout`, with the table above and the `PERIOD_MIN`
        binding case. GitHub issue alongside it ([#60](https://github.com/guycorbaz/smartme_mqtt/issues/60)).
  - [x] PRD NFR2 (both the requirement and the measurable-outcomes table), `epics.md` NFR2, and the
        manual's operations chapter (§ *How soon a silent meter is marked stale*).

- [x] **Task 2 — the snapshot** (AC: 3, 4)
  - [x] `MeterState`, `FleetState` and a `watch<FleetState>` replacing the per-meter atomics, with
        `send_modify` per task. One collection, not two. `LastLoopTick` becomes `MeterPulse`, a
        write handle holding an index.
  - [x] `meters()` keeps returning the served set, and **`app::reconfigure`'s tests passed
        untouched** — which is the check AC4 asked for: the served-set guarantee from 2026-08-08
        did not move.
  - [x] `ui::Phase::fleet()` takes ONE snapshot per request; `failed_sources` and `loop_age` are
        given it rather than reaching for the shared state separately.

- [x] **Task 3 — the measurement** (AC: 1, 2)
  - [x] `a_silent_meters_verdict_arrives_inside_nfr2s_bound`: four meters at `PERIOD_MIN`, injected
        clock, latency read from the judged updates. **Measured 15 000 ms against a 20 000 ms
        ceiling** — exactly `P + T`, the figure ADR 0028 derives, confirmed rather than assumed.
  - [x] **It lives in `tests/nfr2_staleness_latency.rs`, and `arch_purity` is what put it there.**
        Written first in `app::poll_publish`'s test module, it was rejected: `Instant::now(` is
        confined to `core/clock.rs` **with the rule applying inside test modules too** — the third
        field of `CONFINED_TOKENS` is `true` for it, deliberately, unlike `FakeClock`'s. Everything
        under `src/` reads time through the `Clock` seam or not at all.
        A latency measurement needs a clock that advances and `FakeClock` does not follow tokio's
        virtual time, so the honest home is an integration test, where the bridge is exercised from
        outside and `tokio::time` is the instrument rather than a smuggled dependency. **Nothing in
        `src/` gained an `Instant::now(`**, which is the check that this was a relocation and not a
        way around the guard. The guard caught a real thing on its first encounter with this
        story.
  - [x] The elapsed-time guard runs before the bound, and the three healthy meters are asserted
        `Good` individually and by name.

- [x] **Task 4 — falsification** (AC: all)
  - [x] Snapshot rebuilt field by field → AC3 fails, *generation 57 against meters summing to 56*.
        It tears on the 57th write out of 8000: the old read was not unlikely to be torn, it was
        torn almost immediately whenever anything wrote.
  - [x] A 6-second backoff in front of the verdict → AC1 fails on the ceiling itself, *measured
        21 000 ms*. **The first attempt at this mutation changed nothing observable** — a `continue`
        after `step_once`, which has already sent the update by the time it returns — and the test
        stayed green. Story 3.1 paid for that lesson on its cadence test; it cost a second round
        here.
  - [x] Frozen instants → the test dies on the presence assertion rather than on the elapsed guard,
        and the record says so: with every instant equal no non-good update can be `> last_success`.
        The two protections do not subsume each other.
  - [ ] `./scripts/ci-local.sh`, full.

## Closing review, 2026-08-08 (same-day, same hand)

Two findings, neither of which changes a verdict, both worth having on the record.

### The two-borrow shape survives in the API, unused

`MeterPulse::last()` and `MeterPulse::period_ms()` each take their **own** `borrow()`, and since
this story `loop_age` reads the snapshot instead — so the only callers left are tests. The germ of
the defect just repaired is therefore still in the type: a future caller wanting both would read two
instants, which is precisely what `Vec`-of-atomics did. Either they lose their separate borrows or
they go. Left as is today because removing a public accessor its tests use is a change with no
present subject.

### `Policy::new` is called by nothing

The invariant from `2a4d5ca` holds entirely by the field being private — which is the design, and it
is sound: nothing outside `core::state_machine` can build a `Policy` at all. But the constructor
that carries the rule is exercised only by its own test, and `Policy::DEFAULT` is a literal that
does not pass through it. The guard protects a path that does not exist yet. That is the correct
shape for it, and it should be said rather than left for a reader to discover that the "rejection"
rejects nothing anybody calls.

## Dev Notes

### What "signalled" means, and where it is measured

On the wire, not in the process. A verdict the bridge has reached and not published is withheld
(ADR 0027), so a latency measured at the state machine would be measuring the wrong thing — and
would pass over a bridge whose driver never delivered anything. Read it where the host reads it:
the DDATA carrying a non-good quality for that device's serial.

### Why `publish_margin` was never noticed to be undefined

Because nothing ever tried to meet it. NFR2 has been quoted in three planning artifacts and once in
a story's reasoning, always as an argument for a design decision — *"a serialised walk would make
NFR2 unmeetable"* — and never as a threshold anything was measured against. A requirement used only
as an argument does not need its terms to have values, which is exactly why it kept not having one.
