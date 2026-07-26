# ADR 0012 — Quality codes: the specification and the host disagree, and we side with the host

- **Status:** Accepted
- **Date:** 2026-07-26
- **Related:** Story 4.2 (conformance audit, chapter 6), `tck-id-payloads-propertyset-quality-value-value`, contract v2, ADR 0011
- **Supersedes the reasoning of:** the contract-v1 → v2 change, which was made without knowing the specification had an opinion.

## Context

Sparkplug B v3.0.0 is explicit about the quality property's value
(`tck-id-payloads-propertyset-quality-value-value`):

> The 'value' of the Property Value MUST be an int_value and be one of the valid quality codes of
> **0, 192, or 500**.

Ignition does not classify the property by that enumeration. It reads the raw integer as its own
`QualityCode`, where the quality *level* is carried in the top bits. Measured on Ignition 8.3.7
with `quality_code_probe`:

| Published | Ignition displays |
| --- | --- |
| `192` | `Good` |
| `500` | **`Good(500)`** — good level, unrecognised subcode |
| `0` | **`Good_Unspecified`** |

So of the three codes the specification mandates, **two report an unusable value as
trustworthy** on the host this bridge actually publishes to.

This was discovered backwards. Contract v1 published `0`/`192`/`500`; a Tier-3 run showed
Ignition rendering stale data as good; we changed to Ignition's encoding in contract v2 and
recorded it as *fixing our mistake*. The conformance audit then found that v1's codes had come
from the specification, and that v2 violates a `MUST`. The trade-off had been made without
anyone knowing there was one.

## Decision

**The bridge keeps Ignition's encoding. The `sparkplug-b` crate returns to the specification's.**

- `sparkplug_b::Quality::code()` yields `192` / `500` / `0`, as the specification requires, and
  its documentation states plainly that some hosts do not honour them.
- A new `Metric::with_quality_code(u32)` publishes a raw, host-specific code. Deviating is
  possible only by calling it — at the call site, where it is visible.
- `smartme_bridge::adapters::sparkplug_publisher::ignition_quality_code` holds the deviation,
  in the crate that is allowed to name a vendor.

The wire is unchanged: the bridge publishes the same bytes as before this ADR, so
`CONTRACT_VERSION` stays at 2.

### Why not conform

The project has one rule: never show the SCADA a false value dressed as true. Publishing `500`
for a stale reading is conformant *and* causes Ignition to display that reading as good.
Conformance that produces a silent lie on the only consumer we have defeats the purpose of
conforming. Between the two, we do not lie.

### Why the crate must not follow the bridge

`sparkplug-b` is generic and intended for crates.io, and its purity guard forbids the token
`ignition` in its source. Before this ADR the crate satisfied that guard to the letter while
returning Ignition-specific integers from a method documented as *the* quality code — a
vendor-specific choice hidden inside a library claiming to implement a specification. That is
worse than either alternative: a future consumer of the crate would have inherited our
deployment's deviation without being told.

## Consequences

- **A known, recorded MUST violation.** `tck-id-payloads-propertyset-quality-value-value` is a
  `deviation` row in `docs/sparkplug-conformance.md`, pointing here. It is not a gap and not an
  oversight; it is a decision with a reason.
- **A conformance claim for the crate is now defensible**, and one for the bridge is not, at
  least on this clause. That distinction matters for the crates.io publish, where the PRD
  promises a "public conformance guarantee".
- **The regression guard moved with the deviation.** The property that matters — no non-good
  quality may land on the host's *good* level — is asserted in the bridge, beside
  `ignition_quality_code`. A companion test asserts the crate still returns `192`/`500`/`0`, so
  the two cannot silently converge again.
- **This deviation is host-specific and would be wrong for another consumer.** Anyone pointing
  this bridge at a Sparkplug host that honours the specified codes must revisit it. That is a
  configuration concern the bridge does not yet have; if a second consumer ever appears, the
  mapping becomes a setting rather than a constant.
- **Worth raising upstream.** A specification whose mandated quality codes are read as *good* by
  a major host implementation has a problem that is not ours to fix, but is ours to report.

## What this changes about the Epic 1 retrospective

The retrospective recorded the quality-code defect as *our* error — codes "taken from an
OPC-style triple Ignition does not use". That was wrong in an interesting way: the codes came
from the specification. The real finding is narrower and more uncomfortable — **the
specification and the dominant host disagree, and nothing in our process would have surfaced
that**, because we had never read the specification and the host cannot tell us what it expects.

The lesson holds, but its shape changes: an external oracle told us the host's truth, and only
reading the norm told us the specification's. Neither alone was enough.
