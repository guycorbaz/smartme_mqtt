# Story 6.7: The configuration context line — what the bridge knows about its own configuration

Status: review

> **The last requirement Epic 6 owes.** FR35 asks for one auto-written line on the state
> screen: *"configured on X, last change Y, 4 meters, 2 priority Kamstrup"*. Three of those
> four facts do not exist in this bridge — [ADR 0039](../../docs/adr/0039-the-configuration-remembers-when-it-was-written-and-which-meters-matter.md)
> decided where they come from before this story starts, because a screen drawn first would
> have had to invent them.
>
> **The decision that shapes the work: the file's mtime is not a date this bridge may use.** A
> `docker cp`, a restore, a `touch`, an image update rewriting the volume — all move it, and
> the line would carry a plausible date that is nobody's change. A configuration written
> before this story says **"unknown"**, and says why.

## Story

As the operator opening the bridge after weeks away,
I want one line telling me when this configuration was set up, when it last changed, how many
meters it serves and how many of them are the ones I care about,
so that I know what I am looking at before I read anything else.

## Acceptance Criteria

**AC1 — priority is the operator's statement, stored and ticked.**

**Given** the configuration screen
**When** the operator marks a meter as priority and saves
**Then** `priority` persists in `config.toml` beside `enabled`
**And** it survives the **discover** round trip — a pick must not cost a tick the operator has
just made (the defect story 3.4 already repaired once for the other fields)
**And** absent means `false`, so an older file reads and simply claims no priorities.

**AC2 — the dates are the writer's, never the caller's.**

**Given** `store::save`, which already computes `mapping_confirmed` and discards what it was
handed (story 5.3 AC3)
**When** a configuration is written
**Then** `created` and `last_change` are computed there on the same principle
**And** `created` is stamped **only when there was no readable file**, and carried over
otherwise — a file that predates this story never acquires one, because there is nothing true
to write.

**AC3 — a Save that changes nothing is not a change.**

**Given** an operator who opens the form and saves without editing anything
**When** the write completes
**Then** `last_change` does not move
**And** the comparison is on the settings themselves, so "last change" cannot come to mean
"last time somebody pressed a button" — the same distinction story 6.3 drew between
`last_changed_at` and `last_published_at`, one layer down.

**AC4 — the line reads as a sentence, and says "unknown" when it does not know.**

**Given** the state screen
**When** it is rendered
**Then** one line carries: when the configuration was created, when it last changed, how many
meters it serves, and how many of those are priority
**And** a date the file does not carry is rendered as **unknown, with the reason** — never as
the file's mtime, never as "now", never omitted silently.

**AC5 — the schema moves and nothing is lost.**

**Given** a `config.toml` written by the previous schema
**When** this build reads it
**Then** every setting survives, the mapping confirmation survives, and the two dates are
absent rather than invented
**And** `zero_config_loss_update` still passes, which is the test that exists for exactly this.

**AC6 — falsification.**

**Given** each mechanism above
**When** it is broken
**Then** a test goes red, and the run's output is copied next to it.

## Out of scope

