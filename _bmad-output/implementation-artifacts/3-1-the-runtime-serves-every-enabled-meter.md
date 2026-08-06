# Story 3.1: The runtime serves every enabled meter

Status: ready-for-dev

## Story

As the operator,
I want every meter I enabled in the browser to be polled and published,
so that the configuration screen and the wire agree about how many meters this bridge has.

## Why this exists, and why it is the first story of the epic

**Guy set the gate on 2026-08-06:** the next deployment test on panoramix happens *when the web UI
works with all the meters*. That gate lands here and nowhere else.

The configuration **model** has been plural since story 5.1. The **runtime** has not. `config::
RUNTIME_METER_LIMIT = 1`, and enabling more is **refused at startup rather than silently
truncated** — deliberately, because a bridge that quietly published one of four meters would be
indistinguishable, from the outside, from three meters having failed. That guard is right and this
story is what makes it stop firing; it is not to be weakened, it is to be outgrown.

So the screen cannot be made to accept four meters by touching Epic 6. The limit is the thing in
the way, and removing it is a runtime change.

**One of Guy's four meters is physically unplugged.** That is not an edge case to handle later — it
is the steady state of this deployment, and it is why story 3.2 (a meter's silence is published,
not withheld) is sequenced immediately after this one rather than left with Epic 2. Read that
sequencing before starting: shipping the fleet on top of withheld verdicts would make per-meter
isolation unobservable, since three silences would hide behind the one meter that works.

## What the norm requires of several devices under one node

Settled by reading `docs/spec/sparkplug-b-3.0.0/`, not from memory. The device dimension already
exists in `crates/sparkplug-b/src/topic.rs`; what is new is that there is more than one of them.

- **`tck-id-message-flow-device-birth-publish-nbirth-wait`** (`Sparkplug_5:412`) — the NBIRTH MUST
  have been sent within the current session before any DBIRTH. With N devices this stops being
  incidental: the session-establishment path must birth the node once and then each device.
- **`tck-id-topics-dbirth-seq`** (`Sparkplug_4:386`) and **`tck-id-topics-ddeath-seq-num`**
  (`Sparkplug_4:483`) — both certificates carry a sequence number that MUST be one greater than the
  previous message **from the Edge Node**. The sequence is per-node and shared by every device, so
  birthing four devices consumes four sequence numbers. **This is the property most likely to be
  got wrong by a per-meter task design**, because it is the one piece of state that is *not*
  per-meter.
- **`tck-id-operational-behavior-data-publish-dbirth`** (`Sparkplug_5:901`) — a DBIRTH MUST include
  every metric that device will ever publish in the session.
- **`tck-id-topics-dbirth-topic`** / **`tck-id-topics-ddeath-topic`** — the device id is the last
  topic level. The bridge maps it from `meters[].serial`, and `config::validate` already refuses a
  serial that cannot be a topic level and two meters that would collide on one topic.

## Decisions taken at drafting

The repository rule is that an acceptance criterion may not defer a decision to a test, audit or
spike that does not exist. AR13 sat unmade for the whole of Epic 1 that way.

**1. One poll task per meter, not one task iterating the meters.**

The fetch carries a `fetch_timeout` (10 s by default). A single task walking four meters would
serialise four timeouts — 40 s inside a 30 s period — so one unreachable meter would push every
other meter's poll past its own deadline. That is FR12 (*one silent meter doesn't affect the
others*) failing by construction, and NFR2's bound
(`last_success + 2×poll_interval + publish_margin`) being unmeetable for reasons that have nothing
to do with the meter it is measured on. Guy's unplugged meter makes this the normal case, not the
unlucky one.

AR6 says *"poll+publish task"* in the singular; that was written when the runtime served one meter.
The rest of AR6 — the pure `(prev, tick, now) → next` state machine, the mqtt-driver task owning
the EventLoop and `bdSeq`, the `watch` snapshot for the UI — is unchanged and is what makes N tasks
safe: the sequence number and the transport stay behind the single driver task, exactly as now.

**2. The heartbeat becomes per-meter, and the wedge verdict is the WORST of them.**

`LastLoopTick` is one value today, and with N tasks touching it a wedged task would be masked by
its healthy siblings — `/healthz` green while one meter has not been read for an hour. That is the
class of lie this project exists to prevent, and the second review round found its twin (a `Failed`
source reported as publishing).

So: one tick per meter, each recording its own cadence, and `loop_age` is the oldest of them. A
`503` restarts the container, which kills the other three meters' session — and that is the right
trade, because unlike a rejected credential ([ADR 0027]) a wedged poll task **is** what a restart
fixes. This is the one place where the fleet makes the healthcheck stricter rather than looser.

**3. Enabling a meter mid-session stays a DBIRTH, and this story does not change that.**

`app::reconfigure::classify_meters` currently treats `enabled` as a device certificate **only for
the meter the runtime actually serves**, and everything else as a process restart, because there
was only ever one. Which of those become genuine certificates once N are served is story 3.2's
business, not this one's — but the classification table and `docs/manual/chapters/04-configuration.tex`
must not be left describing the one-meter world. See the cost table's stale rows, already recorded.

