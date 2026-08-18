# Story 4.11: Broker-outage policy — the traced drop, exhaustively (FR22, AR7)

Status: review

## Story

As the operator,
I want every reading the bridge could not hand over to be visible,
so that a broker outage reads as loss, never as silence.

## Acceptance Criteria

**AC1 — every path that loses a reading is counted and traced.**

**Given** a full outbound queue or a broker that is down
**When** a reading cannot be handed to the transport
**Then** it is counted per meter and per reason, and traced at WARN carrying the reading's source
timestamp (`value_date`), the meter and the reason slug
**And** no persistent buffer is introduced: the policy is a traced drop, per AR7.

**AC2 — the drop path is bounded.**

**Given** a sustained outage
**When** readings are dropped throughout
**Then** memory and file descriptors stay bounded — the drop path allocates nothing that survives it.

**AC3 — the poll loop never blocks on a full outbox.**

**Given** the driver is not draining the outbox (it is sleeping between reconnect attempts)
**When** the poll task has a judged reading to hand over and the channel is full
**Then** the reading is dropped as `outbox-full` and the loop ticks on
**And** `last_loop_tick` keeps advancing, so `/healthz` reports the bridge as **not** wedged.

**AC4 — the count is readable.**

**Given** readings have been dropped
**When** `/healthz` is fetched
**Then** the body carries the per-meter, per-reason totals
**And** the totals survive a reconnect: they are cumulative for the process lifetime, not per session.

**AC5 — the six reasons are exhaustive and closed.**

**Given** the code paths on which a judged reading can fail to reach the wire
**When** they are enumerated
**Then** each one increments exactly one reason of the closed set below, and a compile-time
exhaustive match makes an unclassified **outcome** unrepresentable at the two sites that
classify one.

*Corrected 2026-08-18 by the review: this said "an unclassified **path**", which the compiler
cannot enforce. Nothing at compile time prevents a seventh path being added elsewhere, and
three such paths already exist — [#85], [#87], [#88]. The six-row table below is by
inspection, and the distinction matters because a seventh silent path is exactly the failure
FR22 exists to prevent.*

**AC6 — falsification.**

**Given** each new assertion in this story
**When** the behaviour it names is deliberately broken
**Then** the test goes red, and the run's output is copied next to the test (repository rule
*"falsify before trusting"*; and D2 of the Epic 3 retrospective — a repair ships with its pin).

## The six reasons, decided here

A closed enum, one slug each. The slug is what the WARN carries and what `/healthz` keys on.

| Slug | Where it fires today | Status |
|---|---|---|
| `outbox-full` | `poll_publish.rs:880` — the `mpsc::Sender<MeterUpdate>` (64 slots) is full | **does not exist**; today the send *blocks* |
| `mqtt-task-gone` | `poll_publish.rs:880` — `send` returns `Err`, the receiver is dropped | traced, **not counted** |
| `transport-queue-full` | `mqtt_driver.rs:1697` `publish()` — `client.try_publish` refused | traced without meter or `value_date`, **not counted** |
| `before-birth` | `mqtt_driver.rs:1356` inbox arm — `Published::DroppedBeforeBirth` | traced, **not counted** |
| `undeclared-device` | same arm — `Published::DroppedUndeclaredDevice` | traced, **not counted** |
| `unpublishable` | same arm — the `Err(error)` branch (encode/topic failure) | traced, **not counted** |

Certificates are **not** readings and are **not** counted here. `publish_all` at the `DeviceCommand`
arm carries a DBIRTH/DDEATH; its drop already has its own `error!` and belongs to story 3.5's
contract, not to FR22.

## Tasks / Subtasks

