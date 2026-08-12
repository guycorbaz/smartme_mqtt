# Story 2.6: A failure says which kind it is, and the bridge waits only when waiting is what the other end asked for

Status: ready-for-dev

## Story

As the operator,
I want a source failure to name what kind of failure it is and what I must do about it,
so that a refused credential does not read like a network hiccup, and so that I am not sent to a meter when the fault is a token.

## Why this exists, and the largest part of it is already built

**FR5's distinction exists in the type and is invisible on the wire.** `SourceError` has carried
`Timeout` / `Transient` / `Fatal` since story 1.4, and `Policy::step` treats them correctly:
transient degrades, fatal latches `Failed`. What an operator actually sees is one cause,
`source-refused`, for **a rejected credential, a configuration smart-me contradicts, and a serial
that is not the declared one** ([ADR 0029]). The 2026-08-11 review of story 2.1 deferred exactly
this here: *"an operator cannot tell NFR7 (wrong meter) from an expired credential, which is the
reproach this story levels at `smartme_source.rs:261`"*.

**FR4's retry already happens, and its backoff already exists — for the broker.**
`mqtt_driver.rs:342-420` carries a doubling backoff from a 1 s floor to a 30 s ceiling with
additive jitter, and its doc argues why the jitter is additive rather than full. **Do not build a
second one.** The reusable part is the arithmetic; what is missing is a caller on the source side.

**And here is the thing to settle before writing any code.** The poll loop already spaces its
fetches by `poll_interval`, which is bounded and cannot be turned off ([ADR 0020]), and [ADR 0027]
requires **every cycle to publish a verdict for every enabled meter — never silence**. So on a
30 s period the loop *is* the retry, spaced further apart than NFR1's 1 s floor and comparably to
its 60 s cap. **A general exponential backoff on the source would therefore be a mechanism with no
work to do, and a second timer competing with the publish period for control of the same loop.**

What genuinely has no answer today is the case where **the other end asks us to wait longer than
our own period**: a `429` with `Retry-After`. That is the parked item
(`deferred-work.md:42`), and it is the one case where a source-side wait is not redundant.

## Acceptance Criteria

**AC1 — `SourceRefused` splits, and each part sends an operator somewhere different.**

**Given** the three faults that publish `source-refused` today
**When** they are classified
**Then** they carry distinct causes: a **credential** the source rejected, a **configuration** the
source contradicts (the device id is unknown to smart-me), and a **serial** that is not the
declared one
**And** the third keeps ADR 0029's latching behaviour unchanged — identity latches, and that rule
is not reopened here
**And** each is falsified by pointing it at its neighbour, the assertion naming the repair the
operator would have been sent to.

**AC2 — Every new cause states whether it latches, and the golden pins it.**

**Given** the latch/degrade rule ([ADR 0032])
**When** the new causes are added
**Then** a credential rejection **latches** — retrying with a credential the source refused is how
a bridge hammers an API it has already been told to stop asking — and so does a contradicted
configuration
**And** `contract_golden.rs` pins `latches` for each, as it already does for the others
**And** the composition's tie rule is exercised by at least one case where a latching and a
degrading cause meet at equal severity.

**AC3 — A `429` is honoured, and it is the only source-side wait this story builds.**

**Given** a `429` carrying `Retry-After`
**When** the poll loop next runs
**Then** no fetch is attempted before that instant has passed, and the cycle still **publishes a
verdict** as [ADR 0027] requires — the meter is republished with its own cause, not skipped
**And** a `429` **without** `Retry-After` falls back to the doubling backoff, reusing
`mqtt_driver`'s arithmetic rather than a second implementation
**And** the wait is bounded: a `Retry-After` of an hour is capped, and the cap is a number this
story states with its reasoning rather than one it inherits silently.

**AC4 — NFR1's backoff is recorded as ALREADY SATISFIED for the loop, and the reasoning is written down.**

**Given** NFR1's *"bounded exponential backoff + jitter, e.g. 1 s → 60 s cap"*
**When** the source path is examined
**Then** the story records that the poll interval already spaces retries — bounded by ADR 0020,
never off — so a general source backoff would be a second timer competing for the same loop, with
nothing to do that the period does not already do
**And** what NFR1 asks for that is genuinely absent is named: honouring a wait the SERVER asked for
(AC3), which the period cannot know about
**And** the broker half is recorded as met by `mqtt_driver`'s existing floor/ceiling/jitter, cited
by line rather than asserted.

**AC5 — A parse failure names the field.**

**Given** story 2.5's residual — a payload the deserializer refused reaches the operator as a
generic failure naming nothing
**When** the fetch fails to decode
**Then** the field serde named is carried into the cause or its diagnostic, so an operator learns
*which* field the API changed
**And** it is falsified by discarding the field name: the assertion must go red naming what was
lost.

**AC6 — Falsified before trusted, and the falsification is RUN before it is recorded.**

**Given** every assertion this story adds
**When** its `FALSIFIED` note is written
**Then** the mutation has already been executed and its result observed
**And** the note records the observed failure message, not a prediction of one.

*This criterion exists because story 2.5 recorded four falsifications and had run one.* All three
unrun claims turned out to be true, which is exactly why the practice is dangerous: it works until
it does not, and nothing distinguishes a checked claim from an unchecked one after the fact.

**AC7 — `CONTRACT_VERSION` moves 7 → 8, additive**, with the golden written out, the manual, the
runbook and the mechanical grep — the same discipline as v4 through v7.

**AC8 — No verdict that is correct today changes**, apart from the split AC1 names.

