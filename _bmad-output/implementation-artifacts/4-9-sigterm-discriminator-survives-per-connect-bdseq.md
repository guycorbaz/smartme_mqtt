# Story 4.9: Give `chaos_sigterm_no_lie` a discriminator that survives per-CONNECT `bdSeq`

Status: done

> **Reviewed out of `review` 2026-08-03. All four ACs met, nothing outstanding.**
>
> - **AC1** — `docs/adr/0011-…md:58-84` carries the expiry, states the *mechanism* rather than the
>   conclusion, and names the count as the replacement.
> - **AC2** — the discriminator is a count of NDEATHs on the drained stream; the test reads no
>   `payload.timestamp` to tell a death from a will. Falsified in **three** experiments, records
>   copied into the module docs, not written from memory.
> - **AC3** — the `death_stamp > birth_stamp` assertion is gone, not supplemented, and the prose that
>   explained it is rewritten.
> - **AC4** — *"What the count does NOT exclude"* names the duplicate-publish case and why it is a
>   different defect.
>
> The experiment worth keeping: **on identical broken code the old discriminator says pass and the
> new one says fail** — the story's whole argument, measured rather than reasoned. That is the
> standard the rest of the project's falsifications should be held to.

## Story

As the maintainer,
I want the SIGTERM proof to stop depending on the will being stamped once,
so that Story 4.10 can change that without silently disarming the test.

## Acceptance Criteria

The epic states two (`epics.md`). **Both stand.** One is amended for precision and two are added,
for reasons in *Dev Notes*.

**AC1 — ADR 0011 records the expiry explicitly**

**Given** `chaos_sigterm_no_lie` distinguishes the explicit NDEATH from the will by comparing
payload timestamps
**When** the will is rebuilt per CONNECT (Story 4.10)
**Then** ADR 0011 states **in its own text** that the discriminator stops discriminating, and states
the mechanism rather than the conclusion
**And** it names the replacement, so a reader arriving from 4.10 does not have to re-derive it.

*Amended: the epic says "ADR 0011 records this explicitly" without saying what "this" is. The
mechanism is the load-bearing part and it is not obvious — see Dev Notes.*

**AC2 — the new discriminator does not rest on any timestamp**

**Given** the test
**When** a new discriminator is introduced
**Then** it does not read `payload.timestamp` for the purpose of telling a death from a will
**And** it is falsified before being trusted: with the explicit publish removed, the test fails;
with it restored, it passes
**And** the falsification is recorded next to the test, **copied from the run** rather than written
from memory.

**AC3 — the OLD discriminator is removed, not merely supplemented** *(added)*

**Given** an assertion that will become a false pass
**When** the new one lands
**Then** the timestamp comparison is deleted rather than kept alongside
**And** the module documentation that explains it is rewritten in the same change.

> Keeping both would leave an assertion that passes for a reason that has stopped being true, next
> to one that works — and the reader cannot tell which carries the proof. This project has been
> caught by exactly that: `chaos_ncmd_subscription` held three assertions asserting the opposite of
> Story 4.7 while appearing to guard it.

**AC4 — the residual risk is stated where the test states its other limits** *(added)*

**Given** the count-based discriminator below
**When** it is written
**Then** the module docs name what it does **not** exclude — a bridge that published its explicit
death twice while the will never fired would produce the same count
**And** state why that is a different defect from the one under test.

## Tasks / Subtasks

- [ ] **Task 1 — read before writing** (AC: all)
  - [ ] `crates/smartme-bridge/tests/chaos_sigterm_no_lie.rs`, the whole file, and especially the
        module section *"How it tells the explicit death from the will"* — that prose is part of
        what this story replaces.
  - [ ] `docs/adr/0011-graceful-shutdown-requires-both-deaths.md`.
  - [ ] `crates/smartme-bridge/src/app/mqtt_driver.rs`, the will construction and the shutdown arm.

