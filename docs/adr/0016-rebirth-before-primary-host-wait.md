# ADR 0016 — Story 4.7 (NCMD Rebirth) is sequenced before Story 4.5 (Primary Host wait)

- **Status:** Accepted
- **Date:** 2026-07-29
- **Related:** [#37](https://github.com/guycorbaz/smartme_mqtt/issues/37), Story 4.4, Story 4.5, Story 4.7, `docs/primary-host-state-observation.md`
- **Supersedes nothing.** It fixes an execution order that Epic 4 left implicit in its numbering.

## Context

Epic 4 numbered the Primary Host work (4.5, deciding whether to implement STATE handling) ahead of
the Rebirth work (4.7, `Node Control/Rebirth`). The numbering carried no argument — it followed the
order in which the gaps were discovered, which was the order the conformance audit walked the
specification.

Story 4.4 then measured the mechanism against this deployment, and the measurement produced a reason
to reverse the two.

**The specification's stated motivation for PHID-wait does not apply here.** The norm justifies an
Edge Node waiting on a Primary Host by store-and-forward — the node should *"store data while the
Host Application is offline … then send all of its stored data"*
(`Sparkplug_5_Operational_Behavior.adoc:191-196`).

**The bridge has no store-and-forward**, and none is planned. `mqtt_driver.rs` drops on a full queue
and traces the drop; nothing buffers across a host outage. So PHID-wait implemented *alone* would
preserve **not one measurement**. It would convert silent publication into deliberate
non-publication — an honest failure instead of a dishonest one, which has value, but not the value
the specification claims for it.

What actually repairs a consumer that has lost its tag definitions is a **Rebirth**. Story 4.7
delivers that; Story 4.5 only creates the condition under which a clean re-birth could be triggered.

## The decision

**Story 4.7 (`Node Control/Rebirth`, including the NCMD subscription plumbing of Story 4.6) is
scheduled before Story 4.5 (Primary Host / STATE).**

Story 4.5 is not cancelled, deferred indefinitely, or reduced in scope. It is sequenced second, and
when it is drafted it **must not justify PHID-wait on the specification's store-and-forward
grounds** — that justification is false for this deployment and saying so is half of what Story 4.4
bought.

## Why this needed an ADR rather than a line in a story

Story 4.4's own scope says *"It measures. It does not decide."* It wrote the re-ordering into
`sprint-status.yaml` anyway, in capitals, with no ADR and no issue. The Story 4.4 adversarial review
found it, and `CLAUDE.md` is explicit: *"Anything that changes a requirement, the wire contract, or
an architectural position gets an ADR in `docs/adr/` and a GitHub issue."* An execution order that
determines which protocol mechanism exists first is an architectural position.

This is also the third time in this project that a number in `epics.md` has been treated as a
decision because nobody wrote the decision down. The pattern is worth naming rather than repeating.

## What is measured, and what is not

Recorded here because the decision rests on a mixture and the mixture should be visible.

**Measured** (`docs/primary-host-state-observation.md`, and greps over `crates/smartme-bridge/src/`):

- The bridge holds **no MQTT subscription of any kind** — one `tracing_subscriber` initialiser and
  two comments are the only hits. **Superseded 2026-07-29 by Story 4.6**, which added a QoS 1
  subscription to `spBv1.0/{group}/NCMD/{node}`, issued before every birth. **Superseded again
  2026-07-30 by Story 4.7**, which answers `Node Control/Rebirth` with a complete birth sequence.
  The bridge still holds no STATE subscription. See *What Story 4.6 changed* and *What Story 4.7
  changed* below, both written rather than the line simply corrected, because this line is a premise
  of the decision.
- Its NBIRTH is published at **QoS 0 with `retain=false`**, so the broker keeps no copy.
- It has **no store-and-forward**; a full outbound queue is a traced drop.
- MQTT Engine v5.0.0-rc1 publishes a **fully conformant** `spBv1.0/STATE/SCADA` birth — checked
  against `-connect-birth-topic`, `-connect-birth-payload`, `-connect-birth-retained` and
  `-connect-birth-qos`.

**Inferred, and labelled as an inference in the record:**

- That Ignition's view of the edge node does not survive its own restart. No bridge tag state was
  checked before or after. It is cheaply falsifiable at the next unrelated Ignition restart, and the
  step is queued for `docs/ignition-contract-runbook.md`.

**A measured fact that cuts against the urgency, and is not hidden:**

- The bridge **does** re-birth on every MQTT reconnect of its own — `pump_transport` emits
  `Transport::Connected` on every `ConnAck` and the driver publishes a full BIRTH on it. A broker
  restart, a network interruption or a keep-alive expiry therefore already repairs the consumer's
  view. What no event on the *host* side can do is prompt that reconnect. The loss this ADR responds
  to is real, but it is "until the bridge next reconnects", not "until the bridge is restarted".

## What Story 4.6 changed — the decision stands, one premise does not

Added 2026-07-29, when Story 4.6 landed. The decision here is an *order*: 4.6 and 4.7 before 4.5.
Story 4.6 is now done, so the question is whether what it did undermines the case for 4.7 preceding
4.5. It does not — it sharpens it, and the sharpening is the part worth recording.

**The premise that has gone.** This ADR argued in part that the bridge could not even *receive* a
Rebirth request, so a Primary-Host wait implemented first would have sat on top of a node with no
repair path at all. That is no longer true: a request published by the live MQTT Engine on this
broker now reaches the driver, which decodes it, traces the metric name and drops it.

**Why the conclusion is unaffected.** The argument never rested on the receiving alone. It rested on
there being no *host-initiated* route back to a correct tag tree, and that needs both halves —
receive and answer. Story 4.6 built the first and deliberately not the second. A Rebirth that arrives
and is ignored repairs exactly as much as one that never arrives: nothing.

> **⏹ The sentence immediately above is SUPERSEDED as of Story 4.7 (2026-07-30).** The bridge now
> answers. It is left in place because it is the reasoning that produced this decision, not because
> it still describes the system — see *What Story 4.7 changed* below before citing it.

**Why the case for 4.7 is now stronger.** Before 4.6, "the bridge cannot be reborn by its host" named
an absent mechanism — two pieces of work, easy to defer as a unit. After 4.6 it names a **single
missing handler** sitting behind a subscription that is already live in production-shaped conditions.
The work is smaller than it has ever been and the omission is more conspicuous than it has ever been.
Nothing about that argues for taking 4.5 first.

**And one hazard 4.6 created that 4.7 inherits.** The subscription is open on a broker where a real
Host Application sends Rebirth requests. Until 4.7 lands, every one of them is answered with a log
line. That is safe — Story 4.6 exists to make the ignoring safe rather than silent — but it is a
state with a cost that grows the longer it lasts, which is another reason not to re-order 4.5 ahead.

## What Story 4.7 changed — and why this ADR's argument is now SPENT

Added 2026-07-30, when Story 4.7 landed. This section is not a correction of wording. The decision
recorded above was an *order*, and the reason for that order has now been consumed by carrying it
out. Saying so is the point of writing it down.

**The load-bearing premise is gone.** *"A Rebirth that arrives and is ignored repairs exactly as much
as one that never arrives: nothing."* That sentence was true when written and is now false. A Rebirth
request published on this broker reaches the driver, is recognised by name and boolean value, and is
answered with a complete NBIRTH + DBIRTH sequence under an unchanged `bdSeq`. Seven conformance rows
moved from `gap (unimplemented)` to `conformant`.

**So there is now a host-initiated route back to a correct tag tree**, which is precisely the thing
this ADR said did not exist and used as its reason for sequencing 4.7 first. The reason has been
spent by being acted on. That is the normal life of a sequencing argument, and it is worth being
explicit about because a spent argument left in place reads exactly like a live one.

**Story 4.5 must therefore be re-weighed, not inherited.** The instruction to 4.5 above — that it
justify itself rather than lean on this ADR — was written for this moment. What 4.5 now has to
answer is a narrower and harder question than the one it would have faced before 4.7:

- The repair exists but is **host-initiated**. It works if and only if the host asks. Ignition's MQTT
  Engine does ask on its own when it receives data from a node whose birth it never saw, which is the
  main case — but that is a fact about one host, and it is *reasoned*, not measured (see the
  hypothesis below).
- A Primary-Host wait would give a **node-initiated** path instead. The Story 4.4 measurement already
  found that PHID-wait without store-and-forward preserves no measurement, which argues against it on
  its own merits and is untouched by anything 4.7 did.
- So the case for 4.5 is not "there is no repair path" — that is settled — but "is a host-initiated
  path sufficient, given a host that may not ask?" **Neither this ADR nor Story 4.7 answers that.**

**A hypothesis for Story 4.8 to settle, recorded as reasoning and not as a finding.** MQTT Engine may
render a Rebirth control only for a node that *declared* the metric. Until Story 4.7 this bridge
declared none. If that is how it works, then the sentence above — *"a real Host Application sends
Rebirth requests \[and\] every one of them is answered with a log line"* — describes a flow that has
**never actually occurred**, and the hazard 4.6 created was theoretical. That would not change the
decision, which was right for other reasons, but it would change what the pre-production run is
expected to show. It is measurable in Story 4.8 and is not measured here.

## Consequences

- `epics.md` records the Epic 4 execution order with 4.6/4.7 ahead of 4.5, pointing here.
- `sprint-status.yaml` stops asserting the priority and points here instead.
- Story 4.5, when drafted, carries the instruction above about its justification.
- Story 4.7 carries the existing instruction from [#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)
  to re-examine RBE once Rebirth lands, since the periodic publish currently substitutes for it.
  **Discharged 2026-07-30, without implementing RBE**: the blocker has lifted and the deviation's
  verdict is unchanged, but its *reason* moved from "cannot safely be changed" to "has not been
  decided". Recorded in the matrix under `principles-rbe-recommended`; the decision itself belongs
  to #32's own story.
- **This ADR's own argument is spent as of Story 4.7** (see the section above). Story 4.5 is to be
  decided on its own evidence. Any future document citing this ADR for *"there is no host-initiated
  repair path"* is citing a premise that no longer holds.

## Alternatives considered

**Leave the order as numbered.** Rejected: it would put the bridge in a state where it deliberately
withholds data during a host outage and still cannot recover afterwards — strictly worse than today
for the same implementation cost.

**Do both in one story.** Rejected: the Rebirth path needed an NCMD subscription that did not exist at
the time of this decision (Story 4.6, which has since landed it), and Story 4.4's evidence bears on the
two differently. Merging them would hide which half the evidence supports. *(Tense corrected by the
Story 4.6 code review, 2026-07-29 — the rejection still stands on the evidence argument, which never
depended on the subscription being absent.)*

**Treat the re-ordering as a recommendation for Story 4.5 to accept or refuse.** This was the
alternative offered at review time and it is defensible — 4.5 is the deciding story. Rejected
because the ordering determines whether 4.5 is written at all before 4.7, so leaving it to 4.5 is
circular.
