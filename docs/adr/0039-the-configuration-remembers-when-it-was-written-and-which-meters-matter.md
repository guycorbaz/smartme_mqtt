# ADR 0039 — The configuration remembers when it was written, and which meters matter

- **Status:** accepted
- **Date:** 2026-08-20
- **Amends:** nothing. **Extends:** the configuration file [ADR 0021](0021-configuration-is-editable-from-the-ui.md) made editable and [ADR 0023](0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md) made authoritative.
- **Issue:** [#104](https://github.com/guycorbaz/smartme_mqtt/issues/104)

## Context

FR35 asks for *"an auto-written, human-readable, timestamped configuration context line
(created, last change, meter count, priority meters) on the state screen"*, and the PRD's
example spells it: *"configured on X, last change Y, 4 meters, 2 priority Kamstrup"*.

**Three of the four have no source in this bridge.**

| The line wants | Where it would come from today |
|---|---|
| meter count | the configuration — **available** |
| created | nothing records it |
| last change | nothing records it |
| priority meters | nothing — and nothing can deduce it |

`StoredConfig` carries settings and no history. `MeterConfig` carries `meter`, `device_id`,
`serial` and `enabled`. And the priority notion is **a fact about the author's installation**,
not about anything the account says: the smart-me `Device` payload has an id, a name, a serial
and two values — no make, no model. The idea comes from the product brief, where *"2×
Kamstrup + smart-me module"* are named **the author's PRIORITY data**.

So FR35 could not be implemented without either inventing data or being amended. This is the
decision to take before a screen is drawn, not while drawing it.

## Decision

**1. `priority` becomes a per-meter flag, beside `enabled`, ticked on the configuration
screen.** It is the operator's statement about their own installation, which is the only place
the fact exists. `StoredMeter` gains `priority: bool`, absent meaning `false`.

**2. `created` and `last_change` are written by `store::save`, and the caller's values are
discarded** — exactly as `mapping_confirmed` has been since story 5.3, and for the same
reason: a fact the writer is trusted to supply is a fact that goes wrong at the first edit
through a path nobody thought of.

- `created` is stamped only when there was no readable file, and carried over otherwise. A
  file that predates this change never acquires one, because there is nothing true to write.
- `last_change` moves only when the written settings differ from the stored ones. A Save that
  changes nothing is not a change, and a line that said otherwise would make "last change"
  mean "last time somebody pressed a button".

**3. The file's mtime is not the answer, and this is the load-bearing half of the decision.**
A `docker cp`, a restore from backup, a `touch`, or an image update that rewrites the volume
all move it. It would be **a plausible date that is not the date of any change the bridge
made** — the exact shape of lie this project exists to refuse, arriving on the one screen
whose job is to orient somebody at three in the morning. **A file written before this change
says "unknown", and says why.**

## Alternatives refused

**Amend FR35 and drop the priority half.** Cheapest, and it loses something the brief asked
for explicitly: during an incident the first question is whether the meters that matter are
affected, and a context line that cannot answer it is a line that gets read once. Refused by
Guy, 2026-08-20.

**Free-text labels per meter instead of a flag.** More general, and it cannot produce the
sentence: summarising arbitrary labels as *"2 priority"* needs a convention nobody wrote down,
so the generality buys an ambiguity. Refused.

**Deduce priority from the meter name or serial.** Refused on sight: the bridge would be
asserting a fact about hardware it has no evidence for, which is the failure mode named in
this repository's own working rules.

## Consequences

- `SCHEMA_VERSION` moves 4 → 5. **The three fields being `#[serde(default)]` is not enough for
  an existing file to read**, and this ADR said otherwise until the implementation measured it:
  the version check lives in `read`, so a version-4 file was refused on every path —
  `current_or_blank` included, which is what pre-fills the configuration screen, so the
  operator would have found an empty form. [ADR 0040](0040-the-first-schema-migration.md) is
  the answer: version 4 migrates in memory, keeping every setting, and the two dates are absent
  rather than invented.
- The configuration screen gains one checkbox per meter row, and `save_config` a value to
  carry through the round trip — including the round trip through **discover**, which must not
  cost the operator a tick they had just made.
- `store::save` needs a wall-clock instant. `UiState` gains one, and **exposes only the wall**:
  a `MonotonicMs` from a second `SystemClock` would count from a second origin, which is the
  defect `Control::clock`'s own documentation warns about. What the UI gets is `wall_now()`,
  not a clock.
- Nothing on the wire moves. `CONTRACT_VERSION` is untouched: this file is read by the bridge,
  never published.
