# Story 4.19: Finish chapter 4 — the 29 clauses Story 4.1 did not record

Status: review

> **This story changes no code.** It makes "chapter 4 is done" a countable claim rather than a
> remembered one — and the counting is what found a `MUST` nobody had read.

## Story

As the maintainer,
I want chapter 4's payload clauses audited and its tally made to close,
so that "chapter 4 is done" is a countable claim rather than a remembered one.

## Acceptance Criteria

**AC1 — the 29 are ruled on, each against chapter 4's own wording.**

**Given** `Sparkplug_4_Topics.adoc`'s 70 `tck-id`s, of which 41 carried a verdict
**When** the remaining 29 are walked against the implementation
**Then** each gains a row, or a place in a collective block that names its member ids
**And** no verdict is copied from its chapter-6 twin — twins are cited as cross-references, and
each clause is read against chapter 4's own sentence.

**AC2 — the arithmetic closes.**

**Given** the chapter-4 tally
**When** it is restated
**Then** `conformant + deviation + gap + n/a = 70`, and the whole-specification table agrees with
it.

**AC3 — the Status row changes.**

**Given** the Status table's *"audited, not complete"*
**When** AC1 and AC2 hold
**Then** it reads **done**.

**AC4 — `topics-nbirth-bdseq-increment` is ruled on, not assumed.**

**Given** the drafting note that expected a `deviation` owned by Story 4.10, and the chapter-5
note that later asked for *"the evidence above rather than a fresh gap"*
**When** the clause is read
**Then** whichever verdict the clause's own words earn is recorded, with its evidence
**And** if the two halves of the clause disagree, both are stated.

*Amended at implementation, 2026-08-19. The epic's own criterion — "recorded as a `deviation`
owned by Story 4.10" — was written before 4.10 fixed the defect, and the matrix then asked for
the opposite. Following either instruction would have produced a wrong row; reading the clause
produced a third answer. See the notes.*

**AC5 — the mechanism that missed it gains the check.**

**Given** that the document disagreed with itself for a day and the gate did not notice
**When** the checker is extended
**Then** each chapter's own tally must match its row in the whole-specification table, and the
extension is falsified against the real defect.

## Tasks / Subtasks

- [x] **Task 1 — the inventory, mechanically** (AC1)
  - [x] Extract the 70 ids; extract what carries a verdict (table rows + the host block's named
        members); difference them.
  - [x] **Count by matching, not by reading prose.** A first pass counted mentions in prose as
        records and got 25 where the answer is 26 — `topics-nbirth-rebirth-metric` is discussed in
        two paragraphs and has no row.

- [x] **Task 2 — rule on the 29** (AC1)
  - [x] 16 edge-node clauses, 10 device clauses, 3 Host Application ids.
  - [x] Each conformant row names a test. No row asserted from reading the code alone.

- [x] **Task 3 — the arithmetic** (AC2, AC3)
  - [x] Chapter-4 tally `17 · 0 · 3 · 21` (41 rows) → `31 · 4 · 3 · 32` (70 clauses).
  - [x] Whole-specification table and Status row updated.

- [x] **Task 4 — the checker** (AC5)
  - [x] Each chapter tally must equal its row. Falsified against the real defect.

