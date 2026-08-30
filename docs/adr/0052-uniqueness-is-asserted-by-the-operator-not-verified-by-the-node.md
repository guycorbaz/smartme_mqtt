# ADR 0052 — Identifier uniqueness is asserted by the operator, not verified by the node

- **Status:** accepted
- **Date:** 2026-08-28
- **Decides:** what this bridge does about the two Sparkplug uniqueness MUSTs it cannot check.
- **Issue:** [#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)

## Context

Two clauses require uniqueness and neither can be answered from inside an Edge Node:

> *"The group_id combined with the edge_node_id element MUST be unique from any other
> group_id/edge_node_id assigned in the MQTT infrastructure."*
> — `tck-id-topic-structure-namespace-unique-edge-node-descriptor`

> *"The device_id MUST be unique from other devices being reported on by the same Edge Node."*
> — `tck-id-topic-structure-namespace-unique-device-id`

The harm is concrete rather than theoretical. Two nodes sharing a descriptor on one broker
interleave their sessions: each `NBIRTH` invalidates the other's, `bdSeq` pairing becomes
meaningless, and a host sees one node flapping. On this deployment the broker is shared with other
publishers, so a second instance — a chaos run, a colleague's copy, a container left behind — is the
likely collision rather than an exotic one.

**[#27] proposed three options.** Document it; detect a collision at startup by subscribing to the
node's own birth topics before birthing; or refuse defaults so two instances cannot silently
collide.

## Decision

**The device-id clause is verified. The descriptor clause is not, and is documented instead.**

1. **`device_id` uniqueness is enforced** — `config::validate` already refuses duplicate meter ids,
   naming both rows. It is the clause an Edge Node *can* answer: it owns its own device list.
2. **Descriptor uniqueness is asserted by the operator**, and the manual says so where the two
   identifiers are configured.
3. **Option 3 is already in force, and more strongly than it was proposed.** `SMARTME_GROUP_ID` and
   `SMARTME_NODE_ID` no longer exist, and `group_id`/`node_id` have no default at all: a
   configuration that does not name them fails to load. The collision [#27] feared — a second
   instance defaulting into the production namespace — is unreachable as of this decision.
4. **Option 2 is REFUSED.** A pre-birth subscription would detect the common case and prove nothing:
   a node that is merely quiet at that instant reads as absent, and a node whose session opens a
   second later is missed entirely. It would buy a check that answers *"no collision seen in the
   last 500 ms"* while reading as *"the descriptor is unique"* — and this project's whole discipline
   is that a guard which cannot state what it proves is worse than an admitted gap. It also delays
   every start-up for a negative result, on the path that matters most: the one where the operator is
   waiting to see the bridge come up.

## Consequences

**The matrix keeps `gap (unimplemented)` for the descriptor clause**, now pointing here rather than
reading as an oversight. That is the honest verdict: we do not do what the clause requires, and we
have decided not to.

> **AMENDED 2026-08-30 — [ADR 0060](0060-a-declined-clause-is-a-gap-that-says-so.md),
> [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42).** The decision is unchanged; the
> verdict word is now **`gap (declined)`** on both rows this ADR touches
> (`intro-edge-node-id-uniqueness`, `topic-structure-namespace-unique-edge-node-descriptor`). The
> paragraph above is the argument for the label: *"we do not do what the clause requires, and we
> have decided not to"* is precisely what `declined` says, and `gap (unimplemented)` — a debt with
> an owner — was the nearest word available when this was written.

**What the operator gets instead is the one thing that helps** — the two identifiers have no
defaults, so nothing collides by accident, and the manual states the requirement where they are
typed.

**If a collision ever happens**, its signature is worth knowing in advance and is now in the
troubleshooting chapter: a node that appears to flap, births that arrive unasked, `bdSeq` pairs that
do not match. Nothing in this bridge will name it, because nothing in this bridge can see it.
