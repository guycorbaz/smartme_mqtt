# Story 2.6: A failure says which kind it is, and the bridge waits only when waiting is what the other end asked for

Status: review — **all eight ACs now met**, AC1 and AC5 both on 2026-08-13.
AC5 closes **on its letter**: serde names a missing field and names none for a type mismatch, so
[#73](https://github.com/guycorbaz/smartme_mqtt/issues/73) stays open on that residual, which the
API's nullability declarations make the likely case. See *AC5 — 2026-08-13* below.
Three of the review's nine remaining findings still want a decision before this story closes.

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

- [x] **Task 1 — Split the refusal** (AC1, AC2)
  - [x] Three causes where `SourceRefused` stood; decide its fate and record it, as story 2.5 did
        for `ValueUnusable`
  - [x] **The configuration refusal actually reaches the wire** — added 2026-08-13 after the review
        found AC1's own worked example (`404`, an unknown device id) classified as a network fault
  - [x] `latches` decided per cause and pinned in the golden
  - [x] `Cause::successor`'s chain — a variant that misses it is a compile error by design
- [x] **Task 2 — Honour `Retry-After`** (AC3)
  - [x] Parse it where the HTTP status is already classified (`smart-me-client`)
  - [x] The wait respected by the poll loop **without** suppressing the cycle's verdict
  - [ ] ~~Fallback to `mqtt_driver`'s doubling arithmetic when the header is absent~~ — NOT built: it contradicts AC4 of this same story, see the notes. The cap IS stated.
- [x] **Task 3 — Record what NFR1 already has** (AC4)
- [x] **Task 4 — The field name survives a parse failure** (AC5) — done 2026-08-13; the measurement
      the note asked for was made, and it needed a second half nobody had noticed. Residual on [#73]
- [x] **Task 5 — Contract 7 → 8** (AC7)
- [x] **Task 6 — Falsify, running each mutation BEFORE writing its note** (AC6)
- [x] **Task 7 — `./scripts/ci-local.sh` full run**, then `gh run list`

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

**2026-08-12 — seven ACs met, AC5 recorded UNMET.**

**AC1/AC2 — the refusal names itself.** `SourceError::Fatal` carries a `Refusal`
(`Credential` / `Configuration` / `Identity`) and `Policy::step` publishes the matching cause. The
2026-08-11 finding from story 2.1's review is closed: an operator is now sent to the token, to the
configuration, or to the physical meter, rather than to all three at once.

> **This paragraph was true of the type and false of the wire, and the 2026-08-13 review is what
> established it.** The configuration refusal had no live producer for the case AC1 names — an
> unknown device id — because a `404` was classified transient. Kept as written, with the
> correction below rather than in place of it.

**`SourceRefused` is NARROWED, not retired** — the same treatment story 2.5 gave `ValueUnusable`,
and for a reason found during implementation rather than at drafting. `State::Failed` is absorbing,
so a tick arriving AFTER the latch is published under it even when that tick is not itself a
refusal. None of the three names that case. Inventing a fourth would claim a diagnosis we do not
have; borrowing one would name a fault that is not happening.

**A test demanded the justification, and that is the test working.**
`identity_latches_and_value_does_not` failed the moment the three refusals were classified, with
its own message: *"if it genuinely does latch, it belongs beside SourceRefused above"*. The
extension is argued on `core::source::Refusal`: retrying against a refusal the other end has
already given is how a bridge hammers an API.

**ADR 0032 had named this moment in advance, and the clause was checked rather than noticed.** Its
*"what would reopen this"* said a second latching cause makes tier 3 break ties within the latching
set. The set went from one to four. **The condition is met in letter and not in fact**, and the
reason is structural: `Policy::step` returns at its first guard on a fatal tick, before any other
oracle, and a fatal tick carries no reading — so `SourceFaults` is empty and every metric-scoped
judgement is `good()`. A latching cause reaches `compose` alone; two cannot meet. Verified in the
code (`poll_publish.rs:422`), recorded in the ADR.

**AC3 — the one wait this bridge honours.** `429` with `Retry-After` (seconds form only; the date
form needs a trusted local clock, which is the thing this bridge doubts), capped at 300 s with the
argument written at the cap. No fetch is attempted while waiting, **and the cycle still publishes**
— skipping it would make a rate limit look like silence.

**AC4 — the backoff NFR1 names is answered in writing, not in code.** The poll interval already
spaces retries; the broker half already exists in `mqtt_driver`. Nothing was built.

**AC3's fallback clause is deliberately NOT implemented, because it contradicts AC4 of the same
story.** AC3 asked that a `429` without `Retry-After` fall back to a doubling backoff. AC4 argues
that the poll interval already spaces retries and that a second timer would either duplicate it or
fight it. **The contradiction is mine, and it was invisible until implementation.** A fallback
below the interval does nothing; above it, it is exactly the competing timer AC4 refuses. So when
the server names no delay, no wait is armed. Recorded rather than resolved by silently picking one.

**AC5 IS UNMET AND NOT STARTED.** A parse failure still reaches the operator without the field
name. `reqwest`'s decode error may or may not preserve serde's message, and I did not measure it —
so I do not know whether this is a mapping fix or a client change. Ticking it would have been the
defect this epic keeps finding. It needs an issue and a measurement, not a guess. Opened as [#73](https://github.com/guycorbaz/smartme_mqtt/issues/73).

**AC6 — every falsification below was RUN BEFORE its note was written**, which is the criterion
this story added because story 2.5 recorded four and had run one.

| mutation | result |
|---|---|
| `waiting` guard forced to `false` | RED — *"no fetch may be attempted before the instant the source named"*, 3 fetches where 2 were owed |
| the three refusals classified (no mutation — the change itself) | RED on `identity_latches_and_value_does_not` and on the state-machine table, both naming what they expected |
| `contract_golden` before the bump | RED — *"the cause vocabulary changed size (19 live, 15 in the v7 golden)"* |

**AC7** — `CONTRACT_VERSION` 7 → 8, additive, `GOLDEN_*_V8` written out. Manual rebuilt: 71 pages,
five overfull boxes, the committed baseline exactly.

## Review findings — 2026-08-13

Reviewed at `high` on a branch carrying only this story's commit (`review/story-2.6`, the
implementation replayed onto its own parent so the diff was exactly the story). Ten findings. This
section records the one that was repaired; the rest are listed at the end, undecided.

### AC1 WAS UNMET, and its own worked example had no live producer

**AC1 names *"a configuration the source contradicts (the device id is unknown to smart-me)"*.**
That case could not occur. An unknown device id returns `404`, which fell through to
`SmartMeError::HttpStatus`; `is_fatal` names only `NotHttps | Misconfigured | AuthRejected`, so the
`404` became `Transient` → `source-unreachable`. **The most likely configuration error there is — a
mistyped device id — was published as a network fault, never latched, and the bridge polled a
device that does not exist for ever.** The manual and the runbook meanwhile told an operator that
v8 distinguishes a credential from a configuration.

This is the failure this epic keeps finding, in its purest form: an acceptance criterion ticked,
its behaviour absent, and the suite green. It was found by a review and not by a test because no
test could reach the classification — see below.

### The repair, decided 2026-08-13

**`SmartMeError::UnknownDevice` is carved out of `HttpStatus`**, joins `is_fatal`, and maps to
`Refusal::Configuration`. Fatal rather than transient on ADR 0029's own reasoning transposed from
the serial to the id: a device id does not come into existence on its own, so retrying reports a
fault as weather. The latch costs nothing an operator does not already pay — correcting a device id
is a configuration change, which `reconfigure::classify_meters` already prices at a
`ProcessRestart`, so the repair and the latch ask for the same thing.

**Two origins, and the message names both** (Guy, 2026-08-13). A `404` is a typo *or* a device
removed from the account after having worked — the second arrives on a configuration that was
correct yesterday, and an operator sent hunting for a typo they never made is worse off than one
told nothing. The disappearance case keeps its own home in **story 3.5 (FR6)**: this story owns
*"smart-me refuses this id, on this fetch"*; 3.5 owns *"the meter is no longer in the account's
inventory"*.

**`404` only, and no other status.** A `400` would plausibly also mean "the id is wrong", but this
API has never been observed returning one; guessing would be a fact about smart-me nobody measured,
the refusal story 2.2 AC4 and ADR 0033 both made. A `400` arrives as `HttpStatus`, visibly.

**`CONTRACT_VERSION` does not move.** `configuration-contradicted` already exists in the v8 golden
and no cause was added — this fix gives an existing cause its missing producer.

### The classification is now a pure function, and that is the point

`classify_device_status` was extracted from `get_device` because **`smart-me-client` has no HTTP
test harness and no dev-dependencies at all**, so while the status-to-error decision lived inside an
`async` method behind a live request, nothing could reach it. That is why the `404` branch could be
missing with a green suite, and why story 2.6's own `429` branch and `Retry-After` parse shipped
with no test either. Both are the Epic 2 retrospective's subject — a property tested one layer above
where it lives, or not at all because that layer is out of reach. Adding a mock-HTTP dependency was
rejected as a heavier answer than the question deserved.

### Falsification — four mutations, each RUN before its note was written (AC6)

| mutation | result |
|---|---|
| the `404` arm deleted from `classify_device_status` | RED — *"a 404 must name the device smart-me refused, got HttpStatus { status: 404 }"*, i.e. the pre-fix behaviour |
| `UnknownDevice` removed from `is_fatal` | RED **on both layers** — client: *"…does not come into existence on its own…"*; bridge: *"must latch, not degrade: got Transient { … }"* |
| the `UnknownDevice` arm pointed at `Refusal::Credential` | RED — *"must send the operator to the device id in the configuration; left: Credential, right: Configuration"* |
| the second origin dropped from the `#[error]` string | RED — *"the operator is sent to one place only; \"removed from the smart-me account\" missing from …"* |

## AC5 — 2026-08-13, and the measurement found a second half

The story parked AC5 rather than guessing: *"`reqwest`'s decode error may or may not preserve
serde's message, and I did not measure it."* Measured now, by reading the code that is compiled:
**`reqwest::Error`'s `Display` writes its kind and the URL and nothing else** (`error.rs:227-272`,
v0.13.1) — a decode failure renders as `error decoding response body for url (…)`. serde's text is
in the `source()` chain, which `from_reqwest` never walked. The field name was lost at the client.

**And then the second half, which the story did not know it needed.** Carrying the name up would
have delivered it nowhere: **no `SourceError`'s `reason` was rendered on any operator surface.**
Verified rather than argued — deleting both `impl Display` and `impl Error` for `SourceError` left
the library compiling with **zero errors**.

Be precise about what that means, because the first version of this note overstated it: the
**cause token** did reach the operator, on the wire and in an existing `INFO` line carrying
`meter=` and `Some(ConfigurationContradicted)`. What reached nobody was the **sentence** — ADR
0029's *"correct the serial or the device id in the configuration, then restart"*, this morning's
two origins for an unknown device id, and any field name AC5 might carry. Every repair instruction
this codebase has written for an operator was invisible.

### What was done

1. **`get_device` parses the body itself** (`resp.text()` then `serde_json::from_str`) instead of
   `resp.json()`, so serde's message survives. No new dependency: `serde_json` was already direct.
2. **`decode_device` is a function**, for the reason `classify_device_status` is: the property
   lives in the parse, and nothing could reach it behind an `async fn` and a live request.
3. **The poll loop logs the failing tick** — `warn!(meter, %error, "this meter could not be
   read")`. One line per failing cycle, the cadence this codebase already uses; a latched meter
   repeats, deliberately, because ADR 0027's rule is that silence is the lie.

### AC5 closes on its letter; the residual is real and stays on [#73]

**serde names a field it did not find, and names none when the field is there with the wrong
type** — an explicit `null` included:

```
missing field `ActivePower` at line 2 column 76      ← named
invalid type: null, expected f64 at line 3 column 31 ← not named
```

AC5 asks that *"the field serde named"* be carried. Where serde names one it now is; where serde
names none there is none to carry. **That is met on the letter and it is not the stronger claim** —
the same standing as story 2.1's AC4.

**The residual matters more than it looks, and it is today's other finding**: the API's own
description declares **six of the eight fields this client consumes as nullable**
(`docs/spec/smart-me-api/`). The nameless case is the one the wire is most likely to produce.
Closing it needs `serde_path_to_error` or `Option` fields judged per metric — the second is
ADR 0031's logic applied to the payload, and it is a design decision, recorded rather than taken.
A test pins the current behaviour and says to delete itself if serde ever starts naming it.

### Falsification — four mutations, each RUN before its note (AC6)

| mutation | result |
|---|---|
| `decode_device` returns reqwest's text | RED — *"the refusal must at least say what arrived: \"response decode failed: error decoding response body\""*, which is what shipped before |
| the `warn!` deleted | RED — *"no line reported the failure to the operator; the log was: … INFO … no reading this tick and none ever …"* |
| `meter` dropped from the `warn!` | RED — *"and which meter it was, ON THIS LINE …"* |
| (found by mutation, not by reading) the first version asserted `log.contains("garage")` over the WHOLE capture, which the pre-existing `INFO` line satisfies with the `warn!` deleted. `unreadable_line` exists because of it — the story 4.6 needle problem in a new place |

### What the review found and this commit did NOT settle

Nine findings are left undecided, deliberately — they are recorded here so they are not re-derived:

1. **A latched meter reverts to the generic `source-refused` on any non-fatal tick**
   (`state_machine.rs:156`). Credentials expire → `credential-rejected`; one hiccup later →
   `source-refused`; the network returns → `credential-rejected`. The `Cause` flaps with the
   weather. Carrying the `Refusal` inside `State::Failed` would fix it — a state-machine change, so
   an ADR.
2. **Nothing tests that the rate-limit wait ever ends** (`poll_publish.rs:405`). Replacing the
   deadline comparison with `is_some()` — a wait that never expires — leaves the suite green.
3. **`RETRY_AFTER_CAP` is untested** (`smartme_source.rs`): `.min` → `.max` leaves the suite green.
4. **The token endpoint bypasses the rate-limit mechanism entirely** (`client.rs`, `fetch_token`):
   a `429` on `POST /oauth/token` arms no wait. AC3 covers one of the two endpoints called.
5. **The stated reason for dropping the date form of `Retry-After` is contradicted by its own
   function**: the response's `Date` header is already parsed a few lines below, so
   `Retry-After(date) − Date(header)` needs no local clock.
6. **`_ => Refusal::Configuration` still absorbs future fatal variants.** The three configuration
   variants are now enumerated, but the wildcard remains and gives no compile-time protection.
7. Two stale comments (`poll_publish.rs:512` claims `latches()` is true only for `SourceRefused`;
   it is true for four) and one stale bullet in ADR 0032 left directly under the blockquote that
   corrects it.

**AC2's structural argument was checked and HOLDS.** The review confirmed that all four latching
causes are produced only by `Policy::step`'s fatal guard, which already returns `Failed`, so two
cannot meet in `compose`. The premise written in the comment is what is wrong, not the conclusion.

### File List
