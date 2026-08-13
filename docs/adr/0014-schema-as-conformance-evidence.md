# ADR 0014 — The pinned protobuf schema is admissible conformance evidence, for field types only

- **Status:** Accepted
- **Date:** 2026-07-28
- **Related:** [#31](https://github.com/guycorbaz/smartme_mqtt/issues/31), `docs/sparkplug-conformance.md` §"How to read this", Stories 4.1–4.3, ADR 0012
- **Amends:** the verdict rules inherited from Story 4.1, which admitted only a named test.

## Context

The conformance matrix has one rule holding it up: **a `conformant` row must name a test that proves
the clause; otherwise the verdict is `gap`.** That rule exists because contract v1 shipped quality
codes a real host read as `Good` while 148 internal tests agreed with each other (ADR 0012). Nothing
in the matrix is allowed to rest on our own reading of our own code.

The chapter-6 audit met a class of clause the rule handles badly. Several clauses constrain the
*type* of a wire field:

> `tck-id-payloads-metric-datatype-value-type` — the datatype MUST be an unsigned 32-bit integer.
>
> `tck-id-payloads-metric-propertyvalue-type-type` — the 'type' of the Property Value MUST be an
> unsigned 32-bit integer.

These are discharged by the generated Rust type being `Option<u32>`. There is no program we could
write that puts anything else in that field: the violation is not untested, it is **unrepresentable**.
Demanding a test here would produce a test that cannot fail, which is precisely what
`CLAUDE.md`'s falsification rule forbids writing.

The chapter-6 pass invented this exception in passing, in the shared "How to read this" section that
governs every chapter, with no ADR and no issue — and justified it on a claim that is simply false:
that the schema is *"external to this repository"*. It is kept **inside** it, at
`crates/sparkplug-b/proto/sparkplug_b.proto`, and is editable by the same hands as the code the rule
refuses to trust. The code review of Story 4.2 caught both the missing ADR and the false premise.

## Decision

**The pinned `sparkplug_b.proto` schema is an admissible witness, and the property that makes it
one is compile-time unrepresentability — not the file's location.**

A row may cite the schema as its proof when, and only when, the generated type makes the clause
impossible to violate. The failure mode is then a **build error**, not a red test, which is a
stronger guarantee than a test provides: a test can be deleted, skipped or made vacuous; a type
cannot be quietly not-enforced.

**The witness is bounded to field types, and the boundary is the point.**

- It discharges clauses about the **type** of a field: *"MUST be an unsigned 32-bit integer"*.
- It can **never** discharge a clause about a **value**.
  `tck-id-payloads-propertyset-quality-value-value` names the literals `0`, `192` and `500`; no
  schema constrains which of them we send, and the bridge in fact sends none of them (ADR 0012). A
  schema witness applied to a value clause would manufacture exactly the false assurance this matrix
  exists to prevent.
- It can never discharge a guarantee that comes from **our own code shape** — a loop that happens to
  push in pairs, a field we happen to always set, a constant that happens to equal the number the
  clause names. Those regress silently, and the verdict stays `gap (unproven)` until a test says
  otherwise.

Rows relying on this witness say so explicitly, in the Proof column.

## Consequences

- **Two rows in chapter 6 rest on it**: `payloads-metric-datatype-value-type` (which also names a
  test) and `payloads-metric-propertyvalue-type-type` (which does not, and is the only `conformant`
  row in the chapter naming no test). Both are type clauses.
- **The rule change is retroactive across chapters**, since "How to read this" governs all of them.
  No chapter-4 verdict moves as a result: none of its rows cited a schema.
- **The pinned schema becomes load-bearing.** Editing
  `crates/sparkplug-b/proto/sparkplug_b.proto` away from the v3.0.0 release would silently weaken
  two conformance claims. It is pinned to the release tag and should be treated as read-only; a
  version bump invalidates the matrix anyway, which the document already states at its head.
- **The narrowness is what keeps this honest.** A wider version of this rule — "the compiler proves
  it" — would swallow half the matrix and turn correct-by-construction into conformant, which is the
  downgrade the chapter-6 pass performed eight times on purpose.

## What this ADR is really correcting

Not the exception, which is sound, but the way it arrived: a standing rule amended mid-audit, inside
a document, on a stated reason that did not survive being checked. The rule was right and its
justification was wrong, which is the harder failure to notice — a reader agreeing with the
conclusion never audits the premise.