- [ ] **Task 2 — replace the discriminator** (AC: 2, 3)
  - [ ] Count NDEATHs from the drained stream after the signal. **Exactly two**, same `bdSeq`.
  - [ ] Delete the `death_stamp > birth_stamp` assertion and the module prose explaining it.
  - [ ] Keep `birth_stamp` only if something else still needs it; do not leave it unused.

- [ ] **Task 3 — falsify** (AC: 2)
  - [ ] Remove the explicit NDEATH publish in `mqtt_driver.rs`; the test must go RED because only
        one certificate arrives. Copy the failure text into the record.
  - [ ] Restore it; the test must go GREEN. **Demonstrate the green direction too** — an expected-red
        that is never shown green proves only that the test can fail.
  - [ ] Second mutation, and this is the one that matters: make the will's timestamp LATER than the
        birth (simulating Story 4.10) and confirm the NEW discriminator is unaffected while the OLD
        one would have passed. That is the whole point of the story.

- [ ] **Task 4 — ADR 0011** (AC: 1, 4)
  - [ ] Add the expiry with its mechanism, and name the replacement.

- [ ] **Task 5 — the record**
  - [ ] `./scripts/ci-local.sh` — not `--fast`: this story's only test is Docker-dependent, and
        `--fast` skips exactly it.

## Dev Notes

### Why the current discriminator dies with per-CONNECT wills — the mechanism

Today the will is **serialised once and handed to the broker inside CONNECT**, stamped just before
that CONNECT. The NBIRTH is stamped just after CONNACK. So for the life of the process:

```
will_stamp  <=  birth_stamp  <  death_stamp
```

`death_stamp > birth_stamp` therefore identifies the explicit death *with certainty*, because the
will can never be stamped after any birth.

**Story 4.10 rebuilds the will per CONNECT.** After the first reconnect, the will registered on
connection *N* is stamped at connection *N*'s CONNECT — which is **later than the birth the test
captured on connection 1**. The inequality then holds for a will:

```
birth_stamp(1)  <  will_stamp(2)      ← a WILL now satisfies `death_stamp > birth_stamp`
```

The test would go green on precisely the regression it exists to catch. Nothing would announce it:
no compile error, no changed assertion, no failing run — the same shape as the `bdSeq` comparison
that compared a constant against itself, and as the drain that ran where nothing could fail.

### The replacement, and why it needs no clock at all

**A graceful stop produces TWO NDEATHs; a lost process produces ONE.** The driver publishes its
explicit certificate and then **drops the socket rather than sending DISCONNECT** — deliberately, so
that the broker fires the will as well (ADR 0011). The will can fire **once**. Therefore:

- two NDEATHs after the signal ⟹ one of them was published by the bridge;
- one NDEATH ⟹ only the will fired, and the explicit path is broken.

No timestamp is read, so per-CONNECT wills change nothing. This is also what ADR 0011 already
*measured* — *"a consumer sees TWO NDEATHs per graceful stop (same bdSeq)"* — so the story converts
an existing measurement into the test's discriminator rather than inventing a mechanism.

**Assert exactly two, not at least two.** A transport blip during shutdown could add a third; the
strict form turns that into a spurious failure rather than a spurious pass, which is the direction
this project has chosen every time.

### What the new discriminator does NOT exclude — say so in the module docs

A bridge that published its **explicit death twice** while the will never fired would also produce
two. That is a different defect — a duplicate publish, not a missing one — and it is not the
regression under test. It is also excluded in practice by the socket drop, which always fires the
will. State it rather than let a reader assume the count proves more than it does.

### The trap this story must not repeat

The 4.6 review established that **a conformant explicit death is byte-identical to the will**:
`tck-id-…-death-payload` (`Sparkplug_5_Operational_Behavior.adoc:808-812`) says the payload
published on shutdown is the one *registered as the will*. So no content-based discriminator can
work, and any proposal to "tag" the explicit death would be a conformance violation invented to
make a test easier. The count is the only property the two messages do not share.

### Why this story precedes 4.10

Stated in the epic and worth keeping: reversing the order leaves a window in which the test passes
for a reason that has stopped being true. There is no compile-time link between the will's stamping
and the assertion, so nothing would fail during that window.
