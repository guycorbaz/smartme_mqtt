# ADR 0032 — At equal severity, a latching cause outranks a degrading one

- **Status:** accepted
- **Date:** 2026-08-11
- **Amends:** [ADR 0029](0029-the-declared-serial-is-checked-against-the-one-smart-me-reports.md) by
  giving its latch rule a mechanism. **Supersedes:** nothing.
- **Decided by:** Guy, 2026-08-11, on the review of stories 2.1 and 2.2.
- **Issue:** carried by story 2.3.

## Context

Story 2.1's `compose` resolved ties by keeping the first verdict of a given severity, and said so
in its own documentation:

> Ties keep the FIRST verdict of that severity … but which of two equally severe causes is reported
> is arbitrary by construction, and **no caller may rely on it**.

**A caller relied on it.** `poll_publish` called `compose([freshness, monotonicity])`, so
*"freshness wins ties"* was the real behaviour of the bridge, decided by the order of two elements
in a literal. Reversing that literal — a one-token edit no test forbade — changed what an operator
was told.

### The tie is reachable, and it was reachable that day

A response whose unit could not be converted yields `bad(ValueUnusable)` from the freshness path,
while the monotonicity oracle, handed the same reading's energy field, independently yields
`bad(CounterWentBackwards)`. Equal severity, two different diagnoses, and the operator sees exactly
one of them with no indication that the other applied. The two send them to different places: an
API contract change versus a meter that was reset.

### And the latch rule had no mechanism at all

ADR 0029 established *identity latches, value degrades*. Story 2.1 wrote it into `Cause::latches`
and `Verdict::latches` — and **nothing in production ever called either**. `Policy::step` computed
`State::Failed` from its own guards, so the rule lived in two places and the one that documented it
was inert. Tests asserting `Cause::SourceRefused.latches()` restated `matches!(SourceRefused,
SourceRefused)` in a second file.

## Decision

**Composition is a total order over verdicts, in three tiers. Bigger wins; any permutation of the
same set yields the same verdict.**

| tier | rule | why |
| --- | --- | --- |
| 1 | worse quality wins, `Good < Stale < Bad` | unchanged since story 2.1 |
| 2 | at equal quality, a **latching** cause outranks a degrading one | the two mean different things about the future |
| 3 | at equal quality and equal latching, the cause **earlier in `Cause::ALL`** wins | a stated tie-break, so the order is total |

### Tier 2 is the decision; the others are its scaffolding

A latching cause says *this meter is not the meter you asked for* — it survives the reading and no
later reading repairs it. A degrading cause describes a number that has already passed. Reporting
the degrading one at equal severity sends an operator to inspect a value when the configuration is
wrong.

**This gives `Verdict::latches()` its first production caller.** `State::Failed` now follows the
composed verdict instead of being recomputed by `Policy::step` from its own guards. The rule is
read where it is written.

### Tier 3 is a tie-break and is not offered as a principle

It exists so the order is *total*, which is what makes composition independent of argument order.
`Cause::ALL` runs roughly from transport outwards to value, so the earlier cause is the one closer
to the source — defensible, and no more than that. What matters is that it is stable and stated
rather than being whatever the call site's array order happened to be.

## Consequences

### Order-independence becomes testable, and is tested

`compose` over every permutation of a set containing a tie must return one verdict. That test
exists and was falsified against the pre-2.3 comparison.

### One published cause can change

Where the two collided, the operator now sees the latching cause. That is a change in what the
wire says for a reading that is refused either way — the quality is identical, so no consumer's
trust decision moves, and `CONTRACT_VERSION` moves for
[ADR 0031](0031-a-verdict-belongs-to-a-metric.md) in the same story regardless.

### A new cause must be classified

`Cause::latches` is a `matches!`, so an unlisted cause defaults to non-latching — silently. The
test that walks `Cause::ALL` and asserts every non-`SourceRefused` cause does not latch is what
turns that default into a question somebody has to answer. **A new latching cause belongs beside
`SourceRefused` and in this ADR's list, or it does not latch.**

### It does not make composition commutative in the arithmetic sense

Two *identical* causes compose to themselves, and the order of equal elements cannot matter. What
is guaranteed is that the RESULT is a function of the input set. Nothing here promises that
composing incrementally in a different grouping is meaningful — `compose` takes the whole set.

## What would reopen this

- **A second latching cause whose collision with `SourceRefused` matters.** Today the latching set
  has one member, so tier 2 never has to break a tie *within* itself. A second one makes tier 3 do
  that work, on an ordering chosen for a different purpose. That is the moment to give latching
  causes their own explicit order.
- **A need to publish more than one cause.** The layer computes several and reports one. If an
  operator surface ever needs the full set, the property is the wire-level obstacle, not this
  ordering — and it would be a contract change with its own measurement on Ignition.
