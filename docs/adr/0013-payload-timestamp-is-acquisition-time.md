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

## Amendment, 2026-08-29 — the revisit condition has an instrument

The clause above said *"if a future host is **shown**"* and nothing could do the showing, which is
the shape `CLAUDE.md` forbids: a decision deferred to an artifact that does not exist. The artifact
now exists.

`sparkplug-b/tests/ignition_contract.rs::ddata_shape_probe` publishes one DDATA carrying two tags
with the same value, one stamped with the payload instant and one **37 minutes earlier** — an offset
no time zone can imitate, so a host displaying local time cannot manufacture a false answer. The
operator compares the two tags against each other.

Two outcomes, both decisive, and neither is a reading of a table:

- **they display 37 minutes apart** — Ignition reads the metric timestamp, this ADR's premise fails,
  and the conformant shape costs nothing. Revisit it: `now` at payload level, `ValueDate` at metric
  level, two MUSTs satisfied and the anti-replay invariant intact.
- **they display identically** — Ignition reads the payload timestamp. The deviation is
  **load-bearing** rather than merely deliberate, and this ADR is re-affirmed on measured ground
  instead of on a prediction.

### The probe answers a second question, which had been folded into the first

Historisation has run on this deployment since commissioning, 2026-08-28 15:02. From that instant a
change to what the wire carries can cost a history rewrite — and the usual test for that (does an
identity, a topic level, a metric name or a unit move?) **does not catch this one**. Option 2 renames
nothing, and yet:

- if the host reads the **metric** timestamp, each point is already filed under its `ValueDate` and
  option 2 changes nothing that is stored;
- if the host reads the **payload** timestamp, option 2 moves every future point from its
  `ValueDate` to its publish instant — a discontinuity in a running series, produced without
  renaming anything.

So one probe answers both halves at once: whether the conformant shape is **honest**, and whether it
is **affordable**. Those two had been treated as one question and they are not.

## The probe was run on 2026-08-29, and this ADR's premise does not hold

**Ignition 8.3.7 / MQTT Engine 5.0.0-rc1 reads metric timestamps and acts on them.** The measurement
was not the one the probe expected to make: `ts_stamped_37_min_back` **did not move at all**. It kept
its birth value and its birth timestamp, while its neighbour — same payload, same message — went to
22.0. Engine read the metric timestamp, found it older than the value it held, and refused it. Had it
trusted the payload timestamp, which said *now*, it would have accepted.

That is the third of the three outcomes the probe admits, and it is the strongest form of *"the host
reads metric timestamps"*: not merely displaying one, but ordering by it.

**So the reason this ADR gives for refusing option 2 is falsified for this consumer.** The refusal
was *"it relies on every consumer reading metric-level timestamps, which is the assumption contract
v1 disproved"*. Contract v1 disproved it for the QUALITY property; it is now measured true for the
TIMESTAMP, on the host this bridge publishes to.

**And the second question came back safe.** The refusal raised one nobody had asked: this bridge's
staleness republication carries an **equal** metric timestamp, not a later one — it re-sends the last
known value under the acquisition instant it already sent. If Engine applied a metric only when its
timestamp advanced, a degradation would never reach the screen.
`staleness_republication_probe`, written during the session, measured that **an equal timestamp is
applied**: the value moved and the quality moved. Engine refuses what goes backwards, not what
repeats.

### What this ADR now owes, and what it does not

**It is not superseded here.** Adopting option 2 — `now` at payload level, `ValueDate` at metric
level — is a wire change on a bridge whose history is running, and it deserves its own decision, its
own issue and its own attestation rather than being taken as a consequence of a measurement. Two
things must be settled first:

- **whether the metric timestamp is what Engine STORES**, and not only what it compares by. The
  refusal does not separate the two. The discriminator is one publish: a metric stamped LATER than
  its payload. If the tag displays that later instant, the question is closed.
- **what the change costs a running series.** If Engine files points by the metric timestamp, option
  2 stores nothing differently and is free. If it files by the payload timestamp, option 2 moves
  every future point from its `ValueDate` to its publish instant — a discontinuity produced without
  renaming anything.

Until that decision is taken, **this ADR stands and the two matrix rows stay `deviation`**. What has
changed is that they are now deviations from a position whose premise has been measured false, which
is a different thing from deviations nobody has re-examined. [#29](https://github.com/guycorbaz/smartme_mqtt/issues/29)
carries the run, and it is the last thing that issue owes: the manual states the deviation and what
it deviates from (chapter 5), which was the other half.
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
