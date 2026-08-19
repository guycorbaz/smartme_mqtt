# Story 4.14: `chaos_poller_wedge`

Status: review

> **THE WEDGE THIS STORY IS NAMED AFTER CANNOT BE CAUSED BY THE SOURCE, and that was
> established by reading the code before drafting rather than discovered mid-implementation.**
>
> `epics.md` asks for *"a source that hangs beyond every deadline"*. There is no such source.
> Every fetch is wrapped in `tokio::time::timeout(config.fetch_timeout, source.fetch(meter))`
> (`poll_publish.rs:671`), and the heartbeat is written **before** it — *"Heartbeat FIRST: before
> anything that can block"* (`:659`). So a hanging source costs at most `fetch_timeout`, and the
> loop ticks on either side of it.
>
> **And the arithmetic says it can never be enough.** `fetch_timeout` is **10 s**, fixed in code
> and not settable by an operator (`config.rs`, `FETCH_TIMEOUT`). The wedge allowance is
> `WEDGED_AFTER_PERIODS × period` = `3 × period` (`ui/mod.rs:51`, `:363`), and `PERIOD_MIN` is
> **5 s** (`config.rs:49`, ADR 0020). At the worst legal setting the allowance is **15 s** against
> a **10 s** maximum block. A hanging source cannot make `/healthz` say `wedged`, at any
> configuration this bridge accepts.
>
> That is not a reason to drop the story. It is the reason AC1 below **pins the arithmetic as an
> invariant** instead of asserting a scenario, and the reason AC3 has to build its wedge out of a
> `PollConfig` no validator would produce. The one wedge this bridge has actually suffered —
> every poll task parked in a blocking `outbox.send().await` during a broker outage — was removed
> by story 4.11, and its regression test already stands.

## Story

As the maintainer,
I want a wedged poll loop to be detectable from outside the process,
so that lying by omission is caught the same way lying by commission is.

## Acceptance Criteria

**AC1 — a hanging source cannot be mistaken for a wedge, and the margin is pinned.**

**Given** the three constants that decide it — `fetch_timeout`, `PERIOD_MIN` and
`WEDGED_AFTER_PERIODS`
**When** any of them is changed so that a single blocked fetch could exceed the wedge allowance
**Then** a test fails, naming the consequence: under Epic 7 the container is restarted, and every
meter's Sparkplug session dies, because somebody else's server was slow.

*Decided at drafting, 2026-08-19. This is the property AR12 actually rests on — "an honest STALE
never triggers a restart" — and today it holds by an arithmetic coincidence that nothing records.
`fetch_timeout` is not operator-settable, so the margin is ours to keep; it is exactly the kind of
constant a future story raises for a good local reason with no idea what it is holding up.*

**AC2 — the honest silence, end to end, and it must read as healthy.**

**Given** a bridge whose source accepts the connection and never answers
**When** several poll periods elapse
**Then** an independent subscriber sees the metrics published `Stale` — never `Good`, never
absent
**And** `/healthz` answers **200** with `wedged: false`, its `loop_age_ms` advancing across two
readings
**And** the meter is named in `degraded_meters`, so the silence is REPORTED while not being
treated as a fault.

