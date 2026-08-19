# Sparkplug B conformance matrix

**Specification version: v3.0.0**, pinned at `docs/spec/sparkplug-b-3.0.0/` (EPL-2.0).

A conformance claim is meaningless without a version. **A version change invalidates this matrix
rather than merely dating it** — the clause set moves, so the audit must be re-run, not patched.

## How to read this

One row per normative clause, keyed by its `tck-id`. Verdicts:

| Verdict | Meaning |
| --- | --- |
| **conformant** | We do what the clause requires, **and a named test proves it** |
| **deviation** | We knowingly do otherwise; the row carries the rationale and its ADR or deferred-work entry |
| **gap (unimplemented)** | We do not do the thing the clause requires; the row carries an owning story, epic or issue |
| **gap (unproven)** | We do do it, but nothing proves that we do; the row carries an owning story, epic or issue |
| **n/a** | The clause addresses a role we do not play, or a message we do not emit |

**The two kinds of `gap` are one verdict wearing two labels**, and the inherited definition already
covered both — "we do not do it, **or** nothing proves that we do". Splitting the label changes no
verdict and no count; it exists because a reader of "every gap carries an owner" could not otherwise
tell a broken behaviour from an untested one, and the two need different work: a fix versus a test.

**A row claiming `conformant` with no test named is a `gap`, not a `conformant`.** A behaviour
nothing exercises is not a proven behaviour — that rule exists because contract v1 shipped
quality codes a real host read as `Good` while every internal test agreed with itself.

**One addition, made during the chapter-6 pass and ratified by
[ADR 0014](adr/0014-schema-as-conformance-evidence.md).** A named test is not the only admissible
witness: the **pinned `sparkplug_b.proto` schema** is one too, where it makes the violation
*unrepresentable*. A clause requiring a field to be an unsigned 32-bit integer is discharged by the
generated type being `Option<u32>`: there is no program we could write that emits anything else, so
the guarantee fails at **compile time** rather than on a test run — stronger than a test, not
weaker. That is the property that matters. It is *not* that the witness is external to this
repository: the schema is kept **inside** it, and the first draft of this paragraph claimed
otherwise, which was simply wrong.

**A second non-test witness, ratified at the code review of Story 4.3 —
[ADR 0015](adr/0015-language-type-invariants-as-conformance-evidence.md).** ADR 0014's admissibility
test is a *property*, not an artifact: it says so itself — "compile-time unrepresentability, not the
file's location". A **type invariant enforced by the language or its standard library** therefore
qualifies too, under three jointly necessary conditions: the clause must be about a **type**; the
invariant must **not be ours to change**; and the row must name the type rather than pad its Proof
column with adjacent tests. Chapter 1's three `-string` rows are the only rows using it.

Condition two is the load-bearing one, and it is what stops this becoming "the compiler proves it" —
the wider rule ADR 0014 explicitly refuses. `String`'s UTF-8 guarantee is the standard library's and
no edit here can weaken it. `MqttConfig`'s single `host`/`port` is **ours**, so
`operational-behavior-primary-application-state-with-multiple-servers-single-server` stays
`gap (unproven)` — decided under the same rule, in the same pass, the other way.

**The boundary is the whole point.** The schema witnesses **field types** and nothing else. It can
never discharge a clause about a **value** — `payloads-propertyset-quality-value-value` names the
literals `0`, `192` and `500`, and no schema constrains which of them we put on the wire. Where the
guarantee comes from *our own code shape* — a loop that happens to push in pairs, a field we happen
to always set, a constant that happens to equal the number the clause names — it can regress
silently, and the verdict is `gap (unproven)` until a test says otherwise. Rows using the schema
witness say so.

## Status

| Chapter | Story | State |
| --- | --- | --- |
| 1 — Introduction, Sparkplug identifiers | 4.3 | **done** — all 8 `tck-id`s accounted for |
| 2 — Principles | 4.3 | **done** — all 4 `tck-id`s accounted for |
| 3 — Architecture components | 4.3 | **done** — the chapter's single `tck-id` accounted for |
| 4 — Topics & namespace | 4.1, completed by **4.19** | **audited, not complete** — 41 of the chapter's 70 `tck-id`s carry a row; the other 29 are Story 4.19. See the chapter-4 tally |
| 5 — Operational behaviour, session lifecycle, host interaction | 4.3 | **done** — all 99 `tck-id`s accounted for |
| 6 — Payloads, metrics, datatypes | 4.2 | **done** — all 109 `tck-id-payloads-*` clauses accounted for |
| 10 — Conformance profiles | 4.3 | **done** — all 12 `tck-id`s accounted for |

**The whole specification is 303 `tck-id`s and 274 of them now carry a row or a named collective
block.** The 29 outstanding are all in chapter 4 and belong to Story 4.19.

Chapters 7, 8, 9 and both appendices carry **zero** `tck-id`s. That is load-bearing — the
disjointness argument at the head of chapter 5 assumes it — so it gets its own command rather than
the citation the first draft offered, which pointed at an enumeration that never greps them:

```bash
for f in 7_Security 8_HA 9_Acknowledgements Appendix_A Appendix_B; do
  printf '%s: ' "$f"
  grep -c 'tck-id-' docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_$f.adoc
done                                    # -> 0 0 0 0 0
```

Chapter 4 also carries the Host Application `STATE` clauses (`host-topic-phid-*`). They are
listed once, collectively, as **n/a**: this bridge is an Edge Node, not a Host Application. That
is a separate question from whether the bridge should *react* to a Host Application's STATE,
which is Stories 4.4–4.5 and is not settled by these clauses.

---

## Chapter 1 — Introduction and Sparkplug identifiers