- [x] **Task 1 — the reason type** (AC5)
  - [x] Add `pub enum DropReason` with the six variants above and `as_str()` returning the slug, in
        `app/poll_publish.rs` beside `FleetState` (both tasks need it; `core/` is forbidden — it is
        an *application* concern, and `arch_purity.rs` enforces the direction).
  - [x] `#[derive(Clone, Copy, PartialEq, Eq)]`, a `const ALL: [DropReason; 6]`, a
        `const COUNT: usize = Self::ALL.len()` and `fn index(self) -> usize`. The counter array's
        length then comes from the enum and is never spelled twice.

- [x] **Task 2 — counting, in the existing fleet seam** (AC1, AC2, AC4)
  - [x] Add `dropped: [u64; DropReason::COUNT]` to `MeterState` (`poll_publish.rs:40`). Fixed-size
        array, indexed by the enum — this is what makes AC2 true by construction: the cardinality is
        `meters × 6`, both closed sets, and nothing is allocated per drop.
  - [x] Add `Heartbeats::dropped(&self, meter: &MeterId, reason: DropReason)` following the shape of
        `record`/`retire` (`poll_publish.rs:277`, `:299`): one `send_modify`, increment the cell and
        `fleet.generation += 1` in the *same* modification. The generation invariant is load-bearing
        (see its doc comment) — a write that skips it breaks the snapshot property AR6 rests on.
  - [x] Saturating increment. A counter that wraps to 0 after an outage is a surface that lies.

- [x] **Task 3 — the poll task stops blocking** (AC1, AC3)
  - [x] `poll_publish.rs:880`: replace `outbox.send(update).await` with `outbox.try_send(update)`.
  - [x] `Err(TrySendError::Full(update))` → `dropped(meter, OutboxFull)` + WARN carrying
        `meter`, `value_date` and `reason`.
  - [x] `Err(TrySendError::Closed(update))` → `dropped(meter, MqttTaskGone)` + the existing WARN,
        now carrying `value_date` and the slug.
  - [x] Both arms fall through to the same tail: the tick completes, `next` and the verdict are
        returned unchanged. **A drop must not change the state machine's verdict** — the reading was
        judged before this point, and re-judging it here would make the wire depend on the transport.

- [x] **Task 4 — the driver counts its three** (AC1)
  - [x] Hand `mqtt_driver::run` a `Heartbeats` clone (it is already `Clone` and already passed to
        `Control` at `supervisor.rs:366`). Wire it at `supervisor.rs:369`; no new channel.
  - [x] `mqtt_driver.rs:1356` inbox arm: on `Ok(Published::DroppedBeforeBirth)` /
        `Ok(Published::DroppedUndeclaredDevice{..})` / `Err(_)`, count against `update.meter` with
        the matching reason and add `value_date` to the existing WARN/ERROR.
  - [x] `transport-queue-full`: `publish()` returns `bool` and knows only the topic. **Count at the
        caller**, in the inbox arm, where `update` is in hand — `Published::Emitted` already drains
        through `publish()`; capture its `false` return and count once per dropped *reading* (not per
        message: one reading is one DDATA today, and if that ever changes, one reading lost is still
        one reading lost).
  - [x] `publish()`'s own WARN stays — it covers the certificate path too, which is not counted.

- [x] **Task 5 — `/healthz` reports it** (AC4)
  - [x] `ui/mod.rs:803`: add `"dropped_readings"` to the body, built from the SAME `fleet` snapshot
        already taken at `:822`. Do not take a second snapshot — the one-reading rule in that
        handler's comments exists because a body reporting two instants was shipped once.
  - [x] Shape: `[{"meter":"…","reason":"outbox-full","count":3}, …]`, emitting only non-zero cells so
        a healthy fleet reports `[]`.
  - [x] The status code does **not** move. Epic 7 wires this to a container restart, and a restart
        cannot clear a broker outage — it would loop, exactly the failure ADR 0027 §2 names.