## Tasks / Subtasks

- [ ] **Task 1 — Split the refusal** (AC1, AC2)
  - [ ] Three causes where `SourceRefused` stood; decide its fate and record it, as story 2.5 did
        for `ValueUnusable`
  - [ ] `latches` decided per cause and pinned in the golden
  - [ ] `Cause::successor`'s chain — a variant that misses it is a compile error by design
- [ ] **Task 2 — Honour `Retry-After`** (AC3)
  - [ ] Parse it where the HTTP status is already classified (`smart-me-client`)
  - [ ] The wait respected by the poll loop **without** suppressing the cycle's verdict
  - [ ] Fallback to `mqtt_driver`'s doubling arithmetic when the header is absent; the cap stated
- [ ] **Task 3 — Record what NFR1 already has** (AC4)
- [ ] **Task 4 — The field name survives a parse failure** (AC5)
- [ ] **Task 5 — Contract 7 → 8** (AC7)
- [ ] **Task 6 — Falsify, running each mutation BEFORE writing its note** (AC6)
- [ ] **Task 7 — `./scripts/ci-local.sh` full run**, then `gh run list`

## Dev Notes

### Decisions taken at drafting

**1. No general source backoff.** Argued in *Why this exists* and recorded by AC4. The temptation
is strong because NFR1 names one; the reason to resist is that ADR 0020 and ADR 0027 already own
the loop's cadence, and a second timer would either fight them or duplicate them.

**2. A credential rejection latches.** It is not the identity/value distinction ADR 0032 draws —
a credential is neither — so the rule is extended rather than applied: **retrying against a
refusal the other end already gave is how a bridge hammers an API**, and no reading it obtained
that way would be more trustworthy. Recorded here so 2.7 does not re-choose it.

**3. `Retry-After` is capped, and the cap is not inherited.** A server may say an hour. Honouring
it literally would take a meter off the wire for an hour on one header, and ADR 0027's rule is that
every cycle publishes a verdict — the verdict may be degraded, but the loop does not stop.

### The trap this story is most likely to fall into

**Building the backoff NFR1 names and calling the requirement met.** It would pass its tests, add
a timer, and change nothing an operator can observe — a mechanism with no work to do, which is what
`Verdict::latches` was until story 2.3's review proved its branch could not change an answer.
AC4 exists so the requirement is answered in writing rather than in code that does nothing.

**And the second trap is AC6's, which is this epic's newest lesson.** Write the mutation, run it,
read the message, *then* write the note — in that order. Story 2.5's review found three notes
recording falsifications that had never been performed.

### Where the code lives

- `crates/smartme-bridge/src/core/source.rs:83` — `SourceError`, the three variants
- `crates/smartme-bridge/src/adapters/smartme_source.rs:241` — `map_error`, which collapses
  everything non-fatal and non-timeout into `Transient`
- `crates/smart-me-client/src/client.rs:80` — `is_fatal`, today `NotHttps | Misconfigured |
  AuthRejected`; `:52` is where a non-success status including `429` is minted
- `crates/smartme-bridge/src/app/mqtt_driver.rs:342-420` — `RECONNECT_FLOOR` (1 s),
  `RECONNECT_CEILING` (30 s), `jittered`, and the argument for additive rather than full jitter
- `crates/smartme-bridge/src/core/oracle.rs` — `Cause`, `latches`, `successor`
- `crates/smartme-bridge/src/app/poll_publish.rs` — the loop whose cadence ADR 0020 bounds

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:272`] — FR4
- [Source: `_bmad-output/planning-artifacts/prd.md:273`] — FR5
- [Source: `_bmad-output/planning-artifacts/prd.md:338`] — NFR1
- [Source: `_bmad-output/implementation-artifacts/deferred-work.md:42`] — the parked 429 item
- [Source: `_bmad-output/implementation-artifacts/2-1-the-oracle-layer-and-how-verdicts-compose.md`]
  — the deferred finding this story closes: `source-refused` shared by a credential and a meter
- [Source: `_bmad-output/implementation-artifacts/2-5-payload-completeness-and-numeric-domain.md`]
  — the parse-failure residual (AC5), and the falsification lesson (AC6)
- [Source: `docs/adr/0020-the-publish-period-is-bounded-and-cannot-be-turned-off.md`]
- [Source: `docs/adr/0027-a-failed-source-is-a-fault-the-screen-must-name.md`]
- [Source: `docs/adr/0029-the-declared-serial-is-checked-against-the-one-smart-me-reports.md`]
- [Source: `docs/adr/0032-at-equal-severity-a-latching-cause-outranks-a-degrading-one.md`]

### An open question for Guy, saved for the end

**[#61] may belong here.** *"A serial that is merely legal takes a meter off the wire in silence,
and every surface reports it healthy"* is labelled `epic-2, epic-3` and is about a refusal an
operator cannot see — which is this story's subject. It is not folded in, because whether it is a
taxonomy fault or a discovery fault (story 3.4's business) is a judgement, not a deduction.

[ADR 0020]: ../../docs/adr/0020-the-publish-period-is-bounded-and-cannot-be-turned-off.md
[ADR 0027]: ../../docs/adr/0027-a-failed-source-is-a-fault-the-screen-must-name.md
[ADR 0029]: ../../docs/adr/0029-the-declared-serial-is-checked-against-the-one-smart-me-reports.md
[ADR 0032]: ../../docs/adr/0032-at-equal-severity-a-latching-cause-outranks-a-degrading-one.md

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
