# Story 4.13: `chaos_broker_recovery`

Status: done

> **ONE CLAUSE OF THIS STORY'S ACCEPTANCE CRITERION IS NOT ACHIEVABLE BY THE MECHANISM IT
> NAMES, and that was established at drafting rather than discovered mid-implementation.**
> `epics.md` asks the subscriber to observe *"the node dying, the node rebirthing"* across a
> broker container stop. **A will is published by a living broker.** Stop the container and the
> will dies with the process that held it — and the observer, being a client of the same broker,
> is disconnected in the same instant. There is nothing to see and nobody to see it.
>
> This is not a reason to weaken the story. It is the reason AC2 below **measures** the death
> rather than asserting it, and it connects to [#43], which recorded the same absence on the
> session-takeover path and left the cause open.

## Story

As the maintainer,
I want the down→up transition proven from outside,
so that anti-replay is a measured property rather than a reviewed one.

## Acceptance Criteria

**AC1 — the recovery, observed from outside the process.**

**Given** a running bridge and a broker container that is **stopped and restarted**
**When** an independent subscriber records the whole sequence
**Then** it observes the node **re-birthing** under a session number that advanced
**And** no published timestamp post-dates its own `ValueDate` — across the outage and after it.

**AC2 — the death is measured, and the record says what was seen.**

**Given** the same run
**When** the broker is stopped
**Then** the test **records whether any NDEATH reaches a subscriber**, and the completion notes
state the observation — including "none, and here is why the mechanism cannot deliver one".

*Decided at drafting. The epic asked for an assertion; the mechanism cannot support one, and
asserting it would produce either a permanently red test or — worse — a green one that waited
for something else. [#43] asked this exact question for the takeover path and left it open;
this story answers it for the outage path, by measurement.*

**AC3 — the drop counters are pinned end to end, which closes [#86].**

**Given** readings handed to the driver while the broker is down
**When** the outage ends
**Then** `/healthz`-visible counters have moved for at least one driver-side reason
**And** deleting the driver's `lost(...)` calls makes this test fail.

*Decided at drafting. Story 4.11 shipped six drop reasons with only two pinned end to end;
[#86] records that deleting the driver's four counting call sites leaves the whole suite green.
This is the harness that makes them reachable, and it is why 4.13 was named as [#86]'s owner
rather than left ownerless.*

**AC4 — falsification.**

**Given** the anti-replay stamping is deliberately broken
**When** this test runs
**Then** it fails, and the run's output is copied into the test.

## Tasks / Subtasks

- [x] **Task 1 — a broker whose port survives a restart**
  - [x] `common::start_broker_on_fixed_port()`: `GenericImage::…with_mapped_port(host, 1883.tcp())`.
        A container restarted with an **ephemeral** mapping comes back on a different host port
        and the bridge, holding the old one, reconnects forever to nothing — a test that would
        hang rather than fail.
  - [x] Choose the host port by binding an ephemeral socket, reading its number and releasing
        it. **Do not hardcode one**: story 4.12's own gate was blocked twice by a fixed port
        another project held, and ADR 0037 exists because of it.
  - [x] Keep it beside `start_broker` rather than replacing it: every other chaos test wants the
        ephemeral mapping, which cannot collide.

- [x] **Task 2 — the outage** (AC1, AC2)
  - [x] Birth, publish one reading with a `ValueDate` **hours in the past**, assert it on the
        wire — the premise, so a later assertion cannot pass by measuring the wrong thing.
  - [x] `container.stop_with_timeout(Some(0))`, then hand the driver more readings **while the
        broker is down**. They are what AC3 counts.
  - [x] Record every message the observer received between the stop and the restart. **This is
        AC2's measurement**: expect none, and say so in the notes with the reason, rather than
        asserting a death the mechanism cannot deliver.
  - [x] `container.start()`, re-subscribe the observer — **it was disconnected too**, and a
        subscriber that does not re-subscribe observes silence and calls it a bridge failure.

- [x] **Task 3 — what recovery must look like** (AC1)
  - [x] Assert a new NBIRTH arrives with `bd_seq` **greater than** the first — story 4.10's
        property, re-observed here through a different failure.
  - [x] Assert the DBIRTH re-declaring the known reading carries **that reading's** `ValueDate`,
        not the recovery instant. This is the anti-replay clause, measured.
  - [x] Send a reading after recovery and assert it carries **its own** time — so the test
        catches a bridge that froze on one timestamp as well as one that followed the clock.

- [x] **Task 4 — the counters** (AC3, [#86])
  - [x] Build a real `Heartbeats::for_meters([...])` rather than `default()`, so a driver-side
        count lands somewhere. **The three existing chaos tests pass `default()`**, which counts
        nothing — that is exactly why [#86] exists.
  - [x] After recovery, snapshot the fleet and assert at least one driver-side reason moved.
        **Do not assert WHICH**: whether an outage produces `transport-queue-full`,
        `before-birth`, or both depends on timing this test does not control, and a test that
        pins the timing would be pinning the harness rather than the property.
  - [x] Falsify by deleting the `lost(...)` calls from the driver's inbox arm.

- [x] **Task 5 — the record**
  - [x] `docs/sparkplug-conformance.md`: move a row **only** if this test witnesses its clause.
        Recovery is not a clause; do not move anything on this test's strength without reading
        the clause it would move.
  - [x] Comment on [#43] with AC2's measurement, whichever way it goes.
  - [x] Comment on [#86] and close it if AC3 holds.
  - [x] `CONTRACT_VERSION` is **not** bumped: nothing about the payload changes.

## Dev Notes

### What the harness can and cannot do, measured before drafting

- **`testcontainers` 0.27 has `stop_with_timeout` and `start`** on `ContainerAsync`, so the same
  container can be stopped and restarted. Verified in the vendored source.
- **`with_mapped_port(host, container)` exists**, which is what makes the restart usable.
- **A stopped broker publishes nothing**, including wills. This is the finding in the header.
- **The observer is a client of the same broker**, so it dies and must be rebuilt. Any test that
  keeps its old subscription across the restart is measuring its own disconnection.

### The trap this story must not fall into

`chaos_no_replay_at_reconnect` (story 4.12) already proves anti-replay across a **session
takeover**. It is easy to write 4.13 as the same test with a different break and call the
property twice-proven.

**They are different code paths and the difference is the point.** A takeover drops the socket
while the broker keeps running: the bridge reconnects immediately, the broker still holds
session state, and the observer never loses its subscription. A container stop removes the
broker: reconnection fails repeatedly against nothing, the backoff ladder actually runs, and
every client's session is gone. The second is what a real outage does, and it is the one the
`RECONNECT_CEILING` and the drop counters were built for.

Cite 4.12 as the *unit-and-takeover* proof; do not cite it as evidence for anything here.

### What must not break

- **`RECONNECT_FLOOR` is a durability bound** (one `bdSeq` fsync per session), not politeness.
  Do not shorten the ladder to make this test converge; inject a clock or wait.
- **The test must not be timing-pinned.** A broker restart takes as long as it takes. Use
  `common::wait_for` with generous deadlines and assert on **values**, never on ordering the
  harness does not control.
- **`arch_purity`**: this is a test; nothing here may reach into `core/` or `domain/` for a
  capability production does not have.

### Previous story intelligence (4.11 and 4.12, both closed 2026-08-18)

- **A test asserting over an empty set is worse than no test.** 4.12's first draft swept zero
  messages and was caught only because the count was printed. If this test loops over "whatever
  arrived", print what it saw.
- **A note written before its run is not a falsification.** Three notes across the two stories
  did not survive their mutation and had to be rewritten from the real output.
- **Claims in doc comments get audited.** If a comment here says a property is proven, name the
  assertion that proves it.
- **`Heartbeats::default()` swallows counts silently** — that is [#86]'s mechanism, and Task 4
  exists to stop using it.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md:1167`] — Story 4.13's original ACs
- [Source: `crates/smartme-bridge/tests/common/mod.rs`] — `start_broker`, `named_subscriber`, `wait_for`
- [Source: `crates/smartme-bridge/tests/chaos_no_replay_at_reconnect.rs`] — story 4.12's takeover proof, the sibling this must not duplicate
- [Source: `crates/smartme-bridge/tests/chaos_bdseq_per_connect.rs`] — the `bdSeq`-advances property, re-observed here
- [Source: `crates/smartme-bridge/src/app/mqtt_driver.rs`] — `RECONNECT_FLOOR`, `RECONNECT_CEILING`, the inbox arm's `lost(...)` calls
- [Source: `docs/adr/0011-*.md`] — the two-mechanism death design this story measures
- [Source: `docs/adr/0037-*.md`] — why no fixed port is hardcoded
- [Source: `CLAUDE.md`] — falsify before trusting; never defer a decision to an artifact that does not exist

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-18.

### Debug Log References

`chaos_broker_recovery` prints five measurement lines — four `AC2 MEASUREMENT` and one
`AC3 MEASUREMENT` — under
`--nocapture`; they are the story's measurements, not diagnostics, and they stay.

### Completion Notes List

**AC1 — met.** The bridge is observed re-birthing under a session number that advanced, and
neither the re-declared reading nor the one acquired after recovery carries anything but its own
`ValueDate`. Falsified twice, both mutations run and their output copied into the test's module
header: moving `Emission::DeviceBirthRedeclaring` to `PublicationInstant` (`left: 1787081115581,
right: 1786968000000`), and removing `publisher.new_session()` (`born 1, reborn 1`).

**AC2 — met, and THE DRAFTING PREMISE WAS WRONG.** The story predicted no death at all. **Exactly
one NDEATH reaches the independent subscriber, on every run measured, carrying the ended session's
own `bdSeq`.** The will is `(QoS 1, retain false)`, so it is not a retained message surviving the
restart: mosquitto's SIGTERM path publishes the wills of the sessions it tears down and gets them
out before closing subscriber sockets. **A stopped broker is not a crashed one, and the contrast
was measured rather than reasoned** — the same test run with `docker kill --signal SIGKILL`
observes `0 []`, nothing at all. So the residue of [#43] is real and stays open: a broker that
crashes, loses power or is killed still delivers no death. What 4.13 removes is the *unqualified*
form of the claim.

**AC2's own measurement had to be repaired before it could be believed.** The first version
counted only what arrived before the restart and measured `0` on 2 runs of 37 against `1` on the
other 35 — which reads as "the will sometimes goes missing", a false property produced by real
runs. It was the boundary that moved, not the will. The count is now taken on **both** sides of
the restart and reported as a total; the total was `1` on all 17 runs measured that way. This is
the same defect story 4.12 caught in itself (an assertion over an empty set) wearing different
clothes: a measurement whose window the harness chose.

**AC3 — met, and [#86] is answered.** Three readings handed over during the outage are counted,
`("garage", "before-birth", 3)`, through the driver's own `lost(...)` calls. Falsified: deleting
both call sites from the driver's inbox arm turns this test red with `NOT ONE DRIVER-SIDE DROP WAS
COUNTED … The whole fleet reads: []`. **The same mutation leaves the 258 unit tests passing** —
run, not assumed, and that is exactly the hole [#86] recorded. The mechanism is
`Heartbeats::for_meters([...])` instead of `default()`; `default()` is `for_meters([])`, the
driver's lookup finds no entry, and every count goes nowhere.

**AC4 — met.** Four mutations, all run, all output copied into the test's module header: the two
under AC1, the counter deletion under AC3, and the fixed-port helper under Task 1.

**A FLAKE WAS FOUND AND ITS CAUSE WAS THE HARNESS, not the bridge.** The test failed 2 runs in
about 12 with `no NBIRTH arrived within 60 s`. `wait_for_broker` returned on a successful **TCP**
handshake, and **`docker-proxy` binds the host port before mosquitto is accepting** — so the test
declared the outage over while the broker was still starting. A bridge reconnect attempt landing
in that window spends `rumqttc`'s five-second connection timeout, fails, and doubles its backoff
toward the 30 s ceiling plus up to 50 % jitter, which overran a 60 s deadline. Two repairs, both
recorded in the code: the readiness probe now waits for an **MQTT CONNACK**, and
`REBIRTH_DEADLINE` is sized from the ladder's own arithmetic (30 s ceiling × 1.5 jitter + one 5 s
wasted attempt ≈ 50 s; set to 120 s) rather than guessed. **30 consecutive passes since.**

**One thing checked along the way and worth keeping:** `rumqttc` wraps TCP connect, CONNECT and
CONNACK in a single five-second `time::timeout` (`eventloop.rs:150-158`). A half-started broker
therefore cannot wedge the bridge waiting for a CONNACK that never comes — in this test or in
production.

**Not asserted, deliberately:** which drop reason moves. Whether an outage produces `before-birth`,
`transport-queue-full` or both depends on where the reconnect ladder stood when each reading
arrived, which this test does not control. Pinning it would pin the harness.

**`CONTRACT_VERSION` unchanged** at 10 — `git diff` over `crates/*/src/` is empty; nothing about
the payload changes.

**[#86] commented and CLOSED** (`issuecomment-5334037516`); **[#43] commented and left OPEN**
(`issuecomment-5334040415`) — its residue is genuine, in two parts: the session-takeover path this
test does not exercise, and a broker that crashes or is killed, which was measured here to deliver
no death at all.

**`docs/sparkplug-conformance.md`: no row moved.** Recovery is not a clause. One prose paragraph
was amended: the Story 4.10 note claiming *"no NDEATH reaches a subscriber on the reconnect path at
all"* is now qualified by 4.13's measurement, with the SIGKILL contrast stated so the correction
cannot be read wider than it goes.

**Adversarial code review, 2026-08-18, run on a different model (Sonnet) than the one that wrote
the code.** Verdict: sound with reservations. It re-ran all four mutations and confirmed their
exact messages, re-ran `--lib` under the counter mutation (`258 passed`), checked the issue states
on GitHub, and confirmed no conformance row moved. Two findings, both accepted and both fixed:

1. **The AC3 comment argued a mechanism's robustness without naming its window.** A reading
   reaching the inbox arm before the driver notices the transport is gone is published into a
   live-looking session and discarded with the aborted event loop, uncounted — so all three
   readings landing in that window would turn AC3 red against a correct bridge. The reviewer
   established this by reading and presented it as new; it is in fact [#85], already open and
   already recorded at the call site in `reason_for`'s doc comment. The behaviour is known and
   tracked; **the omission in this test's comment was real**, and the comment now names the
   window, says the failure is the safe one, and points at [#85] first.
2. **A false count in the Debug Log References** — "four measurement lines" where there are five.
   Corrected. Exactly the kind of claim `CLAUDE.md` says gets audited, in the notes rather than
   the code.

**A limit the review named and this record should keep:** the run tallies cited above ("2 runs of
37", "30 consecutive passes") live in a comment and in these notes, with no committed artefact
behind them. They are reproducible but not verifiable after the fact.

### File List

- `crates/smartme-bridge/tests/chaos_broker_recovery.rs` — new
- `crates/smartme-bridge/tests/common/mod.rs` — modified (`start_broker_on_fixed_port`,
  `an_unused_host_port`, `wait_for_broker`)
- `docs/sparkplug-conformance.md` — modified (the 4.10 note qualified; no row verdict changed)
- `_bmad-output/implementation-artifacts/4-13-chaos-broker-recovery.md` — modified
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Review Findings (2026-08-19)

A second review, run against the code rather than the record, and by mutation rather than by
reading. It confirms the story's own adversarial pass and adds one finding that pass did not
reach: **the evidence [#86] was closed on covers one of the two call sites it names.**

- [x] [Review][Patch] **[#86] was closed on a proof of half its subject, and the half is not
      the one its title counts.** The driver has TWO `lost(...)` call sites, and this test
      reaches one of them — the `Some(reason) => lost(reason, fault)` arm, through
      `before-birth`. **Measured: deleting only `lost(DropReason::TransportQueueFull, None)`
      (`mqtt_driver.rs:1408`) leaves this test green.** The closing claim (*"deleting both call
      sites turns this test red"*) is true and was read as more than it says. The test's module
      header and its AC3 assertion now name the residue, and [#95] carries it. **[#86] stays
      closed**: its literal question — deleting *every* call leaves the suite green — is
      answered.
- [x] [Review][Patch] **A count in the assertion message was wrong** — *"deleting all four of
      them"* about the `lost(...)` calls, of which there are two; the four belongs to [#86]'s
      title, which counts reasons. This is the second miscount this story has had to repair
      (the first was four measurement lines against five), and both were in prose about
      mechanisms that are themselves correct.

**Verified and left standing:** `FleetState::dropped()` omits zero cells, so AC3's
`!moved.is_empty()` is a real assertion and not a shape that passes on an empty fleet; the AC2
measurement and its two-sided window; the readiness probe's CONNACK wait; and the fixed-port
helper's stated race. The mutations recorded by the story's own review were not re-run — the one
they claimed to cover was, and it behaved as the record says.

### Change Log

- 2026-08-18 — Story 4.13 implemented. `chaos_broker_recovery` proves the down→up transition from
  outside the process across a real broker container stop/restart: session number advances, no
  timestamp follows the clock, and the driver's drop counters are pinned end to end, which answers
  [#86]. AC2's measurement corrected the story's drafting premise — one NDEATH does reach a
  subscriber on an orderly stop, none on a SIGKILL. A harness flake (TCP readiness mistaken for
  broker readiness) was diagnosed and repaired.
- 2026-08-19, review — two patches applied. [#95] opened for the `transport-queue-full` call
  site, which this test does not reach; [#86] stays closed on its own words. Story closed `done`.
