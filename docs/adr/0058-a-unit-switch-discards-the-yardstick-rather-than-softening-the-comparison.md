# ADR 0058 — A unit switch discards the yardstick rather than softening the comparison

- **Status:** accepted
- **Date:** 2026-08-30
- **Decides:** what the energy-monotonicity oracle does when the source changes the unit it reports.
- **Issue:** [#78](https://github.com/guycorbaz/smartme_mqtt/issues/78)
- **Related:** story 2.2 (the oracle), ADR 0055 (the same shape for the feed yardstick), FR15.

## Context

`energy_reference` is kept in kWh and so is every reading, but each reached kWh through whatever
conversion its own unit required: `value`, `value / 1000`, or `value * 1000`. So the same physical
index reported once in `Wh` and once in `kWh` arrives by **two different float paths** and can differ
by an ULP either way.

A downward ULP nulls a perfectly good reading with `counter-went-backwards` — a fault reported about
a meter that did nothing, which is a lie of the shape this bridge exists to refuse, arriving from
the guard that exists to refuse lies.

**Nothing produces it today**: smart-me has sent `kW`/`kWh` on every captured response
(`crates/smart-me-client/fixtures/`). [#78] filed it as an unhandled input rather than a live defect,
and rated it one nulled tick per switch, self-clearing.

## Decision — the yardstick is discarded, the comparison is untouched

When the unit differs from the one the source last reported, `energy_reference` is set to `None`
before the judgements run. The reading is then the first one for that oracle, and the next tick is
judged against it.

**The strict `<` is not softened, and that is the point.** [#78] says so explicitly and it is right:
a tolerance band weakens the guard for *every* reading in order to survive an input that is not a
measurement problem at all. The unexamined input is the **unit switch itself** — a comparison made
across two conversion paths without noticing they were different — and that is what is now examined.

**The unit travels on `Reading`, not on `Measurement`.** Everything in a `Measurement` is canonical,
deliberately: a consumer must never have to ask what scale it is in. The unit is not part of the
measurement — it is a fact about the response, kept because one oracle compares two readings and
cannot do so across a conversion change.

**It is the same shape as ADR 0055 and the opposite direction.** There, the feed's yardstick was made
to move forward only, because a rewindable memory could be walked backwards by the thing it judged.
Here, a yardstick that cannot be compared is dropped rather than trusted. Both say: *a comparison is
only worth what the two sides have in common.*

## The cost, stated rather than discovered

**A genuine backwards step that coincides with a unit switch goes unjudged on that tick.** The guard
asserts the mechanism at exactly that discomfort — its second reading is strictly lower and is still
expected to pass — so nobody has to infer the cost from the implementation.

It is the trade a restart already makes: `load_persisted_memory` answers `None` for a meter with no
stored reference, and the module calls that *"unjudged for one reading, then judged for ever after"*.
A unit switch across a restart lands in exactly the state a restart produces anyway.

The event is logged at `WARN` with both units, so it is never silent.

## Consequences

- **No new cause, and therefore no contract bump.** Adding one to the vocabulary is what moved
  `CONTRACT_VERSION` to 5, and it would now owe a Tier-3 attestation as well (ADR 0053's widened H7).
  Nothing on the wire changes: a reading that would have been nulled is published as what it is.
- **`Reading` gains a field**, and eight test fixtures with it. The production construction site is
  one.
- **The memory is not persisted**, unlike the energy reference beside it — and it does not need to
  be: a restart already discards the comparison for one reading, so a unit switch across a restart
  produces the state that restart produces anyway.
- **The manual says what an operator would otherwise have to deduce from a log line**: one unjudged
  reading, and the backwards step it cannot catch.
