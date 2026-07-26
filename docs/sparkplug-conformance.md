# Sparkplug B conformance matrix

**Specification version: v3.0.0**, vendored at `docs/spec/sparkplug-b-3.0.0/` (EPL-2.0).

A conformance claim is meaningless without a version. **A version change invalidates this matrix
rather than merely dating it** — the clause set moves, so the audit must be re-run, not patched.

## How to read this

One row per normative clause, keyed by its `tck-id`. Verdicts:

| Verdict | Meaning |
| --- | --- |
| **conformant** | We do what the clause requires, **and a named test proves it** |
| **deviation** | We knowingly do otherwise; the row carries the rationale and its ADR or deferred-work entry |
| **gap** | We do not do it, or nothing proves that we do; the row carries an issue number |
| **n/a** | The clause addresses a role we do not play, or a message we do not emit |

**A row claiming `conformant` with no test named is a `gap`, not a `conformant`.** A behaviour
nothing exercises is not a proven behaviour — that rule exists because contract v1 shipped
quality codes a real host read as `Good` while every internal test agreed with itself.

## Status

| Chapter | Story | State |
| --- | --- | --- |
| 4 — Topics & namespace | 4.1 | **done** (this pass) |
| 6 — Payloads, metrics, datatypes | 4.2 | pending |
| 2, 5 — Principles, session lifecycle, host interaction | 4.3 | pending |

Chapter 4 also carries the Host Application `STATE` clauses (`host-topic-phid-*`). They are
listed once, collectively, as **n/a**: this bridge is an Edge Node, not a Host Application. That
is a separate question from whether the bridge should *react* to a Host Application's STATE,
which is Stories 4.4–4.5 and is not settled by these clauses.

---

## Chapter 4 — Topic namespace and message topics

### Namespace structure

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `topic-structure` | MUST | Topics are built only through `EdgeNode::node_topic` / `device_topic`; never by concatenation | `topic.rs::node_topics_follow_the_namespace_grammar`, `device_topics_append_the_device_identifier` | conformant |
| `topic-structure-namespace-a` | MUST | `NAMESPACE = "spBv1.0"`, a constant | same tests (literal `spBv1.0/...` asserted) | conformant |
| `topic-structure-namespace-valid-group-id` | MUST | `check_identifier` rejects `+`, `/`, `#` and empty | `wildcards_and_separators_are_refused_in_every_element` | conformant |
| `topic-structure-namespace-valid-edge-node-id` | MUST | as above, validated in `EdgeNode::new` | same | conformant |
| `topic-structure-namespace-valid-device-id` | MUST | validated in `device_topic`, at the last moment before it becomes a level | same | conformant |
| `topic-structure-namespace-device-id-associated-message-types` | MUST | `is_device_level()` gates DBIRTH/DDATA/DDEATH onto device topics | `a_message_type_cannot_address_the_wrong_level` | conformant |
| `topic-structure-namespace-device-id-non-associated-message-types` | MUST NOT | the same gate refuses a node-level type on a device topic, **in release builds too** | same | conformant |
| `topic-structure-namespace-unique-edge-node-descriptor` | MUST | nothing verifies that `group_id/edge_node_id` is unique across the MQTT infrastructure | — | **gap** ([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)) |
| `topic-structure-namespace-unique-device-id` | MUST | one device today; uniqueness across a fleet is unenforced | — | **gap** (Epic 3) |
| `topic-structure-namespace-duplicate-device-id-across-edge-node` | MAY | permissive; nothing to do | — | n/a |

**On the two gaps.** Neither is a coding error. An Edge Node cannot verify infrastructure-wide
uniqueness alone — but "cannot be verified from inside" is exactly what a conformance matrix is
for, and silence would let it read as satisfied. At minimum the operator manual must state the
constraint; ideally the bridge refuses to start on a detectable collision. Issue #27.