## Acceptance Criteria

**AC1 — a configuration with several enabled meters starts, and serves all of them**

**Given** a valid, confirmed `config.toml` with **four** enabled meters
**When** the bridge starts
**Then** it does not refuse to start
**And** `config::RUNTIME_METER_LIMIT` no longer exists, or no longer bounds the enabled count
**And** one poll task exists per enabled meter.

> The falsification to run: re-introduce the limit and watch AC1 fail on the refusal, not on a
> later assertion.

**AC2 — the wire carries one NBIRTH and one DBIRTH per enabled meter, in that order**

**Given** the bridge connects with four enabled meters
**When** the session is established
**Then** exactly one NBIRTH is published, before any DBIRTH
(`tck-id-message-flow-device-birth-publish-nbirth-wait`)
**And** exactly one DBIRTH is published per enabled meter, on its own `.../DBIRTH/<node>/<serial>`
topic
**And** a disabled meter gets neither.

**AC3 — the node sequence is monotonic across every device**

**Given** an NBIRTH followed by four DBIRTHs
**When** the sequence numbers are read off the wire in publication order
**Then** they increase by exactly one per message, wrapping 255 → 0
(`tck-id-topics-dbirth-seq`)
**And** the same holds for DDATA from different meters interleaved.

> **This is the assertion to write first and falsify hardest.** A per-meter design invites a
> per-meter sequence, and the norm's sequence is per-node. Assert the full ordered list of sequence
> numbers, not that they are "increasing" — a counter that advances by two would satisfy the weaker
> claim.

**AC4 — a slow or unreachable meter does not delay another meter's poll**

**Given** four enabled meters, one of which never answers (its fetch always reaches `fetch_timeout`)
**When** the bridge runs for three publish periods
**Then** each of the other three meters has been polled once per period, within the publish margin
**And** the failing meter's fetch timeout does not appear in the others' cadence.

> Drive this with the in-process fake source and an injected clock. **Do not point it at
> TEST-NET-1**: chaos tests that do publish nothing at all, so an assertion about cadence over an
> empty stream would hold vacuously — the trap already recorded for the DDATA absence assertions.
> Prove the stream flows first.

**AC5 — the UI reports the fleet, and the count it reports is the count on the wire**

**Given** four enabled meters
**When** the operator opens the configuration screen
**Then** it lists all four
**And** no page says or implies that the runtime serves one
**And** the note in `docs/manual/chapters/09-appendix-config-reference.tex` — *"The model holds any
number of meters; the runtime currently serves one"* — is amended in the same commit.

**AC6 — the refusal that this story removes leaves no unamended consequence**

**Given** `RUNTIME_METER_LIMIT` is gone
**When** the repository is searched for what it justified
**Then** every passage stating that the runtime serves one meter is amended — the appendix note
above, `epics.md`, `architecture.md`, the story 5.1 record, and `config.rs`'s own doc comment
**And** the search is done by a per-passage table, not by one grep.

> Six times now, a corrected claim has left its consequences standing, and a grep reaches about
> half. This AC exists because the number `1` will appear in surviving passages as prose, not as a
> constant.

## Tasks

- [ ] Remove `RUNTIME_METER_LIMIT` and the AC6 refusal in `app/config.rs`; keep the duplicate-serial
      and topic-legality guards, which are unrelated and still needed.
- [ ] `app/poll_publish.rs`: spawn one task per enabled meter; each owns its own `State` and its own
      heartbeat.
- [ ] `LastLoopTick` → a per-meter collection; `ui::loop_age` takes the oldest.
- [ ] `app/supervisor.rs`: birth every enabled device after the NBIRTH; keep `seq` and `bdSeq`
      behind the driver task.
- [ ] `watch<[MeterState; N]>` per AR6, so the UI reads a coherent snapshot rather than N values
      that never agreed at any instant.
- [ ] Sweep the `deferred-work.md` items parked on Epic 3 that this story touches — at minimum
      `Policy::max_age_ms` validation, and `Serial::new("")` key collisions now that serials key a
      multi-meter map rather than a single one.
- [ ] Amend the manual, `epics.md`, `architecture.md` and the story 5.1 record together (AC6).

## Falsification

Every assertion added here must be run against deliberately broken code and observed to fail, and
the record **copied** from the run next to the test — not written from memory. Two specific traps
this story walks past:

- **AC3's mutation must break the sharing, not the counter.** A mutation that stops the sequence
  advancing at all fails every seq assertion in the suite and proves nothing about *sharing*. Give
  each device its own counter and watch AC3 alone go red.
- **AC4's assertion is about a cadence, and cadences are easy to assert vacuously.** If the harness
  publishes nothing, "the others were polled on time" holds over an empty record. Assert the count
  of polls per meter, and prove the stream flows before asserting anything about its shape.
