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
witness: the **vendored `sparkplug_b.proto` schema** is one too, where it makes the violation
*unrepresentable*. A clause requiring a field to be an unsigned 32-bit integer is discharged by the
generated type being `Option<u32>`: there is no program we could write that emits anything else, so
the guarantee fails at **compile time** rather than on a test run — stronger than a test, not
weaker. That is the property that matters. It is *not* that the witness is external to this
repository: the schema is vendored **inside** it, and the first draft of this paragraph claimed
otherwise, which was simply wrong.

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
| 4 — Topics & namespace | 4.1 | **audited, not complete** — 41 of the chapter's 70 `tck-id`s carry a row; see the chapter-4 tally |
| 6 — Payloads, metrics, datatypes | 4.2 | **done** — all 109 `tck-id-payloads-*` clauses accounted for |
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
| `topics-nbirth-mqtt` | MUST | QoS 0, retain false | `mqtt_driver.rs::every_edge_node_message_is_qos_zero_and_never_retained` | conformant |
| `topics-ndeath-topic` | MUST | `spBv1.0/{group}/NDEATH/{node}` | `node_topics_follow_the_namespace_grammar` | conformant |
| `topics-ndata-topic` | MUST | topic construction supports it; **NDATA is never emitted** — the bridge carries no node-level measurement | — | n/a |
| `topics-ndata-mqtt` | MUST | as above | — | n/a |
| `topics-ncmd-topic` | MUST | **not implemented** — no NCMD subscription exists, and a subscriber must build this topic form too | — | **gap (unimplemented)** (Story 4.6, [#23](https://github.com/guycorbaz/smartme_mqtt/issues/23)) |
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
| `topics-dbirth-mqtt` | MUST | QoS 0, retain false | `every_edge_node_message_is_qos_zero_and_never_retained` | conformant |
| `topics-ddata-topic` | MUST | `spBv1.0/{group}/DDATA/{node}/{device}` | `device_topics_append_the_device_identifier` | conformant |
| `topics-ddata-mqtt` | MUST | QoS 0, retain false | `every_edge_node_message_is_qos_zero_and_never_retained` | conformant |
| `topics-ddeath-topic` | MUST | topic construction supports it; **DDEATH is never emitted** | — | **gap (unimplemented)** (Epic 3 — with one meter a device only stops when its node does) |
| `topics-ddeath-mqtt` | MUST | as above | — | **gap (unimplemented)** (Epic 3) |
| `topics-dcmd-topic` | MUST | **not implemented** — no DCMD subscription, and a subscriber must build this topic form too | — | **gap (unimplemented)** (Story 4.6) |
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

## Chapter 6 — Payloads, metrics and datatypes

**The clause set is 109 ids, and that number was established mechanically, not by reading:**

```bash
grep -oE 'tck-id-[A-Za-z0-9-]+' docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_6_Payloads.adoc \
  | sed 's/-$//' | sort -u          # -> 109
```

The chapter boundary was verified rather than assumed: `grep -rl 'tck-id-payloads-'` over
`docs/spec/sparkplug-b-3.0.0/chapters/` returns **only** `Sparkplug_6_Payloads.adoc`, and the same
pattern over the whole vendored tree yields the same 109 ids. No `payloads-*` clause hides in
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
| `payloads-metric-datatype-value-type` | MUST | `datatype` is an unsigned 32-bit integer | the vendored `sparkplug_b.proto` types the field `optional uint32`; `DataType::code` is a `#[repr(u32)]` cast (`datatype.rs:55`) — **schema witness**, plus `datatype.rs::codes_match_the_specification_numbering` | conformant |
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
| `payloads-propertyset-keys-array-size` | MUST | `encode_properties` pushes key and value together in each branch, so the arrays cannot diverge (`encode.rs:273-288`) | — **correct by construction; no test asserts the invariant**, though one incidentally notices a surplus key — see the mutation note below | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `payloads-propertyset-values-array-size` | MUST | as above — the same invariant stated from the other side | — **correct by construction, wholly unproven**: a surplus value passes entirely unnoticed | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `payloads-metric-propertyvalue-type-type` | MUST | property `type` is an unsigned 32-bit integer | vendored `sparkplug_b.proto` types it `optional uint32` — **schema witness** ([ADR 0014](adr/0014-schema-as-conformance-evidence.md)); a non-`u32` here does not fail a test, it fails to compile | conformant |
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
| `payloads-nbirth-timestamp` | MUST | NBIRTH is stamped with `clock.wall()` (`mqtt_driver.rs:178`) — the publish instant, as the clause requires | — **the cited evidence is a presence check, not a value check**: `chaos_sigterm_no_lie:274` only unwraps `birth.payload.timestamp`, and `:331-332` bounds it from *above* via `death_stamp > birth_stamp`. Replace `clock.wall()` with a small constant and every test stays green — it passes more easily | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `payloads-dbirth-timestamp` | MUST | **cold start conforms** (stamped `now`); **a rebirth re-declaring a known reading is stamped with that reading's `ValueDate`** (`sparkplug_publisher.rs:277-280`) | `a_rebirth_redeclares_what_is_known_instead_of_blanking_it` asserts the deviating behaviour by name | **deviation** ([#29](https://github.com/guycorbaz/smartme_mqtt/issues/29)) |
| `payloads-ddata-timestamp` | MUST | **the payload timestamp is the reading's `ValueDate`, not the publish instant** (`sparkplug_publisher.rs:315`) | `a_good_reading_carries_units_serial_and_the_source_timestamp`, `a_stale_verdict_never_publishes_a_fresh_looking_metric` — both assert it deliberately | **deviation** ([#29](https://github.com/guycorbaz/smartme_mqtt/issues/29)) |
| `payloads-ndata-timestamp` | MUST | NDATA is never emitted | — | n/a |
| `payloads-ddeath-timestamp` | MUST | DDEATH is never emitted | — | **gap (unimplemented)** (Epic 3) |
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
| `payloads-nbirth-bdseq-repeat` | MUST | the NBIRTH's `bdSeq` **does** match the registered will's — but only because neither ever changes | `the_will_matches_the_session_before_and_after_the_birth` | **deviation** (Story 4.10) — see below |
| `payloads-nbirth-edge-node-descriptor` | MUST | nothing verifies that `group_id/edge_node_id` is unique across the infrastructure | — | **gap (unimplemented)** ([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)) |
| `payloads-nbirth-rebirth-req` | MUST | NBIRTH carries no `Node Control/Rebirth` metric | — | **gap (unimplemented)** (Story 4.7) |
| `payloads-nbirth-qos` | MUST | QoS 0 | `mqtt_driver.rs::every_edge_node_message_is_qos_zero_and_never_retained` | conformant |
| `payloads-nbirth-retain` | MUST | retain false | same, plus `chaos_sigterm_no_lie`'s late-subscriber check — an **external** witness that the broker replays nothing | conformant |

**Why `-nbirth-bdseq-repeat` is a deviation despite matching.** The clause reads *"The bdSeq number
value MUST match the bdSeq number value that was sent in the prior MQTT CONNECT packet WILL
Message"* (`:1075`), and taken alone it is satisfied. But it is satisfied *vacuously*: the will is
serialised into `MqttOptions` once at construction and `rumqttc` rebuilds every reconnect's CONNECT
packet from that same snapshot (`mqtt_driver.rs:29-30, 156-163`), so the two values agree because
neither can move. The clause's own accompanying requirement — *"any new CONNECT packet must
increment the bdSeq number in the payload compared to what was in the previous CONNECT packet"*
(`:1521-1525`) — is therefore **violated on every internal reconnect**, and a Host Application
cannot distinguish a current session from a superseded one. Recording this as `conformant` would be
the trap this matrix exists to avoid: right answer, wrong reason. **Story 4.10** owns the fix
(own the reconnect loop, one `bdSeq` per CONNECT).

#### DBIRTH

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-dbirth-seq` | MUST | device messages draw from the node's single counter | `encode.rs::device_messages_share_the_edge_node_numbering`, `sparkplug_publisher.rs::sequence_numbering_is_continuous_across_node_and_device_messages` | conformant |
| `payloads-dbirth-seq-inc` | MUST | +1 per message, wrapping 255 → 0 (`seq.rs::SeqCounter`, a `u8`) | `seq.rs::seq_wraps_255_to_0`, `prop_seq_bdseq.rs::prop_published_messages_wrap_255_to_0`, `sequence_numbering_is_continuous_across_node_and_device_messages` | conformant |
| `payloads-dbirth-order` | MUST | `birth()` emits the NBIRTH then every DBIRTH in one call, and `publish()` refuses before that (`Published::DroppedBeforeBirth`) | `cold_start_birth_declares_tags_with_no_value_and_stale_quality` (order), `a_drop_before_the_birth_is_reported_not_silent`, and `chaos_sigterm_no_lie` observes NBIRTH-then-DBIRTH on a real broker | conformant |
| `payloads-dbirth-qos` | MUST | QoS 0 | `every_edge_node_message_is_qos_zero_and_never_retained` | conformant |
| `payloads-dbirth-retain` | MUST | retain false | same, plus `chaos_sigterm_no_lie`'s late-subscriber check | conformant |

#### DDATA

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-ddata-seq` | MUST | as DBIRTH — one node-wide counter | `sequence_numbering_is_continuous_across_node_and_device_messages` | conformant |
| `payloads-ddata-seq-inc` | MUST | as DBIRTH | `seq_wraps_255_to_0`, `prop_published_messages_wrap_255_to_0` | conformant |
| `payloads-ddata-order` | MUST NOT (until births) | a reading before the BIRTH is dropped and **reported**; so is one for a device no BIRTH declared | `a_drop_before_the_birth_is_reported_not_silent`, `a_reading_for_an_undeclared_device_is_reported_not_silent` | conformant |
| `payloads-ddata-qos` | MUST | QoS 0 | `every_edge_node_message_is_qos_zero_and_never_retained` | conformant |
| `payloads-ddata-retain` | MUST | retain false | same, plus the external late-subscriber check | conformant |

#### NDATA — n/a

`payloads-ndata-seq`, `payloads-ndata-seq-inc`, `payloads-ndata-order`, `payloads-ndata-qos`,
`payloads-ndata-retain` (and `payloads-ndata-timestamp`, filed under Timestamps).

**n/a — the bridge holds no node-level datum that could ever change**, consistent with chapter 4's
`topics-ndata-*` rows.

**The criterion, because `n/a` and `gap` are one judgement apart here.** DDEATH below is a `gap`
while NDATA is `n/a`, and the two verdicts must not rest on taste. The test is: *does the bridge
hold the datum or the event that this message type exists to carry?*

- **NDATA — no.** The node's only metric is `Contract/Version`, a constant fixed for the life of the
  session (`sparkplug_publisher.rs:243,251`). NDATA exists to report a *change* to a node-level
  metric; there is nothing here that could change, so the clause governs no behaviour of ours.
- **DDEATH — yes.** A device's death is an event the bridge already detects: meter unreachability
  drives the stale/bad quality verdict today. We hold the event and do not publish the message.

**And the falsification condition, so this row cannot quietly rot**: the moment the node gains a
mutable metric — bridge health, uptime, connection state — these six clauses become
`gap (unimplemented)`. Whoever adds that metric owns the change to this section.

#### DDEATH

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-ddeath-seq` | MUST | **DDEATH is never emitted by the bridge** | — | **gap (unimplemented)** (Epic 3) |
| `payloads-ddeath-seq-inc` | MUST | as above | — | **gap (unimplemented)** (Epic 3) |
| `payloads-ddeath-seq-number` | MUST | as above | — | **gap (unimplemented)** (Epic 3) |

**Gap, not n/a, and the distinction is deliberate** — it is the criterion stated under NDATA above,
applied the other way: the bridge already detects the event (meter unreachability drives the
stale/bad quality verdict) and does not publish the message. A device *can* die while its node
lives; with one meter it simply never has, which is a deployment fact rather than a role we do not
play. Consistent with `topics-ddeath-topic` in chapter 4.

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

**This was the larger of this pass's two judgement calls, and the code review of Story 4.2 took it
the rest of the way.** A `gap` asserts "we do not do something we should"; applied to
`payloads-ncmd-qos` it would claim the bridge ought one day to *publish* an NCMD, which is false.
The pass originally left chapter 4's `topics-ncmd-mqtt` / `-dcmd-mqtt` as `gap`s, which meant one
obligation carried two verdicts in one document — `Sparkplug_4_Topics.adoc:344` and `:508` are the
same publish-side requirement as the rows above. **Those two chapter-4 rows are now `n/a` as well**;
see the note under chapter 4's device-messages table.

**The unimplemented command path is not hidden by any of this.** It stays recorded at
`topics-ncmd-topic`, `topics-dcmd-topic` and `payloads-nbirth-rebirth-req` (Stories 4.6 and 4.7),
and the obligation we actually fail — that an Edge Node must **subscribe** — has its own clauses in
chapter 5: `tck-id-message-flow-edge-node-ncmd-subscribe`
(`Sparkplug_5_Operational_Behavior.adoc:158`) and `-device-dcmd-subscribe` (`:403`), owned by
**Story 4.3**.

The receiving-side obligation these clauses imply — that once Story 4.6 lands the bridge must
*tolerate* an NCMD carrying no `seq` — is the *same* clause read from the other side:
`tck-id-payloads-ncmd-seq` (`:1417-1418`, *"Every NCMD message MUST NOT include a sequence
number"*), the id filed `n/a` above. It is flagged here for Story 4.6 rather than given a second
row, because one clause gets one row.

#### NDEATH

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `payloads-ndeath-will-message` | MUST | the will is registered in the CONNECT packet (`mqtt_driver.rs:159`) | `chaos_stale_on_death` — the bridge is **SIGKILLed** and an independent subscriber receives the certificate the broker was holding. An external witness, not a unit test | conformant |
| `payloads-ndeath-will-message-qos` | MUST (QoS 1) | will registered at QoS 0 — `qos_for` returns `AtMostOnce` for every type including `NDeath` | — | **gap (unimplemented)** ([#26](https://github.com/guycorbaz/smartme_mqtt/issues/26), Story 4.17) |
| `payloads-ndeath-will-message-retain` | MUST | will retain false (`mqtt_driver.rs:123-125,158`) | — **no test observes the registered will's retain flag.** `every_edge_node_message_is_qos_zero_and_never_retained` does not reach the will (the findings table says so); `chaos_stale_on_death`, the one test in which the broker actually publishes the will, asserts only `bdSeq` and `seq == None` (`:76-87`). See below | **gap (unproven)** ([#30](https://github.com/guycorbaz/smartme_mqtt/issues/30)) |
| `payloads-ndeath-seq` | MUST NOT | `death_payload` sets `seq: None` (`encode.rs:219`) | `encode.rs::the_will_matches_the_birth_and_carries_no_sequence`, `sparkplug_publisher.rs::the_will_matches_the_session_before_and_after_the_birth` | conformant |
| `payloads-ndeath-bdseq` | MUST | the death carries the birth's `bdSeq` | same, plus `prop_will_birth_and_death_agree_on_bdseq_for_every_session_number` and `chaos_stale_on_death` (asserted against a real broker) | conformant |
| `payloads-ndeath-will-message-publisher` | SHOULD | the bridge publishes NDEATH itself before disconnecting (`mqtt_driver.rs:240`) | `chaos_sigterm_no_lie` — and it proves the *explicit* death rather than the will, because it asserts the death is stamped **later** than the birth, which a CONNECT-time will never can be | conformant — **and it vindicates [ADR 0011](adr/0011-graceful-shutdown-requires-both-deaths.md)**, which reached the same conclusion by reasoning before this clause was read |
| `payloads-ndeath-will-message-publisher-disconnect-mqtt311` | MUST | the bridge speaks MQTT 3.1.1 and **never sends a DISCONNECT packet** — it publishes the NDEATH and drops the socket (ADR 0011) | `chaos_sigterm_no_lie` | conformant — see below |
| `payloads-ndeath-will-message-publisher-disconnect-mqtt50` | MUST | the bridge does not speak MQTT 5.0 | — | n/a |

**The will's retain flag was `conformant` until the code review of Story 4.2, on two witnesses that
do not reach the will.** The registered will is almost certainly retain-false — `qos_for`
(`mqtt_driver.rs:123-125`) returns `(AtMostOnce, false)` and `:158` feeds it straight into
`MqttOptions::set_last_will`. But *almost certainly* is what this column is not for. There is a
plausible third witness — `chaos_sigterm_no_lie`'s late-subscriber check (`:397-405`) would surface
a retained will, since a retained anything on that topic tree fails it — and it is not enough
either: it only fires if the will is published in that run, which the test neither ensures nor
asserts. Caught-if-we-are-lucky is the same standard the array-size rows were downgraded under.

**A second thing `qos_for` costs us, worth naming while we are here.** Its parameter is `_message:
MessageType` — ignored. So `every_edge_node_message_is_qos_zero_and_never_retained` loops six
message types past a function that cannot tell them apart: one assertion repeated six times. The
verdicts it supports still stand today, because both call sites (`:158` for the will, `:278` for
every publish) derive from that one function and mutating its return goes red. But the day `qos_for`
grows a real `match`, five of the six retain verdicts silently revert to unproven with no test
change to signal it.

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
decision, not a `tck-id` row, so it is not one of the 109 and does not appear in the count of five
deviations. A reader counting rendered `deviation` verdicts in this chapter finds six; five is the
number the arithmetic uses, and this is the sixth.

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

## Findings carried forward

| Finding | Chapter | Where |
| --- | --- | --- |
| Will registered at QoS 0; the specification requires QoS 1 | 4, 6 | [#26](https://github.com/guycorbaz/smartme_mqtt/issues/26), Story 4.17 |
| `every_edge_node_message_is_qos_zero_and_never_retained` asserts QoS 0 for all six types and claims the spec requires it uniformly — true for the six published types, false for the will | 4 | Story 4.17 |
| No verification of edge-node-descriptor or device-id uniqueness | 4, 6 | [#27](https://github.com/guycorbaz/smartme_mqtt/issues/27) |
| NCMD/DCMD not implemented — no subscription, and no `Node Control/Rebirth` metric to act on. The clause we actually fail is the *subscribe* one, `message-flow-edge-node-ncmd-subscribe` (ch. 5); the publish-side QoS/retain clauses are `n/a` in both chapters | 4, 5, 6 | Stories 4.6 / 4.7, and **4.3** for the chapter-5 subscribe clauses, [#23](https://github.com/guycorbaz/smartme_mqtt/issues/23) |
| DDEATH never emitted (the crate-side encoder is conformant and tested; the bridge never calls it) | 4, 6 | Epic 3 |
| **`datatype` is sent on every DDATA metric; `-metric-datatype-not-req` says SHOULD NOT.** One encoder serves every message type, so the same line satisfies the BIRTH MUST and violates the DATA SHOULD NOT | 6 | [#28](https://github.com/guycorbaz/smartme_mqtt/issues/28) |
| **The DDATA and re-declaring-DBIRTH payload timestamps are the reading's `ValueDate`, not the publish instant.** Deliberate — the anti-replay invariant — and contrary to two MUSTs. Recorded as [ADR 0013](adr/0013-payload-timestamp-is-acquisition-time.md) | 6 | [#29](https://github.com/guycorbaz/smartme_mqtt/issues/29) |
| **Eight invariants are correct by construction and proven by no test** — raised from four by the code review of Story 4.2: both property-set array-length clauses, the `engUnit` property's `type` field, the metric-level `timestamp` field, and four more the review found — the quality property's `type` (whose test asserted production's own expression against itself), `-propertyvalue-type-req` (witnessed for `int_property` only), the NBIRTH payload timestamp (a presence check, not a value check), and the registered will's retain flag (no test reaches the will) | 6 | [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30) — **its scope needs widening from "encoder invariants" to match** |
| **`Int32` is the one datatype code no test pins to its literal.** `codes_match_the_specification_numbering` covers 1, 4, 8–13, 17; change `Int32 = 3` and the suite stays green while `-quality-value-type` violates a MUST | 6 | [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30) |
| `qos_for` ignores its `MessageType` argument, so the six-type QoS/retain test is one assertion repeated six times. Harmless today; five retain verdicts revert to unproven the day it grows a real `match` | 4, 6 | Story 4.17 |
| **`bdSeq` is fixed for a client's lifetime**, so `-nbirth-bdseq-repeat` passes for the wrong reason and the per-CONNECT increment the clause requires never happens | 6 | Story 4.10 |
| Specification editorial: `sequence-num-req-nbirth` / `-zero-nbirth` are one clause with two spellings, so a mechanical count of chapter 6 reads 109 where 108 requirements exist | 6 | recorded above; upstream, not ours |
| Specification editorial: `-name-birth-data-requirement` and `-name-cmd-requirement` are timestamp clauses carrying `name` ids | 6 | recorded above; upstream, not ours |

## Tally for chapter 4

**14 conformant · 0 deviations · 6 gaps · 21 n/a** (16 Host Application, 3 messages we do not emit,
2 command clauses that bind a Host Application publisher)

`14 + 0 + 6 + 21 = 41` rows. **This corrects a miscount**: the tally read `17 · 0 · 8 · 21` until the
code review of Story 4.2 recounted the rows mechanically — the conformant and n/a figures were
over-stated, and two of the gaps (`topics-ncmd-mqtt`, `topics-dcmd-mqtt`) then became `n/a`.

The 6 gaps are all `gap (unimplemented)`: two uniqueness checks ([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27), Epic 3),
two DDEATH topics (Epic 3), two command-topic subscriptions (Story 4.6).

**41 rows is not the chapter's clause set.** `Sparkplug_4_Topics.adoc` carries **70** `tck-id`s, so
**27 are recorded nowhere** — among them `topics-nbirth-metrics`, `-nbirth-seq-num`,
`-nbirth-timestamp`, `topics-ndeath-payload`, `-ndeath-seq`, `topics-ddata-seq-num`, and three
`host-topic-phid-death-payload-timestamp-*` ids the STATE block omits. Most pointedly,
**`tck-id-topics-nbirth-bdseq-increment` is absent** — chapter 4's own id for the per-CONNECT
`bdSeq` increment, the very deviation chapter 6 records under Story 4.10. Chapter 4 was audited
before this matrix required a countable pass; completing it is deferred work, recorded in
`_bmad-output/implementation-artifacts/deferred-work.md`. **The Status table says "audited, not
complete" for exactly this reason** — it read `done` until the code review of Story 4.2 applied
chapter 6's own completeness check to chapter 4 and found it failed.

Every chapter-4 `conformant` row names a test. No row is asserted from reading the code alone.

## Tally for chapter 6

**30 conformant · 5 deviations · 15 gaps · 59 n/a**

`30 + 5 + 15 + 59 = 109` — the enumerated clause set, with no remainder.

**The count of 109 is a count of ids, not of requirements.** Two of them,
`payloads-sequence-num-req-nbirth` and `-zero-nbirth`, are one clause under two spellings (see the
editorial note at the head of this chapter), and both hold a `conformant` row. So **30 conformant is
29 distinct**, and the chapter states **108 distinct requirements**. The arithmetic is kept against
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

**The 15 gaps, split by kind** (see "How to read this"):

- **8 × `gap (unproven)`** — we do the thing; nothing proves it. Both property-set array-length
  clauses, the `engUnit` property's `type`, the quality property's `type`, `-propertyvalue-type-req`,
  the metric-level `timestamp`, the NBIRTH payload timestamp, and the will's retain flag. All
  [#30](https://github.com/guycorbaz/smartme_mqtt/issues/30).
- **7 × `gap (unimplemented)`** — we do not do it. Three DDEATH clauses and the DDEATH timestamp
  (Epic 3), the will's QoS ([#26](https://github.com/guycorbaz/smartme_mqtt/issues/26), Story 4.17),
  `Node Control/Rebirth` (Story 4.7), edge-node-descriptor uniqueness
  ([#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)).

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

Every `conformant` row names a test, or names the vendored protobuf schema where the schema makes
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
