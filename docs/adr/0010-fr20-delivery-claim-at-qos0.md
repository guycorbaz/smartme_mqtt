# ADR 0010 — FR20 amended: no broker ACK exists at Sparkplug's mandatory QoS 0

- **Status:** Accepted
- **Date:** 2026-07-26
- **Related:** Story 1.12 (mqtt_driver), issue #19, FR20, FR22 (broker-outage policy)
- **Amends:** FR20's original wording ("can confirm a value is actually acknowledged by the
  broker before reporting it as delivered").

## Context

FR20 was written to prevent a specific dishonesty: the bridge telling the operator "published ✓"
for a value that never left the process. The requirement expressed that as *confirm the broker
acknowledged it*.

Story 1.12's code review established that this is not merely unimplemented but **unimplementable
as written**, for a protocol reason rather than an engineering one:

- The Sparkplug B specification requires **QoS 0** for every edge-node message
  (NBIRTH/NDATA/NDEATH/DBIRTH/DDATA/DDEATH). Only host STATE messages use QoS 1.
- **MQTT defines no acknowledgement for QoS 0.** There is no PubAck packet at that quality of
  service — not "we do not read it", but "it does not exist on the wire".

So the only ways to obtain an ACK are to publish edge-node data at QoS 1, which violates the
specification and risks a strict host rejecting the node, or to invent a confirmation, which is
the exact category of lie the requirement was written to forbid.

Three options were put to the maintainer (issue #19):

1. Re-scope FR20 to what is honestly observable at QoS 0.
2. Keep an ACK requirement only where QoS 1 is legal (host STATE — not applicable to this bridge).
3. Publish DATA at QoS 1 to obtain PubAcks, accepting the conformance break.

## Decision

**Option 1.** FR20 becomes:

> The bridge never over-claims delivery: a value is reported as published only once it has been
> accepted for transmission, and a value it could not hand over yields a per-device traced drop
> rather than silence.

This keeps the requirement's intent — no confident claim about a value that did not go out —
and states it in terms the transport can actually witness.

## Consequences

- **What the bridge now guarantees.** `try_publish` hands the message to the transport without
  blocking (a blocked driver stops pumping the event loop, after which *nothing* is sent). A
  refused hand-over — full queue, broker down — is traced per device, never swallowed. Nothing
  is ever reported as delivered on the strength of a fabricated acknowledgement.
- **What it does not guarantee, and where that is covered.** Between "accepted for transmission"
  and "the host has it" lies the QoS-0 gap: a message can be lost in the socket with no local
  signal. Two existing mechanisms bound the damage rather than hiding it. First, freshness is
  carried IN the payload — a consumer that misses a DDATA sees the next one stamped with its own
  `ValueDate`, so a gap reads as old data, not as current data. Second, the BIRTH/DEATH lifecycle
  and the sequence numbering let a consumer detect that it missed messages at all.
- **Delivery confirmation, if it is ever wanted**, has to come from the consumer side (a host
  that reports what it received) rather than from the broker — that is a different requirement,
  for a later epic, not a correction to this one.
- **The `Sink` seam has no failure channel** today (an unpublishable message is indistinguishable
  from a delivered one *at that layer* — the driver traces it one layer out). Recorded in
  `deferred-work.md`; it is the natural place to hang any future confirmation.

## Note on how this was found

The requirement survived PRD review, architecture review and epic breakdown; the conflict only
surfaced when an adversarial code review checked the AC against the protocol rather than against
the code. Worth remembering the class: a requirement can be perfectly clear, perfectly reviewed,
and still impossible — and the thing that catches it is asking "could this be true?" rather than
"does the code do it?".