*The Stale half is story 1.14's (`chaos_stale_on_cloud_timeout`) and is not re-proven here; what
is new is the three together. "Stale on the wire", "named on `/healthz`" and "still 200" have
never been asserted in the same run, and the whole distinction this story is named for — wedged
versus idle — is the contrast between them. **This is also the answer [#62] is owed.** That issue
says a stale meter is published `Bad_Stale` while `/healthz` reads healthy; `degraded_meters`
exists since story 3.3 and names it, so the reporting half is already built and the healthy status
code is AR12's intent rather than an oversight. Asserting both in one run is what lets [#62] be
closed on evidence instead of argued.*

**AC3 — the wedge, observed from outside the process, and it must read as unhealthy.**

**Given** a bridge whose poll loop is blocked for longer than its wedge allowance
**When** `/healthz` is read
**Then** it answers **503** with `wedged: true`, and the body's `loop_age_ms` exceeds its
`allowed_ms`
**And** when the block ends, a later reading answers **200** again — the verdict is a measurement,
not a latch.

*Decided at drafting, and the mechanism is stated rather than left to the implementation: the
test constructs `PollConfig { interval: 300 ms, fetch_timeout: 5 s }` directly. **That is a
configuration `config.rs` would refuse** — `PERIOD_MIN` is 5 s — and the struct is public, which
is how `chaos_stale_on_cloud_timeout` already runs at a 300 ms period. So this AC proves the
WIRING (heartbeat → `FleetState` → `loop_age` → status code) against a cause an operator cannot
produce today. Written down because the alternative is a test that looks like it proves a
reachable failure. AC1 is what keeps it unreachable.*

**AC4 — falsification.**

**Given** each new assertion
**When** the mechanism it names is deliberately broken
**Then** the test goes red, and the run's output is copied next to the test.

## What is already true, and where

**None of this is this story's work to redo.** Read it before writing anything.

| Property | Where | Since |
|---|---|---|
| A hanging source times out into `Stale` rather than wedging | `poll_publish::tests::a_silent_cloud_times_out_into_stale_instead_of_wedging` | story 1.14 |
| An independent subscriber sees `Stale`, never a stale value dressed as good | `chaos_stale_on_cloud_timeout` | story 1.14 |
| `/healthz` reports `wedged` past three periods and not before | `ui::tests::a_wedged_poll_loop_is_unhealthy_and_a_slow_one_is_not` — **on a hand-built `FleetState`** | story 6.1 |
| A hot period change does not make a healthy loop look wedged | `ui::tests::a_hot_period_change_does_not_make_a_healthy_loop_look_wedged` | story 3.5 |
| A full outbox costs the reading and not the loop | `poll_publish::tests::a_full_outbox_costs_the_reading_and_not_the_loop` | story 4.11 |
| The heartbeat is written before anything that can block | `poll_publish.rs:659`, and an idle loop still re-paces (`:1382`) | stories 1.11, 3.5 |

**The gap is one link, and it is the link Epic 7 will hang a container restart on:** nothing
drives a REAL bridge into either state and reads the REAL endpoint. `wedged` is proven over a
`FleetState` a test wrote by hand, and the honest-silence path is proven on the wire with nobody
looking at `/healthz`.

## Tasks / Subtasks

- [x] **Task 1 — the arithmetic invariant** (AC1)
  - [x] A unit test beside `WEDGED_AFTER_PERIODS` asserting
        `fetch_timeout < PERIOD_MIN × WEDGED_AFTER_PERIODS`, with the three values named and the
        consequence spelled out in the failure message.
  - [x] `fetch_timeout` is not a constant today — it is a bare `Duration::from_secs(10)` at
        **one** production site, `config.rs`, `FETCH_TIMEOUT`. (The drafting note said two; the second,
        `reconfigure.rs:483`, is inside `mod tests` and was corrected on reading. Counting a test
        as production is how a margin gets pinned in the wrong place.) **Give it a name**
        (`FETCH_TIMEOUT`) so the invariant below pins the value the bridge actually runs with.
        This is the only production change this story is allowed to make.
  - [x] Falsify: raise the named constant to 20 s and observe the message.

- [x] **Task 2 — the honest silence, end to end** (AC2)
  - [x] A tarpit source: a `TcpListener` that accepts and never writes. **Not TEST-NET-1** —
        `chaos_stale_on_cloud_timeout` uses an unroutable address, which is a connect timeout;
        this AC needs a connection that is established and then silent, which is the shape of a
        server that is up and stuck.
  - [x] Drive `app::run` with `ui_port: Some(port)` and an ephemeral port (ADR 0037: never a
        constant).
  - [x] Assert on the wire (`Stale`, no value) AND on `/healthz` (200, `wedged: false`) in the
        same run, plus `loop_age_ms` advancing between two readings taken a period apart.

- [x] **Task 3 — the wedge, end to end** (AC3)
  - [x] Same tarpit, `PollConfig { interval: 300 ms, fetch_timeout: 5 s }` built directly.
  - [x] Poll `/healthz` until 503, with a deadline sized from the arithmetic (allowance 900 ms +
        one fetch 5 s ≈ 6 s; give it 30 s) rather than guessed.
  - [x] Assert the body agrees with the code: `wedged: true` and `loop_age_ms > allowed_ms`. The
        two were read from separate clock samples until a review of story 6.1; asserting them
        together is what keeps that repaired.
  - [x] **Then let the fetch time out and assert a later 200.** Without this the test would pass
        against a bridge that latched `wedged` for ever, which is a worse failure than a missing
        verdict — under Epic 7 it is a restart loop.

- [x] **Task 4 — falsification** (AC4)
  - [x] Mutation: `WEDGED_AFTER_PERIODS` 3 → 100. Task 3's test must redden.
  - [x] Mutation: remove the `heartbeat.touch(...)` at `poll_publish.rs:659`. Task 2's test must
        redden on the advancing `loop_age_ms`.
  - [x] Mutation: `wedged` forced to `false` in `healthz`. Task 3 must redden on the status code.
  - [x] Run each, copy the output next to the test. A note written before its run is not a
        falsification — four such notes across stories 4.11, 4.12 and 4.13 did not survive theirs.

- [x] **Task 5 — the record**
  - [x] `docs/sparkplug-conformance.md`: **nothing moves.** Liveness is not a Sparkplug clause.
  - [x] Comment on [#62] with AC2's result: a stale meter reading healthy is AR12's intent, and
        this is the test that says so.
  - [x] The manual's troubleshooting chapter already describes `wedged`; check it against AC1's
        margin and amend only if it is now wrong.
  - [x] `CONTRACT_VERSION` is **not** bumped: nothing about the payload changes.

## Dev Notes

### The decision this story does NOT take

**Whether `fetch_timeout` should be operator-settable** ([#52] asks the neighbouring question for
`api_base` and `http_timeout`). Naming the constant is not the same as exposing it, and exposing
it would put AC1's margin in an operator's hands — which is an Epic 7 decision, taken with the
healthcheck in front of it. Name it, pin it, leave it in code.

### What must not break

- **`/healthz` must never answer 503 for a deliberate silence** (ADR 0026, ADR 0027, story 6.1).
  The unconfigured and unconfirmed phases have no loop at all, and `loop_age` returns `None`
  there — which is not a wedge. A test that made an unconfigured bridge unhealthy would put a
  fresh deployment in a restart loop under Epic 7, destroying the screen needed to configure it.
- **`loop_age` takes the meter most over its OWN allowance**, not the oldest age
  (`ui/mod.rs:378`). A test with one meter cannot see that; do not weaken it by accident.
- **An idle loop still re-paces** (`poll_publish.rs:1382`, story 3.5's review). A disabled meter
  must not read as wedged.
- **`arch_purity`**: this is a test; nothing here may reach into `core/` or `domain/` for a
  capability production does not have.

### Previous story intelligence (4.11, 4.12 and 4.13, all closed 2026-08-18/19)

- **A test asserting over an empty set is worse than no test.** Print what was observed —
  `/healthz` bodies included — so a green run says what it saw.
- **A window the harness chose can lie.** 4.13's AC2 measured `0` on 2 runs of 37 because it
  stopped looking too early. Read `/healthz` on both sides of the event, not once.
- **A branch that cannot change what it emits is not a mechanism.** 4.12 shipped a `match` whose
  two arms returned the same value. If a helper here routes through a constant, moving the
  constant must change the outcome.
- **Count what you claim**, and cite the call site rather than the function: 4.12's conformance
  row moved on a mutation applied one layer away from the one [#30] prescribed.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md:1184`] — Story 4.14, the original AC
- [Source: `_bmad-output/planning-artifacts/epics.md:146`] — AR12, the liveness heartbeat
- [Source: `crates/smartme-bridge/src/app/poll_publish.rs:659`] — the heartbeat, written first
- [Source: `crates/smartme-bridge/src/app/poll_publish.rs:671`] — the fetch deadline
- [Source: `crates/smartme-bridge/src/ui/mod.rs:51`] — `WEDGED_AFTER_PERIODS`
- [Source: `crates/smartme-bridge/src/ui/mod.rs:363`] — `loop_age`, per-meter allowance
- [Source: `crates/smartme-bridge/src/ui/mod.rs:821`] — the `wedged` verdict and the status code
- [Source: `crates/smartme-bridge/src/app/config.rs:49`] — `PERIOD_MIN` (ADR 0020)
- [Source: `crates/smartme-bridge/src/app/config.rs`, `FETCH_TIMEOUT`] — the fixed `fetch_timeout`
- [Source: `crates/smartme-bridge/tests/chaos_stale_on_cloud_timeout.rs`] — story 1.14, the sibling this must not duplicate
- [Source: `docs/adr/0027-*.md`] — non-200 codes are for a wedged poller only
- [Source: `CLAUDE.md`] — falsify before trusting; never defer a decision to an artifact that does not exist

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-19.

### Debug Log References

`chaos_poller_wedge` prints four measurement lines under `--nocapture` — two per test. They are
the story's measurements, not diagnostics, and they stay.

### Completion Notes List

**AC1 — met, and it is the story's real deliverable.** `FETCH_TIMEOUT` is now a named constant
(`config.rs`) rather than a literal, and
`ui::tests::the_wedge_allowance_outlives_a_blocked_fetch` asserts
`FETCH_TIMEOUT < PERIOD_MIN × WEDGED_AFTER_PERIODS` — 10 s against 15 s, a margin of five
seconds that nothing was watching. Falsified: raising the constant to 20 s goes red naming the
consequence.

**THE DRAFTING NOTE MISCOUNTED THE SITES.** It said `fetch_timeout` was a literal in two
production places; the second (`reconfigure.rs:483`) is inside `mod tests`. Corrected in the
story before any code was written. Counting a test as production is how a margin gets pinned in
the wrong place.

**AC2 — met, and it answers [#62].** A tarpit source (accepts, never replies) with a 500 ms fetch
deadline inside a 900 ms allowance: metrics `Stale` on the wire to an independent subscriber,
`degraded_meters` naming the meter with `cause: source-unreachable`, and **200 at every sample
across the whole observation window** rather than at one instant. `loop_age_ms` was sampled
`[79, 181, 283, 387, 491, 91]` — a live heartbeat, which is the positive control: without it the
test would pass over a loop that had stopped dead.

**AC3 — met, including the half nobody asks for.** The same tarpit with a 5 s deadline against
the same 900 ms allowance: `/healthz` answers 503 with `wedged:true` and
`loop_age_ms: 951 > loop_age_allowed_ms: 900`, and then **200 again** once the fetch expires and
the loop ticks. A wedge verdict that latched would, under Epic 7, restart the container every few
seconds for a fault that had passed.

**THE HARNESS HAD TO BE CORRECTED TWICE, and both corrections are recorded in the file.** The
first draft addressed the tarpit as `http://` and the bridge refused to start at all — *"refusing
endpoint: scheme is \"http\", require https"* — which surfaced only because the startup result
is now printed instead of discarded; every assertion had been failing with "nothing arrived",
which reads as a bridge defect and was a harness one. The tarpit is now addressed as `https://`
and simply never answers the ClientHello: no certificate is needed to say nothing. The second was
AC2's first `/healthz` read landing before the first fetch had been attempted, when
`degraded_meters` is legitimately empty — the single read became a sampled window, which is
strictly stronger.

**A PREDICTION DID NOT SURVIVE ITS RUN — the fifth in four stories.** The note for the
heartbeat mutation predicted a frozen age (`2093 then 2093`). Deleting `heartbeat.touch(…)`
gives a meter no `last_tick` at all, so `loop_age` returns `None` and the endpoint reports
`"loop_age_ms":null` beside `"wedged":false` — a dead loop reading as healthy. **A `number()`
helper that returned `0` for `null` would have made this mutation green**, which is precisely why
the assertion is written against the real shape rather than the imagined one.

**What this file drives, stated so it is not read wider.** `app::run` does not serve the UI;
`main.rs::lifecycle` spawns `ui::serve` beside it, so the test assembles those two pieces itself.
The poll loop, the driver, `ui::serve`, `healthz` and `loop_age` are the production ones. AC3's
configuration — a 300 ms period with a 5 s fetch deadline — **cannot be written in
`config.toml`**: `PERIOD_MIN` is 5 s and `FETCH_TIMEOUT` is fixed. That is AC1's guarantee, and
it is why AC3 proves the wiring rather than a reachable failure.

**`docs/sparkplug-conformance.md`: nothing moved.** Liveness is not a Sparkplug clause. The
manual gained one paragraph stating the margin, in the section that already tells an operator not
to wire a healthcheck yet. `CONTRACT_VERSION` unchanged at 10.

**The manual was BUILT BOTH WAYS rather than assumed, and the local baseline was stale.** The
first build after the edit showed seven overfull boxes where an old `build/` log showed six,
which read as a box introduced by the new paragraph. It is not: `docs/manual/build/` is
`.gitignore`d, so that log was an artefact of an earlier session rather than a committed
baseline. Building HEAD without the change gives **seven boxes and 76 pages**, and building with
it gives **seven boxes and 76 pages** — the same list, value for value. The 2.35 pt box the first
comparison blamed on this story is a JSON example in the troubleshooting chapter, which arrived
with story 4.11. Worth recording because earlier stories cite a baseline of *five*: it is seven
today, and the only way to know is to build both sides.

### Falsification record

| # | Mutation | Test | Went red with |
|---|---|---|---|
| 1 | `FETCH_TIMEOUT` 10 s → 20 s | `the_wedge_allowance_outlives_a_blocked_fetch` | `A BLOCKED FETCH CAN NOW OUTLIVE THE WEDGE ALLOWANCE: a fetch may hold the loop for 20s while /healthz calls it wedged after 3 x 5s = 15s` |
| 2 | `WEDGED_AFTER_PERIODS` 3 → 100 | `a_loop_blocked_past_its_allowance…` | `THE BLOCKED LOOP NEVER READ AS WEDGED … loop_age_ms: 4957, loop_age_allowed_ms: 30000` |
| 3 | `wedged = false` forced in `healthz` | `a_loop_blocked_past_its_allowance…` | same sentence, and a body that contradicts itself: `loop_age_ms: 4947, loop_age_allowed_ms: 900` |
| 4 | `heartbeat.touch(…)` deleted from `step_once` | `a_source_that_is_up_and_stuck…` | `THE LOOP HAS NO AGE AT ALL. loop_age_ms is null, which is what a bridge whose poll task never ticked reports` — **not** the frozen age the note predicted |

### Review Findings (2026-08-19, same day)

Reviewed mechanically: every identifier cited against the functions that exist, every `file.rs:N`
against the file. Four identifiers resolve; one line citation did not.

- [x] [Review][Patch] **The story invalidated its own citation.** It cited `config.rs:751` for the
      fixed `fetch_timeout` three times — and then inserted `FETCH_TIMEOUT` seventeen lines above
      it, moving the target to `:766`. Written, correct, and stale by the end of the same commit.
      Now cited by symbol, which is [#101]'s prescription and the reason that issue exists.

- [x] [Review][Patch] **The harness could fail to start and blame the bridge for it.**
      `bridge_with_ui` probed `/healthz` up to a hundred times and then **fell through silently**,
      so a UI that never bound left every assertion below to fail on its own terms. A pre-push
      gate reported `THE BLOCKED LOOP NEVER READ AS WEDGED … Last body: ` — with an **empty**
      body, which is the tell: nothing had answered at all. The probe now asserts, naming the
      bind race this file already documents (`an_unused_host_port` releases the port before
      `ui::serve` binds it). Found by the gate, not by reading.

**Verified and left standing:** `config.rs:49` (`PERIOD_MIN`), `poll_publish.rs:659` (the heartbeat
before anything that can block), `:671` (the fetch deadline), `ui/mod.rs:51`
(`WEDGED_AFTER_PERIODS`) — all four still point at exactly what they name, and they are the four
the AC1 invariant rests on.

### File List

- `crates/smartme-bridge/src/app/config.rs` — modified (`FETCH_TIMEOUT` named and used)
- `crates/smartme-bridge/src/ui/mod.rs` — modified (AC1's invariant test)
- `crates/smartme-bridge/tests/chaos_poller_wedge.rs` — new (AC2, AC3)
- `crates/smartme-bridge/tests/common/mod.rs` — modified (`an_unused_host_port` made reusable)
- `docs/manual/chapters/03-installation.tex` — modified (the margin, one paragraph)
- `_bmad-output/implementation-artifacts/4-14-chaos-poller-wedge.md` — new
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Change Log

- **2026-08-19** — Story 4.14 written and implemented. The wedge the epic asked for cannot be
  caused by a source, so AC1 pins the arithmetic that keeps it that way and AC2/AC3 prove the
  two verdicts end to end against a tarpit, differing in one number. Four mutations run, one
  prediction rewritten from its output, [#62] answered by measurement. One production change:
  a literal given a name.
