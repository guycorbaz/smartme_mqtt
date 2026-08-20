# Story 8.1: The manual honours its eight promises

Status: review

> **Eight `\stub{}` marks stand in the manual, and four of them are overdue.** Each one names
> what it defers and the condition for writing it. Three say *"once Epic 6 exists"* or *"once
> Epic 6 documents it"* — Epic 6 closed on 2026-08-20. One says *"authored in Epic 7"* — Epic 7
> closed the same day. **No retrospective saw them, because nobody re-reads the manual when an
> epic closes.**
>
> That is the epic-6 defect in its mirror image: there, two delivered requirements were counted
> as owed; here, four documentation debts carry the name of epics that closed without paying
> them.

## Story

As the author installing this bridge from its manual a year from now,
I want the manual to describe what the bridge does rather than what it was going to do,
so that FR42's promise — *documentation sufficient to install and troubleshoot without reading
the code* — is true of the document rather than of its table of contents.

## Acceptance Criteria

**AC1 — the four overdue stubs are written, from what shipped.**

**Given** the stubs in chapters 4, 6, 8 and 9
**When** each is replaced
**Then** it describes the screens and procedures Epics 6 and 7 actually delivered — the meter
view and its columns, the two independent healths, the end-to-end check, the configuration
context line, the per-cause gestures, the update procedure with its rollback point
**And** nothing is described that does not exist: every claim is checkable against a route, a
file or a test.

**AC2 — the four remaining stubs are either written or re-dated.**

**Given** the stubs in chapters 1, 5, 7 and 10
**When** the epic ends
**Then** each is written, or its condition is restated in terms of something that will actually
happen — a stub whose condition has no owner is a promise nobody is keeping.

**AC3 — the troubleshooting chapter carries the eight failure modes the PRD names.**

**Given** the PRD's list — meter silent, auth 401, rate-limit 429, broker unreachable,
single-meter vs global, priority-Kamstrup partial, stale-but-alive, clock/DST, Docker
restart/retain
**When** chapter 7 is read
**Then** each is a Symptom → Cause → Action → **Confirmation** entry, and the confirmation says
what the operator should SEE once the repair worked — that fourth field is what makes it a
procedure rather than a hint.

**AC4 — the manual still builds, and nothing claims a screen that is not there.**

**Given** `latexmk` and the routes the bridge serves
**When** the manual is built
**Then** it compiles with no new warnings
**And** every path and route it names exists — checked mechanically, because a manual is the
one artefact whose defects nobody's compiler catches.

**AC5 — falsification.**

**Given** AC4's mechanical check
**When** the manual names a route that does not exist
**Then** it goes red, and the run's output is copied next to it.

## Out of scope

- **The crate's publication bar** — README, CHANGELOG, `cargo publish --dry-run`. Story 8.2.
- **The Tier-3 session.** Story 8.3, and it needs a live Ignition rather than an author.
- **Rewriting what is already written.** Eight chapters carry real content; this story fills
  holes rather than revisiting prose that stands.

## Dev Notes

### What must not break

- **The manual describes the product, not the plan.** A sentence about what a later story will
  do belongs in a stub with a condition, never in the body.
- **Every route named must exist.** AC4 makes that mechanical; the check is the story's lasting
  contribution, more than any paragraph.
- **No credential, anywhere** — the manual documents `SMARTME_CLIENT_ID`/`SECRET` as variables
  and never shows a value.

### References

- [Source: `docs/manual/preamble/style.tex`] — `\stub`, and what it promises
- [Source: `_bmad-output/planning-artifacts/prd.md:155`] — the eight failure modes and the four-field shape
- [Source: `_bmad-output/implementation-artifacts/epic-6-retro-2026-08-20.md`] — F3, whose mirror image this story is
- [Source: `CLAUDE.md`] — falsify before trusting

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-20.

### Completion Notes List

**AC1 and AC2 — met: all eight stubs are written, and none was re-dated.** Four were overdue
(chapters 4, 6, 8, 9); the four that were not (1, 5, 7, 10) turned out to be writable too,
because what they were waiting for had arrived. **No stub remains in the manual.**

What was written, chapter by chapter: the meter view's nine columns and the pair an operator
must read together (6); the two independent healths and why an unreachable broker is not a
bridge fault (6); the end-to-end check and why it publishes nothing (6); the configuration
screen, the account pick-list and the first-run confirmation (4); where the UI listens and how
a meter becomes a topic (9); the update procedure with its rollback point (8); the closed
metric list and `CONTRACT_VERSION`'s governance (5); eight failure modes in Symptom → Cause →
Action → Confirmation (7); six glossary terms (10); and a TikZ data-flow figure whose captions
are its point (1).

**AC3 — met, and the fourth field is what makes it a procedure.** Each of the eight modes ends
with what the operator should SEE once the repair worked. Two of them confirm *negatively* —
the stale-but-alive entry's confirmation is that the quality on the wire is **not** `Good`,
because that is the bridge working rather than failing.

**The rollback section carries the trap nobody would find until they needed it.** A
configuration written by a newer build is refused by an older one — the schema check ADR 0040
governs — so a rollback across a schema change leaves a bridge that starts, serves its UI and
refuses to publish. The manual now says to copy the state directory before an update that
changes the schema.

**AC4 — met, and it is this story's lasting contribution.** A test reads every `\code{/…}` in
the manual and every `.route(` in `ui/mod.rs`, and fails when the manual names a route the
bridge does not serve. Routes move — story 6.6 added `/check` — and nothing would have noticed
a renamed one.

### The scan that would have accused the manual of a defect it did not have

The route scan read `ui/mod.rs` **line by line** and so missed every route whose path sits on
the line below `.route(` — the shape `axum` routes take once they carry two handlers. It
reported `/config` and `/confirm` as inventions of the manual. **The guard caught it**: the
test asserts the scan found at least five routes including `/healthz` before it trusts the set.
Without that assertion this test would have failed loudly against correct documentation, which
is the harness defect Epic 4's action E2 named and Epic 7 met twice.

### Falsification record

| # | Mutation | Went red with |
|---|---|---|
| 1 | the troubleshooting chapter sends the operator to `/status` | `the manual names routes this bridge does not serve: 07-troubleshooting.tex: names /status` |
| — | *(caught during development, recorded)* the line-by-line route scan | `names /config`, `names /confirm` — a correct manual accused by a broken scan, caught by the scan's own presence guard |

### File List

- `docs/manual/chapters/01, 04, 05, 06, 07, 08, 09, 10` — modified (eight stubs written)
- `crates/smartme-bridge/tests/manual_names_real_routes.rs` — **new**
- `_bmad-output/implementation-artifacts/8-1-…md`, `sprint-status.yaml` — new/modified

### Change Log

- **2026-08-20** — Story 8.1. Eight promises honoured, one mechanical guard added. No
  production code changed. `CONTRACT_VERSION` untouched.
