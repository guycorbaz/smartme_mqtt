# Story 4.18: Correct ADR 0010's wording — its conclusion stands, its premise is overstated

Status: done

> **This story changes no code.** It repairs a document of record whose overstatement made a
> `MUST` violation invisible for an epic, and it amends the one requirement that still rested on
> the same false premise. The deliverable is that the next person reasoning from ADR 0010 is not
> misled the way we were.

## Story

As the maintainer,
I want ADR 0010 to say something true about the specification,
so that the next person reasoning from it is not misled the way we were.

## Acceptance Criteria

**AC1 — the conclusion is confirmed, clause by clause.**

**Given** ADR 0010's decision that no broker acknowledgement exists for data
**When** it is checked against the pinned specification
**Then** `tck-id-topics-ndata-mqtt` and `tck-id-topics-ddata-mqtt` are cited as mandating QoS 0
with retain false, so FR20 was genuinely unimplementable as written.

**AC2 — the premise is corrected, per message type.**

**Given** the blanket claim *"QoS 0 for every edge-node message … only host STATE messages use
QoS 1"*
**When** each of the six messages it names is read
**Then** an addendum states what the norm actually says for each, citing the `tck-id`
**And** it records that NDEATH has **no** chapter-4 QoS clause and that the Will **MUST be QoS 1**
**And** it records what the overstatement cost: [#26] invisible from 2026-07-28 to 2026-08-10.

**AC3 — NFR10 is amended in the same pass, and story 4.16 unblocks.**

**Given** NFR10's *"read→broker-ACK latency"*
**When** it is resolved
**Then** it reads read→accepted-for-transmission, with the thresholds' treatment stated rather
than left implicit
**And** `epics.md`, the PRD and `sprint-status.yaml` carry it, and 4.16 is no longer BLOCKED.

**AC4 — the overstatement is corrected wherever it was copied, not only at its source.**

**Given** that a false sentence about the norm travels
**When** the repository is searched for it
**Then** every copy is annotated or corrected.

*Decided at drafting. The reason ADR 0010's sentence cost an epic is that it was read as a
document of record; a copy of it in another artifact is exactly as authoritative to the next
reader.*

## Tasks / Subtasks

- [x] **Task 1 — read the norm, clause by clause** (AC1, AC2)
  - [x] `tck-id-topics-{nbirth,dbirth,ndata,ddata,ddeath}-mqtt` — all five mandate QoS 0, retain
        false. Line numbers recorded, not paraphrased.
  - [x] Establish that **no** `tck-id-topics-ndeath-mqtt` exists.
  - [x] Find both clauses that mandate QoS 1 for the will:
        `tck-id-message-flow-edge-node-birth-publish-will-message-qos` (chapter 5) and
        `tck-id-payloads-ndeath-will-message-qos` (chapter 6).

- [x] **Task 2 — the addendum** (AC2)
  - [x] Written into ADR 0010 itself, dated, with the per-message table and the cost.
  - [x] The Status line says the premise was corrected, so a reader who stops at the header knows.

- [x] **Task 3 — NFR10** (AC3)
  - [x] Amend the PRD and `epics.md` together with the addendum, the FR20 / ADR 0010 precedent.
  - [x] Unblock story 4.16 **without drafting it**, and name the two decisions its author must
        take at drafting rather than defer.

- [x] **Task 4 — the copies** (AC4)
  - [x] `_bmad-output/implementation-artifacts/1-11-12-13-async-shell.md` repeated the claim;
        annotated in place rather than rewritten, the way this repository keeps its corrections.
  - [x] `mqtt_driver.rs` already carries the correction (story 4.17); `CLAUDE.md` already states
        it as a lesson. Neither needed changing — checked rather than assumed.

## Dev Notes

### What must not break

- **ADR 0010's decision is not reopened.** FR20's amendment stands. Only the reasoning is
  repaired, and the addendum says so in its first line.
- **The thresholds in NFR10 are not silently weakened.** They are unchanged, which makes the
  requirement *easier*, and that is stated in the ADR, the PRD, the epic and [#99] rather than
  left for someone to notice.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md:1250`] — Story 4.18's criteria
- [Source: `docs/adr/0010-fr20-delivery-claim-at-qos0.md`] — the ADR and its addendum
- [Source: `docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_4_Topics.adoc`] — the five QoS clauses
- [Source: `CLAUDE.md`] — read the norm first; this story is that rule applied to a past failure

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-19.

### Completion Notes List

**AC1 — met.** `tck-id-topics-ndata-mqtt` (`Sparkplug_4:273`) and `tck-id-topics-ddata-mqtt`
(`:444`) both read *"MUST be published with MQTT QoS equal to 0 and retain equal to false"*. No
acknowledgement exists at QoS 0, so ADR 0010's decision was right.

**AC2 — met, and the premise is wrong in two distinct ways.** *"Every edge-node message"* is
wrong about **NDEATH**: there is no `tck-id-topics-ndeath-mqtt` in chapter 4 at all. And *"only
host STATE messages use QoS 1"* is wrong about the **will**, which two clauses in two chapters
require to be QoS 1 — `tck-id-message-flow-edge-node-birth-publish-will-message-qos`
(`Sparkplug_5:183`) and `tck-id-payloads-ndeath-will-message-qos` (`Sparkplug_6:1513`). The will
IS the NDEATH registered at CONNECT, so the sentence was wrong precisely about the message whose
QoS the project then got wrong.

**The cost, stated in the addendum rather than softened.** The bridge registered its will at
QoS 0 from 2026-07-28 to 2026-08-10 — a MUST violation — and this ADR is why nobody looked. The
test that existed asserted the violation rather than catching it, so it had to be replaced
wholesale by story 4.17.

**AC3 — met, and it unblocks 4.16 without writing it.** NFR10 becomes
read→accepted-for-transmission. **The thresholds are unchanged and that makes the requirement
easier**, since acceptance happens strictly earlier than any acknowledgement would; that sentence
is in the ADR, the PRD, the epic and [#99], because a weaker requirement wearing the same numbers
is exactly the kind of substitution the epic said must not happen quietly. Story 4.16 stays
unwritten, with the two decisions its author must take named: **where** acceptance is observed
(`try_publish` answers `Ok` on entering `rumqttc`'s queue, not on leaving the socket — [#85]),
and whether to tighten the budget once measured.

**AC4 — met, and one copy was found.** `1-11-12-13-async-shell.md` repeated *"Sparkplug mandates
QoS 0 for every edge-node message"* as its own reasoning. Annotated in place, not rewritten:
this repository keeps its corrections beside what they correct. `mqtt_driver.rs` was already
repaired by story 4.17 and `CLAUDE.md` already carries the lesson — both checked rather than
assumed.

**No code changed, no conformance row moved, `CONTRACT_VERSION` unchanged at 10.** The two rows
that state the will's QoS moved to `conformant` in story 4.17 and are untouched here.

### Review Findings (2026-08-19, same day)

Reviewed mechanically alongside 4.14 and 4.19: every identifier cited against the functions that
exist, every `file.rs:N` against the file it names. **This story cites neither** — its claims are
figures it printed and clauses it quotes, both of which were checked at writing against the run
output and the pinned specification.

**Nothing found.** Recorded rather than left silent: a review that finds nothing and says so is
worth more than one that is assumed to have happened. The two stories reviewed beside it each
carried a false citation, which is the base rate this one was measured against.

### File List

- `docs/adr/0010-fr20-delivery-claim-at-qos0.md` — modified (addendum, Status line)
- `_bmad-output/planning-artifacts/epics.md` — modified (NFR10, the Epic 4 note, story 4.16 unblocked)
- `_bmad-output/planning-artifacts/prd.md` — modified (NFR10)
- `_bmad-output/implementation-artifacts/1-11-12-13-async-shell.md` — modified (the copied claim, annotated)
- `_bmad-output/implementation-artifacts/4-18-correct-adr-0010-wording.md` — new
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Change Log

- **2026-08-19** — Story 4.18. ADR 0010 keeps its decision and loses its overstatement, with a
  per-message table citing `tck-id`s. NFR10 amended to a measurable analogue; story 4.16
  unblocked and left unwritten. One copy of the false premise found elsewhere and annotated.
  [#99].