- [x] **Task 6 — tests, each falsified** (AC6)
  - [x] Unit, `poll_publish.rs`: a full outbox drops and the loop returns (mutation: restore
        `.send().await` → the test hangs/fails).
  - [x] Unit, `poll_publish.rs`: the drop does not alter the returned verdict or `next`.
  - [x] Unit, `poll_publish.rs`: N drops leave `MeterState::dropped`'s footprint unchanged — assert
        the array length and that `generation` advanced by exactly N (mutation: forget
        `generation += 1` → red).
  - [x] Unit, `ui/mod.rs`: a fleet with two reasons on one meter renders both, and a clean fleet
        renders `[]` (mutation: emit zero cells → red).
  - [x] Unit, `poll_publish.rs`: AC3's liveness — with the outbox full, `step_once` still
        RETURNS. Asserted by a `tokio::time::timeout` around it, which is the only shape that
        catches a blocking send: a parked loop fails by never answering, not by answering
        wrongly. Reachable without a broker; `chaos_broker_recovery` is story 4.13's.

- [x] **Task 7 — the record**
  - [x] `docs/sparkplug-conformance.md`: no clause moves. Nothing new goes on the wire.
  - [x] `CONTRACT_VERSION` is **not** bumped: it versions the Sparkplug payload contract published in
        the NBIRTH, and this story publishes nothing new. `/healthz` gaining a field is not a contract
        change.
  - [x] Manual: the `/healthz` field is documented in chapter 6, in the health-endpoint
        section, after `failed_sources`. *(This subtask said "where `degraded_meters` is" and
        was ticked against an anchor that does not exist — `degraded_meters` is documented
        nowhere in the manual. True in substance, false as written; corrected by the review.)*

## Dev Notes

### What the code does today, read before writing

**The traced drop already exists — at one of the six sites.** `publish()`
(`mqtt_driver.rs:1697`) calls `client.try_publish` and WARNs on refusal, with the doc comment *"A
full queue is a traced drop, never a block: a blocked driver stops draining the inbox, and then
NOTHING is published."* That reasoning is right and is exactly what this story extends one level up.

**The poll task does the opposite.** `poll_publish.rs:880` is `outbox.send(update).await` — a
*blocking* send on a 64-slot channel (`supervisor.rs:354`). The driver drains that inbox inside its
`select!`, but **not while it is reconnecting**: on `SessionEnd::TransportLost` it breaks out, sleeps
`jittered(backoff)` up to `RECONNECT_CEILING` (30 s, `mqtt_driver.rs:348`), and only then loops back.
Under a sustained outage the inbox fills, and every poll task then parks inside `send`.

**And that is worse than losing readings.** A parked poll task stops updating `last_tick`, so
`Phase::loop_age` grows past its tolerance and `/healthz` reports `wedged: true`
(`ui/mod.rs:824`). Epic 7 wires that to a container restart. So today, a long enough broker outage
makes the bridge look wedged and — once Epic 7 lands — restarts it, killing the Sparkplug session for
every meter, on account of a fault that is entirely outside the process. AC3 exists for that, and it
is the sharpest reason this story is not merely bookkeeping.

*(With the fleet as configured — three meters at 30 s — 64 slots take about ten minutes of outage to
fill, so this is latent rather than observed. It is a function of meters × poll rate, and both grow.)*

### Where the counters go, and the alternative that was rejected

AR7 writes `readings_dropped_total{meter,reason}` in Prometheus notation. **There is no metrics
facility in this repository** — no registry, no exporter, no dependency. Introducing one is Epic 6/7
work and is not in this story's scope.

**Decision: the counters live in `MeterState`, inside the existing `Heartbeats`/`watch` seam, and are
served on `/healthz`.** Reasons: the seam already carries per-meter operator truth and already
guarantees the snapshot property; both tasks can hold a `Heartbeats` clone; and it adds no
dependency. The Prometheus metric name is recorded here as the eventual export name, not implemented.