- **[#103]** — the per-cause repair gesture. Still FR31's.
- **Any use of `priority` beyond the count in this line.** Ordering the meter table by it,
  alerting differently on it, or filtering `/healthz` are all defensible and none is FR35. The
  flag is stored and counted here; what else reads it is a later decision, not a side effect
  of this story.

## Dev Notes

### What must not break

- **`save` is the boundary every writer passes.** The dates go there, not into the screen, for
  the reason its own documentation already gives about `mapping_confirmed`.
- **Zero config loss.** `SCHEMA_VERSION` 4 → 5 with all three new fields `#[serde(default)]`.
- **The UI gets a wall clock, not a clock.** A second `SystemClock` would give a second
  monotonic origin, which is the defect `Control::clock`'s documentation warns about — so
  `UiState` exposes `wall_now()` and never the clock itself.
- **The credential stays out.** Nothing in this story renders or stores one (ADR 0023).

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:314`] — FR35
- [Source: `_bmad-output/planning-artifacts/prd.md:155`] — the sentence the PRD writes out
- [Source: `docs/adr/0039-the-configuration-remembers-when-it-was-written-and-which-meters-matter.md`] — where the three missing facts come from, and why not the mtime
- [Source: `crates/smartme-bridge/src/app/store.rs:676`] — `save`, and the `mapping_confirmed` precedent
- [Source: `crates/smartme-bridge/src/app/store.rs:59`] — `SCHEMA_VERSION`
- [Source: `CLAUDE.md`] — falsify before trusting; decide at drafting

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-20.

### Completion Notes List

**AC1 — met, in both directions.** `priority` is stored, rendered ticked, and read back. The
round trip is asserted both ways because **a field the form reads but does not render is worse
than one it ignores**: the tick survives one Save and vanishes at the next, silently. That is
the defect story 3.4 repaired for the other fields, and it would have arrived here through a
new one.

**AC2 and AC3 — met, at the boundary every writer passes.** `save` computes both dates and
discards the caller's, exactly as it has computed `mapping_confirmed` since story 5.3.
`same_settings` is a second, distinct comparison beside `same_mapping`, and the difference is
load-bearing: marking a meter as mattering **is** a settings change (the date moves) and is
**not** a mapping change (the confirmation stands). Both halves are pinned.

**AC4 — met, and the unknown case is the one the test asserts first.** A configuration written
before this story says so in words. The alternative the ADR refused — the file's mtime — was
implemented as a mutation and produced *"created just now"* about a file planted seconds
earlier: a plausible date that is nobody's change, which is precisely the shape of lie this
project exists to refuse.

**AC5 — met, and it cost an ADR that this story did not plan for.** See below.

### The finding that changed the story: the version bump would have emptied the form

**Written into ADR 0039 at drafting: "the three new fields are all `#[serde(default)]`, so an
existing file reads, keeps working, and simply has no dates." That was false**, and the
implementation measured it rather than trusting it. The version check lives in **`read`**, not
only in `load` — and `read` is what `current_or_blank` calls to pre-fill the configuration
screen. So a version-4 file would have been refused everywhere, and the operator would have
opened the form to find it **empty**: group, node, broker, period and every meter row to
retype, then the mapping to confirm again. FR27 says the configuration survives an image
update; that is the requirement failing quietly through the back door.

Guy chose the migration ([#105], 2026-08-20). [ADR 0040] writes the first one — version 4
reads, in memory, keeping every setting; the file becomes version 5 at the next save; anything
older than 4 and anything from the future stays refused. The module's own doctrine had named
this exception in advance: *"refusing is the only honest answer **until a migration exists to
be the other one**"*.

**ADR 0039 is corrected rather than left standing**, with what was measured written into it.

### Ages, not calendar dates — recorded rather than left to be noticed

The line says *"created three weeks ago"*, not a date. The PRD asks for a human timestamp and
gives that exact form as its own example, and `ago` is what every other screen here speaks. A
calendar date would need a calendar: this workspace has no date library among its direct
dependencies, and adding one to render one line is not a trade this story is allowed to make
quietly. **Recorded as a residue**: if an absolute date is wanted later, it is a dependency
decision, not a formatting one.

### Falsification record

| # | Mutation | Went red with |
|---|---|---|
| 1 | `created_ms` stamped on every write | `a later write must not restamp the creation … left: Some(1784984793000), right: Some(1784984000000)` |
| 2 | `last_change_ms` stamped on every write | `a Save that changed nothing must not move the last change …` |
| 3 | `priority` added to `mapping_projection` | `marking a meter as one that matters must not cost a confirmation click` |
| 4 | the context line falls back to the file's mtime | `a configuration whose creation nobody recorded must SAY so` — the dump read *"created just now"* |
| 5 | the priority checkbox rendered without `{starred}` | `a meter marked as mattering must come back TICKED` |
| 6 | the migration arm deleted (the state before [ADR 0040]) | `a version-4 file must READ: the version check is in read, so refusing it empties the very form the operator would repair it in` |

### One existing test AMENDED, and why that is not a weakening

`a_file_from_a_different_schema_is_refused_and_says_why` planted `SCHEMA_VERSION - 1` and
asserted a refusal. Since [ADR 0040] that file **reads**, so the test now asserts the gate that
remains — a version from the FUTURE, which is the case the rule was written for — **and**, in
the same test, that the migrated step reads with every setting intact. The two halves only mean
something together: a build that had simply stopped checking versions would pass either one
alone.

### File List

- `crates/smartme-bridge/src/app/store.rs` — modified (`priority`, `created_ms`, `last_change_ms`, `SCHEMA_VERSION` 4→5, `migrate`, `same_settings`, `save`'s new argument, four tests)
- `crates/smartme-bridge/src/app/config.rs` — modified (`priority` on `MeterConfig` and `RawMeter`)
- `crates/smartme-bridge/src/app/reconfigure.rs` — modified (`priority` classified: hot, costs nothing)
- `crates/smartme-bridge/src/ui/screens.rs` — modified (the checkbox, `configuration_context`, one test)
- `crates/smartme-bridge/src/ui/mod.rs` — modified (`UiState`'s clock and `wall_now`, the context line on `/`, one test)
- `crates/smartme-bridge/src/main.rs` — modified (the UI's wall clock)
- `docs/adr/0039-…md` — modified (the false consequence corrected)
- `docs/adr/0040-the-first-schema-migration.md` — **new**
- `docs/manual/chapters/04-configuration.tex`, `09-appendix-config-reference.tex` — modified
- every test constructing a `MeterConfig`, `StoredMeter`, `StoredConfig` or `UiState` — modified mechanically

### Change Log

- **2026-08-20** — Story 6.7. FR35, and the three facts it needed that did not exist. Six
  mutations run. `CONTRACT_VERSION` stays at 10 — the configuration file is read by the
  bridge, never published.