Eight clauses. Six are the identifier rules that sit **underneath** chapter 4's topic grammar, and
reading them as a restatement of it is the mistake this section exists to prevent: chapter 4 asks
for a valid *topic level*, chapter 1 asks for a valid *MQTT string*, and the two sets are not the
same.

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `intro-sparkplug-host-state` | MUST | *"Sparkplug Host Applications MUST publish STATE messages denoting their online and offline status"* (`:276-277`) — we are an Edge Node | — | n/a — same subject as `components-ph-state` (ch. 3) and `conformance-primary-host` (ch. 10); ruled on once, here |
| `intro-group-id-string` | MUST | the Group ID is a Rust `String` (`topic.rs:99`) | — **no test, and none is possible**: `String`'s UTF-8 invariant is the standard library's, so invalid UTF-8 here fails to compile rather than failing a test. **Language type-invariant witness** ([ADR 0015](adr/0015-language-type-invariants-as-conformance-evidence.md)) | conformant |
| `intro-edge-node-id-string` | MUST | as above (`topic.rs:100`) | — same witness, same ADR | conformant |
| `intro-device-id-string` | MUST | `device_topic` takes `&str` (`topic.rs:140`) | — same witness, same ADR | conformant |
| `intro-group-id-chars` | MUST | **`check_identifier` rejects `/`, `+`, `#` and empty — and nothing else** (`topic.rs:155-165`). That is chapter 4's wildcard rule, not MQTT's character set | — **measured: a `U+0000` passes validation and appears in the constructed topic.** See below | **gap (unimplemented)** ([#34](https://github.com/guycorbaz/smartme_mqtt/issues/34)) |
| `intro-edge-node-id-chars` | MUST | as above, same function | — same measurement | **gap (unimplemented)** ([#34](https://github.com/guycorbaz/smartme_mqtt/issues/34)) |
| `intro-device-id-chars` | MUST | as above, same function | — same measurement | **gap (unimplemented)** ([#34](https://github.com/guycorbaz/smartme_mqtt/issues/34)) |
| `intro-edge-node-id-uniqueness` | MUST | nothing verifies that `group_id/edge_node_id` is unique across the infrastructure | — | **gap (unimplemented)** ([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)) |

**The three `-chars` clauses were the one place this pass expected agreement and did not find it,
and the finding was measured rather than reasoned.** All three read *"MUST only contain characters
allowed for MQTT topics **per the MQTT Specification**"* (`:307-309`, `:316-318`, `:325-327`). A
probe against `EdgeNode::new` returned:

```
group  with U+0000 accepted: true
  topic = "spBv1.0/a\0b/NBIRTH/node"
node   with U+0000 accepted: true
device with U+0000 accepted: true
```

**Reproduce it in one command** — the original probe was a throwaway test file, removed afterwards,
and a deleted artefact is not evidence anyone can check:

```bash
cargo test -p sparkplug-b --doc 2>/dev/null; cat <<'EOF' > /tmp/nul_probe.rs
fn main() {
    let n = sparkplug_b::EdgeNode::new("a\u{0}b", "node").expect("NUL is accepted");
    println!("{}", n.node_topic(sparkplug_b::MessageType::NBirth).unwrap().escape_debug());
}
EOF
echo "  (or add /tmp/nul_probe.rs as crates/sparkplug-b/tests/, run, and delete)"
```

**What this shows, stated no more strongly than the evidence.** A NUL passes `check_identifier` and
appears in the topic string the publisher would hand to the broker. The probe constructs the topic;
it does **not** publish, so "reaches the wire" — which an earlier draft of this section claimed and
the code review of Story 4.3 struck — is one step further than was measured. What is measured is
enough: the character survives the only validation the bridge performs.

Assuming chapter 1's set equalled chapter 4's `+`/`/`/`#` rejection — which is the natural reading
and which this matrix nearly took — would have produced three `conformant` rows over a demonstrated
defect.

**And the audit's reach stops short of the clause.** This repository keeps **no copy of the MQTT specification in
this repository**; only Sparkplug B is. So the admissible set cannot be cited the way `CLAUDE.md`
requires, and this matrix does not claim to know it. What is established is narrower and enough:
the implemented set is chapter 4's, the clause's is MQTT's, and at least one character separates
them. [#34](https://github.com/guycorbaz/smartme_mqtt/issues/34) should begin by pinning the MQTT
clause, so the fix is written against a norm rather than against memory.

**`-edge-node-id-uniqueness` is one requirement stated three times, and it gets one owner.** Chapter
4's `topic-structure-namespace-unique-edge-node-descriptor` and chapter 6's
`payloads-nbirth-edge-node-descriptor` are the other two. All three point at
[#27](https://github.com/guycorbaz/smartme_mqtt/issues/27); no second issue was opened.

---

## Chapter 2 — Principles

Four clauses. Two of them bite, and one passes by a dependency default nobody in this repository
chose.

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `principles-rbe-recommended` | SHOULD NOT | **the bridge publishes on every poll tick, changed or not** — `tokio::time::interval` (`poll_publish.rs:143`, 5 s default) and `step_once` publishes each outcome. There is no change detection anywhere in the tree | `poll_publish.rs::step_once`, `StateMachine::step` returns a verdict, never a publish decision (`core/state_machine.rs:82`) | **deviation** ([#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)) — RBE is *blocked*, not rejected; see below |
| `principles-birth-certificates-order` | MUST | `publish()` refuses before the birth (`Published::DroppedBeforeBirth`, `sparkplug_publisher.rs:343`); `birth()` emits the NBIRTH first | `a_drop_before_the_birth_is_reported_not_silent` (**remove the `Session::Live` guard and it goes red**) and `cold_start_birth_declares_tags_with_no_value_and_stale_quality`, which pins NBIRTH at `sink.emitted[0]`. `chaos_sigterm_no_lie` is **not** a witness for *first-ness* — see the bound below | conformant — see the bound below |
| `principles-persistence-clean-session-311` | MUST | the flag **is** true — and nothing in this repository sets it | — **the guarantee is rumqttc's default**, verified in the registry source (`rumqttc-0.25.1/src/lib.rs:513`, `mqttbytes/v4/connect.rs:27`); `mqtt_driver.rs:905-940` never calls `set_clean_session`, and no test asserts the flag | **gap (unproven)** ([#35](https://github.com/guycorbaz/smartme_mqtt/issues/35), Story 4.10) |
| `principles-persistence-clean-session-50` | MUST | the bridge speaks MQTT 3.1.1 | — | n/a — `rumqttc = "0.25"` (`Cargo.toml:42`) and the driver imports `rumqttc::{AsyncClient, …}`, **not** `rumqttc::v5::*` (`mqtt_driver.rs:93`) |

**On RBE, the verdict is `deviation` and the row must say the behaviour is blocked rather than
rejected.** The clause reads *"Because of the stateful nature of Sparkplug sessions, data SHOULD NOT
be published from Edge Nodes on a periodic basis and instead SHOULD be published using a RBE based
approach"* (`Sparkplug_2_Principles.adoc:50-52`). Three facts carry the decision, taken 2026-07-28:

1. **For an active meter RBE would suppress almost nothing.** `smartme_sample.json` publishes
   `CounterReading` to six decimals (`4843.822`, `6330.412207`); at the fixture's `0.754 kW` energy
   advances ~`0.001 kWh` per 5-second poll, which is visible at the published precision. The values
   genuinely change every tick.
2. **For a dead meter it suppresses everything, and that case is live.** Fixture meter `30000003`
   reads `0.0 kW` with a `ValueDate` of `2026-04-20` — the physically unplugged meter. The bridge
   republishes byte-identical content for it roughly **17 000 times a day**, indefinitely. That is
   precisely the case the clause addresses, and it is the honest half of this deviation.
3. ~~**RBE cannot land before `Node Control/Rebirth`.**~~ **This ground is spent (Story 4.7,
   2026-07-30).** Sparkplug assumes a late-joining consumer issues a Rebirth to relearn state. The
   bridge answered none, so the periodic publish was *substituting* for the missing Rebirth, and
   implementing RBE first would have meant a new consumer never learning the unplugged meter's
   value — a functional regression wearing conformance as a costume. **The bridge now answers a
   Rebirth**, so that objection no longer applies. What survives of it is narrower and still real:
   the repair is *host-initiated*, so a consumer that never asks still never learns. That is an
   argument about which hosts ask, not about whether the mechanism exists, and it is a reason to
   decide RBE deliberately rather than a reason it cannot be decided.

**Revisit condition, now DUE and deliberately not discharged here.** This said that when Story 4.7
landed the deviation must be re-examined. Story 4.7 has landed, so the stated blocker is gone: the
periodic publish was substituting for a repair path that did not exist, and a repair path now
exists — a consumer that has lost its tag definitions can ask for them and be answered.

**The verdict is unchanged and RBE is deliberately not implemented in Story 4.7.** What changed is
the *reason* for the verdict: it was "cannot safely be changed", and it is now "has not been
decided". Those are different states and collapsing them would hide a live decision behind a stale
excuse. The remaining question — whether a rebirth-on-request is sufficient substitute for periodic
publishing, given that a host which never asks never learns — belongs to
[#32](https://github.com/guycorbaz/smartme_mqtt/issues/32) and its own story, with its own evidence.
The old revisit condition is discharged; a new one replaces it: **RBE must be ruled on explicitly,
not left to inherit this paragraph.** Chapter 5's `operational-behavior-data-publish-dbirth-change` is the same obligation
stated operationally; it carries the same verdict and the same owner, and is not a second defect.

**One thing the PRD said and the code never did.** `prd.md` and `architecture.md` described the
downstream publisher as *"NDATA/DDATA (report-by-exception)"*, and **no FR requires it** — FR17–FR22
cover units, serial binding, timestamps, rebirth, delivery and outage policy, and none mentions RBE.
The planning artifacts were corrected on 2026-07-28; `docs/manual/chapters/05-mqtt-sparkplug-contract.tex:237`
had recorded it correctly all along.

*(Those two corrections were an **earlier commit** on 2026-07-28, not this audit's work; the first
draft of this paragraph read as though the audit had made them. And the moral it drew — that the
manual is the document that gets maintained — needs qualifying: the same commit that carries this
matrix also **repairs** that manual chapter, which claimed QoS 0 was "as the Sparkplug specification
requires" when the registered will must be QoS 1. The manual was right about RBE and wrong about
delivery semantics. No document in this project has earned unconditional trust.)*

**The bound on `-birth-certificates-order`, because the row would otherwise over-claim — and the
first draft did.** The clause says a Birth Certificate must be the *first* MQTT message published.
The cited tests prove that DATA before the BIRTH is refused and reported, and that the NBIRTH sits
at index 0 of the emitted sequence. They do not enumerate the message space — that argument is
structural: the bridge emits only BIRTH, DATA and DEATH, the DEATH is published at shutdown, and the
will is registered inside CONNECT rather than published by us. Those two tests cover every message
type that could precede a birth, which is why this is `conformant` and not `gap (unproven)`.

**What this row must *not* claim, corrected at the code review of Story 4.3.** The draft cited
`chaos_sigterm_no_lie` as observing the NBIRTH "on a real broker before anything else". It observes
no such thing. The test finds the birth with `common::wait_for(…, |s| s.topic.contains("/NBIRTH/"))`,
and that helper **silently discards every non-matching message** (`tests/common/mod.rs:157-161`): a
DDATA published before the NBIRTH would be dropped on the floor and the test would pass unchanged.
The observer *is* connected before the child process is spawned, so the test could have asserted
first-ness and does not. This is the "assertion adjacent to the clause" class `CLAUDE.md` names, and
it was caught by reading the test rather than the row.

---

## Chapter 3 — Architecture and infrastructure components

One clause.

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `components-ph-state` | MUST | *"A Sparkplug Host Application MUST utilize the STATE messages to denote whether it is online or offline at any given point in time"* (`Sparkplug_3_Components.adoc:84-86`) — we are an Edge Node | — | n/a — see `intro-sparkplug-host-state` |

**This clause is `n/a` and the STATE gap is still recorded**, which is the distinction the whole
chapter-5 host section turns on. What a Host Application *publishes* is not our clause. What an Edge
Node must *do* when its Primary Host goes offline is, and it is `gap` in four places below.
**Those four gaps are now a recorded decision** — [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md)
rules the mechanism out on four grounds. The verdicts below are unchanged and the reason is
[#42](https://github.com/guycorbaz/smartme_mqtt/issues/42): a story does not re-grade rows on the
strength of an ADR it wrote itself.

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
| `topic-structure-namespace-unique-edge-node-descriptor` | MUST | nothing verifies that `group_id/edge_node_id` is unique across the MQTT infrastructure | — | **gap (unimplemented)** ([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)) |
| `topic-structure-namespace-unique-device-id` | MUST | one device today; uniqueness across a fleet is unenforced | — | **gap (unimplemented)** (Epic 3) |
| `topic-structure-namespace-duplicate-device-id-across-edge-node` | MAY | permissive; nothing to do | — | n/a |

**On the two gaps.** Neither is a coding error. An Edge Node cannot verify infrastructure-wide
uniqueness alone — but "cannot be verified from inside" is exactly what a conformance matrix is
for, and silence would let it read as satisfied. At minimum the operator manual must state the
constraint; ideally the bridge refuses to start on a detectable collision. Issue #27.

### Edge-node messages

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `topics-nbirth-topic` | MUST | `spBv1.0/{group}/NBIRTH/{node}` | `node_topics_follow_the_namespace_grammar` | conformant |
| `topics-nbirth-mqtt` | MUST | QoS 0, retain false | `mqtt_driver.rs::the_delivery_table_matches_the_specification_clause_by_clause` | conformant |
| `topics-ndeath-topic` | MUST | `spBv1.0/{group}/NDEATH/{node}` | `node_topics_follow_the_namespace_grammar` | conformant |
| `topics-ndata-topic` | MUST | topic construction supports it; **NDATA is never emitted** — the bridge carries no node-level measurement | — | n/a |
| `topics-ndata-mqtt` | MUST | as above | — | n/a |
| `topics-ncmd-topic` | MUST | `spBv1.0/{group}/NCMD/{node}` — built by `node_topic(MessageType::NCmd)` from the same validated grammar as every published topic, and used as the subscription filter (Story 4.6, `mqtt_driver.rs:882`) | `the_ncmd_topic_follows_the_namespace_grammar` pins the full literal; `chaos_ncmd_subscription` reads the filter back out of the **broker's** log, so the form is witnessed on the wire and not only at the call site | conformant — **moved from `gap (unimplemented)` by Story 4.6.** The clause is about the topic *form*, and the form now exists because something subscribes to it |
| `topics-ncmd-mqtt` | MUST | a **publication** clause — see the note below | — | n/a |

> **`topics-ndata-mqtt` settles a question that was got wrong twice.** It reads *"NDATA messages
> MUST be published with MQTT QoS equal to 0 and retain equal to false"*, and `topics-ddata-mqtt`
> says the same for DDATA. So QoS 0 **is** mandated for data, no broker acknowledgement can
> exist, and ADR 0010's amendment of FR20 was correct. An earlier reading of chapter 5 alone
> found no data-QoS clause and wrongly concluded none existed — see #26.

### Device messages

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `topics-dbirth-topic` | MUST | `spBv1.0/{group}/DBIRTH/{node}/{device}` | `device_topics_append_the_device_identifier` | conformant |
| `topics-dbirth-mqtt` | MUST | QoS 0, retain false | `the_delivery_table_matches_the_specification_clause_by_clause` | conformant |
| `topics-ddata-topic` | MUST | `spBv1.0/{group}/DDATA/{node}/{device}` | `device_topics_append_the_device_identifier` | conformant |
| `topics-ddata-mqtt` | MUST | QoS 0, retain false | `the_delivery_table_matches_the_specification_clause_by_clause` | conformant |
| `topics-ddeath-topic` | MUST | `spBv1.0/{group}/DDEATH/{node}/{device}`, built by `node.device_topic(MessageType::DDeath, …)` (`sparkplug_publisher.rs::device_death`). **Emitted since Story 5.2** — disabling a meter buries its device | `device_topics_append_the_device_identifier`; and `chaos_device_certificates` reads the DDEATH off a **real broker** and asserts the topic names the device that went away | conformant |
| `topics-ddeath-mqtt` | MUST | QoS 0, retain false | `the_delivery_table_matches_the_specification_clause_by_clause` enumerates `DDeath` explicitly | conformant |
| `topics-dcmd-topic` | MUST | **not implemented** — no DCMD subscription, and a subscriber must build this topic form too. **⏳ TIME-LIMITED, recorded at Story 4.7:** a planned meter relay command is a writable Device output, which is the condition its chapter-5 twin `-device-dcmd-subscribe` is `n/a` upon; when it lands both go live together. Not re-verdicted here — [#38](https://github.com/guycorbaz/smartme_mqtt/issues/38) owns the expiry | — | **gap (unimplemented)** (**Story 4.19**, re-owned) — Story 4.6 declined DCMD deliberately: `-device-dcmd-subscribe` is conditional on *"if the Device supports writing to outputs"* (`:403-407`) and no device here does. `MessageType` has no `DCmd` variant on purpose. See the criterion note below: this row probably belongs at `n/a` |
| `topics-dcmd-mqtt` | MUST | a **publication** clause — see the note below | — | n/a |

> **`topics-ncmd-mqtt` and `topics-dcmd-mqtt` are `n/a`, and they were `gap`s until the code review
> of Story 4.2 (2026-07-28).** Both read *"NCMD/DCMD messages **MUST be published** with MQTT QoS
> equal to 0 and retain equal to false"* (`Sparkplug_4_Topics.adoc:344`, `:508`). That binds whoever
> publishes an NCMD or a DCMD, and per chapter 6 that is *always* a Host Application
> (`Sparkplug_6_Payloads.adoc:1411`, `:1455`). An Edge Node never publishes either message in any
> configuration, so the clause governs no behaviour of ours. Their chapter-6 twins
> (`payloads-ncmd-qos`, `-retain`, and the DCMD pair) are `n/a` for the same reason, and the two
> chapters must not disagree about one obligation.
>
> **Nothing is hidden by the change.** The obligation we genuinely fail — that an Edge Node must
> *subscribe* — is a different clause with its own id: `tck-id-message-flow-edge-node-ncmd-subscribe`
> (`Sparkplug_5_Operational_Behavior.adoc:158`) and `-device-dcmd-subscribe` (`:403`), both in
> chapter 5 and therefore owned by **Story 4.3**. It also stays visible here at `topics-ncmd-topic`,
> `topics-dcmd-topic` and `payloads-nbirth-rebirth-req`, all owned by Stories 4.6 and 4.7.

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

## Chapter 5 — Operational behaviour: session lifecycle and host interaction

**The clause set is 99 ids, established mechanically.** The same enumeration also settled the
chapter partition for the whole specification, which no earlier pass had done:

```bash
for f in 1_Introduction 2_Principles 3_Components 4_Topics \
         5_Operational_Behavior 6_Payloads 10_Conformance; do
  grep -oE 'tck-id-[A-Za-z0-9-]+' \
    docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_$f.adoc | sed 's/-$//' | sort -u | wc -l
done                                    # -> 8 4 1 70 99 109 12
grep -rhoE 'tck-id-[A-Za-z0-9-]+' docs/spec/sparkplug-b-3.0.0/ \
  | sed 's/-$//' | sort -u | wc -l      # -> 303
```

`8 + 4 + 1 + 70 + 99 + 109 + 12 = 303`, and 303 is the whole-tree total. **Because the sum equals
the total, no id appears in two chapters** — the property chapter 6 checked for itself, established
here for every chapter at once. Chapters 7, 8, 9 and both appendices carry zero ids.

**This chapter restates lifecycle requirements chapters 4 and 6 also state, and no verdict below was
copied from a twin.** Each was read against chapter 5's own wording. Where two chapters genuinely
state one obligation, both get a row (the matrix is keyed by `tck-id`) and they point at the **same**
owner rather than inventing a second — as `-nbirth-payload-bdSeq` and `-will-message-payload-bdSeq`
do below.

### Edge-node session flow — births, deaths, will, `seq`, `bdSeq`

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `message-flow-edge-node-ncmd-subscribe` | MUST (QoS 1) | the driver subscribes to `spBv1.0/{group}/NCMD/{node}` at **QoS 1**, and it does so **before** the birth publish, in the `Transport::Connected` arm — so the ordering holds on every reconnect, not only the first (`mqtt_driver.rs:956-971`, `subscribe_to_commands` at `:1318`; topic built by `node_topic(MessageType::NCmd)` at `:882`) | `chaos_ncmd_subscription` reads the **broker's own verbose log**: `Received SUBSCRIBE from <node>` followed by `spBv1.0/…/NCMD/<node> (QoS 1)`, then `Received PUBLISH … '/NBIRTH/…'`. One MQTT client cannot observe another's SUBSCRIBE, so the broker is the only external witness there is. Falsified — moving the subscribe after the birth turns it red; and, added by the Story 4.6 review, hoisting the subscribe out of the `Transport::Connected` arm turns it red on the SECOND connect, which the test now forces by evicting the bridge's client id | conformant — **for the subscription only, and on one stated condition.** The clause's trailing bullet (*"mandatory as Edge Nodes MUST be able to respond to 'rebirth requests'"*) is not a `tck-id`; the responding is `-rebirth-action-1/2/3` below, **all three `conformant` since Story 4.7**. **The condition:** if `try_subscribe` itself fails — a full request channel at the CONNACK instant, or a closed client — the driver traces at ERROR and **births anyway** without retrying for the life of the session (`subscribe_to_commands` at `mqtt_driver.rs:1318`). That is deliberate (publishing without a command path beats not publishing) but it is a path on which this row's clause is unmet, it is distinct from the broker *refusing* the subscription, and it is recorded in `deferred-work.md` |
| `message-flow-edge-node-birth-publish-connect` | MUST | `publisher.birth(...)` is driven from `Transport::Connected`, which is raised only on `Packet::ConnAck` (`mqtt_driver.rs:956-971`; `Packet::ConnAck` at `:1109`) | `chaos_sigterm_no_lie` observes the NBIRTH arriving on a real broker after a real CONNECT | conformant |
| `message-flow-edge-node-birth-publish-will-message` | MUST | the will is registered in the CONNECT packet (`mqtt_driver.rs:931`), built **before** the client exists | `chaos_stale_on_death` — the bridge's task is aborted without a shutdown signal (`:68`), the socket drops, and an independent subscriber receives the certificate the broker was holding. An external witness against a real broker | conformant |
| `message-flow-edge-node-birth-publish-will-message-topic` | MUST | `spBv1.0/{group}/NDEATH/{node}`, built by `node_topic` (`sparkplug_publisher.rs:242`, `:367`) | `node_topics_follow_the_namespace_grammar` pins the full literal. `chaos_stale_on_death` is **not** a second witness for the grammar: it tests only `.contains("/NDEATH/")` (`:70-72`), which `foo/NDEATH/bar` would satisfy | conformant |
| `message-flow-edge-node-birth-publish-will-message-payload` | MUST | the will payload is `encode(&payload)` — the pinned protobuf | `chaos_stale_on_death` decodes it from a real broker; `prop_every_numbered_payload_round_trips` | conformant |
| `message-flow-edge-node-birth-publish-will-message-payload-bdSeq` | MUST | the metric is present, named `bdSeq`, INT64, **and the value now increments per CONNECT** — the driver owns its reconnect loop and registers a fresh will carrying the new session number (Story 4.10, 2026-08-01) | `the_will_matches_the_session_before_and_after_the_birth`, `prop_will_birth_and_death_agree_on_bdseq_for_every_session_number` (presence and pairing) + `chaos_bd_seq_advances_on_every_connect` (the INCREMENT, observed by an independent subscriber across a real disconnect, and falsified) | **conformant** (Story 4.10) |
| `message-flow-edge-node-birth-publish-will-message-qos` | MUST (QoS 1) | the will is registered at **QoS 1** — `qos_for(MessageType::NDeath)` returns `AtLeastOnce` and the will is built from it (`mqtt_driver.rs`, `qos_for` and the `set_last_will` call) | `the_delivery_table_matches_the_specification_clause_by_clause` — falsified 2026-08-10 by restoring QoS 0, red with the clause named | **conformant** (Story 4.17, closes [#26](https://github.com/guycorbaz/smartme_mqtt/issues/26)) |
| `message-flow-edge-node-birth-publish-will-message-will-retained` | MUST (false) | retain false, from the same `qos_for` | `the_registered_will_carries_the_qos_and_retain_the_norm_mandates` — reads the will back out of the `MqttOptions` the broker receives, so this row and its QoS sibling now rest on the SAME artefact at the same standard. Falsified 2026-08-11 (retain hard-coded true goes red). **Upgraded from `gap (unproven)` on 2026-08-11**, by the story's own review | **conformant** (Story 4.17 review) |
| `message-flow-edge-node-birth-publish-nbirth-topic` | MUST | `spBv1.0/{group}/NBIRTH/{node}` | `node_topics_follow_the_namespace_grammar`; `cold_start_birth_declares_tags_with_no_value_and_stale_quality` pins the literal `spBv1.0/Site/NBIRTH/Bridge` | conformant |
| `message-flow-edge-node-birth-publish-nbirth-payload` | MUST | protobuf, one encoder for every message type | `a_birth_is_self_describing`, `prop_every_numbered_payload_round_trips` | conformant |
| `message-flow-edge-node-birth-publish-nbirth-payload-bdSeq` | MUST | the NBIRTH's `bdSeq` **is** the previous CONNECT's — both read the one `publisher.bd_seq()` | `the_will_matches_the_session_before_and_after_the_birth`, `prop_will_birth_and_death_agree_on_bdseq_for_every_session_number`, and `chaos_sigterm_no_lie` reads it off a real broker against a **seeded** number, so it is not a constant compared with itself | conformant — see below on why the vacuity is recorded on the other row |
| `message-flow-edge-node-birth-publish-nbirth-qos` | MUST (QoS 0) | QoS 0 | `the_delivery_table_matches_the_specification_clause_by_clause` — **mutation-verified at the Story 4.3 review**: changing `qos_for` to return `AtLeastOnce` goes red, and both call sites (`:295` the will, `:572`, inside the `publish` helper, every publish) derive from that one function | conformant |
| `message-flow-edge-node-birth-publish-nbirth-retained` | MUST (false) | retain false | same, **plus an external witness**: `chaos_sigterm_no_lie:397-405` connects a late subscriber after the bridge is gone and asserts the broker replays nothing — and unlike the will, the NBIRTH is *certainly* published in that run, because the test waited for it | conformant |
| `message-flow-edge-node-birth-publish-nbirth-payload-seq` | MUST (0–255) | a BIRTH resets the counter and takes `0`; `SeqCounter` is a `u8`, so the range is a type invariant | `birth_carries_seq_zero_and_the_session_number`, `prop_seq_stays_in_range_and_wraps_at_the_boundary`, `prop_rebirth_always_restarts_numbering_at_zero` | conformant |
| `message-flow-edge-node-birth-publish-phid-wait` | MUST | **the bridge has no Primary Host Application configuration at all** | — | **gap (unimplemented)** · **DECIDED, not pending: [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism** (Story 4.5); the verdict word is under review in [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) · [4.4 measured](primary-host-state-observation.md#the-eleven-clauses-ruled): relevance *relevant* |
| `message-flow-edge-node-birth-publish-phid-wait-id` | MUST | as above — no STATE topic is parsed | — | **gap (unimplemented)** · **DECIDED, not pending: [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism** (Story 4.5); the verdict word is under review in [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) · [4.4 measured](primary-host-state-observation.md#the-eleven-clauses-ruled): relevance *relevant* |
| `message-flow-edge-node-birth-publish-phid-wait-online` | MUST | as above — no STATE payload is read | — | **gap (unimplemented)** · **DECIDED, not pending: [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism** (Story 4.5); the verdict word is under review in [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) · [4.4 measured](primary-host-state-observation.md#the-eleven-clauses-ruled): relevance *relevant* |
| `message-flow-edge-node-birth-publish-phid-wait-timestamp` | MUST | as above — no STATE timestamp is compared | — | **gap (unimplemented)** · **DECIDED, not pending: [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism** (Story 4.5); verdict word under review in [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) · [4.4 measured](primary-host-state-observation.md#the-eleven-clauses-ruled): relevance *relevant*, monotonicity undetermined |
| `message-flow-edge-node-birth-publish-phid-offline` | MUST | as above — nothing makes the bridge disconnect on an offline STATE | — | **gap (unimplemented)** · **DECIDED, not pending: [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism** (Story 4.5); verdict word under review in [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) · [4.4 measured](primary-host-state-observation.md#the-eleven-clauses-ruled): relevance *relevant* |
| `operational-behavior-edge-node-intentional-disconnect-ndeath` | MUST | the bridge publishes its own NDEATH before dropping the socket (`mqtt_driver.rs:1083`) | `chaos_sigterm_no_lie` — and it proves the **explicit** death rather than the will, because it asserts `death_stamp > birth_stamp`, which a CONNECT-time will can never satisfy | conformant — **and it vindicates [ADR 0011](adr/0011-graceful-shutdown-requires-both-deaths.md)** |
| `operational-behavior-edge-node-intentional-disconnect-packet` | MAY | the bridge **never** sends a DISCONNECT packet, deliberately: it would instruct the broker to discard the will, removing the fallback (ADR 0011) | `chaos_sigterm_no_lie` observes the will still firing after the explicit death | n/a — a permission declined, not an obligation missed |
| `operational-behavior-edge-node-birth-sequence-wait` | MUST | the bridge births as soon as the broker answers; it waits for no STATE | — | **gap (unimplemented)** (Stories 4.4–4.5) · **DECIDED, not pending: [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism** (Story 4.5); verdict word under review in [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) · [4.4 measured](primary-host-state-observation.md#the-eleven-clauses-ruled): relevance *relevant*, but which of two readings applies is undetermined |
| `operational-behavior-edge-node-termination-host-offline` | MUST | nothing disconnects the bridge on an offline STATE | — | **gap (unimplemented)** · **DECIDED, not pending: [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism** (Story 4.5); verdict word under review in [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) · [4.4 measured](primary-host-state-observation.md#the-eleven-clauses-ruled): relevance *relevant* |
| `operational-behavior-edge-node-termination-host-offline-reconnect` | MUST | there is no server list to walk | — | **gap (unimplemented)** (Story 4.5) · **DECIDED, not pending: [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism** (Story 4.5); verdict word under review in [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) · [4.4 measured](primary-host-state-observation.md#the-eleven-clauses-ruled): relevance *relevant* — one broker, so *"the next available MQTT Server"* is the same server and a literal implementation loops on itself; 4.5 must specify the alternative *(was *irrelevant*; changed by the 4.4 review)* |
| `operational-behavior-edge-node-termination-host-offline-timestamp` | MUST NOT | the anti-replay rule for a stale offline STATE — nothing implements it because nothing reads STATE | — | **gap (unimplemented)** (Story 4.5) · **DECIDED, not pending: [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism** (Story 4.5); verdict word under review in [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) · [4.4 measured](primary-host-state-observation.md#the-eleven-clauses-ruled): relevance *relevant*, occurrence here undetermined |

**Why `-nbirth-qos` is `conformant` on `qos_for` while `-will-message-will-retained` is a `gap` on
the same function — a convention, made explicit at the code review of Story 4.3 because the rows
otherwise look inconsistent.** Both values flow from `qos_for`'s single return. The difference is
what a test would *observe*:

- **QoS 0 on a published message** is asserted end-to-end: the mutation above goes red, and
  `chaos_sigterm_no_lie` observes the messages arriving on a real broker.
- **Retain on the *registered will*** was asserted nowhere until 2026-08-11.
  `the_delivery_table_matches_the_specification_clause_by_clause` checks the *function*, not the
  `LastWill` the driver builds from it, and the one test in which the broker actually publishes the
  will (`chaos_stale_on_death`) asserts only `bdSeq` and `seq == None`. A `set_last_will` call that
  dropped the `retain` argument passed every test in the tree.

**CLOSED 2026-08-11.** The prediction directly above — *"one test on the constructed `LastWill`
closes both rows"* — is exactly what happened, and it took extracting `register_will` so the will
could be read back out of the `MqttOptions`. Both rows are now `conformant` on the same artefact:
this one and chapter 6's `payloads-ndeath-will-message-retain`, which the Story 4.2 review had
downgraded for precisely this reason. The asymmetry the note went on to describe — this row refused
while `-nbirth-qos` was accepted on the same derivation — is therefore gone, and it is worth keeping
the record of it: the matrix under-claimed for four months rather than over-claim, and the row that
was *accepted* on weak evidence is the one that turned out to be hiding a real violation ([#26]).

**Six more `operational-behavior-edge-node-*` clauses are `n/a`, and they are listed by id rather
than by heading**, and by **full** id so the coverage check can diff them:
`operational-behavior-edge-node-termination-host-action-ndeath-node-offline`,
`operational-behavior-edge-node-termination-host-action-ndeath-node-tags-stale`,
`operational-behavior-edge-node-termination-host-action-ndeath-devices-offline`,
`operational-behavior-edge-node-termination-host-action-ndeath-devices-tags-stale`,
`operational-behavior-edge-node-termination-host-action-ddeath-devices-offline`,
`operational-behavior-edge-node-termination-host-action-ddeath-devices-tags-stale`.

**Their ids say `edge-node`; every one of them binds a Host Application.** Each reads *"Immediately
after receiving an NDEATH/DDEATH from an Edge Node, **Host Applications MUST mark**…"* (`:341-353`,
`:503-510`) — they describe what a consumer owes when it receives our certificate, not what we owe
when we send it. Filing them by id prefix would have produced six rows discussing the wrong side of
the wire, which is the same trap chapter 6 recorded for `-name-birth-data-requirement`. What the
bridge owes on death is `-intentional-disconnect-ndeath` above, and it is conformant.

**`-ncmd-subscribe` is the most consequential row in this chapter, and until this pass it had no
home.** The code review of Story 4.2 moved chapter 4's `topics-ncmd-mqtt` and chapter 6's
`payloads-ncmd-qos`/`-retain` to `n/a`, correctly: they bind whoever *publishes* an NCMD, and that
is always a Host Application. But that reclassification left the obligation the bridge genuinely
failed resting entirely on this clause — *"The MQTT client associated with the Edge Node MUST
subscribe to a topic of the form 'spBv1.0/group\_id/NCMD/edge\_node\_id' … with a QoS of 1"*
(`Sparkplug_5_Operational_Behavior.adoc:158-162`), followed by *"This subscription is mandatory as
Edge Nodes MUST be able to respond to 'rebirth requests'"*.

**Story 4.6 closed it, and the QoS trap it warned about was real but not where it looked.** The risk
recorded here was that `qos_for` returns `AtMostOnce` unconditionally and the subscription would
inherit it. It does not: `qos_for` governs what the edge node *publishes*, and the subscribe QoS is a
different field in a different packet travelling the other way, so the two never meet. The clause is
honoured with a literal `QoS::AtLeastOnce` in `subscribe_to_commands`, and the proof is taken from
the broker's log rather than from that call site — a test reading our own constant back would assert
nothing.

The **ordering** turned out to be the harder half. The epic's wording (*"as part of the same
post-CONNACK sequence that publishes NBIRTH"*) permits birth-then-subscribe; the specification's own
section preamble (`:155-156`) does not — *"**Prior to sending an NBIRTH message**"* — and a host that
answers a birth with a rebirth request would be talking to a node that is not yet listening. The row
above is scored against the preamble.

Three clauses in this chapter — `-rebirth-action-1/2/3` — remain `gap` after Story 4.6, and their
verdicts are right, but **their wording was not**. `-rebirth-action-1` is about what happens *on
receipt* (*"nothing receives a Rebirth Request"*), not about answering, so Story 4.6 made its evidence
cell false and it has been amended; `-action-2/3` are about answering and needed no change.

*Corrected by the Story 4.6 code review, 2026-07-29. The paragraph this replaces asserted that all
three clauses "are about *answering* one" and certified them as still-right — while the story that
wrote it was the story that falsified one of them. This is the fifth instance of the pattern the
story's own Dev Notes catalogue: the claim gets corrected, the sentence describing its consequence
gets read for intent instead of against the code. Recording it rather than quietly overwriting it,
because the correction is the evidence that the mechanism AC5 asked for — a per-passage report — is
not ceremony.*

Story 4.6 built the plumbing and threw every command away on purpose, tracing the metric names it
saw. Receiving without answering was not a half-measure — it was the state in which the ignoring
could be shown safe before Story 4.7 gave one command meaning. **Story 4.7 has since given
`Node Control/Rebirth` meaning, and every other command is still thrown away** on the same traced
paths, which are unchanged.

**Why `-will-message-payload-bdSeq` WAS a deviation, and what closed it (Story 4.10, 2026-08-01).**
The will clause carries the increment requirement in its own text: *"the value MUST be incremented by
one from the value in the previous MQTT CONNECT packet unless the value would be greater than 255"*
(`:178-182`). Until 4.10 the bridge incremented **per process start** — `load_bd_seq` →
`NodeSession::start` → `store_bd_seq` before connecting — but never per *reconnect*: the will was
serialised into `MqttOptions` once and `rumqttc` rebuilt every reconnect's CONNECT from that
snapshot, so it could not be updated. A Host Application therefore could not distinguish a current
session from a superseded one after a transport blip.

**The driver now owns its reconnect loop**: each CONNECT builds a new client and registers a new
will carrying the session number that CONNECT will use. Both rows are `conformant`.

**The order is what made it safe, and it is worth keeping.** Advancing `bdSeq` *without*
re-registering the will would have been strictly worse than the deviation — the broker would hold a
certificate for a session that no longer exists, a consumer pairing death to birth would discard it,
and a frozen value would stay on screen presented as live. That is why the fix is *own the loop*
rather than *advance the counter*.

Chapter 6 folded both halves onto `payloads-nbirth-bdseq-repeat` because chapter 6 gives the
increment no id of its own; chapter 5 does. **One defect, recorded once per chapter, on the row each
chapter provides for it.** Chapter 4's own id for it, `tck-id-topics-nbirth-bdseq-increment`, is one
of the 29 clauses that chapter records nowhere and belongs to **Story 4.19** — which should now cite
the evidence above rather than open the row as a fresh gap.

**One thing 4.10 could NOT verify, recorded rather than glossed:** no NDEATH reaches a subscriber on
the *reconnect* path at all, so the will's new number is observed only on the SIGTERM path. See
[#43](https://github.com/guycorbaz/smartme_mqtt/issues/43).

**Story 4.13 answered half of that, by measurement, on 2026-08-18 — and the half it answered is the
opposite of what was expected.** `chaos_broker_recovery` stops the broker container and records
everything an independent subscriber receives: **exactly one NDEATH arrives, on every run measured,
carrying the ended session's own `bdSeq`.** The will is `(QoS 1, retain false)`, so this is not a
retained message surviving the restart — mosquitto's SIGTERM path publishes the wills of the sessions
it tears down, and gets them out to subscribers before closing their sockets. So the will's number
**is** observable off the reconnect path, on a broker that is stopped.

**No row moves on that**, and the limit is the point. It is a measurement of mosquitto's shutdown
behaviour, not of this bridge's conformance: the same test run with `docker kill --signal SIGKILL`
observes **nothing at all**, because there is no shutdown path to run. A broker that crashes, loses
power, or is killed still delivers no death, which is the residue of [#43] and stays open. What 4.13
closes is the *unqualified* form of the sentence above — "at all" was too strong — not the clause.

**The five Primary-Host clauses are `gap`, not `n/a`, and that is the single most reversible
judgement in this chapter.** Every one is conditional — *"If the Edge Node is configured to wait for
a Primary Host Application…"* — and the bridge has no such configuration, so a reflexive reading
makes all five vacuous.

**The strongest case against this row's verdict, quoted rather than paraphrased**, because the code
review of Story 4.3 found it missing and a reader should not have to go looking:

> *"Specifying a Primary Host is not required for an Edge Node. But it is often desired."*
> — `Sparkplug_5_Operational_Behavior.adoc:190-191`
>
> *"It is not required that an Edge Node must have a Primary Host configured but it may be useful in
> certain applications."* — `Sparkplug_1_Introduction.adoc:285-286`

The specification could hardly be plainer that this is optional. On that reading all five are `n/a`,
chapter 5 becomes `29 · 1 · 15 · 54` and the whole-specification totals become `80 · 6 · 39 · 149`.

*(This counterfactual read `22 · 2 · 21 · 54` and `70 · 8 · 47 · 149` until 2026-08-03: it was
computed against the tallies of the day and not recomputed when they moved. The **delta** it applies
is what carries — one `conformant` and four `gap` rows becoming `n/a` — and it is unchanged, because
no story since has touched these five verdicts: 4.5 decided the mechanism without moving them
([ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md)), 4.6 added NCMD and no
STATE handling, and 4.7 and 4.10 moved rows elsewhere. Applied to today's `30 · 1 · 19 · 49` and
`81 · 6 · 43 · 144`, the figures above are what that reading would produce.)*

**The reading is nonetheless rejected, on a distinction that must be stated because the same pass
ruled the other way on a structurally identical clause.** `message-flow-device-dcmd-subscribe` is
also conditional — *"If the Device supports writing to outputs"* — and it is `n/a` below. The
difference is **what the absent capability is a fact about**:

- **DCMD's antecedent is a fact about the meter.** A smart-me meter has no writable output. Nothing
  we build changes that, and there is no datum a DCMD could address. Same shape as NDATA.
- **The Primary-Host antecedent is a fact about our software.** The bridge *has* a session whose
  behaviour could depend on a host; we simply never built the option. And the host is not
  hypothetical — the broker this bridge publishes to carries live `spBv1.0/STATE` topics today.

Calling the second `n/a` would let the STATE blind spot — a whole mechanism nobody had considered
until this epic — disappear into the same column as the MQTT-Server profiles. AC1 of Story 4.3
requires these to appear as gaps pointing at Stories 4.4–4.8, and they do.

**A reviewer wanting to overturn this should attack the distinction above, not the verdict.** If
"absent capability" is one category rather than two, the five move and the totals move with them.

### Data behaviour and report-by-exception

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `operational-behavior-data-publish-nbirth` | MUST | the NBIRTH declares `Contract/Version` and `Node Control/Rebirth` (Story 4.7), which with the `bdSeq` the crate prepends are the only node-level metrics the bridge ever publishes — **no NDATA is emitted anywhere** (`MessageType::NData` appears in the bridge only inside a test loop, `mqtt_driver.rs:1391`) | `the_node_birth_publishes_the_contract_version` | conformant |
| `operational-behavior-data-publish-nbirth-values` | MUST | `Contract/Version` carries its current value, an `Int64` compile-time constant | `the_node_birth_publishes_the_contract_version` asserts the value on the wire | conformant |
| `operational-behavior-data-publish-nbirth-change` | SHOULD | NDATA is never published | — | n/a — consistent with chapter 6's NDATA block and its stated criterion |
| `operational-behavior-data-publish-nbirth-order` | MUST | the NBIRTH carries **three** metrics — `build_birth` prepends `bdSeq` before `Contract/Version` and `Node Control/Rebirth` (`encode.rs:180-182`) — both stamped with the same `timestamp_ms`, so the list is chronologically ordered | — nothing asserts the ordering. **This clause is live today, not latent**: two metrics was already enough to be out of order, and Story 4.7 made it three | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `operational-behavior-data-publish-dbirth` | MUST | DBIRTH declares `Power` and `Energy`; `metrics_for` publishes exactly those two on DDATA (`sparkplug_publisher.rs:472`) | `cold_start_birth_declares_tags_with_no_value_and_stale_quality` and `a_good_reading_carries_units_serial_and_the_source_timestamp` both locate metrics **by name**, so a divergence panics | conformant |
| `operational-behavior-data-publish-dbirth-values` | MUST | cold start publishes `MetricValue::Null(Double)` → `is_null: Some(true)` with **no value field**; a re-declared device carries its last known value | `cold_start_birth_declares_tags_with_no_value_and_stale_quality` asserts `value == None`, `is_null == Some(true)` and the surviving datatype; `a_null_metric_carries_no_value_but_keeps_its_datatype` asserts it across the wire | conformant — with one bound, below |
| `operational-behavior-data-publish-dbirth-change` | SHOULD | **DDATA is published on every tick, changed or not** — the operational twin of `principles-rbe-recommended` | `poll_publish.rs:143`; no change detection exists in the tree | **deviation** ([#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)) — ruled on under chapter 2, not re-decided here |
| `operational-behavior-data-publish-dbirth-order` | MUST | `metrics_for` stamps `Power` and `Energy` with the **same** timestamp (`sparkplug_publisher.rs:472`), so the list is chronologically ordered | — **mutation-tested: stamping `Energy` 60 s *earlier* than `Power` — an outright violation — leaves all 69 tests green.** Nothing in the tree observes metric ordering at all | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `operational-behavior-data-commands-rebirth-name` | MUST | every NBIRTH carries a metric named exactly `Node Control/Rebirth` — `rebirth_metric` (`sparkplug_publisher.rs:419`), built into `node_metrics` (`:387`) which BOTH session arms of `birth()` use, so the first birth and every rebirth carry it | `every_node_birth_declares_the_rebirth_command` decodes both the `Session::Pending` and the `Session::Live` NBIRTH and locates the metric by name; `chaos_ncmd_rebirth` reads it off the payload an independent subscriber received | conformant |
| `operational-behavior-data-commands-rebirth-datatype` | MUST | `MetricValue::Boolean(false)` encodes as `DataType::Boolean` = code 11; the variant determines the wire datatype, so the value cannot be tagged with a type it does not have | `every_node_birth_declares_the_rebirth_command` asserts `datatype == Some(11)` on the decoded payload, not on the builder | conformant |
| `operational-behavior-data-commands-rebirth-value` | MUST | `Boolean(false)`, deliberately **not** `Null(DataType::Boolean)`: a null metric declares a type and carries no value, which would satisfy `-rebirth-datatype` and fail this clause | same test asserts `value == Some(BooleanValue(false))` and `is_null != Some(true)`; **mutation-tested** — `Null(Boolean)` and `Boolean(true)` each turn it red | conformant |
| `operational-behavior-data-commands-rebirth-name-aliases` | MUST NOT | conditional on aliases being used; `encode_metric` hard-codes `alias: None` (`encode.rs:241`), so the feature is unreachable. **Story 4.7 did not change this**: the metric now exists and still carries no alias | `every_node_birth_declares_the_rebirth_command` asserts `alias == None`, mutation-tested by making `encode_metric` emit `Some(1)`. That assertion guards the construction; it does not make the clause applicable | n/a — the condition never holds; consistent with chapter 6's three alias clauses |
| `operational-behavior-data-commands-rebirth-action-1` | MUST | satisfied **by shape**: the answer is inline and synchronous in the command arm (`mqtt_driver.rs:1033-1039`), and `select!` runs one branch to completion, so the `inbox` branch cannot publish DATA between the request and the last DBIRTH | `chaos_ncmd_rebirth` asserts on an independent subscriber's transcript that no `/DDATA/` appears between the NCMD and the last DBIRTH — **over a DATA stream it first proves is flowing**, so the window is not empty. **Mutation-tested:** deferring the answer behind a flag consumed one message later puts exactly 1 DDATA inside the window and turns it red | conformant |
| `operational-behavior-data-commands-rebirth-action-2` | MUST | the trigger now exists: `announce` (`mqtt_driver.rs:1256`) publishes NBIRTH + one DBIRTH per meter, and is called from BOTH the CONNACK arm and the rebirth arm — one code path, so the answer cannot drift from the connect birth | `chaos_ncmd_rebirth` asserts the complete sequence off the wire, with the NBIRTH at `seq = 0` and the DBIRTH at `seq = 1`; `a_rebirth_redeclares_what_is_known_instead_of_blanking_it` and `prop_rebirth_always_restarts_numbering_at_zero` still pin the encoder | conformant |
| `operational-behavior-data-commands-rebirth-action-3` | MUST | `new_session()` is **not** called on this path — it advances `bdSeq`, which the clause forbids here. Nothing calls it anywhere today | `chaos_ncmd_rebirth` compares the `bdSeq` in the answering NBIRTH with the one in the FIRST NBIRTH, both read from the transcript — not one against a constant, which is the shape of the Epic 1 tautology. **The clause is birth-versus-WILL, and this is birth-versus-birth**; the missing link is supplied by `the_will_matches_the_session_before_and_after_the_birth`, which pins the will's `bdSeq` to the birth's. Named here because the Story 4.7 code review found the chain resting on an assumption this cell did not state. **Mutation-tested:** inserting `publisher.new_session()` before the answer turns it red (bdSeq 1 → 2). Asserted through the NCMD path, which is what the clause describes | conformant |
| `operational-behavior-data-commands-ncmd-rebirth-verb` | MUST | a Rebirth **Request** is published by a Host Application | — | n/a |
| `operational-behavior-data-commands-ncmd-rebirth-name` | MUST | as above | — | n/a |
| `operational-behavior-data-commands-ncmd-rebirth-value` | MUST | as above | — | n/a |
| `operational-behavior-data-commands-ncmd-verb` | MUST | an NCMD is published by a Host Application | — | n/a |
| `operational-behavior-data-commands-ncmd-metric-name` | SHOULD | as above | — | n/a |
| `operational-behavior-data-commands-ncmd-metric-value` | MUST | as above | — | n/a |
| `operational-behavior-data-commands-dcmd-verb` | MUST | a DCMD is published by a Host Application | — | n/a |
| `operational-behavior-data-commands-dcmd-metric-name` | SHOULD | as above | — | n/a |
| `operational-behavior-data-commands-dcmd-metric-value` | MUST | as above | — | n/a |

**Three rows about the same one-NBIRTH payload carry three different verdicts, and the rule that
selects among them was implicit until the code review of Story 4.3 asked for it.**
`-data-publish-nbirth` is `conformant`, `-nbirth-change` is `n/a`, `-nbirth-order` is
`gap (unproven)`. The rule is two questions in order:

1. **Does the clause bind a message the bridge actually emits?** If not, `n/a`. `-nbirth-change`
   governs NDATA, which the bridge never publishes — so it governs no behaviour of ours, exactly as
   chapter 6's NDATA block reasons.
2. **If it does bind, is the behaviour proven?** `-data-publish-nbirth` is proven (the NBIRTH
   declares the one node metric a test reads by name). `-nbirth-order` is not (nothing asserts
   ordering, and a mutation showed a mis-ordered payload passes) — so `gap (unproven)`.

Vacuity is not the criterion and never was; *which message the clause binds* is. Stating it here
because three adjacent rows disagreeing looks arbitrary until the rule is written down.

**The nine command clauses are `n/a` for the reason chapter 6's nine are**, and the two chapters must
not disagree about one obligation: *"NCMD messages are used by Host Applications to write to Edge
Node outputs"* (`Sparkplug_6_Payloads.adoc:1411`). An Edge Node never publishes either verb in any
configuration. What we genuinely fail — subscribing, and answering a Rebirth — is recorded above and
in the six `-rebirth-*` rows, not hidden here.

**`-rebirth-action-2` and `-action-3` were gaps whose *behaviour* was already correct, and that
prediction held.** `SparkplugPublisher::birth` already re-emitted the complete NBIRTH + DBIRTH
sequence under an unchanged `bdSeq`, and two tests pinned it; what was missing was only the
**trigger**. Story 4.6 supplied half of it (the subscription) and Story 4.7 the other half (the
handler), and Story 4.7's work was indeed a caller and not an encoder — the published crate needed
no change at all. The same shape as DDEATH in chapter 6, where the crate is conformant and the
bridge never calls it.

**One thing the scoping got wrong, recorded because it is the interesting half.** This paragraph
implied Story 4.7 was *only* a caller. It was not: the NBIRTH carried no `Node Control/Rebirth`
metric, so five MUST clauses across three chapters were unmet and a conformant host had no declared
endpoint to address — the handler would have been unreachable by the very hosts it was for. The
matrix recorded those five as `gap (unimplemented)` all along; what it did not do was connect them
to this sentence. A clause-by-clause audit and a prose summary can disagree, and here they did.

**The two ordering rows were wrong twice before they were right, and both corrections came from
outside the author's reading.**

The first draft called them vacuous — nothing that *could* be out of order. **A mutation refuted
that**: stamping `Energy` 60 seconds earlier than `Power` in `metrics_for` produces a payload whose
metrics are genuinely out of chronological order, and **all 69 tests pass**. So these are not
clauses satisfied by construction; they are clauses satisfied by habit, with nothing to stop the
habit changing.

The second draft then claimed `-nbirth-order` was "the weaker of the two — the NBIRTH really does
carry one metric". **The code review of Story 4.3 refuted that too, and it is the more embarrassing
error**: the NBIRTH carries **two** metrics, because `build_birth` prepends the `bdSeq` metric ahead
of whatever the caller passes (`encode.rs:180-182`), which
`birth_carries_seq_zero_and_the_session_number` asserts by reading `metrics[0]`. So both ordering
clauses are live **today**, over real multi-metric payloads, and neither is latent. The claim
survived a full pass because "the node publishes one metric" was true of the *caller* and the row
never looked at the encoder.

**The falsification condition, kept because the rows are still only satisfied by habit:** any change
that gives a payload's metrics differing timestamps turns an unasserted ordering into a violation
nothing would report. [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30) should assert the
ordering directly on both message types.

**The bound on `-dbirth-values`.** A *re-declaring* DBIRTH after a reconnect publishes the device's
**last known** reading rather than a freshly acquired one, degraded to `Stale`
(`sparkplug_publisher.rs:259-334`). That is the honest reading of "the current value" for a bridge
whose meter may be unreachable — the alternative, blanking the tag, discards information the bridge
still holds. The *timestamp* of that same payload is a recorded deviation
([ADR 0013](adr/0013-payload-timestamp-is-acquisition-time.md),
[#29](https://github.com/guycorbaz/smartme_mqtt/issues/29)); the *value* is not.

### Device lifecycle

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `message-flow-device-birth-publish-nbirth-wait` | MUST | `birth()` emits the NBIRTH and every DBIRTH in one call, NBIRTH first (`sparkplug_publisher.rs:259-334`) | `cold_start_birth_declares_tags_with_no_value_and_stale_quality` pins index 0 = NBIRTH, index 1 = DBIRTH; `chaos_sigterm_no_lie` waits for the NBIRTH **then** the DBIRTH on a real broker | conformant |
| `message-flow-device-birth-publish-dbirth-topic` | MUST | `spBv1.0/{group}/DBIRTH/{node}/{device}` | `device_topics_append_the_device_identifier`; the literal `spBv1.0/Site/DBIRTH/Bridge/30000001` is asserted | conformant |
| `message-flow-device-birth-publish-dbirth-match-edge-node-topic` | MUST | both topics are built from the one `EdgeNode` the publisher holds (`sparkplug_publisher.rs:193`) | `cold_start_birth_declares_tags_with_no_value_and_stale_quality` asserts **both literals in one test**, so a divergence in group or node fails it | conformant |
| `message-flow-device-birth-publish-dbirth-payload` | MUST | protobuf | `a_birth_is_self_describing`, `prop_every_numbered_payload_round_trips` | conformant |
| `message-flow-device-birth-publish-dbirth-qos` | MUST (QoS 0) | QoS 0 | `the_delivery_table_matches_the_specification_clause_by_clause` | conformant |
| `message-flow-device-birth-publish-dbirth-retained` | MUST (false) | retain false | same, plus `chaos_sigterm_no_lie`'s late-subscriber check — and the DBIRTH is certainly published in that run, because the test waited for it | conformant |
| `message-flow-device-birth-publish-dbirth-payload-seq` | MUST | device messages draw from the node's single counter, one more than the previous message | `device_messages_share_the_edge_node_numbering`, `sequence_numbering_is_continuous_across_node_and_device_messages`, `prop_published_messages_wrap_255_to_0` | conformant |
| `message-flow-device-dcmd-subscribe` | MUST (QoS 1) | conditional — *"**If the Device supports writing to outputs**, the MQTT client associated with the Device MUST subscribe…"* (`:403-407`). The bridge declares `Power` and `Energy`, both read-only measurements; no writable output exists on any device | — | n/a — see the criterion below. **⏳ TIME-LIMITED, recorded at Story 4.7:** the stated condition is scheduled to start holding. A **meter relay command** is planned for the pre-production Ignition run, and a relay is exactly *"writing to outputs"* on a Device. When it lands, this verdict expires and the clause becomes live. **Not re-verdicted here** — the condition does not hold today, and pre-dating a verdict is as wrong as missing one. [#38](https://github.com/guycorbaz/smartme_mqtt/issues/38) owns the expiry |
| `operational-behavior-device-ddeath` | MUST | **The DDEATH is now emitted — but not for this clause's reason, and the distinction is the verdict.** Story 5.2 publishes one when an operator *disables* a meter. The clause triggers on the Edge Node *losing connection* with a Device, and that case still degrades the reading's quality to `Bad_Stale` instead of burying the device (ADR 0012, the two-mechanism design) | mechanism proven end to end on a real broker by `chaos_device_certificates`; crate side by `device_messages_share_the_edge_node_numbering` and `a_device_death_carries_no_bdseq` | **gap (unimplemented)** — **narrowed, not closed, 2026-08-04.** What was missing is now half present: the message exists, the trigger does not. Recorded rather than flipped, because a row that read `conformant` here would claim the bridge buries an unreachable meter, and it does not |

**`-dcmd-subscribe` is `n/a` while its NCMD twin is a `gap`, and the two verdicts rest on the
criterion this matrix already adopted** under chapter 6's NDATA section: *does the bridge hold the
datum or the event that this message type exists to carry?*

- **DCMD — no.** The clause's antecedent is a capability, and it is genuinely absent: there is no
  writable output on any device, so nothing a DCMD could address exists. Same shape as NDATA.
- **NCMD — yes.** The subscription is not conditional at all, and the specification says why:
  *"This subscription is mandatory as Edge Nodes MUST be able to respond to 'rebirth requests'"*
  (`:163`). Every Edge Node has a session that can be reborn, including this one.

**And the falsification condition:** the moment any device declares a writable metric,
`-dcmd-subscribe` becomes `gap (unimplemented)`, and the subscription must be **QoS 1**. The owner is
no longer Story 4.6: that story landed and added an `NCmd` variant and no `DCmd` one, on the grounds
that an unused variant invites a subscription nothing needs. A future writable output is a new
story, not an unfinished part of this one.

**One inconsistency this pass found and deliberately did not fix.** Chapter 4 records
`topics-dcmd-topic` as `gap (unimplemented)` (now Story 4.19) on the grounds that *"a subscriber must
build this topic form too"*. Under the criterion above that row should be `n/a` for the same reason
as the row here. Re-deciding chapter-4 rows is outside Story 4.3's scope, so it is recorded in the
findings for **Story 4.19** rather than changed here — but a reader comparing the two chapters
should know the difference is known, not overlooked.

### Host Application and Primary Host

**33 clauses in this chapter, and 30 of them are `n/a`** (a 34th, `components-ph-state`, states the
same subject in chapter 3 and is ruled on there). That is the largest dismissible block in this
matrix and therefore the one most at risk of being waved through, so each was read for **which role
it binds** rather than classified by prefix. The three that are not `n/a` are the substance of
Stories 4.4–4.5.

The 30 `n/a` clauses, listed **by id** so the set can be diffed rather than trusted:

**Host Application session establishment and STATE (7)** — `message-flow-phid-sparkplug-clean-session-311`,
`message-flow-phid-sparkplug-clean-session-50`, `message-flow-phid-sparkplug-subscription`,
`message-flow-phid-sparkplug-state-publish`, `message-flow-phid-sparkplug-state-publish-payload`,
`message-flow-phid-sparkplug-state-publish-payload-timestamp`,
`message-flow-hid-sparkplug-state-message-delivered`.

**Host Application connect, birth, death and multi-server bookkeeping (18)** —
`operational-behavior-host-application-host-id`,
`operational-behavior-host-application-connect-will`,
`operational-behavior-host-application-connect-will-topic`,
`operational-behavior-host-application-connect-will-payload`,
`operational-behavior-host-application-connect-will-qos`,
`operational-behavior-host-application-connect-will-retained`,
`operational-behavior-host-application-connect-birth`,
`operational-behavior-host-application-connect-birth-topic`,
`operational-behavior-host-application-connect-birth-payload`,
`operational-behavior-host-application-connect-birth-qos`,
`operational-behavior-host-application-connect-birth-retained`,
`operational-behavior-host-application-multi-server-timestamp`,
`operational-behavior-host-application-termination`,
`operational-behavior-host-application-death-topic`,
`operational-behavior-host-application-death-payload`,
`operational-behavior-host-application-death-qos`,
`operational-behavior-host-application-death-retained`,
`operational-behavior-host-application-disconnect-intentional`.

**Host Application message reordering (4)** — `operational-behavior-host-reordering-param`,
`operational-behavior-host-reordering-start`, `operational-behavior-host-reordering-rebirth`,
`operational-behavior-host-reordering-success`.

**The 18-clause block was dismissed by a bucket name, and the repair turned up something worth
knowing — flagged at the code review of Story 4.3.** They sit at
`Sparkplug_5_Operational_Behavior.adoc:753-796` (host id, will, birth, multi-server) and `:801-819`
(termination, death, disconnect). **Only 12 of the 18 name a Host Application in their own
sentence**; the other six inherit the subject from the section lead-in — *"Sparkplug Host
Applications must follow the following rules when connecting to the MQTT Server"* (`:751`) and the
*Host Application Session Termination* heading (`:798-799`). Read in isolation,
`-connect-will-qos` says only *"The MQTT Will Message's MQTT QoS MUST be 1"*, which an Edge Node
auditor could easily mistake for its own obligation.

**What settles them is their content, not their heading, and the content is unmistakable.** These
six require a **JSON UTF-8** payload carrying `online` and `timestamp` keys, a will at **QoS 1 with
retain true**, on `spBv1.0/STATE/sparkplug_host_id`. The Edge Node's death certificate is
**protobuf**, at QoS 0, retain false, on `spBv1.0/{group}/NDEATH/{node}` — every attribute is the
opposite. No Edge Node could satisfy these clauses while remaining an Edge Node.

That is a firmer footing than the first draft had, and the pass's own headline discovery is why it
was worth re-doing:
**prefix does not determine which role a clause binds** — six `operational-behavior-edge-node-*` ids
bind Hosts, and the four `operational-behavior-primary-*` ids split three-to-one. A block argued by
heading in the one family where that is demonstrably unreliable is a weak link even when its answer
is correct. The line references above are the minimum repair; a future pass should quote per clause.

**Primary Host STATE publication (1)** —
`operational-behavior-primary-application-state-with-multiple-servers-state`: *"every time a
Primary Host Application establishes a new MQTT Session … the STATE Birth Certificate … MUST be the
first message that is published"* (`:591-595`). It binds the Host, and only the Host.

**One of the reordering clauses is worth naming rather than dismissing.**
`-reordering-rebirth` requires a Host Application to send a `Node Control/Rebirth` NCMD when a
message goes missing. It is `n/a` because we are not that Host — but it is the clearest statement in
the specification of *why* the bridge must answer a Rebirth, and it is the operational cost of the
gap recorded at `-rebirth-action-1/2/3`: a real host on this broker will send Rebirth requests, and
today the bridge ignores every one.

The three clauses that bind **us**:

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `operational-behavior-primary-application-state-with-multiple-servers-state-subs` | MUST | the clause binds both sides — *"all Edge Nodes configured with a Primary Host Application MUST subscribe to this STATE message"* (`:586-589`). Since Story 4.6 the bridge does hold a subscription — to its own NCMD topic, and to that alone. It subscribes to no STATE topic, so this clause is unmet for the same reason as before, but the reason must now be stated precisely rather than as a blanket absence | — | **gap (unimplemented)** · **DECIDED, not pending: [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism** (Story 4.5); verdict word under review in [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) · [4.4 measured](primary-host-state-observation.md#the-eleven-clauses-ruled): relevance *relevant* — the host half is satisfied, the edge-node half is not |
| `operational-behavior-primary-application-state-with-multiple-servers-walk` | MUST | *"the Edge Node MUST terminate its session with this MQTT Server and move to the next available MQTT Server"* (`:610-613`) — there is no server list and no STATE to trigger it | — | **gap (unimplemented)** (Story 4.5) · **DECIDED, not pending: [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism** (Story 4.5); verdict word under review in [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) · [4.4 measured](primary-host-state-observation.md#the-eleven-clauses-ruled): relevance *irrelevant* — one broker |
| `operational-behavior-primary-application-state-with-multiple-servers-single-server` | MUST NOT | *"The Edge Nodes MUST not connected to more than one server at any point in time"* (`:600-601`, typo the specification's). `MqttConfig` holds one `host` and one `port` and the driver builds one `AsyncClient` | — **correct by our own code shape, and nothing asserts it**; the guarantee dissolves the day Story 4.5 adds a server list, which is exactly when it would matter | **gap (unproven)** ([#35](https://github.com/guycorbaz/smartme_mqtt/issues/35), Story 4.10) |

**Why `-single-server` is not `conformant`.** A second concurrent session is unreachable today, which
is tempting to score as a type invariant like `SeqCounter`'s `u8`. It is not one.
[ADR 0014](adr/0014-schema-as-conformance-evidence.md) admits a witness only where the violation is
*unrepresentable at compile time*; here it is merely unwritten, and the matrix has already refused
this exact argument for the property-set array lengths. Recording it as unproven is what makes
Story 4.5's server-walking arrive with the obligation attached.

**Its owner is Story 4.10 and its trigger is Story 4.5, which is not the same thing.** 4.10 owns the
CONNECT packet and the reconnect loop, so the assertion belongs in its work; 4.5 is the change that
would break the guarantee. On the reasoning above, **4.10 must land before or with 4.5** — otherwise
the guard arrives after the change it guards. Recorded here because the code review of Story 4.3
noticed the rationale and the owner naming different stories with nothing tying them together.

### Case sensitivity

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `case-sensitivity-sparkplug-ids` | SHOULD NOT | nothing checks that the configured group, node and device ids do not collide once lower-cased | — | **gap (unimplemented)** ([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)) |
| `case-sensitivity-metric-names` | SHOULD NOT | the metric names are four constants — `Power`, `Energy`, `Contract/Version` and, since Story 4.7, `Node Control/Rebirth` — whose lower-cased forms are all distinct. *(The count was three until Story 4.7 and the row was not revisited; the Story 4.7 code review re-ran the check rather than assuming the verdict survived. `node control/rebirth` collides with none of the other three, so the verdict is unchanged — but it was unchecked, and an unchecked verdict on a live SHOULD NOT is the thing this column exists to prevent.)* | — **mutation-tested: renaming `Energy` to `POWER`, so the DBIRTH carries `Power` and `POWER`, leaves all 69 tests green** | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |

**`-sparkplug-ids` is a stricter form of the uniqueness requirement [#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)
already carries** — case-folded collision rather than exact collision — and it points there rather
than opening a second issue for one subject. #27's scope needs widening to say so.

---

## Chapter 6 — Payloads, metrics and datatypes

**The clause set is 109 ids, and that number was established mechanically, not by reading:**

```bash
grep -oE 'tck-id-[A-Za-z0-9-]+' docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_6_Payloads.adoc \
  | sed 's/-$//' | sort -u          # -> 109
```

The chapter boundary was verified rather than assumed: `grep -rl 'tck-id-payloads-'` over
`docs/spec/sparkplug-b-3.0.0/chapters/` returns **only** `Sparkplug_6_Payloads.adoc`, and the same
pattern over the whole pinned tree yields the same 109 ids. No `payloads-*` clause hides in
another chapter, and chapter 6 holds nothing else. The regex is case-inclusive on purpose: two ids
carry uppercase (`…-timestamp-in-UTC`, `…-metric-timestamp-in-UTC`) and a lowercase-only pattern
truncates them.

Every one of the 109 appears below, either as its own row or inside a collective block that
**names its member ids**. The arithmetic is stated at the end of the chapter and it closes.

### Two editorial defects in the specification itself

Recorded here because both distort a mechanical count, and a future re-run against v3.0.1 will hit
them again.

1. **`tck-id-payloads-sequence-num-req-nbirth` and `tck-id-payloads-sequence-num-zero-nbirth` are
   one clause with two spellings.** At `Sparkplug_6_Payloads.adoc:426` the AsciiDoc anchor reads
   `…-req-nbirth` while the rendered identifier in the same line reads `…-zero-nbirth`. A grep
   finds both, so the mechanical count of 109 contains one phantom: there are **108 distinct
   requirements**. Both ids get a row below, pointing at the same clause text, because a TCK could
   legitimately cite either.
2. **`tck-id-payloads-name-birth-data-requirement` and `-name-cmd-requirement` are timestamp
   clauses, not naming clauses.** Both hang off the metric `timestamp` bullet (`:475`, `:477`) and
   their text is about the timestamp; only their ids say `name`. They are filed below under
   **Timestamps**, where their content belongs, not under metric identity where their ids suggest.
   Filing them by id would have produced two rows whose "Our behaviour" column discussed the wrong
   field entirely.

### Metric identity — naming and datatype

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-name-requirement` | MUST | `encode_metric` always sets `name` (`encode.rs:240`) | `encode.rs::a_birth_is_self_describing`, `sparkplug_publisher.rs::cold_start_birth_declares_tags_with_no_value_and_stale_quality` (both locate metrics *by name*; drop the field and they panic) | conformant |
| `payloads-metric-datatype-value-type` | MUST | `datatype` is an unsigned 32-bit integer | the pinned `sparkplug_b.proto` types the field `optional uint32`; `DataType::code` is a `#[repr(u32)]` cast (`datatype.rs:55`) — **schema witness**, plus `datatype.rs::codes_match_the_specification_numbering` | conformant |
| `payloads-metric-datatype-value` | MUST | every `MetricValue` variant maps to an enumerated code; no variant can produce an unlisted one | `model.rs::value_variants_pin_their_datatype`, `a_float_value_is_always_double_never_float32`, `a_null_value_still_declares_its_type`, `datatype.rs::codes_match_the_specification_numbering` | conformant |
| `payloads-metric-datatype-req` | MUST | set on every metric of every BIRTH | `encode.rs::birth_carries_seq_zero_and_the_session_number`, `sparkplug_publisher.rs::cold_start_birth_declares_tags_with_no_value_and_stale_quality` | conformant |
| `payloads-metric-datatype-not-req` | SHOULD NOT | **set on DDATA metrics too** — one encoder serves every message type (`encode.rs:243`) | — | **deviation** ([#28](https://github.com/guycorbaz/smartme_mqtt/issues/28)) |

**`payloads-name-requirement` is conditional, and the condition always fires here.** It reads *"The
name MUST be included with every metric unless aliases are being used"* (`:453`). The bridge never
uses aliases — `encode_metric` hard-codes `alias: None` (`encode.rs:241`), so the feature is not
merely unused but unreachable — therefore the exemption never applies and the MUST always binds.

**On `Contract/Version` as a metric name.** The prose accompanying the clause discourages a list of
special characters that **does not include `/`** (`:456`), and `/` is in fact the specification's
own folder separator (`:448-452`). So the name is legal. But the specification reserves the
`Node Control/…` and `Properties/…` namespaces by convention (`:1116-1151`), and `Contract/Version`
is **our invention in the same shape** — a reader must not infer that the specification blesses it.
It does not mention it.

**On `engUnit`.** `Metric::ENG_UNIT_KEY = "engUnit"` (`model.rs:147`) is a **convention, not a
specification clause**. Chapter 6 defines the PropertySet mechanism and names exactly one key,
`Quality` (`:617`); it says nothing about engineering units. The key is ours, chosen to match common
host practice, and carries no conformance weight in either direction.

**On the datatype enum's gaps.** `DataType` (`datatype.rs:17-51`) omits `0` (Unknown), `16`
(DataSet), `18` (File), `19` (Template), `20-21` (PropertySet types) and `22-34` (arrays). Omission
is not a violation: `-datatype-value` requires that what we *publish* be one of the enumerated
values, not that we be able to publish all of them. The omissions are the scope limit below,
expressed in the type system.

### Property sets

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-propertyset-keys-array-size` | MUST | `encode_properties` pushes key and value together in each branch, and Story 2.1's caller-supplied properties are pushed as pairs from a `Vec<(String, String)>` — a shape in which they cannot diverge | — **correct by construction; no test asserts the invariant**, though one incidentally notices a surplus key — see the mutation note below | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `payloads-propertyset-values-array-size` | MUST | as above — the same invariant stated from the other side; the Story 2.1 property carries its key in the same tuple | — **correct by construction, wholly unproven**: a surplus value passes entirely unnoticed | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `payloads-metric-propertyvalue-type-type` | MUST | property `type` is an unsigned 32-bit integer | pinned `sparkplug_b.proto` types it `optional uint32` — **schema witness** ([ADR 0014](adr/0014-schema-as-conformance-evidence.md)); a non-`u32` here does not fail a test, it fails to compile | conformant |
| `payloads-metric-propertyvalue-type-value` | MUST | we emit `Int32` (3) for quality and `String` (12) for `engUnit`; both enumerated | **neither half is proven** — see the row below; delete the `r#type` line from `string_property` (`encode.rs:300`) and the suite stays green, and the `Int32` half is a tautology | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `payloads-metric-propertyvalue-type-req` | MUST | `int_property` / `string_property` always set `type` (`encode.rs:290-306`) | — **half-witnessed at best**: deleting `r#type` from `int_property` goes red, from `string_property` goes green. The clause says *every* property value, so a witness for one of the two constructors does not prove it. The BIRTH scope (`:593-594`) is bridged only by "one `encode_metric` serves every message type" — the code-shape reasoning this matrix refuses elsewhere | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `payloads-propertyset-quality-value-type` | MUST | property type `Int32` (code 3) | — **the cited assertion is a tautology; see below** | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `payloads-propertyset-quality-value-value` | MUST | **the bridge publishes Ignition's codes, not `0`/`192`/`500`** | `sparkplug_publisher.rs::no_non_good_quality_can_be_mistaken_for_good_by_ignition`, `::the_generic_crate_still_publishes_the_specified_codes` | **deviation** ([ADR 0012](adr/0012-quality-codes-spec-versus-host.md)) |

**`-quality-value-type` was `conformant` until the code review of Story 4.2, and it was the most
dangerous row in the chapter.** The clause names a literal: *"The 'type' of the Property Value MUST
be a value of **3** which represents a Signed 32-bit Integer"* (`:631-632`). Production writes
`r#type: Some(DataType::Int32.code())` (`encode.rs:292`), and the test cited as proof asserted
`assert_eq!(props.values[0].r#type, Some(DataType::Int32.code()))` (`encode.rs:435-437`) — **both
sides of the assertion are the same expression**. `codes_match_the_specification_numbering`
(`datatype.rs:66-74`) pins the literals 1, 4, 8–13 and 17; `Int32` is **not among them**, and no
test anywhere asserts `Int32.code() == 3`. Change `Int32 = 3` (`datatype.rs:23`) to any other
discriminant and the entire suite stays green while the wire violates a MUST.

This is the failure this document exists to catch, found inside the document itself: a quality-code
field agreeing with itself, exactly as contract v1 did. It survived because it was one of the eight
rows written before the chapter was walked, and re-verification meant re-reading it. The contrast
that sharpens it is `payloads-metric-datatype-value` two tables up, which *is* soundly witnessed
because `codes_match_the_specification_numbering` pins a literal for every code the bridge can
publish. The same rigour was available here and was not applied.

**The two array-size rows were mutation-tested, and the result refines them rather than confirming
them.** Both clauses require `keys` and `values` to be the same length (`:570-577`). Three mutations
were applied to `encode_properties` — one of them discarded, and it is recorded here because *how a
mutation can go red for the wrong reason* is the same failure this matrix guards against:

| Mutation | Result | What it means |
| --- | --- | --- |
| Delete `values.push(string_property(unit))` — **discarded** | **red**, but confounded | It also removes the engineering unit, and the failure was an index-out-of-bounds inside the test helper `unit_of`, not a length assertion. A red that proves nothing about the clause. Re-run cleanly as the row below |
| Append an unpaired **key** after the pairs | **red** — `encode.rs::a_birth_is_self_describing` fails | Caught, but *incidentally*: that test asserts `keys == ["Quality", "engUnit"]`, pinning one scenario's exact key list. It does not assert the invariant, and a third legitimate property would fail it just the same |
| Append an unpaired **value** | **green** — all 114 tests pass | Nothing in the tree observes the values array's length at all |

So the `values` side is unwitnessed outright, and the `keys` side is witnessed only by a
coincidence of an unrelated assertion. Both stay `gap` under this matrix's own rule — a
`conformant` must name a test that *proves the clause*, and neither clause has one. The distinction
is recorded because "correct by construction, unproven" was, for the keys side, very slightly
stronger than the evidence supports. [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)
should assert both lengths directly.

**The most consequential row in this matrix, and the values in it were measured, not read.** The
specification admits exactly `0`, `192` and `500` (`:624-636`). The codes the bridge actually
publishes — `192`, `0x8000_0204`, `0x8000_0200` — were established by **publishing six tags with
identical values and differing quality codes to a real Ignition 8.3.7 and reading back what the
host displayed** (`sparkplug-b/tests/ignition_contract.rs::quality_code_probe`, Story 1.15,
[ADR 0012](adr/0012-quality-codes-spec-versus-host.md)). That measurement is the whole point: it
showed Ignition reading `500` as `Good(500)` and `0` as `Good_Unspecified`, so two of the three
mandated codes report an unusable value as trustworthy on the host this bridge publishes to.

Contract v1 shipped the opposite way round — its codes came from an "OPC-style triple" someone had
*read about*, and a live host displayed `Good(500)` for every stale reading while 148 internal tests
agreed with each other. Conforming here produces the exact silent lie the project exists to
prevent, so the bridge deviates and says so.

The generic `sparkplug-b` crate returns to the specified codes; only the bridge deviates, via
`Metric::with_quality_code`. A test asserts each side stays on its own footing, so they cannot
silently converge again.

**One thing the clause does not require.** Quality is *optional*: *"This property is optional and is
only required if the quality of the metric is not GOOD"* (`:617-619`). Publishing it unconditionally
is legal and is a deliberate choice — a consumer should never have to infer good from absence.

### Timestamps — units and interpretation

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-timestamp-in-UTC` | MUST | epoch-milliseconds throughout; `UtcMillis` is UTC by construction (AR15) and every producer is UTC-referenced — `SystemTime::now()` since `UNIX_EPOCH` (`clock.rs:80-85`), the RFC 7231 `Date` header (GMT), and the smart-me `ValueDate` (ISO-8601, mandatory `Z`, `smart-me-client/src/types.rs:39`) | `smart-me-client::http_date::skew_fixtures_parse_to_their_documented_offsets` (a timezone offset introduced anywhere in the conversion moves the parsed value and fails it), `types.rs::rejects_malformed_value_dates` | conformant |
| `payloads-metric-timestamp-in-UTC` | MUST | the metric timestamp is the reading's own `ValueDate`, carried through the same UTC pipeline | same proofs — **and the claim is bounded to the unit**: they prove no offset enters the `ValueDate` → epoch-millis conversion, which is what *"in UTC"* asks. That the field is *populated at all* is a separate obligation, and it is the `gap` on the row below | conformant |
| `payloads-name-birth-data-requirement` | MUST | *(a timestamp clause — see the editorial note above)* `encode_metric` always sets the metric-level `timestamp` (`encode.rs:242`) | — **every timestamp assertion in the tree is payload-level**; nothing reads an encoded metric's `timestamp` field, and the mutation that drops it stays green | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `payloads-name-cmd-requirement` | MAY | *(a timestamp clause)* governs metrics in NCMD/DCMD, which are Host-published | — | n/a — see the NCMD/DCMD note |
| `payloads-nbirth-timestamp` | MUST | NBIRTH is stamped with `clock.wall()`, passed into `announce` at the CONNACK call site (`mqtt_driver.rs:1223`) and at the rebirth one (`:1292`) — the publish instant, as the clause requires | `chaos_no_replay_at_reconnect` bounds the stamp on the wire between two readings of the driver's own clock, one taken before the driver existed and one after the NBIRTH arrived. **Falsified against #30's own words, at the call site the issue named** — replacing both `clock.wall()` arguments with `UtcMillis(42)` goes red with `THE NODE BIRTH DOES NOT SPEAK THE PUBLICATION INSTANT … published somewhere in [1787132581436, 1787132581477] and carries 42`. Story 4.12's `an_hour_of_outage_does_not_move_the_re_declared_reading_forward` asserts the publisher's half — that `birth()` stamps the instant it is handed — and **that alone did not earn this row**: see the note under the tally | conformant |
| `payloads-dbirth-timestamp` | MUST | **cold start conforms** (stamped `now`); **a rebirth re-declaring a known reading is stamped with that reading's `ValueDate`**, and since story 4.12 that choice is routed through `timestamp_source_for(Emission::DeviceBirthRedeclaring)` — a table, not two call sites | `a_rebirth_redeclares_what_is_known_instead_of_blanking_it`; and story 4.12's `an_hour_of_outage_does_not_move_the_re_declared_reading_forward` (unit, an hour of simulated outage) and `chaos_no_replay_at_reconnect` (a real transport break, an independent subscriber). Moving the table row changes what is **emitted**: `left: Some(1784988392050), right: Some(1784984792050)` | **deviation** ([#29](https://github.com/guycorbaz/smartme_mqtt/issues/29)) |
| `payloads-ddata-timestamp` | MUST | **the payload timestamp is the reading's `ValueDate`, not the publish instant.** Enforced by `publish`'s SIGNATURE rather than a branch: it is handed no clock, so the publication instant is unrepresentable there (story 4.12) | `a_good_reading_carries_units_serial_and_the_source_timestamp`, `a_stale_verdict_never_publishes_a_fresh_looking_metric`, and story 4.12's two new tests — the second observing it on the wire, through a reconnect, from an independent subscriber | **deviation** ([#29](https://github.com/guycorbaz/smartme_mqtt/issues/29)) |
| `payloads-ndata-timestamp` | MUST | NDATA is never emitted | — | n/a |
| `payloads-ddeath-timestamp` | MUST | the **publish instant** (`clock.wall()` at `device_death`), which is what the clause asks for — *"the time at which the message was published"*. **Deliberately unlike `payloads-ddata-timestamp`**, where the deviation is to stamp the reading's `ValueDate`: a DDEATH reports an event, not a measurement, so it has no earlier truth to be stamped with | `chaos_device_certificates` (the message reaches a real subscriber); form by `a_device_death_carries_no_bdseq` | conformant |
| `payloads-ncmd-timestamp` | MUST | Host-published | — | n/a |
| `payloads-dcmd-timestamp` | MUST | Host-published | — | n/a |

**The interpretation matters more than the unit, and it is where the bridge parts company with the
specification.** Both `-ddata-timestamp` and `-dbirth-timestamp` say the payload timestamp denotes
*the time at which the message was published*. The bridge stamps DDATA — and a re-declaring DBIRTH —
with **when the values were TRUE**. That is the anti-replay invariant, and it is load-bearing: a
stale reading must read as old *even to a consumer that ignores the quality flag*, which is exactly
the consumer contract v1 proved exists. Stamping `now` would make a 45-minute-old value look fresh
to the reader least equipped to notice.

The specification's own answer is that acquisition time belongs on the **metric** timestamp
(`:481`), which the bridge also sets — so the conformant shape is available and was not taken. This
is an architectural position, not a bug, and it is recorded as one:
[**ADR 0013**](adr/0013-payload-timestamp-is-acquisition-time.md), with
[#29](https://github.com/guycorbaz/smartme_mqtt/issues/29) carrying the work.

**The DEATH timestamp means something else again.** `death_payload` stamps the payload at the moment
it is **built**, not at the moment of death (`encode.rs:168-172`) — the broker does not rewrite a
registered will before publishing it. A consumer must read a DEATH timestamp as *"no later than
this"*. The specification agrees and goes further, telling Host Applications not to use it for
pairing at all (`:1542-1546`); pairing is `bdSeq`'s job. No `tck-id` attaches to this, so it carries
no row.

### Per-message-type clauses

**Two rules applied throughout this section.** First, a chapter-6 clause gets its own row even when
chapter 4 covers the same behaviour — the matrix is keyed by `tck-id`, and `payloads-nbirth-qos` and
`topics-nbirth-mqtt` are two clauses. Second, no verdict here was copied from its chapter-4 twin;
each was read against chapter 6's own wording, which is how `-nbirth-edge-node-descriptor` turned
out to be a restatement rather than a new requirement.

#### NBIRTH

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-nbirth-seq` | MUST | a BIRTH resets the counter and takes `0`; the counter is a `u8`, so 0–255 is a type invariant | `encode.rs::birth_carries_seq_zero_and_the_session_number`, `prop_seq_bdseq.rs::prop_seq_stays_in_range_and_wraps_at_the_boundary`, `::prop_rebirth_always_restarts_numbering_at_zero` | conformant |
| `payloads-nbirth-bdseq` | MUST | `build_birth` prepends the `bdSeq` metric before anything else (`encode.rs:181`) | `birth_carries_seq_zero_and_the_session_number`, `prop_will_birth_and_death_agree_on_bdseq_for_every_session_number`, and `chaos_sigterm_no_lie` reads it off a real broker | conformant |
| `payloads-nbirth-bdseq-repeat` | MUST | the NBIRTH's `bdSeq` matches the registered will's, and **both now change together per CONNECT** — the reason the row passed was the defect, and it is gone (Story 4.10) | `the_will_matches_the_session_before_and_after_the_birth`, `chaos_bd_seq_advances_on_every_connect` | **conformant** (Story 4.10) |
| `payloads-nbirth-edge-node-descriptor` | MUST | nothing verifies that `group_id/edge_node_id` is unique across the infrastructure | — | **gap (unimplemented)** ([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)) |
| `payloads-nbirth-rebirth-req` | MUST | *"**Every** NBIRTH"* — satisfied by building the node metric list once (`sparkplug_publisher.rs:387`) and using it in both session arms, so the clause cannot hold on the first birth and fail on later ones | `every_node_birth_declares_the_rebirth_command` asserts it on the `Session::Pending` **and** the `Session::Live` NBIRTH; **mutation-tested** — omitting it from the `Live` arm alone is red on the second birth only, which a single-birth test would have missed | conformant |
| `payloads-nbirth-qos` | MUST | QoS 0 | `mqtt_driver.rs::the_delivery_table_matches_the_specification_clause_by_clause` | conformant |
| `payloads-nbirth-retain` | MUST | retain false | same, plus `chaos_sigterm_no_lie`'s late-subscriber check — an **external** witness that the broker replays nothing | conformant |

**Why `-nbirth-bdseq-repeat` WAS a deviation despite matching, and what closed it (Story 4.10,
2026-08-01).** The clause reads *"The bdSeq number value MUST match the bdSeq number value that was
sent in the prior MQTT CONNECT packet WILL Message"* (`:1075`), and taken alone it was satisfied. But
until 4.10 it was satisfied *vacuously*: the will was serialised into `MqttOptions` once at
construction and `rumqttc` rebuilt every reconnect's CONNECT packet from that same snapshot, so the
two values agreed because neither could move. The clause's own accompanying requirement — *"any new
CONNECT packet must increment the bdSeq number in the payload compared to what was in the previous
CONNECT packet"* (`:1521-1525`) — was therefore **violated on every internal reconnect**, and a Host
Application could not distinguish a current session from a superseded one. Recording it as
`conformant` then would have been the trap this matrix exists to avoid: right answer, wrong reason.

**The driver now owns its reconnect loop**, so each CONNECT registers a new will carrying the session
number that CONNECT will use, and the match holds because both values move together rather than
because neither moves. The row is `conformant` on the same evidence chapter 5 cites for
`-will-message-payload-bdSeq`, whose prose carries the full account of the fix and of the one thing
4.10 could not verify ([#43](https://github.com/guycorbaz/smartme_mqtt/issues/43)).

#### DBIRTH

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-dbirth-seq` | MUST | device messages draw from the node's single counter | `encode.rs::device_messages_share_the_edge_node_numbering`, `sparkplug_publisher.rs::sequence_numbering_is_continuous_across_node_and_device_messages` | conformant |
| `payloads-dbirth-seq-inc` | MUST | +1 per message, wrapping 255 → 0 (`seq.rs::SeqCounter`, a `u8`) | `seq.rs::seq_wraps_255_to_0`, `prop_seq_bdseq.rs::prop_published_messages_wrap_255_to_0`, `sequence_numbering_is_continuous_across_node_and_device_messages` | conformant |
| `payloads-dbirth-order` | MUST | `birth()` emits the NBIRTH then every DBIRTH in one call, and `publish()` refuses before that (`Published::DroppedBeforeBirth`) | `cold_start_birth_declares_tags_with_no_value_and_stale_quality` (order), `a_drop_before_the_birth_is_reported_not_silent`, and `chaos_sigterm_no_lie` observes NBIRTH-then-DBIRTH on a real broker | conformant |
| `payloads-dbirth-qos` | MUST | QoS 0 | `the_delivery_table_matches_the_specification_clause_by_clause` | conformant |
| `payloads-dbirth-retain` | MUST | retain false | same, plus `chaos_sigterm_no_lie`'s late-subscriber check | conformant |

#### DDATA

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-ddata-seq` | MUST | as DBIRTH — one node-wide counter | `sequence_numbering_is_continuous_across_node_and_device_messages` | conformant |
| `payloads-ddata-seq-inc` | MUST | as DBIRTH | `seq_wraps_255_to_0`, `prop_published_messages_wrap_255_to_0` | conformant |
| `payloads-ddata-order` | MUST NOT (until births) | a reading before the BIRTH is dropped and **reported**; so is one for a device no BIRTH declared | `a_drop_before_the_birth_is_reported_not_silent`, `a_reading_for_an_undeclared_device_is_reported_not_silent` | conformant |
| `payloads-ddata-qos` | MUST | QoS 0 | `the_delivery_table_matches_the_specification_clause_by_clause` | conformant |
| `payloads-ddata-retain` | MUST | retain false | same, plus the external late-subscriber check | conformant |

#### NDATA — n/a

`payloads-ndata-seq`, `payloads-ndata-seq-inc`, `payloads-ndata-order`, `payloads-ndata-qos`,
`payloads-ndata-retain` (and `payloads-ndata-timestamp`, filed under Timestamps).

**n/a — the bridge holds no node-level datum that could ever change**, consistent with chapter 4's
`topics-ndata-*` rows.

**The criterion, because `n/a` and `gap` are one judgement apart here.** DDEATH below is a `gap`
while NDATA is `n/a`, and the two verdicts must not rest on taste. The test is: *does the bridge
hold the datum or the event that this message type exists to carry?*

- **NDATA — no.** The node's metrics are `Contract/Version` and `Node Control/Rebirth`, both constants fixed for the life of the
  session (`sparkplug_publisher.rs:103`, `:112`). NDATA exists to report a *change* to a node-level
  metric; there is nothing here that could change, so the clause governs no behaviour of ours.
- **DDEATH — yes.** A device's death is an event the bridge already detects: meter unreachability
  drives the stale/bad quality verdict today. We hold the event and do not publish the message.

**And the falsification condition, so this row cannot quietly rot**: the moment the node gains a
mutable metric — bridge health, uptime, connection state — these six clauses become
`gap (unimplemented)`. Whoever adds that metric owns the change to this section.

#### DDEATH

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-ddeath-seq` | MUST | `LiveSession::device_death` takes the next sequence number from the node's single counter (`encode.rs:155-163`). **Emitted since Story 5.2** | `device_messages_share_the_edge_node_numbering`, `a_device_death_carries_no_bdseq`; emission proven on a real broker by `chaos_device_certificates` | conformant |
| `payloads-ddeath-seq-inc` | MUST | one greater than the previous message from the edge node — the counter is shared by node and device messages, so there is no second sequence to drift | `sequence_numbering_is_continuous_across_node_and_device_messages` | conformant |
| `payloads-ddeath-seq-number` | MUST | wraps 255 → 0 with the rest | `prop_published_messages_wrap_255_to_0` | conformant |

**Closed 2026-08-04 by Story 5.2, and the reason is worth keeping.** These three read *"gap, not
n/a"* for as long as the bridge emitted no DDEATH — deliberately, on the criterion stated under
NDATA above: the bridge already detected the event and simply did not publish the message, which is
a missing implementation rather than a role we do not play. That reasoning is what made them
closeable at all; an `n/a` would have had to be re-argued from scratch.

**What closed them was not a decision to implement DDEATH.** It was AC4 needing to disable a meter
without lying to the host: a meter switched off cannot provide real-time information, and the norm
requires a DDEATH in exactly that case (`Sparkplug_5_Operational_Behavior.adoc:470`). The message
these rows describe arrived as a consequence.

**Note what did NOT close.** `operational-behavior-device-ddeath` in chapter 5 stays a gap: it
triggers on the Edge Node *losing connection* with a Device, and that case still degrades quality
rather than burying the device. The form of a DDEATH is now conformant; one of the two situations
that should produce one still does not.

Worth recording precisely: the **crate side is already conformant and tested**.
`LiveSession::device_death` takes the next sequence number (`encode.rs:155-163`), asserted by
`device_messages_share_the_edge_node_numbering` and `a_device_death_carries_no_bdseq`. The gap is
entirely in the bridge, which never calls it — so Epic 3's work is a caller, not an encoder.

#### NCMD and DCMD — n/a

`payloads-ncmd-seq`, `payloads-ncmd-qos`, `payloads-ncmd-retain`, `payloads-dcmd-seq`,
`payloads-dcmd-qos`, `payloads-dcmd-retain` (and `payloads-ncmd-timestamp`,
`payloads-dcmd-timestamp`, `payloads-name-cmd-requirement`, filed under Timestamps).

**n/a — these nine clauses bind the *publisher* of an NCMD/DCMD, and that is always a Host
Application.** *"NCMD messages are used by Host Applications to write to Edge Node outputs"*
(`:1411`); *"DCMD messages are used by Host Applications to write to device outputs"* (`:1455`). An
Edge Node never publishes either message, in any configuration, so there is no behaviour of ours for
these clauses to govern.

**The verdict stands; its reasoning is now narrower than it was, and the difference matters.** Story
4.7 made the bridge a *consumer* of NCMD. A clause that binds the sender is still evidence about what
a legitimate message looks like, and one of these nine is now load-bearing on the receiving side:
`payloads-ncmd-retain` — *"NCMD messages MUST be published with the MQTT retain flag set to false"* —
is precisely why the bridge can refuse a retained Rebirth Request without rejecting anything a
conformant host could send ([ADR 0017](adr/0017-a-retained-ncmd-is-a-replay-not-a-request.md),
[#39](https://github.com/guycorbaz/smartme_mqtt/issues/39)). So *"no behaviour of ours for these
clauses to govern"* was true when it was written and is no longer the whole picture: they govern no
behaviour of ours as a publisher, and they inform what we accept as a subscriber. Recorded by the
Story 4.7 code review.

**This was the larger of this pass's two judgement calls, and the code review of Story 4.2 took it
the rest of the way.** A `gap` asserts "we do not do something we should"; applied to
`payloads-ncmd-qos` it would claim the bridge ought one day to *publish* an NCMD, which is false.
The pass originally left chapter 4's `topics-ncmd-mqtt` / `-dcmd-mqtt` as `gap`s, which meant one
obligation carried two verdicts in one document — `Sparkplug_4_Topics.adoc:344` and `:508` are the
same publish-side requirement as the rows above. **Those two chapter-4 rows are now `n/a` as well**;
see the note under chapter 4's device-messages table.

**The command path is no longer unanswered, and what remains is narrower than it was.** Since
Story 4.6 the *subscribe* obligation — `tck-id-message-flow-edge-node-ncmd-subscribe`
(`Sparkplug_5_Operational_Behavior.adoc:158`) — is met and `topics-ncmd-topic` is `conformant`.
Since **Story 4.7** the *answering* is met too: `payloads-nbirth-rebirth-req` and the six
`-rebirth-*` rows are all `conformant`.

What remains on the command side is the DCMD half — `topics-dcmd-topic` and `-device-dcmd-subscribe`
(`:403`) — which is `n/a` **on a stated condition**, *"if the Device supports writing to outputs"*.
No device here does today. **That condition is scheduled to stop holding**: a meter relay command is
planned for the pre-production Ignition run, which is precisely a writable output. The verdicts are
therefore correct now and **time-limited**; they are not re-verdicted here, and
[#38](https://github.com/guycorbaz/smartme_mqtt/issues/38) owns the expiry. An `n/a` whose condition
is about to flip should say so rather than be discovered later — which is the failure mode this
matrix exists to prevent.

The receiving-side obligation these clauses imply — that once Story 4.6 landed the bridge must
*tolerate* an NCMD carrying no `seq` — is the *same* clause read from the other side:
`tck-id-payloads-ncmd-seq` (`:1417-1418`, *"Every NCMD message MUST NOT include a sequence
number"*), the id filed `n/a` above. It was flagged here for Story 4.6 rather than given a second
row, because one clause gets one row.

**Story 4.6 landed and the tolerance holds, for a structural reason rather than a handled case.**
`seq` is `optional` in the pinned proto, so a payload without one decodes; `classify`
(`mqtt_driver.rs:601`) reads only `metrics`, and nothing in the inbound path ever looks at `seq`.
`a_rebirth_request_is_the_name_and_the_value_never_the_name_alone` builds its payloads with
`seq: None` and passes, so the absence is exercised rather than merely permitted. There is nothing
here that could start requiring a `seq` without a deliberate edit.

#### NDEATH

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-ndeath-will-message` | MUST | the will is registered in the CONNECT packet (`mqtt_driver.rs:931`) | `chaos_stale_on_death` — the bridge is **SIGKILLed** and an independent subscriber receives the certificate the broker was holding. An external witness, not a unit test | conformant |
| `payloads-ndeath-will-message-qos` | MUST (QoS 1) | will registered at QoS 1 — `qos_for(MessageType::NDeath)` returns `AtLeastOnce` | `the_delivery_table_matches_the_specification_clause_by_clause` — falsified 2026-08-10 | **conformant** (Story 4.17, closes [#26](https://github.com/guycorbaz/smartme_mqtt/issues/26)) |
| `payloads-ndeath-will-message-retain` | MUST | will retain false, via `register_will` | `the_registered_will_carries_the_qos_and_retain_the_norm_mandates` — observes the `LastWill` actually registered, not the function behind it. **Upgraded from `gap (unproven)` on 2026-08-11**: the row was downgraded because no test reached the will, and the 4.17 review closed that by extracting `register_will` so a test could read the options back. Its QoS sibling had been graded `conformant` on weaker evidence than this row was refused for | **conformant** (Story 4.17 review) |
| `payloads-ndeath-seq` | MUST NOT | `death_payload` sets `seq: None` (`encode.rs:219`) | `encode.rs::the_will_matches_the_birth_and_carries_no_sequence`, `sparkplug_publisher.rs::the_will_matches_the_session_before_and_after_the_birth` | conformant |
| `payloads-ndeath-bdseq` | MUST | the death carries the birth's `bdSeq` | same, plus `prop_will_birth_and_death_agree_on_bdseq_for_every_session_number` and `chaos_stale_on_death` (asserted against a real broker) | conformant |
| `payloads-ndeath-will-message-publisher` | SHOULD | the bridge publishes NDEATH itself before disconnecting (`mqtt_driver.rs:1083`) | `chaos_sigterm_no_lie` — and it proves the *explicit* death rather than the will, because it asserts the death is stamped **later** than the birth, which a CONNECT-time will never can be | conformant — **and it vindicates [ADR 0011](adr/0011-graceful-shutdown-requires-both-deaths.md)**, which reached the same conclusion by reasoning before this clause was read |
| `payloads-ndeath-will-message-publisher-disconnect-mqtt311` | MUST | the bridge speaks MQTT 3.1.1 and **never sends a DISCONNECT packet** — it publishes the NDEATH and drops the socket (ADR 0011) | `chaos_sigterm_no_lie` | conformant — see below |
| `payloads-ndeath-will-message-publisher-disconnect-mqtt50` | MUST | the bridge does not speak MQTT 5.0 | — | n/a |

**The will's retain flag was `conformant` until the code review of Story 4.2, on two witnesses that
do not reach the will.** The registered will is almost certainly retain-false — `qos_for`
(`mqtt_driver.rs:173`) returns `(AtMostOnce, false)` and `:930` feeds it straight into
`MqttOptions::set_last_will`. But *almost certainly* is what this column is not for. There is a
plausible third witness — `chaos_sigterm_no_lie`'s late-subscriber check (`:397-405`) would surface
a retained will, since a retained anything on that topic tree fails it — and it is not enough
either: it only fires if the will is published in that run, which the test neither ensures nor
asserts. Caught-if-we-are-lucky is the same standard the array-size rows were downgraded under.

**A second thing `qos_for` used to cost us — RESOLVED 2026-08-10, recorded because the prediction
came true.** Its parameter was `_message: MessageType`, ignored, so the old test looped six message
types past a function that could not tell them apart: one assertion repeated six times. This
paragraph warned that *"the day `qos_for` grows a real `match`, five of the six retain verdicts
silently revert to unproven with no test change to signal it"*. Story 4.17 gave it a real `match`,
and that is exactly what happened — the will's row was the one that had been wrong all along.

The replacement, `the_delivery_table_matches_the_specification_clause_by_clause`, enumerates each
message type against its own clause instead of asserting one value for all of them. Two reservations
stand, both recorded by the 2026-08-11 review rather than discovered later:

- it asserts on `qos_for`, a pure function, and **not on the `LastWill` the driver builds from it** —
  which is the same reason `payloads-ndeath-will-message-retain` stays `gap (unproven)` below;
- its "chosen" half claimed the norm was silent on NDATA and DDATA. It is not
  (`tck-id-payloads-ndata-qos`, `tck-id-payloads-ddata-qos`, and their `-retain` siblings), and the
  test now files those rows under mandated. Five of the seven message types are constrained by the
  norm; only DDEATH and an explicitly published NDEATH are genuinely ours to choose.

**The MQTT version was verified, not assumed.** `rumqttc = "0.25"` (`Cargo.toml:42`) and the driver
imports `rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS}` — **not**
`rumqttc::v5::*`, which is where that crate puts its MQTT 5 types. So the 3.1.1 clause is the live
one and the 5.0 clause addresses a protocol we do not speak.

**`-disconnect-mqtt311` is conformant on two independent grounds**, and it is worth saying which,
because the row would otherwise look like a vacuous pass. The clause is conditional — *"If the Edge
Node is using MQTT 3.1.1 **and it sends an MQTT DISCONNECT packet**…"* (`:1531`) — and the bridge
never sends one, so the antecedent never fires. Independently, the bridge does the thing the clause
would demand anyway: it publishes the NDEATH before going away, so both the explicit certificate and
the broker's will reach the host. ADR 0011 chose that belt-and-braces shape before this clause was
read.

#### Sequence numbering

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-sequence-num-always-included` | MUST | every message carries `seq`; only DEATH omits it, which is what the clause excepts | `the_will_matches_the_birth_and_carries_no_sequence`, `sequence_numbering_is_continuous_across_node_and_device_messages`, `prop_every_numbered_payload_round_trips` | conformant |
| `payloads-sequence-num-req-nbirth` | MUST | NBIRTH carries `seq = 0`; the counter is a `u8`, so 0–255 cannot be exceeded | `birth_carries_seq_zero_and_the_session_number`, `prop_seq_stays_in_range_and_wraps_at_the_boundary` | conformant |
| `payloads-sequence-num-zero-nbirth` | MUST | *the same clause under its other spelling — see the editorial note above* | same | conformant |
| `payloads-sequence-num-incrementing` | MUST | +1 per message, 255 → 0 | `seq.rs::seq_wraps_255_to_0`, `prop_seq_bdseq.rs::prop_published_messages_wrap_255_to_0`, `::prop_seq_successor_is_modular_from_every_start` | conformant |

**The 0–255 range is a type invariant, not a check.** `SeqCounter` holds a `u8` and advances with
`wrapping_add` (`seq.rs:14-35`); `peek`/`take` widen to `u64` only at the wire boundary. A sequence
number of 256 is unrepresentable rather than merely untested — and the property tests exercise the
wrap from every starting point regardless.

### Scope limit — no aliases, no templates, no DataSets

| Scope decision | Rationale | Verdict |
| --- | --- | --- |
| **The bridge implements no metric aliases, no Templates and no DataSets.** `encode_metric` hard-codes `alias: None` (`encode.rs:241`); `MetricValue` has no `DataSet` or `Template` variant; `DataType` omits codes 16 and 19. | Out of the walking skeleton's scope — recorded at the code review of Story 1-8 (2026-07-25): *"Device-level messages, metric aliases, templates/datasets … out of the walking skeleton's scope"* (`_bmad-output/implementation-artifacts/deferred-work.md`). The features are not merely unused but **unreachable**: there is no API by which a caller could request one. | **deviation** |

**That row is outside the tally, exactly like the prose-only obligations below.** It is a scope
decision, not a `tck-id` row, so it is not one of the 109 and does not appear in the count of four
deviations. A reader counting rendered `deviation` verdicts in this chapter finds five; four is the
number the arithmetic uses, and this is the fifth.

**The 36 clauses this covers are `n/a`, pointing at the row above.** They are listed by id, not by
heading, so the set can be diffed rather than trusted:

**3 alias clauses** — `payloads-alias-uniqueness`, `payloads-alias-birth-requirement`,
`payloads-alias-data-cmd-requirement`.
All three sit under *"Aliases are optional and not required. **If aliases are used, the following
rules apply.**"* (`:461`) — a condition that never holds here.

**7 DataSet clauses** — `payloads-dataset-column-size`, `payloads-dataset-column-num-headers`,
`payloads-dataset-types-def`, `payloads-dataset-types-num`, `payloads-dataset-types-type`,
`payloads-dataset-types-value`, `payloads-dataset-parameter-type-req`.

**26 Template clauses** — `payloads-template-dataset-value`,
`payloads-template-definition-nbirth-only`, `payloads-template-definition-is-definition`,
`payloads-template-definition-ref`, `payloads-template-definition-members`,
`payloads-template-definition-nbirth`, `payloads-template-definition-parameters`,
`payloads-template-definition-parameters-default`, `payloads-template-instance-is-definition`,
`payloads-template-instance-ref`, `payloads-template-instance-members`,
`payloads-template-instance-members-birth`, `payloads-template-instance-members-data`,
`payloads-template-instance-parameters`, `payloads-template-version`,
`payloads-template-ref-definition`, `payloads-template-ref-instance`,
`payloads-template-is-definition`, `payloads-template-is-definition-definition`,
`payloads-template-is-definition-instance`, `payloads-template-parameter-name-required`,
`payloads-template-parameter-name-type`, `payloads-template-parameter-value-type`,
`payloads-template-parameter-type-value`, `payloads-template-parameter-type-req`,
`payloads-template-parameter-value`.

(`payloads-template-dataset-value` is filed by the specification under *DataSet.DataSetValue*
(`:691`) despite its `template-` id. Counted with the Templates because that is where a grep puts
it; noted so a reader tracing it does not conclude the list is wrong.)

**Why one `deviation` row and 36 `n/a`s, rather than 36 `deviation`s.** The scope limit must be an
explicit, named deviation rather than implicit silence — that is what the row above provides. But we
do not *do otherwise* than these 36 clauses: their conditions never fire. Marking each one a
deviation would inflate the deviation count and misstate what a deviation is. The `n/a`s point at
the row so neither reading is available to a hurried reader.

### Host Application STATE clauses — n/a

`payloads-state-will-message`, `payloads-state-will-message-qos`,
`payloads-state-will-message-retain`, `payloads-state-will-message-payload`,
`payloads-state-subscribe`, `payloads-state-birth`, `payloads-state-birth-payload`

**n/a — we are an Edge Node.** These govern what a Host Application publishes on
`spBv1.0/STATE/…`, exactly like chapter 4's `host-topic-phid-*`. Whether the bridge should *observe*
a Host Application's STATE is a different question, unaddressed by these clauses and open as
Stories 4.4–4.5.

### Prose-only obligations — outside the 109

Chapter 6 states three metric-level obligations in prose with **no `tck-testable` identifier
attached**. They are recorded rather than dropped, and they are excluded from the tally because the
tally counts `tck-id` rows: including them would break the arithmetic against the enumerated 109.

`CLAUDE.md` requires citing the `tck-id` rather than prose. Where the specification offers no id,
saying so *is* the citation.

| Field | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `is_null` *(no tck-id — prose, ch. 6 §Metric, `:503-505`, `:513-516`)* | descriptive | a `Bad` reading publishes **no value at all**: `is_null: Some(true)` and the value field omitted, while the datatype survives via `MetricValue::Null(DataType)` (`encode.rs:235-238`) | `encode.rs::a_null_metric_carries_no_value_but_keeps_its_datatype` (asserts across the wire), `sparkplug_publisher.rs::a_bad_verdict_publishes_no_value_at_all` | conformant |
| `is_historical` *(no tck-id — prose, `:493-498`)* | descriptive | hard-coded `None` (`encode.rs:244`) | — | n/a — the bridge never replays buffered data (v1 policy is traced-drop, no buffer; `deferred-work.md`) |
| `is_transient` *(no tck-id — prose, `:499-502`)* | descriptive | hard-coded `None` (`encode.rs:245`) | — | n/a — every metric the bridge publishes is meant to be historised |

**`is_null` is the mechanism behind the project's central promise**, which is why it gets a row
despite having nothing to cite. Publishing `0.0` with a bad quality flag instead would mean every
consumer that ignores the flag — and there is always one — records a real-looking zero.

---

## Chapter 10 — Conformance profiles

Twelve clauses, and this is the chapter that defines what claiming conformance *means*. A blanket
`n/a` turns out to be right — **which is precisely why it is justified per profile rather than waved
through.** `n/a` because we are not an MQTT Server is legitimate; `n/a` because a chapter is awkward
is the dustbin failure this matrix exists to catch.

The specification names four target application types (`Sparkplug_10_Conformance.adoc:25-28`):

| Profile | Section | Clauses | Verdict and why |
| --- | --- | --- | --- |
| **Sparkplug Edge Node** | `:34-39` | **none** | The profile we implement, and it carries **no `tck-id` of its own** — `:35-41` is prose. See the consequence below |
| **Sparkplug Host Application** | `:41-50` | 1 | n/a — we do not play this role |
| **Sparkplug Compliant MQTT Server** | `:52-68` | 4 | n/a — we are an MQTT *client*. These bind the broker, not the bridge |
| **Sparkplug Aware MQTT Server** | `:70-117` | 7 | n/a — as above, and the profile is optional even for a broker |

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `conformance-primary-host` | MUST | *"Sparkplug Host Applications MUST publish 'STATE' messages that represent its Birth and Death Certificates"* (`:49-50`) | — | n/a — third statement of the subject already ruled on at `intro-sparkplug-host-state` and `components-ph-state` |
| `conformance-mqtt-qos0` | MUST | binds a Sparkplug Compliant MQTT Server | — | n/a — but see the deployment note below |
| `conformance-mqtt-qos1` | MUST | as above | — | n/a — see the deployment note |
| `conformance-mqtt-will-messages` | MUST | as above — the server must support wills *including retain and QoS 1* | — | n/a — see the deployment note |
| `conformance-mqtt-retained` | MUST | as above | — | n/a — see the deployment note |
| `conformance-mqtt-aware-basic` | MUST | binds a Sparkplug **Aware** MQTT Server | — | n/a |
| `conformance-mqtt-aware-store` | MUST | as above | — | n/a |
| `conformance-mqtt-aware-nbirth-mqtt-topic` | MUST | as above | — | n/a |
| `conformance-mqtt-aware-nbirth-mqtt-retain` | MUST | as above | — | n/a |
| `conformance-mqtt-aware-dbirth-mqtt-topic` | MUST | as above | — | n/a |
| `conformance-mqtt-aware-dbirth-mqtt-retain` | MUST | as above | — | n/a |
| `conformance-mqtt-aware-ndeath-timestamp` | MAY | as above | — | n/a |

**The finding this chapter yields is the absence, not the twelve `n/a`s: the specification's Edge
Node conformance profile imposes no additional testable clause beyond those already audited.** It
was verified rather than assumed — the section at `:34-39` is descriptive prose and carries no
`tck-testable` marker, and all twelve ids sit under the Host Application profile (1) and the two
MQTT Server profiles (11).

That is a direct and useful input to **NFR19's "documented conformance scope"**: an Edge Node's
conformance claim is exactly the union of the clauses it satisfies in chapters 1–6, with no separate
profile checklist to satisfy on top. The scope of this matrix *is* the scope of the claim.

**The four MQTT-Server clauses are `n/a` for us and an obligation on the deployment.** They are the
specification's requirements on the broker a Sparkplug infrastructure runs against: QoS 0 and 1,
full retain-flag support, and full will support *including QoS 1*. The bridge is deployed against
Mosquitto, which meets them — and since Story 4.17 (2026-08-10) the last one is load-bearing rather
than idle: the bridge now registers its will at QoS 1, as
`message-flow-edge-node-birth-publish-will-message-qos` requires, so a broker without full will
support at QoS 1 would break it. This belongs in the operator manual's deployment prerequisites,
not only here.

> **Story 2.1 (2026-08-10) added a third property key, `Cause`.** It appears on every metric
> whose quality is not good, carrying a `String`, and it is **not** in the `Quality` property —
> `payloads-propertyset-quality-value-value` admits only `0`/`192`/`500` there and the bridge
> already deviates from that clause (ADR 0012); encoding a reason as a fourth value would
> deepen a deviation accepted only because the alternative was a silent lie. No verdict in this
> matrix moves: the property-set clauses constrain key/value array lengths and the `type` field,
> which the encoder satisfies for the new key exactly as for the existing two. `CONTRACT_VERSION`
> moved 3 → 4, and `tests/contract_golden.rs` now fails if the quality codes or the cause
> vocabulary move without it.

## Findings carried forward

| Finding | Chapter | Where |
| --- | --- | --- |
| ~~Will registered at QoS 0; the specification requires QoS 1~~ **CLOSED 2026-08-10 by Story 4.17** | 5, 6 | [#26](https://github.com/guycorbaz/smartme_mqtt/issues/26) |
| ~~The delivery test asserts QoS 0 for all six types and claims the spec requires it uniformly — true for the six published types, false for the will~~ **CLOSED 2026-08-10 by Story 4.17**, which replaced it with `the_delivery_table_matches_the_specification_clause_by_clause` | 4 | [#26](https://github.com/guycorbaz/smartme_mqtt/issues/26) |
| **The replacement test filed two MUSTs under "chosen".** `tck-id-payloads-ndata-qos` and `-ddata-qos` mandate QoS 0 and their `-retain` siblings mandate false; the story's table and the manual both said the norm was silent on them. Behaviour was always correct — the account of it was not. **CLOSED 2026-08-11** by the story's review | 4, 6 | Story 4.17 review |
| ~~**The delivery test asserts on `qos_for`, not on the registered will.** No test observes the QoS or the retain flag actually handed to `set_last_will`, which is why `payloads-ndeath-will-message-retain` stays `gap (unproven)` while its QoS sibling is `conformant` on evidence of the same strength~~ **CLOSED 2026-08-11** by the Story 4.17 review: `register_will` extracted, `the_registered_will_carries_the_qos_and_retain_the_norm_mandates` added and falsified, two rows upgraded | 5, 6 | Story 4.17 review |
| No verification of edge-node-descriptor or device-id uniqueness | 4, 6 | [#27](https://github.com/guycorbaz/smartme_mqtt/issues/27) |
| ~~NCMD/DCMD not implemented — no subscription~~ ~~**NCMD is subscribed (Story 4.6) and every command is ignored on purpose**~~ **CLOSED for NCMD (Story 4.7).** The subscribe clause `message-flow-edge-node-ncmd-subscribe` is `conformant` (4.6) and so is the acting: the NBIRTH declares `Node Control/Rebirth` and a conformant request is answered with a complete birth sequence — seven rows moved. **DCMD remains `n/a` on a condition that is scheduled to stop holding** (a planned meter relay is a writable output): [#38](https://github.com/guycorbaz/smartme_mqtt/issues/38). The publish-side QoS/retain clauses stay `n/a` in both chapters | 4, 5, 6 | 4.6 closed the subscription, **4.7 closed the answering**; DCMD → [#38](https://github.com/guycorbaz/smartme_mqtt/issues/38), [#23](https://github.com/guycorbaz/smartme_mqtt/issues/23) |
| DDEATH never emitted (the crate-side encoder is conformant and tested; the bridge never calls it) | 4, 6 | Epic 3 |
| **`datatype` is sent on every DDATA metric; `-metric-datatype-not-req` says SHOULD NOT.** One encoder serves every message type, so the same line satisfies the BIRTH MUST and violates the DATA SHOULD NOT | 6 | [#28](https://github.com/guycorbaz/smartme_mqtt/issues/28) |
| **The DDATA and re-declaring-DBIRTH payload timestamps are the reading's `ValueDate`, not the publish instant.** Deliberate — the anti-replay invariant — and contrary to two MUSTs. Recorded as [ADR 0013](adr/0013-payload-timestamp-is-acquisition-time.md) | 6 | [#29](https://github.com/guycorbaz/smartme_mqtt/issues/29) |
| **Eight invariants are correct by construction and proven by no test** — raised from four by the code review of Story 4.2: both property-set array-length clauses, the `engUnit` property's `type` field, the metric-level `timestamp` field, and four more the review found — the quality property's `type` (whose test asserted production's own expression against itself), `-propertyvalue-type-req` (witnessed for `int_property` only), the NBIRTH payload timestamp (a presence check, not a value check), and the registered will's retain flag (no test reaches the will) | 6 | [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30) — **its scope needs widening from "encoder invariants" to match** |
| **`Int32` is the one datatype code no test pins to its literal.** `codes_match_the_specification_numbering` covers 1, 4, 8–13, 17; change `Int32 = 3` and the suite stays green while `-quality-value-type` violates a MUST | 6 | [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30) |
| `qos_for` ignores its `MessageType` argument, so the six-type QoS/retain test is one assertion repeated six times. Harmless today; five retain verdicts revert to unproven the day it grows a real `match` | 4, 6 | Story 4.17 |
| ~~**`bdSeq` is fixed for a client's lifetime**, so `-nbirth-bdseq-repeat` passes for the wrong reason and the per-CONNECT increment the clause requires never happens~~ **CLOSED (Story 4.10, 2026-08-01).** The driver owns its reconnect loop; each CONNECT advances the number and registers a will carrying it. **Found while closing it:** no NDEATH reaches a subscriber on the reconnect path at all — [#43](https://github.com/guycorbaz/smartme_mqtt/issues/43) — so the *will* half is verified only on the SIGTERM path | 6 | ~~Story 4.10~~ done |
| Specification editorial: `sequence-num-req-nbirth` / `-zero-nbirth` are one clause with two spellings, so a mechanical count of chapter 6 reads 109 where 108 requirements exist | 6 | recorded above; upstream, not ours |
| Specification editorial: `-name-birth-data-requirement` and `-name-cmd-requirement` are timestamp clauses carrying `name` ids | 6 | recorded above; upstream, not ours |
| **Identifier validation implements Sparkplug's wildcard rule, not MQTT's character set — measured: a `U+0000` passes `check_identifier` and reaches the published topic.** Three chapter-1 clauses defer their character set to a specification this repository keeps no copy of | 1 | [#34](https://github.com/guycorbaz/smartme_mqtt/issues/34) |
| **This repository keeps no copy of the MQTT specification**, so three `-chars` clauses cannot be audited in full against their own norm — only a demonstrated violation of them | 1 | [#34](https://github.com/guycorbaz/smartme_mqtt/issues/34) — start by pinning the MQTT clause |
| **`Clean Session` is true only because rumqttc defaults it that way** (`rumqttc-0.25.1/src/lib.rs:513`); `set_clean_session` is never called and no test asserts the flag. The first `gap (unproven)` whose guarantee comes from *outside* our code — a dependency default has none of the compile-time force [ADR 0014](adr/0014-schema-as-conformance-evidence.md) requires | 2 | [#35](https://github.com/guycorbaz/smartme_mqtt/issues/35), Story 4.10 |
| **Nothing asserts that only one MQTT server is ever connected**; the guarantee is `MqttConfig`'s shape and dissolves the day Story 4.5 adds a server list | 5 | [#35](https://github.com/guycorbaz/smartme_mqtt/issues/35), Story 4.10 |
| **The Primary Host / STATE mechanism is absent end to end** — no *STATE* subscription, no STATE parsing, no birth-wait, no offline-disconnect, no server walk. **Eleven chapter-5 clauses**, and they are `gap` rather than `n/a` because the condition they turn on is a capability the bridge lacks, not a deployment fact. Story 4.6 added an NCMD subscription and no STATE handling whatever, so all eleven stand unchanged. **Story 4.5 closed this as a DECISION rather than a gap** — [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism, and the verdict word itself is [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) | 5 | Stories 4.4, 4.5 — **done** |
| ~~**`Node Control/Rebirth` is unanswerable, and the mechanism already exists.**~~ **CLOSED (Story 4.7).** The handler landed and the prediction held — the published crate needed no change. What the finding got wrong: it said only the *handler* was missing, but the NBIRTH also carried no `Node Control/Rebirth` metric, so five MUST clauses were unmet and no conformant host had an endpoint to address. The matrix scored those five as gaps throughout; this summary did not connect them | 5 | ~~Story 4.7~~ done |
| **The per-CONNECT `bdSeq` increment is stated by chapter 5 in its own clause** (`-will-message-payload-bdSeq`), where chapter 6 could only fold it into `-nbirth-bdseq-repeat`. One defect, one owner, one row per chapter | 5, 6 | Story 4.10 |
| **Two metric-ordering clauses are satisfied only by habit** — the NBIRTH's three metrics and the DBIRTH/DDATA's two share a timestamp because one line gives it to them, and a mutation proved a mis-ordered payload passes every test. Both are live today, neither is latent | 5 | [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30) |
| **Metric names are four constants whose lower-cased forms happen to differ**, and nothing asserts it (`case-sensitivity-metric-names`) | 5 | [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30) — scope needs widening again. *Three until Story 4.7 added `Node Control/Rebirth`; the count was corrected by the 4.7 code review, which also re-ran the collision check rather than assuming the verdict survived* |
| **Case-folded id collision (`case-sensitivity-sparkplug-ids`) is a stricter form of the uniqueness requirement #27 already carries**; #27's scope needs widening to say so rather than a second issue being opened | 5 | [#27](https://github.com/guycorbaz/smartme_mqtt/issues/27) |
| **Chapter 4's `topics-dcmd-topic` is a `gap` where chapter 5's `-device-dcmd-subscribe` is `n/a`** under the matrix's own hold-the-datum criterion. Found by this pass, deliberately not fixed here — re-deciding chapter-4 rows is outside Story 4.3's scope | 4, 5 | **Story 4.19** |
| **The specification's Edge Node conformance profile imposes no additional testable clause** — the profile section carries no `tck-id`, so the conformance claim is exactly the union of chapters 1–6. A direct input to NFR19's documented conformance scope | 10 | recorded above; feeds NFR19 |
| The four Sparkplug Compliant MQTT Server clauses are `n/a` for the bridge and a **deployment prerequisite** on the broker (QoS 0 and 1, full retain, full will support including QoS 1). Belongs in the operator manual | 10 | operator manual |

## Tally for chapter 1

**3 conformant · 0 deviations · 4 gaps · 1 n/a**

`3 + 0 + 4 + 1 = 8` — the enumerated clause set, with no remainder.

The 3 conformant are the `-string` clauses, and all three rest on a **language type invariant**
rather than a test: a Rust `String`/`&str` is UTF-8 by construction, so a non-UTF-8 identifier is
unrepresentable at compile time.

**This was the one place this pass loosened a rule, and it took an adversarial review to notice.**
The draft cited `node_topics_follow_the_namespace_grammar` and
`device_topics_append_the_device_identifier` as proof — topic-grammar tests that exercise nothing
about UTF-8. Two of the three review layers flagged it independently. The witness is sound but it
extended [ADR 0014](adr/0014-schema-as-conformance-evidence.md), which admits only the pinned
protobuf schema and explicitly warns that *"the compiler proves it"* would swallow half the matrix.
It is now ratified as [**ADR 0015**](adr/0015-language-type-invariants-as-conformance-evidence.md)
([#36](https://github.com/guycorbaz/smartme_mqtt/issues/36)) under three conditions — the clause
must be about a **type**, the invariant must **not be ours to change**, and the row must say so
rather than name adjacent tests. Condition two is what keeps `-single-server` a `gap (unproven)`:
`MqttConfig`'s shape is ours, and Story 4.5 will change it.

All 4 gaps are `gap (unimplemented)`: the three `-chars` clauses
([#34](https://github.com/guycorbaz/smartme_mqtt/issues/34), demonstrated by a `U+0000` reaching the
wire) and edge-node-descriptor uniqueness
([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)).

## Tally for chapter 2

**1 conformant · 1 deviation · 1 gap · 1 n/a**

`1 + 1 + 1 + 1 = 4` — four clauses, four outcomes, which is a coincidence rather than a design.

The deviation is RBE ([#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)), blocked on
Story 4.7 and carrying an explicit revisit condition. The gap is `gap (unproven)`: the clean-session
flag ([#35](https://github.com/guycorbaz/smartme_mqtt/issues/35)).

## Tally for chapter 3

**0 conformant · 0 deviations · 0 gaps · 1 n/a**

`0 + 0 + 0 + 1 = 1`. The chapter's single clause binds a Host Application.

## Tally for chapter 4

**This tally was `15 · 0 · 5 · 21` until Story 5.2** (2026-08-04), which made the bridge emit a
DDEATH for the first time: `topics-ddeath-topic` and `topics-ddeath-mqtt` moved from gap to
conformant. **One clause of this chapter is still recorded nowhere** —
`tck-id-topics-ddeath-seq-num` — and it is one of the 29 that Story 4.19 owns. It is named here
rather than added, because adding a single row of another story's 29 would leave `41 of 70` reading
`42` with no account of the rest; but it is now a clause the bridge SATISFIES and does not claim,
which is the less common direction for this document to be wrong in.

**17 conformant · 0 deviations · 3 gaps · 21 n/a** (16 Host Application, 3 messages we do not emit,
2 command clauses that bind a Host Application publisher)

`17 + 0 + 3 + 21 = 41` rows. **This corrects a miscount**: the tally read `17 · 0 · 8 · 21` until the
code review of Story 4.2 recounted the rows mechanically — the conformant and n/a figures were
over-stated, and two of the gaps (`topics-ncmd-mqtt`, `topics-dcmd-mqtt`) then became `n/a`.

**It then read `14 · 0 · 6 · 21` until Story 4.6**, which moved `topics-ncmd-topic` to `conformant`:
the bridge now builds that exact topic form and subscribes to it. This chapter was not in Story 4.6's
scope and the row was not on its list of documents to amend — it was reached by re-running the
story's own grep and following what the change made false.

The 3 gaps are all `gap (unimplemented)`: two uniqueness checks ([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27), Epic 3)
and `topics-dcmd-topic` (Story 4.19 — see the criterion note in chapter 5's device section; it
probably belongs at `n/a`). **The two DDEATH topic rows left this list on 2026-08-04**, when Story
5.2 made the bridge emit one.

**41 rows is not the chapter's clause set.** `Sparkplug_4_Topics.adoc` carries **70** `tck-id`s, so
**29 are recorded nowhere**, and the shape of the 29 explains itself: **26 are `topics-*` clauses
about payload *content*** — `-nbirth-metrics`, `-nbirth-metric-reqs`, `-nbirth-seq-num`,
`-nbirth-timestamp`, `-nbirth-templates`, the three `-nbirth-bdseq-*`, `-nbirth-rebirth-metric`,
their DBIRTH/NDATA/DDATA/NCMD/DCMD/NDEATH/DDEATH counterparts — plus **3
`host-topic-phid-death-payload-timestamp-*`** ids the STATE block omits (the spec carries seven
`-death-payload*` ids; the block lists four).

Story 4.1 audited the chapter's **topic grammar** and left the payload requirements the same chapter
also states. Most pointedly, **`tck-id-topics-nbirth-bdseq-increment` is absent** — chapter 4's own
id for the per-CONNECT `bdSeq` increment, the very deviation chapter 6 records under Story 4.10.

**Story 4.19 owns closing this**, and the Status table says "audited, not complete" for exactly this
reason: it read `done` until the code review of Story 4.2 applied chapter 6's own completeness check
to chapter 4 and found it failed.

Every chapter-4 `conformant` row names a test. No row is asserted from reading the code alone.

## Tally for chapter 5

**32 conformant · 1 deviation · 17 gaps · 49 n/a**

`32 + 1 + 17 + 49 = 99` — the enumerated clause set, with no remainder.

**This tally was `29 · 2 · 19 · 49` until Story 4.10** (2026-08-01), which moved
`-will-message-payload-bdSeq` from `deviation` to `conformant`: `29 + 1 = 30` and `2 − 1 = 1`. The
row's own prose above records why it was a deviation and what closed it.

**And `23 · 2 · 25 · 49` until Story 4.7**, and `22 · 2 · 26 · 49` until Story 4.6.

Story 4.7 moved **six** rows from `gap (unimplemented)` to `conformant`, all in the rebirth
section: the three NBIRTH-metric clauses (`-rebirth-name`, `-rebirth-datatype`, `-rebirth-value`)
and the three action clauses (`-rebirth-action-1`, `-2`, `-3`). `23 + 6 = 29` and `25 − 6 = 19`.

`-rebirth-name-aliases` stayed **n/a** and was checked rather than assumed: the clause is
conditional on aliases being used, this bridge uses none, and adding the metric did not change
that. Promoting it to `conformant` would have claimed compliance with a rule that does not apply.

The Story 4.6 note said `-rebirth-action-1/2/3` were untouched *because that story built the ear
and not the voice*. That is what 4.7 supplies.

**The 2 deviations** — the per-CONNECT `bdSeq` increment
(`-will-message-payload-bdSeq`, Story 4.10) and periodic publishing instead of RBE
(`-data-publish-dbirth-change`, [#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)). Both are
one defect stated twice across chapters, each pointing at the owner its twin already had — not two
new defects.

**The 17 gaps, split by kind:**

- **13 × `gap (unimplemented)`** — **eleven** Primary-Host/STATE clauses (Stories 4.4 and 4.5 — *not*
  4.6, which added an NCMD subscription and no STATE handling whatever; corrected by the Story 4.6
  code review, which found the same eleven clauses assigned three different owner sets across this
  document: the five
  `-phid-*` birth-wait clauses, the three `-termination-host-offline*` clauses,
  `-birth-sequence-wait`, `-state-subs` and `-walk`), the will's QoS
  ([#26](https://github.com/guycorbaz/smartme_mqtt/issues/26), Story 4.17), DDEATH (Epic 3), and
  case-folded id collision ([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)).
- **5 × `gap (unproven)`** — the will's retain flag, the two metric-ordering clauses, metric-name
  case collision (all [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)), and the
  single-server rule ([#35](https://github.com/guycorbaz/smartme_mqtt/issues/35)).

**Every gap carries an owning story, epic or issue**, and the shape of the 20 is the finding: this
chapter's dominant defect is **missing behaviour**, not untested behaviour — the exact inverse of
chapter 6, where the unproven half outnumbered the unimplemented one. Chapter 6 audits what the
bridge *says*; chapter 5 audits what it *does*, and two whole mechanisms — command handling and
Primary Host STATE — were simply absent when this chapter was audited.

**Story 4.6 changed half of one of them, and the half it changed is the smaller half.** The bridge
now *receives* commands: it subscribes at QoS 1 before birthing and traces the metric names of
everything that arrives. It did not yet *act* on any of them, so the six `Node Control/Rebirth` clauses
were exactly where they had been. Command handling was no longer absent; it was inert, which is a
different state and was a deliberate one — the plumbing had to be shown safe before Story 4.7 gave one
command meaning. **Story 4.7 has since landed**, and all six clauses are `conformant`; every command other than `Node Control/Rebirth` is still
ignored, on the same paths. Primary Host STATE is untouched and still absent end to end — **now by decision** ([ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md), Story 4.5), not by omission.

**The 49 n/a, broken down** — 30 Host Application role · 9 Host-published commands · 6 Host
Application reactions to our own death certificates (the `-termination-host-action-*` ids, whose
`edge-node` prefix names the wrong side of the wire) · 2 features we do not use (aliases, NDATA) ·
1 capability no device has (DCMD writable outputs) · 1 permission declined (the DISCONNECT packet).

**`n/a` covers half this chapter, so the dustbin check matters more here than anywhere.** **49 of the
99** were dismissible by prefix alone — 33 in the Host section and the 16 `-data-commands-*` clauses.
Each was read for which role it binds, and the check paid for itself twice: six
`operational-behavior-edge-node-*` ids turned out to bind Host Applications despite their prefix, and
four `operational-behavior-primary-*` ids turned out to split, three binding the Edge Node and only
one the Host. Classifying either family by prefix would have moved three real gaps into `n/a` and put
six irrelevant rows in the edge-node table.

*(The figure read "45 … and 12 command clauses" until the code review of Story 4.3, which could
reconcile it to no set in the document. It was mis-transposed from the story's count over a
different population — chapter 5's 34 host clauses plus chapter 10's 11 MQTT-Server ones.)*

**One reclassification is flagged rather than made.** Chapter 4's `topics-dcmd-topic` is a `gap`
where this chapter's `-device-dcmd-subscribe` is `n/a`; under the criterion both should be `n/a`.
Re-deciding chapter-4 rows is outside this pass's scope, and it is recorded for **Story 4.19**.

Every `conformant` row names a test. No row is asserted from reading the code alone, and where the
witness is weaker than the clause — the will's retain flag, the ordering clauses — the row was
written down as `gap (unproven)` rather than rounded up.

**Three rows here rest on a chaos test, and the chaos tests were not run when this chapter was
written.** `-birth-publish-connect`, `-birth-publish-will-message` and
`-intentional-disconnect-ndeath` name `chaos_sigterm_no_lie` or `chaos_stale_on_death` as their only
proof; several more are upgraded past a weaker in-process witness by a chaos citation. Those tests
need a Docker daemon, and the machine this pass ran on has none — so the citations were established
by **reading the tests' assertions**, not by observing them pass. Two things bound that admission:
the tests do run in CI (`.github/workflows/ci.yml`, `cargo test --workspace` on `ubuntu-latest`), so
the citations are not hollow; and the code review of Story 4.3 read all three and found one
misdescribed — `chaos_stale_on_death` was said to SIGKILL the bridge and in fact aborts a tokio task
in-process. The verdicts survived; the description did not. **A reader should treat chaos-backed
rows here as verified by CI rather than by this pass.**

**Nine mutations were run against the code rather than the conclusions re-read**, on the principle
that an audit agreeing with itself proves nothing. Seven went red and two went green, and the green
pair is the half that matters — a `gap (unproven)` asserted without demonstrating that nothing
notices is just a guess wearing a label.

**The bound on "all 69 tests", stated because it is narrower than it sounds:** 69 is the
`smartme-bridge` **library** suite, not the workspace's 150. The integration tests under
`crates/smartme-bridge/tests/` and the `sparkplug-b` crate's own suites were not re-run per mutation.
For the two green results this errs conservatively — an integration test *could* in principle have
caught them, which would make the row `conformant` rather than `gap (unproven)`, so the risk is an
over-strict verdict and never an over-generous one.

| Mutation | Suite | The row it tests |
| --- | --- | --- |
| `publish()` returns `Emitted` instead of `DroppedBeforeBirth` when not yet born | **red** | `principles-birth-certificates-order` (ch. 2), `conformant` — as required |
| `birth()` suppresses the NBIRTH on the rebirth path | **red** | `-data-commands-rebirth-action-2`: confirms the re-birth *mechanism* is genuinely witnessed, which is why Story 4.7 is scoped as a caller |
| the DBIRTH topic is built with a different node id than the NBIRTH | **red** | `-device-birth-publish-dbirth-match-edge-node-topic`, `conformant` — the two literals in one test do catch a divergence |
| `metrics_for` drops the `Energy` metric | **red** (3 tests) | `-data-publish-dbirth`, `conformant` — the DBIRTH/DDATA metric sets cannot silently diverge |
| cold start publishes `Double(0.0)` instead of `Null` | **red** | `-data-publish-dbirth-values`, `conformant` — a fabricated value is caught |
| the NBIRTH's `Contract/Version` carries a different value | **red** | `-data-publish-nbirth-values`, `conformant` — the value is asserted, not merely the metric's presence |
| **`Energy` stamped 60 s earlier than `Power`** — metrics out of chronological order | **green** | `-data-publish-dbirth-order`, `gap (unproven)` — and it **strengthens** the row: this is not a vacuous clause satisfied by small payloads, it is a clause that can be violated outright with nothing noticing |
| **`METRIC_ENERGY` renamed to `POWER`** — a DBIRTH carrying `Power` and `POWER` | **green** | `case-sensitivity-metric-names`, `gap (unproven)` — as required |
| `qos_for` returns `AtLeastOnce` instead of `AtMostOnce` | **red** | `-nbirth-qos` / `-dbirth-qos`, `conformant` — **added at the code review of Story 4.3**, see below |

**The ninth mutation was added because the claim it tests had been inherited, not verified.** The
`-nbirth-qos` proof cell asserted that "mutating `qos_for`'s return goes red" — a sentence carried
over from chapter 6's findings, in the one column where an unrun claim is the prohibited move. It is
now run: red. The lesson is narrow and worth keeping: **a proof cell may not borrow another
chapter's mutation result**, because the reader cannot tell an inherited claim from a fresh one.

**The seventh mutation refuted the row it was written to confirm.** Its *outcome* was expected —
green, nothing asserts ordering. What it destroyed was the row's stated **reason**: the draft said
the ordering clauses were satisfied *because the payloads are too small to be out of order*, which
would have made any test pointless. Production can in fact emit genuinely mis-ordered metrics today
and the suite stays green. So the prediction record is clean and the surprise was real — they are
about different things, and an earlier draft of this paragraph claimed credit for both without
saying so.

## Tally for chapter 6

**This tally was `32 · 4 · 14 · 59` until Story 5.2** (2026-08-04). Four DDEATH payload rows —
`-timestamp`, `-seq`, `-seq-inc`, `-seq-number` — moved from gap to conformant when the bridge
began emitting the message. See the note under that table for what did *not* move.

**39 conformant · 4 deviations · 7 gaps · 59 n/a**

`39 + 4 + 7 + 59 = 109` — the enumerated clause set, with no remainder.

**This tally was `38 · 4 · 8 · 59` until Story 4.12** (2026-08-18), which moved
`payloads-nbirth-timestamp` from `gap (unproven)` to `conformant`: `38 + 1 = 39` and `8 − 1 = 7`.
No verdict changed and no other row moved: the two `timestamp` deviations gained evidence and stayed
deviations, because more proof of a deliberate deviation is not a step toward conformance.

**The row was moved a day before its evidence existed, and the review of 4.12 (2026-08-19) found
it.** [#30]'s prescription is *"replace `clock.wall()` with a small constant and every test stays
green"*, and `clock.wall()` is read at the **call site**, `mqtt_driver.rs:1223` and `:1292`. Story
4.12's unit test hands `birth()` the instant itself, so it proves the publisher stamps the argument
it is given and says nothing about what the caller gives it. Applying [#30]'s mutation literally —
both arguments replaced by `UtcMillis(42)` — left **the whole suite green**: 258 unit tests, and
every chaos test that observes an NBIRTH. The row stayed `conformant` rather than being reverted
because the missing assertion was written the same day, in `chaos_no_replay_at_reconnect`, and the
mutation now goes red there. **The tally is unchanged; only the evidence behind it is real.**

**This tally was `31 · 5 · 14 · 59` until Story 4.10** (2026-08-01), which moved
`payloads-nbirth-bdseq-repeat` from `deviation` to `conformant`: `31 + 1 = 32` and `5 − 1 = 4`. That
row is the chapter-6 half of the one defect chapter 5 records as `-will-message-payload-bdSeq`, which
is why both chapter tallies move by one in the same direction on the same story.

**And `30 · 5 · 15 · 59` until Story 4.7.** One row moved: `payloads-nbirth-rebirth-req`,
from `gap (unimplemented)` to `conformant`. `30 + 1 = 31` and `15 − 1 = 14`.

**The count of 109 is a count of ids, not of requirements.** Two of them,
`payloads-sequence-num-req-nbirth` and `-zero-nbirth`, are one clause under two spellings (see the
editorial note at the head of this chapter), and both hold a `conformant` row. So **39 conformant is
38 distinct**, and the chapter states **108 distinct requirements**. The arithmetic is kept against
109 because 109 is what a mechanical enumeration of the specification returns, and a matrix that
cannot be diffed against the norm is worth less than one that double-counts a known phantom.

**This tally was `34 · 5 · 11 · 59` until the code review of Story 4.2.** Four rows moved from
`conformant` to `gap (unproven)`: the quality property's `type`, `-propertyvalue-type-req`, the
NBIRTH payload timestamp, and the will's retain flag. None of the four was a behaviour change — each
was a proof cell that named evidence weaker than its clause, and one of them
(`-quality-value-type`) named a test asserting production's own expression against itself.

**The 5 deviations** — the Ignition quality codes ([ADR 0012](adr/0012-quality-codes-spec-versus-host.md)),
`datatype` on DDATA metrics (#28), the DDATA payload timestamp and the re-declaring DBIRTH timestamp
([ADR 0013](adr/0013-payload-timestamp-is-acquisition-time.md), #29, two clauses), and the frozen
`bdSeq` (Story 4.10). A sixth `deviation` verdict renders in this chapter — the scope limit — and is
deliberately outside the tally, because it is a scope decision rather than a `tck-id` row.

**The 7 gaps, split by kind** (see "How to read this"):

- **6 × `gap (unproven)`** — we do the thing; nothing proves it. Both property-set array-length
  clauses, the `engUnit` property's `type`, the quality property's `type`, `-propertyvalue-type-req`,
  and the metric-level `timestamp`. *The will's retain flag left this list on 2026-08-11:
  `the_registered_will_carries_the_qos_and_retain_the_norm_mandates` now observes it. **The NBIRTH
  payload timestamp left it on 2026-08-18**, story 4.12 — although not on the evidence claimed at
  the time; `chaos_no_replay_at_reconnect` carries the falsification since 2026-08-19, and the note
  under the tally says what was wrong with the first one.* All
  [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30).
- **1 × `gap (unimplemented)`** — we do not do it: edge-node-descriptor uniqueness
  ([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)).
  `payloads-nbirth-rebirth-req` left this list at Story 4.7; **the three DDEATH clauses and the
  DDEATH timestamp left it on 2026-08-04** with Story 5.2; **the will's QoS left it on 2026-08-10**
  with Story 4.17 ([#26](https://github.com/guycorbaz/smartme_mqtt/issues/26)).

**Every gap carries an owning story, epic or issue.** That the unproven half now outnumbers the
unimplemented half is the finding, not an accounting detail: this chapter's dominant defect is not
missing behaviour, it is behaviour nothing would notice losing.

**The 59 n/a, broken down** — 36 scope limit (aliases, Templates, DataSets) · 9 NCMD/DCMD
(Host-published) · 7 Host Application `STATE` · 6 NDATA (never emitted) · 1 MQTT 5.0 DISCONNECT.

The expected figure when this pass was scoped was roughly 50. The 9 excess are the NCMD/DCMD
clauses, reclassified from `gap` to `n/a` after reading chapter 6's own wording — justified in the
NCMD/DCMD section above. That is the single reclassification in this chapter, and it is the one an
`n/a`-as-dustbin check should scrutinise first. It survived that scrutiny during the code review of
Story 4.2 and **propagated**: chapter 4's `topics-ncmd-mqtt` and `-dcmd-mqtt` became `n/a` too,
because one obligation may not carry two verdicts in one document.

Every `conformant` row names a test, or names the pinned protobuf schema where the schema makes
the violation unrepresentable ([ADR 0014](adr/0014-schema-as-conformance-evidence.md)). No row is
asserted from reading the code alone. **Eight behaviours that are correct by construction are
recorded as `gap (unproven)` rather than `conformant`**, because no test proves them: six would
break in total silence; one (a surplus PropertySet key) is caught only by an unrelated assertion
that happens to pin an exact key list; and one (the quality property's `type`) was defended by a
test comparing production's own expression to itself, which is not a witness at all. None of the
eight has a witness for the property it needs.

**Six mutations were run against the code rather than re-reading it**, on the principle that an
audit agreeing with itself proves nothing. Five were clean and one was discarded as confounded (see
the Property sets section, where all six are tabulated). The clean five:

| Mutation | Suite | The row it tests |
| --- | --- | --- |
| `encode_metric` drops `name` | **red** | `-name-requirement`, `conformant` — as required |
| `int_property` drops `r#type` | **red** | `-propertyvalue-type-req` — see below |
| `encode_metric` drops `timestamp` | **green** | `-name-birth-data-requirement`, `gap` — as required |
| `string_property` drops `r#type` | **green** | `-propertyvalue-type-value`, `gap` — as required |
| unpaired PropertySet **value** appended | **green** | `-values-array-size`, `gap` — as required |
| unpaired PropertySet **key** appended | **red** | `-keys-array-size`, and it is still a `gap` |

**Red does not mean `conformant`, and the last two rows are why the symmetry must be stated rather
than assumed.** A surplus *key* goes red, yet its clause stays a `gap`: the failure comes from
`a_birth_is_self_describing` pinning one scenario's exact key list, not from anything asserting the
invariant. And `int_property` dropping `r#type` goes red while `-propertyvalue-type-req` is
nonetheless a `gap`, because the clause covers *every* property value and the `string_property` half
goes green. A mutation result is evidence about one code path; the verdict is about the clause.

**The coverage check was armed before it was trusted.** Run against `git show HEAD:` it reports 101
missing ids; run against this file it reports 0 in both directions. It therefore discriminates
rather than approves — which is the only reason its `0` means anything:

```bash
python3 - <<'PY'
import re, subprocess
spec = open('docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_6_Payloads.adoc').read()
doc  = open('docs/sparkplug-conformance.md').read()
norm = lambda s: s if s.startswith('tck-id-') else 'tck-id-' + s
clauses = {m.rstrip('-') for m in re.findall(r'tck-id-[A-Za-z0-9-]+', spec)}
recorded = {norm(m.strip('`').rstrip('-'))
            for m in re.findall(r'tck-id-[A-Za-z0-9-]+|`payloads-[A-Za-z0-9-]+`', doc)}
print('clauses:', len(clauses))
print('missing :', sorted(clauses - recorded))
print('invented:', sorted(r for r in recorded - clauses if r.startswith('tck-id-payloads-')))
PY
```

`comm` was tried first and abandoned: under a non-C locale it reported *"le fichier 2 n'est pas dans
l'ordre trié"*, and a `comm` over mis-sorted input can print an empty difference that means nothing.
An order-independent set difference cannot fail that way. Use the form above.

## Tally for chapter 10

**0 conformant · 0 deviations · 0 gaps · 12 n/a**

`0 + 0 + 0 + 12 = 12` — the enumerated clause set, with no remainder.

**A whole chapter of `n/a` is the answer this pass most expected to be wrong, and it survived being
checked per profile.** One clause binds a Host Application; eleven bind an MQTT Server, four of them
the Compliant profile and seven the Aware profile. The bridge is an MQTT *client*, so none governs
any behaviour of ours. The four Compliant-server clauses are nonetheless a **deployment
prerequisite** on the broker and belong in the operator manual.

**The chapter's real output is the clause it does not contain**: the Sparkplug Edge Node profile
(`:34-39`) carries no `tck-id` at all. So conformance for an Edge Node is exactly the union of the
clauses in chapters 1–6, with no extra checklist — which is the answer NFR19 needs for "documented
conformance scope".

## Whole-specification total

| Chapter | conformant | deviation | gap | n/a | clauses |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 — Introduction | 3 | 0 | 4 | 1 | 8 |
| 2 — Principles | 1 | 1 | 1 | 1 | 4 |
| 3 — Components | 0 | 0 | 0 | 1 | 1 |
| 4 — Topics | 17 | 0 | 3 | 21 | **41 of 70** |
| 5 — Operational behaviour | 32 | 1 | 17 | 49 | 99 |
| 6 — Payloads | 38 | 4 | 8 | 59 | 109 |
| 10 — Conformance | 0 | 0 | 0 | 12 | 12 |
| **Total** | **91** | **6** | **33** | **144** | **274 of 303** |

**The total was `72 · 8 · 50 · 144` until Story 4.7**, which moved seven rows from
`gap (unimplemented)` to `conformant` — six in chapter 5, one in chapter 6. `72 + 7 = 79`,
`50 − 7 = 43`, and `79 + 8 + 43 + 144 = 274` is unchanged: verdicts moved, no row was added or
removed.

**And `79 · 8 · 43 · 144` until Story 4.10** (2026-08-01), which moved the two `bdSeq` rows from
`deviation` to `conformant` — `-will-message-payload-bdSeq` in chapter 5 and
`payloads-nbirth-bdseq-repeat` in chapter 6, one each, which is why both chapter lines move by one
in opposite columns. `79 + 2 = 81`, `8 − 2 = 6`, and `81 + 6 + 43 + 144 = 274` is again unchanged.
Unlike Story 4.5, this story **earned** the move: it changed the code the verdicts describe, and the
increment is asserted by `chaos_bd_seq_advances_on_every_connect` against an independent subscriber
across a real disconnect, falsified before being trusted. **Chapter 4's tally is deliberately untouched**: its own copy of the clause,
`topics-nbirth-rebirth-metric`, has no row at all — it is one of the 29 that Story 4.19 owns. The
evidence for it exists (see the chapter-5 rows above) and 4.19 can cite it, but opening the row
here would change chapter 4 from *41 of 70 audited* to *42 of 70* through a side door, which is the
kind of drive-by arithmetic this matrix has already had to correct twice.

**And `81 · 6 · 43 · 144` until Story 5.2** (2026-08-04), which made the bridge emit a DDEATH for
the first time and moved six rows from `gap (unimplemented)` to `conformant` — two in chapter 4
(`topics-ddeath-topic`, `-mqtt`) and four in chapter 6 (`payloads-ddeath-timestamp`, `-seq`,
`-seq-inc`, `-seq-number`). `81 + 6 = 87`, `43 − 6 = 37`, and `87 + 6 + 37 + 144 = 274` is again
unchanged. **Chapter 5's `operational-behavior-device-ddeath` deliberately did NOT move**: that
clause triggers on the Edge Node *losing connection* with a Device, and the bridge emits its DDEATH
when an operator *disables* a meter, which is a different event. The form of a DDEATH is conformant;
one of the two situations that should produce one still does not.

**`70 + 109 + 124 = 303`** is the whole-specification clause set: chapter 4 (Story 4.1), chapter 6
(Story 4.2), and chapters 1, 2, 3, 5 and 10 (Story 4.3). Story 4.3's own arithmetic closes at
**`26 + 3 + 31 + 64 = 124`**.

**274 of 303 carry a row or a named collective block.** The 29 outstanding are all in chapter 4 and
belong to **Story 4.19**; the chapter-4 tally explains their shape. Until 4.19 lands, this matrix
audits the specification *in part*, and the Status table says so per chapter — a reader can tell
audited-in-full from audited-in-part without doing arithmetic.

**Four of the six deviations are two defects counted twice**, because two chapters state one
obligation and the matrix is keyed by `tck-id`: the payload timestamp (chapter 6, two clauses,
[ADR 0013](adr/0013-payload-timestamp-is-acquisition-time.md)) and periodic publishing (chapters 2
and 5, [#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)). The remaining two stand alone:
`DataType` on DDATA metrics ([#28](https://github.com/guycorbaz/smartme_mqtt/issues/28)) and the
quality codes ([ADR 0012](adr/0012-quality-codes-spec-versus-host.md)). Counting rows is the honest
way to reconcile against the norm; counting distinct defects gives **four**, and both numbers are
stated here so neither reading is available to a hurried reader.

**This paragraph read `six of the eight … three defects … five` until 2026-08-03**, three days after
Story 4.10 removed the third doubled defect — the frozen `bdSeq`, chapters 5 and 6 — from the
deviation count. 4.10 amended the total table above and the two rows themselves, and left every
number that *followed* from them: this paragraph, both chapter tallies, chapter 6's rendered-count
note and the counterfactual under `-primary-host-app`. Amending a claim is not amending its
consequences, and this document has now had to record that twice.

**The 37 gaps split 23 unimplemented / 14 unproven**, and the balance moved sharply with chapter 5. *(Corrected TWICE, and the second time is worse than the first. The Story 4.7 code review found it reading `50 … 36 / 14` while the total said 43 — three tallies recomputed and this fourth missed. Then Story 5.2 moved six rows on 2026-08-04, amended both chapter tallies and the total, ran a SCRIPT to verify the arithmetic — and the script checked the TABLES only, so this sentence went on saying 43 and 29 for a quantity that was now 37 and 23. The lesson recorded after the first correction was "amend the consequences, not just the claim"; the lesson from the second is that a checker which verifies the tables verifies the half of the document least likely to be wrong.)*
Chapter 6 found a codebase whose dominant defect was behaviour nothing would notice losing; chapter 5
found two whole mechanisms — command handling and Primary Host STATE — that were never built. Both
were real, and they needed different work: a test versus a feature. **Neither is now open**: Story 4.7
built command handling, and Story 4.5 declined Primary Host STATE in writing ([ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md)) — the second is a
position, which is what the Epic 1 retrospective asked for when it found the blind spot.

**Story 4.6 is the first story to move a verdict in this matrix rather than record one.** It closed
two rows in two chapters — `-ncmd-subscribe` (ch. 5) and `topics-ncmd-topic` (ch. 4) — so 38
unimplemented gaps became 36 and 70 conformant rows became 72. The audit stories (4.1–4.4) only ever
described the code; from here the totals move as Epic 4 builds.

The chapter-4 row is the one worth noticing: it was **not** on Story 4.6's list of documents to
amend. That list was produced by grepping for *"no subscription"*, and this row said *"not
implemented"* instead. Re-running the grep found a twelfth passage the list had missed
(`-state-subs`, ch. 5) and reading around it found this one. A mechanical list is a floor, not a
ceiling.

### The six ways this pass could have reported success without doing the work

Story 4.3 lists six failure modes and requires the finished sections to be checked against each.
That check was performed but not **recorded** per mode, which the code review of Story 4.3 pointed
out is the same defect as a manual gate that does not say how it could pass wrongly. The record:

| # | Failure mode | Outcome |
| --- | --- | --- |
| 1 | A `conformant` naming a test that does not exercise the clause | **Live, and it got through the first pass.** Three chapter-1 `-string` rows cited topic-grammar tests for a UTF-8 clause; `-birth-certificates-order` claimed a chaos test observed the NBIRTH "before anything else" when the helper discards non-matching messages; `-will-message-topic` claimed a grammar witness that only tests `.contains("/NDEATH/")`. All three caught at review, all three corrected. Nine mutations now back the `conformant` rows that could be mutated |
| 2 | `n/a` used as a dustbin | **Checked, and it paid for itself twice.** Six `operational-behavior-edge-node-*` ids bind Host Applications despite their prefix; four `operational-behavior-primary-*` ids split three-to-one. Weak point, disclosed: the 18-clause host-application block is argued by content rather than per-clause quotation |
| 3 | A collective block that hides its members | **Live, caught by the instrument.** The coverage check reported 26 missing on its first run because three blocks used abbreviated ids. Rewritten with full ids |
| 4 | A verdict copied from a chapter-4 or chapter-6 twin | **Checked clean.** `-nbirth-payload-bdSeq` was read against chapter 5's own wording and reached a *different* verdict from its chapter-6 twin, with the reason stated. The review verified both clause texts verbatim and confirmed the split is right |
| 5 | Re-verifying by re-reading our own conclusions | **Answered with mutations, not prose.** Nine ran against production code; one refuted the row it was written to confirm |
| 6 | Declaring the audit complete on 103 clauses | **Not applicable — all 124 were audited**, and the `303` reconciliation is stated with a per-chapter table |

**Mode 1 is the one to carry forward.** It was live in the Story 4.2 review and live again here, in a
pass that had read the warning and quoted it. Reading the row is not reading the test.

### Gap ownership — every gap walked against Epic 4's remaining stories

**AC2 of Story 4.3 is a task, not a formality**: each of the 31 gaps this pass opened was walked
against Stories 4.4–4.18, and one that fitted no story got an issue **and** an owning epic. Nothing
is left unowned or unnumbered.

| Gaps | Count | Owner | Already existed? |
| --- | ---: | --- | --- |
| Primary Host STATE — `-phid-wait`, `-wait-id`, `-wait-online`, `-wait-timestamp`, `-phid-offline`, `-birth-sequence-wait`, `-termination-host-offline`, `-host-offline-reconnect`, `-host-offline-timestamp`, `-state-subs`, `-walk` | 11 | **Stories 4.4, 4.5** (4.6 for the subscription plumbing). Story 4.4 measured this deployment and ruled each **relevant / irrelevant** — 10 · 1 after the 4.4 review (`9 · 2` before it), four with a named undetermined residue: [primary-host-state-observation.md](primary-host-state-observation.md#the-eleven-clauses-ruled). That review also found a **cold-start state no clause covers** — a retained `online:false` at bridge start-up — which 4.5 had to rule on alongside the eleven. **RULED 2026-08-01**: it cannot arise, because the bridge subscribes to its own NCMD topic and to nothing else, so a retained STATE payload is never delivered to it — moot by construction rather than handled. Stated in [ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md), whose first version omitted it. **The verdicts above are unchanged and deliberately so; 4.5 DECIDED** ([ADR 0018](adr/0018-no-primary-host-state-the-repair-is-host-initiated.md)), and the verdict *word* is [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42) rather than this story's to change | yes — this epic |
| ~~`Node Control/Rebirth` — NBIRTH metric, datatype, value, and the three receive-side actions~~ **closed** | 6 | ~~Story 4.7~~ done | ~~yes~~ — landed |
| NCMD subscription (QoS 1) | 1 | **Story 4.6**, [#23](https://github.com/guycorbaz/smartme_mqtt/issues/23) | yes |
| The will's QoS 1 | 1 | **Story 4.17**, [#26](https://github.com/guycorbaz/smartme_mqtt/issues/26) | yes |
| The will's retain flag, the two ordering clauses, metric-name case collision | 4 | [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30) — **Epic 4** | issue existed; **scope widened by this pass** |
| Clean-session flag, single-server rule | 2 | [#35](https://github.com/guycorbaz/smartme_mqtt/issues/35) → **Story 4.10** | **new issue** |
| Identifier character set (three `-chars` clauses) | 3 | [#34](https://github.com/guycorbaz/smartme_mqtt/issues/34) — **Epic 3**, where config and identifier validation lives | **new issue** |
| Edge-node-descriptor uniqueness, case-folded id collision | 2 | [#27](https://github.com/guycorbaz/smartme_mqtt/issues/27) — **Epic 3** | issue existed; **scope widened by this pass** |
| DDEATH never emitted | 1 | **Epic 3** | yes |
| **Total** | **31** | | **2 new issues, 2 widened** |

**Two issues were deliberately *not* opened.** `case-sensitivity-sparkplug-ids` is a stricter form of
the uniqueness requirement #27 already carries, and `intro-edge-node-id-uniqueness` is the same
requirement chapter 4 and chapter 6 each state once. One requirement gets one owner however many
times the specification states it — the same rule that kept the frozen `bdSeq` from acquiring a
second issue.

**Epic 4 needed no reshaping.** Stories 4.4–4.7 and 4.10 already covered the two mechanisms this
chapter found missing, which is the epic's planning holding up rather than luck: the STATE blind
spot and the unanswerable Rebirth were both *suspected* when the epic was written, and AC1 asked
this pass to confirm they existed and to find what else did. What else it found was smaller and
sharper — an unvalidated character set, a MUST resting on a dependency default, and two mechanisms
that are correct only because the payloads are too small to break them.

### Coverage check for chapters 1, 2, 3, 5 and 10

**Armed before it was trusted, and it was seen red.** Run against `git show HEAD:` — the state
before this pass — it reports **123 of 124 missing**; run against this file it reports 0 in both
directions. It therefore discriminates rather than approves.

*The expected red figure is 123, not the 124 Story 4.3 predicted.* Exactly one of the 124,
`tck-id-message-flow-edge-node-ncmd-subscribe`, was already cited before this pass — in the
chapter-4 and chapter-6 notes that pointed forward to Story 4.3 for it. The prediction was off by
that one forward reference, which is itself the evidence that the cross-chapter pointers work.

```bash
python3 - <<'PY'
import re, subprocess, sys
CH = ['1_Introduction','2_Principles','3_Components','5_Operational_Behavior','10_Conformance']
clauses = set()
for c in CH:
    clauses |= {m.rstrip('-') for m in re.findall(r'tck-id-[A-Za-z0-9-]+',
                open(f'docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_{c}.adoc').read())}
src = sys.argv[1] if len(sys.argv) > 1 else None
doc = subprocess.run(['git','show',f'{src}:docs/sparkplug-conformance.md'],
                     capture_output=True, text=True).stdout if src \
      else open('docs/sparkplug-conformance.md').read()
norm = lambda s: s if s.startswith('tck-id-') else 'tck-id-' + s
FAM = r'(?:intro|principles|components|case-sensitivity|conformance|operational-behavior|message-flow)'
recorded = {norm(m.strip('`').rstrip('-'))
            for m in re.findall(r'tck-id-[A-Za-z0-9-]+|`' + FAM + r'-[A-Za-z0-9-]+`', doc)}
print('clauses :', len(clauses))
print('missing :', sorted(clauses - recorded))
print('invented:', sorted(r for r in recorded - clauses if re.match('tck-id-' + FAM + '-', r)))
PY
```

Note the `FAM` alternation: this document abbreviates ids in prose, so the check counts a clause as
recorded when its **full** id appears at least once. Every collective block above therefore lists its
members by full id — a block reading "the 18 host-application clauses" would satisfy a reader and
fail this check, which is the point. It caught exactly that on its first run: 26 ids were missing
because three blocks used abbreviated forms.

**What this check does not prove, spelled out because the first draft let it stand for more than it
can bear.** It verifies that an id is **mentioned somewhere in the file** — not that it carries a
verdict. The document's own anomaly demonstrates the gap: `tck-id-message-flow-edge-node-ncmd-subscribe`
counted as "recorded" at `HEAD`, before this pass, purely because chapter 4 and chapter 6 mentioned
it in a forward reference. A mention is not an audit. **The second check below is the one that
establishes AC3**, and the two together are what the claim rests on:

```bash
python3 - <<'PY'
import re
doc = open('docs/sparkplug-conformance.md').read()
CH = ['1_Introduction','2_Principles','3_Components','5_Operational_Behavior','10_Conformance']
clauses = set()
for c in CH:
    clauses |= {m.rstrip('-') for m in re.findall(r'tck-id-[A-Za-z0-9-]+',
                open(f'docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_{c}.adoc').read())}
verdicts = set()
for line in doc.split('\n'):                       # a row whose last cell is a verdict
    if line.startswith('| `') and line.count('|') >= 6:
        v = line.rsplit('|', 2)[1].strip().lower()
        if any(k in v for k in ('conformant','deviation','gap','n/a')):
            m = re.match(r'\| `(?:tck-id-)?([A-Za-z0-9-]+)`', line)
            if m: verdicts.add('tck-id-' + m.group(1))
rows = clauses & verdicts
print('in a verdict row      :', len(rows))
print('in a collective block :', len(clauses - rows))
PY
```

It reports **88 in a verdict row, 36 in a collective block** — and 36 is exactly the membership of
the two blocks that carry verdicts collectively (30 Host Application, 6 `-termination-host-action-*`).
`88 + 36 = 124`, which is the arithmetic AC3 actually asks for. Added at the code review of
Story 4.3, which pointed out that the mention-check had been doing duty for a verdict-check.

`comm` is unusable here for the reason recorded under chapter 6.
