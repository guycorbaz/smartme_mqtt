# ADR 0053 — The datatype leaves the DATA messages, and the contract version does not move

- **Status:** accepted — **attested against a live host on 2026-08-29** (Ignition 8.3.7 Maker /
  MQTT Engine 5.0.0-rc1, six steps on six plus `ddata_shape_probe`; see the runbook's run record)
- **Date:** 2026-08-29
- **Decides:** whether the bridge keeps repeating each metric's `datatype` on every DDATA, and
  whether stopping is a contract change a consumer must be told about.
- **Issue:** [#28](https://github.com/guycorbaz/smartme_mqtt/issues/28)
- **Related:** ADR 0012 (the other deliberate payload deviation), ADR 0042 and Story 5.2 (the two
  precedents for a wire change that does NOT bump the contract version).

## Context

Sparkplug B says opposite things about the same field, and names the message types on each side:

> *"The datatype MUST be included with each metric definition in NBIRTH and DBIRTH messages."*
> — `tck-id-payloads-metric-datatype-req`, `Sparkplug_6_Payloads.adoc:489`

> *"The datatype SHOULD NOT be included with metric definitions in NDATA, NCMD, DDATA, and DCMD
> messages."* — `tck-id-payloads-metric-datatype-not-req`, `:491`

Until this decision `encode_metric` set the field **unconditionally**. One encoder served every
message type, so a single line satisfied the MUST and violated the SHOULD NOT — on the bridge's
highest-volume message, the one that repeats for as long as the bridge runs.

The matrix carried it as a `deviation` pending this decision, which is what [#28] was opened to
force.

## What reading the specification changed about the decision

**The specification contradicts itself, and only reading it shows that.** The chapter that states
the SHOULD NOT prints a DDATA example three hundred lines later whose two metrics both carry
`"dataType": "Boolean"` (`:1391`, `:1396`). The clause is normative and the example is not, so the
clause wins — but a Host Application written against the example is not a strange thing to imagine,
because the example is what an implementer reads first.

That is the whole reason this repair is not simply "delete a line": the risk is not that we are
wrong about the norm, it is that **the consumer may be built on the norm's own counter-example**.

**And the death payloads are outside the clause.** The SHOULD NOT enumerates four message types;
NDEATH and DDEATH are in neither clause, and the specification's own NDEATH payload example carries
`"dataType": "UInt64"` on its `bdSeq` metric (`:1564`). A metric a host reads to reconcile a death
with its birth is not the repetition the clause is aimed at, so it keeps its type.

## Decision

**The `datatype` travels in NBIRTH, DBIRTH, NDEATH and DDEATH, and in nothing else.**

`Datatype::Included` / `Datatype::Omitted` is passed to `encode_metric` by the builder, so the choice
is made once per message type rather than per call site. Five builders reach that function and
`the_datatype_travels_with_the_declaration_and_with_nothing_else` walks all five, in both directions.

**No public API of `sparkplug-b` changes.** The session builders were already message-typed
(`birth`, `rebirth`, `device_birth`, `data`, `device_data`); what was missing was that they used the
distinction they already had. [#28] anticipated a public-API question and there is none.

### The one edit this cannot survive, and the guard written for it

`device_birth` returned `self.data(..)`. That delegation was correct for a month — the two messages
differed only in the caller's topic — and becomes, the instant the encodings diverge, **a DBIRTH
stripped of the field a consumer learns the tag set from**. It is the ordinary shape of this fault:
not a wrong constant, but one builder quietly sharing another's body.

`device_birth` now has its own body, and the guard walks every builder rather than the two that were
repaired, because a guard on the repaired pair would have passed on the day the delegation was
written.

## The sharpest edge: a null metric in DDATA

The bridge publishes nulls in DDATA — a `Bad` verdict withholds the number rather than shipping one
it does not trust (ADR 0012). Such a metric now carries a name, `is_null`, its properties, and
**nothing else**: no value to infer a type from, and no declared type. A consumer that did not read
the DBIRTH cannot tell what kind of tag went null.

Sparkplug says that consumer must not exist — *a consumer may discard DATA for a metric the BIRTH
never declared* — and this decision takes the specification at its word. It is recorded here rather
than discovered later, and it is the case the Tier-3 session must look at first.

## The contract version does not move, and that is a ruling rather than an omission

`CONTRACT_VERSION` bumps on a change to the topic grammar, to a metric name or unit, or to the
meaning of a published quality code. This change is none of the three: **the tag set is untouched.**
No name, unit, quality code or cause moves; `contract_golden` is green without being edited, which
is the mechanical statement of the same fact.

The precedents are ADR 0042 (`bdSeq` starting at zero) and Story 5.2 (DDEATH entering the
repertoire): both changed the wire, neither moved the number, and both wrote down why. The property
the number protects is the runbook's — *two runs sharing a version number attest to the same tag
set* — and it still holds across this boundary.

**But the attestation is owed anyway, and that is the new part.** Action H7 of the epic-8
retrospective ties a Tier-3 attestation to a *bump*, and this change needs one more than most while
earning no bump. So the rule is widened here: **a change to what the wire carries earns an
attestation, whether or not the version moves.** The runbook records the run against the version it
was measured at, unchanged, with a note saying which change it was covering.

## Consequences

- **The manual had been describing the conformant behaviour all along, and nothing noticed.**
  Chapter 2, which explains Sparkplug rather than the bridge, has said *"required in births, and
  deliberately omitted from data messages, because the birth already established it"* since it was
  written. Chapter 5, which describes the bridge, said nothing about the field at all. So a reader
  of the manual would have concluded the bridge was conformant here, and would have been wrong —
  the two chapters could not contradict each other because one of them was silent. Chapter 5 now
  states it, which is what makes the pair checkable.
- **Two clauses move to `conformant`** in `docs/sparkplug-conformance.md`, and the deviation register
  loses its chapter-6 entry.
- **Smaller DDATA**, on every metric of every publication. Not the reason for the change, and not
  measured, because the reason is the clause.
- **The BIRTH becomes load-bearing in a way it was not.** A consumer that missed the DBIRTH used to
  be able to reconstruct types from any DDATA; now it must ask for a rebirth. The bridge answers
  `Node Control/Rebirth`, so the recovery path exists — and it is the second thing the Tier-3 session
  must exercise.
- **Int64 and UInt64 share `long_value`**, so disambiguating them is now the tag set's job and not
  each message's. Both live in one test, on one value, because separating them is how such a pair
  drifts apart.
- **No history is rewritten by this, and that is checkable rather than convenient.** Historisation
  has run on this deployment since commissioning, 2026-08-28 15:02, so from that instant a wire
  change can cost a history rewrite. The test for it is the same enumeration this ADR uses to refuse
  the version bump — identity, topic grammar, metric name, unit — and this change is in none of them.
- **This shipped only after a Tier-3 session, and the session answered both halves.** Steps 2 and 3
  of the bridge gate showed values arriving and changing on DDATA that declare no type;
  `ddata_shape_probe` showed a null metric with no declared type **rendering as null**, which is the
  case no gate step reaches. The norm's own counter-example did not reflect a requirement of this
  host. Recorded in the runbook under *What the 2026-08-29 session established*.
- **The probe found something this ADR did not ask about, and it was worth the evening.** Engine
  refuses a metric stamped BEHIND the value it holds — which raised, and then settled, whether it
  would also refuse an EQUAL one. It does not. That matters here only indirectly, but it matters:
  the bridge's staleness republication carries an equal timestamp, and had the answer gone the other
  way a degraded reading would never have reached the screen.
