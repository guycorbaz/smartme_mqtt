# Story 6.8: The gesture each cause asks for — FR31, and the last thing Epic 6 owes

Status: review

> **[#103] is this story, and it was opened by the review of 6.4 rather than guessed at.**
> `repair()` derives the gesture from `Culprit` alone, so twenty-one causes share three
> sentences. Nothing is false — the cause itself is on the page — but *"open the configuration:
> a credential, a serial or a device id is wrong"* names three repairs where the cause already
> knows which one it is.
>
> **FR31's own words are a list of four**: *"actionable error messages (auth vs permissions vs
> timeout vs empty result), not stack traces"*. The taxonomy answers all four and is asked
> none of them.

## Story

As the operator reading a fault at three in the morning,
I want the screen to name the one thing to do about *this* fault,
so that I act instead of translating a slug into a guess.

## Acceptance Criteria

**AC1 — one gesture per cause, in the domain, on the established pattern.**

**Given** the twenty-one `Cause` variants
**When** a gesture is needed
**Then** `Cause::gesture` returns it, living beside `Cause::culprit` in `core/oracle.rs`
**And** the table is exhaustive by `match` — no wildcard — so a new cause stops the build until
somebody writes what to do about it
**And** it is pinned by a test that names every variant, on the `qos_for` /
`timestamp_source_for` / `culprit` pattern this repository has now applied three times.

**AC2 — the gesture is a thing to do, not a restatement of the fault.**

**Given** a cause and its slug
**When** the two are read side by side
**Then** the gesture says what the operator should DO — and a sentence that merely rewords the
slug is a defect this story's test names as such
**And** the four FR31 distinguishes — auth, permissions, timeout, empty result — reach four
different sentences.

**AC3 — the culprit table stays, and the two agree.**

**Given** `Cause::culprit` and `Cause::gesture`
**When** both are read for the same cause
**Then** they do not contradict: a `World` cause is never given a gesture that only the
operator's configuration could satisfy, and a `You` cause is never told to wait
**And** the agreement is asserted, not assumed — that is the check a table of twenty-one hand-
written sentences actually needs.

**AC4 — every surface that names a fault names its gesture.**

**Given** `/meters`, the end-to-end check, and the state screen's degraded caveat
**When** a fault is shown
**Then** the gesture shown is the cause's, not the culprit's three-way one
**And** `Culprit::repair` remains for the one case that has no cause — a reading the bridge
itself lost, which is `DropReason`'s and not the oracle's.

**AC5 — no stack trace, no raw type, on any of them.**

**Given** FR31's second half
**When** any fault reaches any screen
**Then** what is rendered is the taxonomy's wording, never a `Debug` rendering of an error type
**And** the check's existing rule stands: `SmartMeError`'s `Display` already carries its own
repair (story 2.6 AC5), and this story adds no second opinion about the same fault.

**AC6 — falsification.**

**Given** the table and the agreement rule
**When** either is broken
**Then** a test goes red, and the run's output is copied next to it.

## Out of scope

- **The six `DropReason` variants.** They have no `Cause`, and their gesture is already the
  right one: read the log, then report it — the bridge lost the reading, which is a defect
  here rather than a fault out there. `Culprit::repair` keeps that case.
- **Rewording `SmartMeError`.** Story 2.6 wrote a repair into each variant's `Display`, and the
  end-to-end check renders those words. Two tables about the same failure, in two crates, is
  how they come to disagree.

## Dev Notes

### What must not break

- **AR19's rule**: the screen reads, it does not judge. A gesture derived at render time from a
  stored cause is derivation, not judgement — the same standing as `repair` today.
- **The wording is the domain's.** `core/oracle.rs` holds it, so `/meters`, the check and the
  state screen cannot drift into three vocabularies for one fault.
- **Nothing formatted enters the fleet state** (story 6.3 AC4).

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:310`] — FR31
- [Source: `https://github.com/guycorbaz/smartme_mqtt/issues/103`] — the finding, from the review of story 6.4
- [Source: `crates/smartme-bridge/src/core/oracle.rs`] — `Cause::culprit` and the table pattern this copies
- [Source: `CLAUDE.md`] — falsify before trusting

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-20.

### Completion Notes List

**AC1 — met.** `Cause::gesture` sits beside `Cause::culprit`, exhaustive and wildcard-free, so
a new cause stops the build until somebody writes what to do about it. Fourth application of
the table pattern.

**AC2 — met, and the test is mechanical rather than aspirational.** Two properties are
asserted: the four failures FR31 names reach four different sentences, and every gesture begins
with something to do — the vocabulary is a fixed list of openings — and never contains its own
slug. A sentence that reworded the fault would fail the second.

**AC3 — met, and it is the check this table actually needed.** Twenty-one hand-written
sentences beside a twenty-one-row classification is the shape that drifts. What matters is not
that each row is defensible but that the two tables agree about the same cause: no `World`
gesture sends the operator to the configuration screen, no `You` gesture tells them to wait.

**AC4 — met on three surfaces**, and `Culprit::repair` stays for the case with no cause — a
reading the bridge itself lost, which is `DropReason`'s.

**The new table corrected a wrong instruction the old one had been giving.** `Culprit::You`
said *"open the configuration: a credential, a serial or a device id is wrong"*. For a rejected
credential that screen **has no field** — [ADR 0023] put the credential in the environment on
purpose — so the page was sending an operator to a form that could not hold the repair. The
cause's own gesture names `SMARTME_CLIENT_ID` and `SMARTME_CLIENT_SECRET`. Story 6.6's test was
amended to assert the new wording, with the reason written next to it: it is a repair, not an
accommodation.

### Falsification record

| # | Mutation | Went red with |
|---|---|---|
| 1 | `SourceUnreachable` given the configuration gesture | `source-unreachable is the WORLD's, and its gesture sends the operator to the configuration screen — which cannot repair it` |
| 2 | `DeviceNotInAccount` given the credential sentence | `two of them sharing a sentence is the defect [#103] recorded, one table further in` |
| 3 | `/meters` rendering `repair(culprit)` again | `a rejected credential must name the two variables that repair it` — the dump showed both meters reading identically, which is [#103] itself |

### File List

- `crates/smartme-bridge/src/core/oracle.rs` — modified (`Cause::gesture`, two tests)
- `crates/smartme-bridge/src/ui/screens.rs` — modified (the meter page's gesture)
- `crates/smartme-bridge/src/ui/check.rs` — modified (both places the check names a fault)
- `crates/smartme-bridge/src/ui/mod.rs` — modified (the degraded caveat, one test, one amended)
- `_bmad-output/implementation-artifacts/6-8-the-gesture-each-cause-asks-for.md` — new
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Change Log

- **2026-08-20** — Story 6.8. FR31, and [#103] closed. Three mutations run.
  `CONTRACT_VERSION` stays at 10 — these are words on screens, and nothing here reaches the
  wire.