### Edge-node messages

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `topics-nbirth-topic` | MUST | `spBv1.0/{group}/NBIRTH/{node}` | `node_topics_follow_the_namespace_grammar` | conformant |
| `topics-nbirth-mqtt` | MUST | QoS 0, retain false | `mqtt_driver.rs::every_edge_node_message_is_qos_zero_and_never_retained` | conformant |
| `topics-ndeath-topic` | MUST | `spBv1.0/{group}/NDEATH/{node}` | `node_topics_follow_the_namespace_grammar` | conformant |
| `topics-ndata-topic` | MUST | topic construction supports it; **NDATA is never emitted** — the bridge carries no node-level measurement | — | n/a |
| `topics-ndata-mqtt` | MUST | as above | — | n/a |
| `topics-ncmd-topic` | MUST | **not implemented** — no NCMD subscription exists | — | **gap** (Story 4.6, [#23](https://github.com/guycorbaz/smartme_mqtt/issues/23)) |
| `topics-ncmd-mqtt` | MUST | as above | — | **gap** (Story 4.6) |

> **`topics-ndata-mqtt` settles a question that was got wrong twice.** It reads *"NDATA messages
> MUST be published with MQTT QoS equal to 0 and retain equal to false"*, and `topics-ddata-mqtt`
> says the same for DDATA. So QoS 0 **is** mandated for data, no broker acknowledgement can
> exist, and ADR 0010's amendment of FR20 was correct. An earlier reading of chapter 5 alone
> found no data-QoS clause and wrongly concluded none existed — see #26.

### Device messages

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `topics-dbirth-topic` | MUST | `spBv1.0/{group}/DBIRTH/{node}/{device}` | `device_topics_append_the_device_identifier` | conformant |
| `topics-dbirth-mqtt` | MUST | QoS 0, retain false | `every_edge_node_message_is_qos_zero_and_never_retained` | conformant |
| `topics-ddata-topic` | MUST | `spBv1.0/{group}/DDATA/{node}/{device}` | `device_topics_append_the_device_identifier` | conformant |
| `topics-ddata-mqtt` | MUST | QoS 0, retain false | `every_edge_node_message_is_qos_zero_and_never_retained` | conformant |
| `topics-ddeath-topic` | MUST | topic construction supports it; **DDEATH is never emitted** | — | **gap** (Epic 3 — with one meter a device only stops when its node does) |
| `topics-ddeath-mqtt` | MUST | as above | — | **gap** (Epic 3) |
| `topics-dcmd-topic` | MUST | **not implemented** — no DCMD subscription | — | **gap** (Story 4.6) |
| `topics-dcmd-mqtt` | MUST | as above | — | **gap** (Story 4.6) |

### Host Application clauses

`host-topic-phid-birth-message`, `-birth-qos`, `-birth-retain`, `-birth-topic`,
`-birth-sub-required`, `-birth-required`, `-birth-payload`, `-birth-payload-timestamp`,
`-death-qos`, `-death-retain`, `-death-topic`, `-death-required`, `-death-payload`,
`-death-payload-connect`, `-death-payload-disconnect-clean`,
`-death-payload-disconnect-with-no-disconnect-packet`

**n/a — we are an Edge Node.** These govern what a Host Application publishes on
`spBv1.0/STATE/…`. Whether the bridge should *observe* a Host Application's STATE is a different
question, unaddressed by these clauses and open as Stories 4.4–4.5.

---

## Findings carried forward from this pass

| Finding | Where |
| --- | --- |
| Will registered at QoS 0; chapter 5 requires QoS 1 | [#26](https://github.com/guycorbaz/smartme_mqtt/issues/26), Story 4.17 |
| `every_edge_node_message_is_qos_zero_and_never_retained` asserts QoS 0 for all six types and claims the spec requires it uniformly — true for the six published types, false for the will | Story 4.17 |
| No verification of edge-node-descriptor or device-id uniqueness | [#27](https://github.com/guycorbaz/smartme_mqtt/issues/27) |
| NCMD/DCMD not implemented | Story 4.6, #23 |
| DDEATH never emitted | Epic 3 |

## Tally for chapter 4

17 conformant · 0 deviations · 8 gaps · 21 n/a (16 Host Application, 5 messages we do not emit)

Every `conformant` row names a test. No row is asserted from reading the code alone.
