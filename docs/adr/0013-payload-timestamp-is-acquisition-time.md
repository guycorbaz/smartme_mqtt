# ADR 0013 — The DDATA payload timestamp carries acquisition time, not publish time

- **Status:** Accepted
- **Date:** 2026-07-28
- **Related:** `tck-id-payloads-ddata-timestamp`, `tck-id-payloads-dbirth-timestamp`, [#29](https://github.com/guycorbaz/smartme_mqtt/issues/29), ADR 0012, ADR 0004
- **Records a position already implemented.** The behaviour predates this ADR; the conformance audit
  of chapter 6 (Story 4.2) found the clauses it contradicts, and its code review found the ADR was
  owed and had been deferred to "whoever picks up #29".

## Context

Sparkplug B v3.0.0 says what a payload timestamp means, twice, and both are `MUST`:

> `tck-id-payloads-ddata-timestamp` — DDATA messages MUST include a payload timestamp that denotes
> the time at which the message was published.

`tck-id-payloads-dbirth-timestamp` says the same for DBIRTH.

The bridge does not do this. `sparkplug_publisher.rs:315` stamps the DDATA payload with the
reading's own `ValueDate` — **when the values were true**, not when the message left. A rebirth that
re-declares an already-known reading is stamped the same way (`:277-280`); a cold-start DBIRTH,
having no reading to declare, is stamped `now` and conforms.

The specification's own answer to "where does acquisition time go" is the **metric** timestamp
(`Sparkplug_6_Payloads.adoc:481`), which the bridge also sets. So the conformant shape was available
and was not taken. That is what makes this a decision rather than an oversight.

## Decision

**The payload timestamp on DDATA, and on a re-declaring DBIRTH, stays the reading's `ValueDate`.**

The reason is the invariant this whole project exists to hold: *a stale reading must read as old
even to a consumer that ignores the quality flag.*

That consumer is not hypothetical. Contract v1 shipped quality codes a real Ignition displayed as
`Good(500)`, and every internal test agreed with itself while a live host showed stale data as
trustworthy (ADR 0012). A consumer that reads the payload timestamp and ignores the `Quality`
property is exactly the consumer that failure produced. Stamping `now` would hand that reader a
45-minute-old value wearing a fresh timestamp — the precise silent lie the bridge is built to
prevent, and it would be *conformant*.

Two safeguards make the deviation legible rather than hidden:

- Tests assert the deviating behaviour **by name**, so it cannot drift back accidentally:
  `a_good_reading_carries_units_serial_and_the_source_timestamp`,
  `a_stale_verdict_never_publishes_a_fresh_looking_metric`,
  `a_rebirth_redeclares_what_is_known_instead_of_blanking_it`.
- `docs/sparkplug-conformance.md` carries both clauses as `deviation` rows pointing here.

## Consequences

- **Two recorded MUST violations.** They are deviations, not gaps: we know, we chose, and the choice
  is written down. A conformance claim for the bridge must exclude these two clauses.
- **The generic `sparkplug-b` crate is unaffected.** It takes the timestamp it is given; the choice
  lives entirely in the bridge, as with the quality codes. A crates.io conformance claim for the
  crate remains defensible.
- **A Host Application pairing on payload timestamps will see out-of-order DDATA** if the smart-me
  cloud ever returns readings out of order. It does not today, and the bridge's sequence numbers are
  the ordering mechanism Sparkplug actually specifies for this — `seq`, not the timestamp.
- **The conformant shape stays open.** Moving the acquisition time to the metric timestamp alone and
  stamping the payload `now` would satisfy both clauses. It is rejected here because it relies on
  every consumer reading metric-level timestamps, which is the assumption contract v1 disproved —
  not because it is impossible. If a future host is shown to read metric timestamps correctly, this
  ADR should be revisited rather than worked around.
- **[#29](https://github.com/guycorbaz/smartme_mqtt/issues/29) is no longer the decision.** It is the
  work: making the deviation explicit in the operator manual, and re-testing it against a real host.

## Why this was written late, and what that cost

The chapter-6 audit found the clauses and recorded the deviation, but wrote *"it likely needs an
ADR"* and left it there. `CLAUDE.md` forbids exactly that shape — a decision deferred to an artifact
that does not exist — because AR13 deferred the shutdown mechanism to a chaos test that did not
exist for the whole of Epic 1, and the decision simply sat unmade.

Nothing was undecided here: the code has behaved this way since the publisher was introduced
(`95fe73c`, issues #11/#12). What was missing was the
record. The cost of "likely" is that a reader of the matrix could not tell a considered position
from an unexamined one — which is the same failure as a `conformant` row with no test.
