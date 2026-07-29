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
  two comments are the only hits.
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

## Consequences

- `epics.md` records the Epic 4 execution order with 4.6/4.7 ahead of 4.5, pointing here.
- `sprint-status.yaml` stops asserting the priority and points here instead.
- Story 4.5, when drafted, carries the instruction above about its justification.
- Story 4.7 carries the existing instruction from [#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)
  to re-examine RBE once Rebirth lands, since the periodic publish currently substitutes for it.

## Alternatives considered

**Leave the order as numbered.** Rejected: it would put the bridge in a state where it deliberately
withholds data during a host outage and still cannot recover afterwards — strictly worse than today
for the same implementation cost.

**Do both in one story.** Rejected: the Rebirth path needs an NCMD subscription that does not exist
(Story 4.6), and Story 4.4's evidence bears on the two differently. Merging them would hide which
half the evidence supports.

**Treat the re-ordering as a recommendation for Story 4.5 to accept or refuse.** This was the
alternative offered at review time and it is defensible — 4.5 is the deciding story. Rejected
because the ordering determines whether 4.5 is written at all before 4.7, so leaving it to 4.5 is
circular.