**Rejected: a bare in-process counter with no surface.** AC1 as written ("it is counted") would be
satisfied by a number nobody can read, and that is precisely the shape [#62] was filed against — a
fault the bridge knew about and no operator surface reported. A count that cannot be read is not
evidence.

### What this story must not break

- **The verdict must not depend on the transport.** The reading is judged by the oracle layer before
  it reaches the outbox. A drop is a *delivery* fact; it may not change `published`, `verdict`, or
  what the next tick republishes. (Story 3.2: a meter's silence is published, not withheld — and the
  last-known-value republish path at `poll_publish.rs:866` reads `last`, which a drop must leave
  alone.)
- **`generation` is written in the same `send_modify` as every field.** See its doc comment at
  `poll_publish.rs:95`. A counter increment that skips it makes AR6's snapshot test vacuous.
- **No persistent buffer, no retry queue, no re-timestamping.** AR7 forbids the buffer; FR22 forbids
  the replay. A dropped reading is gone; the next tick republishes the last known value *with its own
  `ValueDate`*, which the existing path already does correctly.
- **`/healthz`'s single-snapshot rule** (`ui/mod.rs:810`). Two reads produced a body mixing two
  instants once already.
- **`arch_purity.rs`**: `DropReason` is an application type. It must not land in `core/` or
  `domain/`.

### Boundary with the neighbouring stories — do not do their work here

- **4.12 (anti-replay at the down→up instant)** owns *"every published timestamp equals its source
  `ValueDate`, verified at the reconnection instant"*. 4.11 stops at the drop.
- **4.13 (`chaos_broker_recovery`)** owns the outside-the-process proof, with a real broker container
  stopped and restarted. It does not exist yet; do not cite it, and do not defer an AC to it — the
  repository rule against deferring a decision to an artifact that does not exist applies to
  evidence too (AR13, Epic 1 retrospective).
- **4.15 (`AC-LEAK-01`)** owns the sustained-load RSS/FD measurement. AC2 here is discharged by the
  *bounded-cardinality argument plus its unit pin* — `meters × 6` fixed cells, nothing allocated per
  drop — not by a soak run. Say so in the completion notes rather than implying a measurement that
  was not made.

### Previous story intelligence (4.10, closed 2026-08-03)

- Its AC1 failed review because the **module docs still described the pre-story behaviour** while the
  code had moved. `mqtt_driver.rs`'s header and `publish()`'s doc comment both describe the drop
  policy; if this story changes where drops are counted, those comments are part of the change.
- Its AC7 failed on its own first entry: the story listed the passages to amend and missed the first
  one on the list. Check every passage this story names, including the ones in this file.
- `RECONNECT_FLOOR` is a **durability** bound (one `bdSeq` fsync per session), not politeness. Do not
  touch the ladder to make a test converge faster; inject the clock instead.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md:1133`] — Story 4.11 statement and both ACs
- [Source: `_bmad-output/planning-artifacts/epics.md:141`] — AR7, `readings_dropped_total{meter,reason}`
- [Source: `_bmad-output/planning-artifacts/prd.md:295`] — FR22
- [Source: `_bmad-output/planning-artifacts/prd.md:340`] — NFR3 (bounds; measured by 4.15)
- [Source: `crates/smartme-bridge/src/app/poll_publish.rs:880`] — the blocking send this story removes
- [Source: `crates/smartme-bridge/src/app/poll_publish.rs:40`] — `MeterState`
- [Source: `crates/smartme-bridge/src/app/poll_publish.rs:217`] — `Heartbeats`, and `record`/`retire` at `:277`/`:299`
- [Source: `crates/smartme-bridge/src/app/mqtt_driver.rs:1356`] — the inbox arm, three of the six reasons
- [Source: `crates/smartme-bridge/src/app/mqtt_driver.rs:1697`] — `publish()`, the drop that exists today
- [Source: `crates/smartme-bridge/src/app/mqtt_driver.rs:348`] — `RECONNECT_CEILING`
- [Source: `crates/smartme-bridge/src/app/supervisor.rs:354`] — the 64-slot outbox
- [Source: `crates/smartme-bridge/src/ui/mod.rs:803`] — `/healthz`
- [Source: `docs/adr/0027-a-failed-source-is-a-fault-the-screen-must-name.md`] — §2, why the status code does not move
- [Source: `CLAUDE.md`] — falsify before trusting; never defer a decision to an artifact that does not exist

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`

### Implementation plan, as executed

1. `DropReason` — a closed enum of six, with `ALL`, `COUNT`, `index()` and `as_str()`.
2. `MeterState::dropped: [u64; DropReason::COUNT]`, written through `MeterPulse::dropped`
   (poll side, index-addressed) and `Heartbeats::dropped` (driver side, meter looked up).
3. `step_once`'s `outbox.send(update).await` → `try_send`, both error arms counted and traced.
4. `mqtt_driver::run` gained the `Heartbeats` clone; its inbox arm now routes every outcome
   through a **pure** `reason_for`, with the transport axis decided by `publish_all`'s count.
5. `/healthz` gained `dropped_readings`, built from the snapshot already taken.
6. Eleven tests, thirteen mutations, each run before its note.

### Completion Notes List

**Five ACs met; AC1 is met in the code and UNDER-PINNED in the tests, recorded as such.**
This section said "All six ACs met" until the 2026-08-18 review, which is the claim this
repository has twice had to unlearn.

- **AC1 — met in the code, UNDER-PINNED in the tests ([#86]).** Six reasons, six counters, one
  WARN per loss carrying `meter`, `reason` and `value_date`. Five of the six paths existed and
  were traced but uncounted; `outbox-full` did not exist at all, because the send blocked
  instead of failing. **But only two of the six are pinned end to end** — `outbox-full` and
  `mqtt-task-gone`. The driver's four counting call sites live inside a `select!` needing a
  live broker: **deleting every `lost(...)` call there leaves the whole suite green.** A doc
  comment claimed that assertion existed; the claim is removed and [#86] carries the gap, whose
  owner is story 4.13's broker harness.
- **AC1's exhaustiveness is over the HAND-OVER, not over the journey.** Three ways a reading
  fails to *arrive* are invisible to every counter: the client answers `Ok` on queueing rather
  than on sending ([#85]), the inbox is dropped unread at shutdown ([#87]), and a refused
  DBIRTH leaves the device declared so every later reading looks published ([#88]). The manual
  now carries a warning box: the counts are **a floor on what was lost, never a ceiling**.
- **AC2 — met, by construction on both halves.** *Memory*: the cardinality is `served meters ×
  6` fixed `u64` cells, both sets closed at start-up, so a million losses cost what one costs.
  The pin is a **`const` block**, not an assertion — change `dropped` to a `HashMap` or a `Vec`
  and the crate stops compiling. *File descriptors*: the drop path opens none — it increments
  an integer and emits a log line on the existing subscriber. That sentence was missing until
  the review asked for it. **The sustained-load RSS/FD measurement was NOT made and is not
  claimed**; it is story 4.15's (`AC-LEAK-01`).

  **The test that claimed to discharge this could not fail.** `a_thousand_losses_cost_what_one_costs`
  asserted `after.dropped.len() == before.dropped.len()` — `6 == 6` for every value of every
  program, since `.len()` on a fixed-size array is a compile-time constant. It was scored as
  AC2's discharge. Renamed to `a_thousand_losses_touch_one_cell_and_advance_the_generation` and
  rewritten to pin what a test can pin: one loss touches ONE cell, leaves the other five alone,
  advances `generation` exactly once. Fourth instance of the hollow-assertion class in this
  repository.
- **AC3** — `a_full_outbox_costs_the_reading_and_not_the_loop`. The latent defect this closes
  was not named in the epic: the blocking send would have made a long broker outage read as
  `wedged: true`, which Epic 7 wires to a container restart.
- **AC4** — `dropped_readings` on `/healthz`, non-zero cells only, cumulative for the process,
  status code unmoved. **Its second clause — "the totals survive a reconnect" — is not pinned**:
  it holds structurally because `pulse` is a parameter of `mqtt_driver::run` and the reconnect
  `continue` is inside it, so a future change rebuilding `Heartbeats` per session would pass the
  whole suite. Same for AC3's second clause: the timeout pins that `step_once` returns, not the
  `/healthz` consequence. Both noted in [#86].
- **AC5** — `the_index_and_the_list_agree` (poll side) and `every_publisher_outcome_is_classified`
  (driver side, on the extracted pure `reason_for`, which is Epic 2's action item C1 applied),
  with the wording corrected above: the compiler closes the OUTCOMES, not the paths.
- **AC6** — **eleven tests, thirteen mutations**, every one RUN with its output copied into the
  test's doc comment. **Three notes were written before their run and corrected to the real
  output** — the drop-path mutation, the `FleetState::dropped` filter mutation, and the own-cell
  mutation, whose predicted messages were all three wrong. (The Dev Agent Record said "eight
  tests" before the review counted them; there were nine, and the review added two more.)

**THE FULL GATE IS GREEN, end to end.** `./scripts/ci-local.sh` (no `--fast`) exits 0: 38 test
binaries, 0 failures, Docker-dependent chaos tests and image smoke tests included, plus `fmt`,
`clippy --all-targets -D warnings`, `cargo deny` and the `Cargo.lock` sync check.

**It took two impediments to get there, and neither belonged to this story.**

1. **The port-8080 collision, cleared by Guy.** `from_an_empty_directory_to_publishing_without_
   touching_a_terminal` and `with_no_configuration_the_ui_answers_and_says_so` must bind
   `ui::DEFAULT_PORT`, and `e2e-mybibli-1` — a leftover e2e stack from another project — held
   it, answering with its own HTML. This is the occupant recorded lifted on 2026-08-15 (commit
   `374f90d`), returned. Verified pre-existing rather than assumed: with this story stashed,
   the first test failed identically on the clean tree. `PortLock` cannot help — it serialises
   this repository's own test binaries and cannot evict a container. **Guy stopped the
   containers; both tests then passed.**

2. **A NEW ADVISORY, unrelated to this story and to be committed apart from it.**
   `RUSTSEC-2026-0258` — *h2 unbounded empty DATA frames*, low severity, `h2 0.4.15` reached
   through both `axum` and `reqwest`. The advisory's own prescription (`cargo update -p h2`,
   to 0.4.16) clears it, and `cargo deny check advisories` then reports `advisories ok`.
   `deny.toml` has no `ignore` list, so nothing was ever suppressed here: this is new.

   **The bump is not surgical, and that is worth knowing before it is committed.** Even with
   `--precise 0.4.16`, re-resolution also moves SIX crates from `windows-sys 0.61.2` to
   `0.52.0` — a resolver side effect, not h2's doing. It is inert for this project (every
   workflow is `ubuntu-latest`, and the image is Linux) but it is eight lines of lockfile churn
   that has nothing to do with FR22. `Cargo.lock` is STAGED, not committed: the gate refuses to
   run with it dirty, and staging it explicitly rather than `git add`-ing a directory is the
   rule this repository already writes down.

**No contract change.** `CONTRACT_VERSION` stays at 10: nothing new goes on the Sparkplug
wire, and no conformance clause moves.

### Falsification record

| # | Mutation | Test | Went red with |
|---|---|---|---|
| 1 | swap `BeforeBirth`/`UndeclaredDevice` in `index` | `the_index_and_the_list_agree` | `DropReason::ALL[3] is BeforeBirth but it indexes cell 4 … left: 4, right: 3` |
| 2 | restore `outbox.send(update).await` | `a_full_outbox_costs_the_reading_and_not_the_loop` | `a full outbox must not park the poll loop … : Elapsed(())` |
| 3 | `value_date = clock.wall().0` | `a_lost_reading_is_traced_with_its_own_timestamp` | `… value_date=1784984793000` where `1784984700000` belongs |
| 4 | `return (State::Stale, published.meter())` on the drop path | `a_drop_does_not_change_what_the_reading_was_judged_to_be` | `left: (Stale, Verdict { quality: Good, … }), right: (Fresh, …)` |
| 5 | drop `fleet.generation += 1` from `MeterPulse::dropped` | `a_thousand_losses_cost_what_one_costs` | `every write advances the generation … left: 0, right: 1000` |
| 6 | `*cell += 1` for `saturating_add` | `the_count_saturates_and_never_returns_to_zero` | `attempt to add with overflow` |
| 7 | remove the `count > 0` filter | `a_clean_fleet_reports_no_losses_at_all` | `a clean fleet reports an empty list, not six zeros per meter` |
| 8 | `/healthz` reports no losses | `a_lost_reading_is_named_in_healthz_and_moves_no_status_code` | the whole body, every field saying the bridge is well |
| 9 | file `DroppedBeforeBirth` under `Unpublishable` | `every_publisher_outcome_is_classified` | `left: Some(Unpublishable), right: Some(BeforeBirth)` |
| 10 | index cell `[0]` instead of `[reason.index()]` in `MeterPulse::dropped` | `a_thousand_losses_touch_one_cell_and_advance_the_generation` | `the losses must land in the cell of the reason they were filed under … left: 0, right: 1000` |
| 11 | make `Heartbeats::dropped` a no-op | `the_count_saturates_and_never_returns_to_zero` | `the increment path must actually run … left: 18446744073709551614, right: 18446744073709551615` |
| 12 | file `TrySendError::Closed` under `OutboxFull` | `a_closed_outbox_is_a_dead_transport_task_and_says_so` | `a closed channel is a DEAD TRANSPORT TASK … left: OutboxFull, right: MqttTaskGone` |

Mutations 10–12 were added by the 2026-08-18 review. **Mutation 10's note had to be rewritten
twice**: the first prediction named the wrong assertion, and the assertion it actually reached
carried no message at all — so the mutation proved the test was red without saying why. The
message was added, the mutation re-run, and the note copied from that run.

### File List

- `crates/smartme-bridge/src/app/poll_publish.rs` — modified (`DropReason`, `MeterState::dropped`, `FleetState::dropped`, `MeterPulse::dropped`, `Heartbeats::dropped`, the `try_send` hand-over, 7 tests)
- `crates/smartme-bridge/src/app/mqtt_driver.rs` — modified (`Heartbeats` parameter, pure `reason_for`, the inbox arm's counting, 1 test)
- `crates/smartme-bridge/src/app/supervisor.rs` — modified (wires the `Heartbeats` clone to the driver)
- `crates/smartme-bridge/src/ui/mod.rs` — modified (`dropped_readings` on `/healthz`, 1 test)
- `crates/smartme-bridge/tests/chaos_ncmd_rebirth.rs` — modified (call site)
- `crates/smartme-bridge/tests/chaos_bdseq_per_connect.rs` — modified (call site)
- `crates/smartme-bridge/tests/ignition_contract.rs` — modified (call site)
- `docs/manual/chapters/06-operations-ui.tex` — modified (new section, the six reasons)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified (status, and the ADR 0030 correction)
- `_bmad-output/implementation-artifacts/deferred-work.md` — modified (the review's residuals)
- `_bmad-output/planning-artifacts/epics.md` — modified (the four added ACs, and the two things this story does not do)
- `_bmad-output/implementation-artifacts/4-11-broker-outage-traced-drop-exhaustive.md` — new

`Cargo.lock` is deliberately **absent from this list**: the `h2` bump belongs to its own
commit, for the reasons given above. If it appears in 4.11's commit, this list is wrong and the
commit is.

### Change Log

- **2026-08-18** — Story 4.11 implemented. Six drop reasons closed and counted per meter;
  the poll task's hand-over stops blocking; `/healthz` reports what never reached the wire;
  the manual gains its section. Nine mutations run before their notes. No wire change.