- [x] **Task 5 — the record**
  - [x] [#100] for the `bdSeq` zero-start; `CONTRACT_VERSION` untouched.

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-19.

### Completion Notes List

**AC1 — met. 29 clauses ruled on: 14 conformant, 4 deviations, 11 n/a.** The four deviations are
`topics-nbirth-bdseq-increment` (new, below), `topics-dbirth-timestamp` and
`topics-ddata-timestamp` (ADR 0013, the deliberate acquisition-time stamping), and
`topics-ddata-payload` (report-by-exception, [#32], ruled under chapter 2 and recorded here rather
than re-decided).

**AC4 — met, and it is this story's finding.** `topics-nbirth-bdseq-increment` states **two**
obligations — *start at zero* **and** *increment by one per CONNECT* — where chapter 6 states only
the second. The increment is delivered (Story 4.10). **The zero start is not:** `load_bd_seq`
returns `BdSeq::before_first()` = 0 on a first run and `SparkplugPublisher::new` advances past it,
so a brand-new bridge publishes **`bdSeq = 1`**. Not deduced — story 4.13's `new_session()`
mutation had already printed `born 1, reborn 1` off the wire. [#100] carries it; the row is a
`deviation` with both halves stated.

**Two instructions pointed at wrong answers, and following either would have been worse than
reading.** The epic said record a `deviation` owned by 4.10 — written before 4.10 fixed the
defect. The matrix's own chapter-5 note said cite 4.10's evidence *"rather than open the row as a
fresh gap"* — correct for the increment, silent about the half nobody had read. The rule that
produced the right answer is AC1's: read chapter 4's own sentence.

**AC5 — met, and the defect it now catches is one I did not catch this morning.** The
whole-specification table read `6 — Payloads | 38 | 4 | 8 | 59` while chapter 6's own tally read
`39 · 4 · 7 · 59` — story 4.12 moved the tally and left the summary row, and **the gate passed
anyway**: the rows summed to a Total built from the stale row, so the document was internally
consistent and wrong. I reviewed 4.12 this morning and did not see it either. The checker now
pairs each chapter tally with its row; falsified by reinstating the defect, which goes red with
`chapter 6: its own tally says [39, 4, 7, 59], the whole-specification table says [38, 4, 8, 59]`.

**The host block was short by three, for the same reason as the chapter.** The specification
carries seven `-death-payload*` ids and the block listed four, so three clauses were recorded
nowhere while the block read as complete.

**The whole specification now closes**: `106 · 10 · 32 · 155 = 303`, with no "of" anywhere in the
total — every `tck-id` in chapters 1–6 and 10 carries a verdict.

### Review Findings (2026-08-19, same day)

Reviewed by checking this story's own citations mechanically — every test name against the
functions that exist, every `file.rs:N` against the file. **Two of its own were wrong**, and that
is what found the larger one.

- [x] [Review][Patch] **A cited test did not exist.** `topics-ddata-seq-num` named
      `the_birth_then_data_sequence_is_contiguous` as its proof. Nothing by that name is defined
      anywhere; the real assertion is
      `sequence_numbering_is_continuous_across_node_and_device_messages`. This is story 4.11's
      defect — *"a doc comment claimed an assertion that does not exist"* — committed again, in a
      row written to close a completeness audit.
- [x] [Review][Patch] **A line citation pointed at the wrong line.** `sparkplug_publisher.rs:419`
      was cited for `rebirth_metric`, which lives at `:614`; `:419` is `self.declared = declared;`.
      **The number was copied from an existing cell rather than checked** — so the drift
      propagated into a new row, which is the cheapest way to write a wrong citation.
- [x] [Review][Defer] **The matrix cites 53 distinct code positions by line number, and the drift
      is general** — 34 point at code, 4 at a bare closing brace, 13 at a comment, 2 at
      unresolvable files. Measured, filed as [#101] with the fix (cite the symbol, which is
      mechanically checkable) rather than migrated here: 53 citations is a chore for one pass, not
      a story's tail.

**Verified and left standing:** the 29 verdicts, the arithmetic (`31 · 4 · 3 · 32 = 70`, total
`106 · 10 · 32 · 155 = 303`), the strengthened checker and its falsification, and every other test
name cited in the chapter — 26 checked, 25 resolve to a function or a test file.

### File List

- `docs/sparkplug-conformance.md` — modified (29 verdicts, tallies, Status row, chapter-6 row corrected)
- `scripts/check-conformance-arithmetic.py` — modified (tally ↔ table check)
- `_bmad-output/implementation-artifacts/4-19-finish-chapter-4-payload-clauses.md` — new
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Change Log

- **2026-08-19** — Story 4.19. Chapter 4 closes at 70 clauses with no remainder; the whole
  specification closes at 303. One `MUST` found unmet by reading the clause instead of its twin
  ([#100]), one stale summary row corrected, and the checker gained the comparison that would
  have caught it.

[#100]: https://github.com/guycorbaz/smartme_mqtt/issues/100
[#32]: https://github.com/guycorbaz/smartme_mqtt/issues/32
