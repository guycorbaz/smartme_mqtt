# ADR 0040 — The first schema migration: version 4 reads, in memory

- **Status:** accepted
- **Date:** 2026-08-20
- **Amends:** the `store` doctrine *"refused rather than read by guesswork"* — on its own terms, which named this exception in advance.
- **Issue:** [#105](https://github.com/guycorbaz/smartme_mqtt/issues/105)

## Context

`store`'s module documentation has said since story 5.1:

> Refusing is the only honest answer **until a migration exists to be the other one**.

And `SCHEMA_VERSION`'s own comment says the bump is not optional:

> An exception made once to "bump whenever a field is added" is how the guarantee stops being
> one.

[ADR 0039](0039-the-configuration-remembers-when-it-was-written-and-which-meters-matter.md) adds
three fields, so the version goes 4 → 5. **That is the moment the refusal stops being free.**

The version check lives in `read`, not only in `load` — so a version-4 file is refused on every
path, `current_or_blank` included. That function is what pre-fills the configuration screen, so
the operator would open the form and find it **empty**: group id, node id, broker host and port,
publish period, and every meter row to retype, then the mapping to confirm again. FR27 says the
configuration survives an image update; a version bump that costs a full retype is that
requirement failing quietly through the back door.

## Decision

**1. A version-4 file reads.** Its three missing fields take exactly the defaults ADR 0039
argued for: no creation date, no change date, nothing marked as mattering. Every other setting
is carried through untouched, because `serde` already parses them — the refusal was the only
thing in the way.

**2. The migration happens IN MEMORY, and the file is rewritten only at the next `save`.**
`read` is called by every screen render. Rewriting there would make a page view a disk write,
and would hand a read-only surface the power to change what is on disk — during, for instance,
the very incident the operator opened the page to diagnose. `save` already stamps the constant;
that is where the file changes.

**3. What stays refused, and the list is the decision's other half:**

- **any version above this build's** — a file from the future is not guesswork-readable, and
  this is the case the original rule was written for;
- **any version below 4** — no migration is written for it. Writing one for a file nobody has
  would be code with no evidence behind it, which is the failure mode this repository's rules
  name first.

## Consequences

- `read` gains a `migrate` step between parsing and validation, and the module documentation
  loses the sentence that said no migration existed.
- Every future field addition now has somewhere to go: the migration list grows by one entry,
  and the version bump stops implying a retype.
- **A file this build writes is still refused by an older build**, and that is correct: the
  older build genuinely does not know what `created_ms` means. Rollback is Epic 8's subject and
  is not weakened by this — it was already the case for versions 2, 3 and 4.
