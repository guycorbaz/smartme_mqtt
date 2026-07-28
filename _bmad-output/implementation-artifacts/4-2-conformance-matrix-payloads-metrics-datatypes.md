# Story 4.2: Conformance matrix — payload, metrics and datatype clauses

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As the maintainer,
I want the payload encoding audited against the specification,
so that the hand-rolled protobuf is trustworthy for reasons beyond "Ignition accepted it once".

## Acceptance Criteria

**AC1 — the clause rows exist**

**Given** the specification's payload and metric clauses
**When** `encode.rs`, `model.rs` and `datatype.rs` are walked against them
**Then** the matrix gains rows for: metric naming, datatype codes, `is_null` semantics, property sets, timestamp units and interpretation, and the `Quality` property
**And** the known deviation "no aliases, no templates, no DataSets" is recorded as a `deviation` with its rationale, not left implicit.

**AC2 — the quality row states its provenance**

**Given** the quality-code defect found in Epic 1
**When** the matrix records the `Quality` property row
**Then** it states explicitly that the *values* are host-defined and were established by measurement (`quality_code_probe`), not by reading a table — the failure mode that caused contract v1.

**AC3 — the pass is complete and countable**

**Given** the 109 `tck-id-payloads-*` clauses of chapter 6 — the whole set, verified to live in that chapter and nowhere else in the vendored specification
**When** the pass ends
**Then** every one of them is accounted for by a row or by a collective block that **names its member ids**, and the arithmetic `conformant + deviation + gap + n/a = 109` is stated in the matrix
**And** a clause satisfied by construction but exercised by no named test is recorded as a `gap`, not as a `conformant`
**And** a `gap` carries an owning story or epic where one exists, and a new issue where none does
**And** the Status table row for chapter 6 is updated.

*Added to `epics.md` on 2026-07-27 (see the amendment note there). The story file and the epic carry the same three criteria — if you find them disagreeing, the epic wins.*

## Tasks / Subtasks

- [x] **Task 1 — enumerate the clause set mechanically, not by reading** (AC: 3)
  - [x] Run the enumeration below and keep the output as the worklist. Hand-reading the chapter will miss clauses; the count is the contract.
    ```bash
    grep -oE 'tck-id-[A-Za-z0-9-]+' docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_6_Payloads.adoc \
      | sed 's/-$//' | sort -u
    ```
    Expected: **109 ids**. Two contain uppercase (`…-timestamp-in-UTC`, `…-metric-timestamp-in-UTC`) — a lowercase-only regex truncates them; that is why the pattern above is case-inclusive.
  - [x] The chapter boundary is clean, and this was verified rather than assumed: `grep -rl 'tck-id-payloads-'` over `docs/spec/sparkplug-b-3.0.0/chapters/` returns **only** `Sparkplug_6_Payloads.adoc`, and the same pattern over the whole vendored tree yields the same **109** ids. No `payloads-*` clause hides in another chapter, and chapter 6 holds nothing else. Re-run both if you doubt the worklist.
  - [x] Subtract the **8 chapter-6 ids already recorded** by Stories 4.1 / the quality fix: `payloads-propertyset-quality-value-type`, `-quality-value-value`, `payloads-nbirth-rebirth-req`, `payloads-ndeath-will-message-qos`, `-will-message-retain`, `-will-message-publisher`, `payloads-ndeath-seq`, `payloads-ndeath-bdseq`. **101 remain.**
  - [x] Re-verify each of the 8 existing rows rather than trusting them: they were written before the full chapter was walked.
  - [x] **Clause budget — the 101 are allocated to tasks with no overlap and no remainder.** Check your worklist against it before starting; if it does not reconcile, the enumeration is wrong, not the budget.

    | Task | Clauses | |
    | --- | ---: | --- |
    | 2 — metric identity | 7 | `name-*` 3, `metric-datatype-*` 4 |
    | 3 — property sets | 5 | `propertyset-*-array-size` 2, `metric-propertyvalue-type-*` 3 (+2 quality rows re-verified, already counted in the 8) |
    | 4 — timestamps | 9 | `timestamp-in-UTC`, `metric-timestamp-in-UTC`, 7 per-message-type |
    | 5 — per-message-type | 44 | 37 message clauses + 7 `state-*` |
    | 6 — scope limit | 36 | `template-*` 26, `dataset-*` 7, `alias-*` 3 |
    | **Total** | **101** | + 8 already recorded = **109** |

- [x] **Task 2 — metric identity: naming, alias, datatype, `is_null`** (AC: 1)
  - [x] `payloads-name-requirement`, `-name-birth-data-requirement`, `-name-cmd-requirement` against `Metric.name` (`model.rs:125`) and the bridge's fixed names `Power` / `Energy` / `Contract/Version` (`sparkplug_publisher.rs:76-84`).
  - [x] While there: `payloads-name-requirement` is **conditional** — *"The name MUST be included with every metric unless aliases are being used"* (`Sparkplug_6_Payloads.adoc:453`). We never use aliases, so the condition always fires and the MUST always applies; say so rather than letting the row imply the clause is unconditional. The accompanying prose discourages a list of special characters that **does not include `/`**, so `Contract/Version` is legal — but note in the row that the spec reserves the `Node Control/…` and `Properties/…` namespaces by convention (`:1116-1151`) and that `Contract/Version` is **our invention in the same shape**, not a spec-sanctioned name. A reader must not infer blessing from resemblance.
  - [x] `payloads-metric-datatype-req`, `-datatype-not-req`, `-datatype-value`, `-datatype-value-type` against `DataType` (`datatype.rs:17-51`) and `encode_metric` (`encode.rs:243`), which always sets `datatype`. **See "A defect found while drafting" below — `-datatype-not-req` is not a formality here.**
  - [x] `is_null` semantics: `encode.rs:235-238` sets `is_null: Some(true)` and omits the value; the type survives via `MetricValue::Null(DataType)`. Proof: `encode.rs::a_null_metric_carries_no_value_but_keeps_its_datatype`. **`is_null` has no `tck-id`** — see the ruling below; do not invent one.
  - [x] The three `payloads-alias-*` ids — see Task 6.

- [x] **Task 3 — property sets** (AC: 1, 2)
  - [x] `payloads-propertyset-keys-array-size`, `-values-array-size` against `encode_properties` (`encode.rs:273-288`), which pushes keys and values in lockstep so the arrays cannot diverge. Name the test that proves it, or mark `gap` — the equal-length invariant may currently be implied by construction and asserted by nothing.
  - [x] `payloads-metric-propertyvalue-type-req`, `-type-type`, `-type-value` against `int_property` / `string_property` (`encode.rs:290-306`).
  - [x] Re-verify the two existing quality rows and extend the prose to discharge **AC2**: the codes on the wire are `0x8000_0000`-family Ignition values, established by publishing six tags with identical values and differing quality codes to a real Ignition 8.3.7 and reading back what it displayed (`ignition_contract.rs::quality_code_probe`, ADR 0012, story 1-15). Say the measurement happened; do not restate the table as if it were read from one.

- [x] **Task 4 — timestamps: units and interpretation** (AC: 1)
  - [x] `payloads-timestamp-in-UTC` (payload level) and `payloads-metric-timestamp-in-UTC` (metric level).
  - [x] Record the *interpretation*, not only the unit: the payload carries epoch-milliseconds `u64`; the metric timestamp is the reading's own `ValueDate`, never `now` — the anti-replay invariant. The DEATH timestamp is the moment the payload was **built**, not the moment of death (`encode.rs:168-172`), which a consumer must read as "no later than this".
  - [x] The 7 per-message-type timestamp clauses belong to **this** task, not Task 5: `payloads-nbirth-timestamp`, `-dbirth-timestamp`, `-ndata-timestamp`, `-ddata-timestamp`, `-ddeath-timestamp`, `-ncmd-timestamp`, `-dcmd-timestamp`. **Task 4 total: 9 clauses.**

- [x] **Task 5 — per-message-type payload clauses, excluding timestamps** (AC: 1, 3)
  - [x] `nbirth` 6 · `dbirth` 5 · `ddata` 5 · `ndata` 5 · `ddeath` 3 · `ncmd` 3 · `dcmd` 3 · `sequence` 4 · `ndeath` 3 = **37 clauses**, plus the 7 `state-*` below.
  - [x] **Do not drop a clause because chapter 4 already covers the same behaviour.** `payloads-nbirth-qos` and `topics-nbirth-mqtt` are two clauses; the matrix is keyed by `tck-id`, so both get a row. Cross-reference the proof instead of duplicating the analysis.
  - [x] `payloads-ndeath-will-message-publisher-disconnect-mqtt311` / `-mqtt50`: the bridge speaks **MQTT 3.1.1**, so the 3.1.1 clause applies and the 5.0 one is `n/a`. The evidence, so you can re-check it rather than take it from here: `rumqttc = "0.25"` (`Cargo.toml:42`) and the driver imports `rumqttc::{AsyncClient, MqttOptions, …}` — **not** `rumqttc::v5::*`, which is where that crate puts its MQTT 5 types (`mqtt_driver.rs:43,156`). ADR 0011's "never a clean DISCONNECT, drop the socket" is very likely `conformant` — name `chaos_sigterm_no_lie` as the proof.
  - [x] **`payloads-nbirth-edge-node-descriptor` is the same requirement as a gap already recorded.** It reads *"Every Edge Node Descriptor in any Sparkplug infrastructure MUST be unique in the system"* (`:1067`) — which is `topic-structure-namespace-unique-edge-node-descriptor` restated in chapter 6, already a `gap` under **issue #27**. Point the new row at **#27**; do not open a second issue for one requirement stated twice.
  - [x] `ndata` (6) is `n/a`: the bridge carries no node-level measurement and never emits NDATA — consistent with the chapter-4 rows.
  - [x] `ddeath` (4) is a `gap` owned by Epic 3, matching `topics-ddeath-topic`.
  - [x] `ncmd` / `dcmd` (8) are `gap`s owned by Story 4.6, matching the chapter-4 rows.
  - [x] The 7 `payloads-state-*` ids are `n/a` — Host Application clauses, exactly like chapter 4's `host-topic-phid-*`. List them collectively, once.

- [x] **Task 6 — the scope-limit deviation: no aliases, no templates, no DataSets** (AC: 1)
  - [x] Write **one `deviation` row** named for the scope limit, carrying the rationale and its `deferred-work.md` entry (code review of 1-8, 2026-07-25: "Device-level messages, metric aliases, templates/datasets … out of the walking skeleton's scope").
  - [x] List the **36 clauses it covers collectively as `n/a` pointing at that row**: 26 `template-*`, 7 `dataset-*`, 3 `alias-*`. Rationale for the split — the AC requires the limit be an explicit deviation rather than implicit silence, and one named row achieves that; marking all 36 individually as `deviation` would inflate the deviation count and misstate what a deviation is (we do not *do otherwise* than the clause, the clause never fires). This is a drafting decision, taken here rather than left to the dev agent.
  - [x] Note in the row that `encode_metric` hard-codes `alias: None` (`encode.rs:241`) — the feature is not merely unused, it is unreachable.

- [x] **Task 7 — findings, tally, status** (AC: 3)
  - [x] Extend **Findings carried forward** with everything this pass turns up. Anything that is a defect rather than a gap gets an issue (`gh issue create`) per CLAUDE.md.
  - [x] Add a **chapter-6 tally** in the shape of the chapter-4 one: `N conformant · N deviations · N gaps · N n/a`, with the n/a broken down.
  - [x] Update the **Status table** row for chapter 6 from "in progress" to "done", and correct its description.
  - [x] Reconcile: `conformant + deviation + gap + n/a` must equal **109** for chapter 6. State the arithmetic; an audit whose numbers do not add up has not been completed.

- [x] **Task 8 — verification** (AC: 3)
  - [x] Diff the enumerated 109 ids against the ids present in `docs/sparkplug-conformance.md` and confirm the empty set. A collective block counts as present only if it **names its ids** — compare ids, never headings:
    ```bash
    comm -23 \
      <(grep -oE 'tck-id-[A-Za-z0-9-]+' docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_6_Payloads.adoc | sed 's/-$//' | sort -u) \
      <(grep -oE 'tck-id-[A-Za-z0-9-]+|`payloads-[A-Za-z0-9-]+`' docs/sparkplug-conformance.md \
          | tr -d '`' | sed 's/^payloads-/tck-id-payloads-/' | sort -u)
    ```
    (The matrix writes some ids without the `tck-id-` prefix inside its tables; the second branch normalises both spellings. Empty output is the pass.)
    **This check was falsified before being written into the story**: run against the matrix as it stands today it returns exactly **101** ids and lists none of the 8 already recorded. It therefore discriminates present from absent, rather than reporting whatever it is pointed at. Re-run it once at the start to see it red, so you know it is armed.
  - [x] Re-read **"How this audit could pass wrongly"** and check the finished section against all five modes. This is the step that makes the previous one mean something.
  - [x] `./scripts/ci-local.sh --fast` — this story should change no Rust, so the run is a regression check, not a gate on new behaviour.
  - [x] `git diff -- crates/*/src/` must be empty. See "Scope boundaries" below.

### Review Findings

Code review of 2026-07-28. Three layers: Blind Hunter (diff only), Edge Case Hunter (diff + project),
Acceptance Auditor (diff + story + CLAUDE.md). 29 raw findings → 22 kept after dedup, 7 dismissed.

**All three ACs were independently re-verified and hold**: the 109-clause enumeration, the chapter
boundary, the coverage set-difference in both directions (0 missing, 0 invented), the tally
arithmetic, the existence of all 34 cited tests, the six open issues, all 25 spec line citations,
and both claimed specification editorial defects. What follows is what survived that.

**Decisions — all five resolved by Guy on 2026-07-28.** Resolutions are recorded inline against each
finding below; each has become patch work.

- [x] [Review][Decision] **The "schema witness" rule was added to the shared verdict rules without an ADR or issue, and its stated rationale is factually false** — the diff inserts into "How to read this" (which governs chapter 4 as well as 6) a new admissible witness: the vendored `sparkplug_b.proto`, justified because "the witness is **external to this repository**". It is not: it is vendored *inside* the repo at `crates/sparkplug-b/proto/sparkplug_b.proto`, editable by the same hands as the code shape the rule refuses to trust. CLAUDE.md requires an ADR + issue for anything that moves a requirement, and the story calls the inherited verdict rules "non-negotiable". Exactly one row depends on it — `payloads-metric-propertyvalue-type-type`, the only `conformant` in the chapter naming no test. Under the inherited rule the chapter-6 tally is **33 conformant / 12 gaps**, not 34/11. — **RESOLVED: keep the rule, fix its justification, add ADR + issue.** The load-bearing property is *unrepresentability against the normative schema* (it fails at compile time, which is stronger than a test), not externality of location. Scope stays explicit: the schema witnesses **field types**, never **values**. Tally unchanged at 34/11.
- [x] [Review][Decision] **NCMD/DCMD `n/a` contradicts chapter 4's `gap` for the same obligation, inside the same document** — the reclassification is defended by "chapter 4's `topics-ncmd-topic` / `-mqtt` are `gap`s *rightly*, because subscribing is an Edge Node obligation". That covers `-ncmd-topic`. It does not cover `-ncmd-mqtt`: `Sparkplug_4_Topics.adoc:344-345` reads "NCMD messages **MUST be published** with MQTT QoS equal to 0 and retain equal to false" — word for word the same publish-side obligation as `payloads-ncmd-qos` / `-retain`, now `gap` in one table and `n/a` in another. Same for `topics-dcmd-mqtt` (`:508-509`). Either the chapter-4 `-mqtt` rows move to `n/a`, or the chapter-6 reclassification is wrong. (Also: story Task 5 said "`ncmd` / `dcmd` (8) are `gap`s owned by Story 4.6" and the box stayed `[x]` while the delivered verdict reversed it. The reasoning for reversing is sound — `:1411` and `:1455` say what the diff quotes — but the checkbox asserts the instruction was followed as written.) — **RESOLVED: chapter 4's `topics-ncmd-mqtt` / `topics-dcmd-mqtt` move to `n/a`**, matching chapter 6. Verified safe during the review: the "we do not subscribe" obligation has its own clause — `tck-id-message-flow-edge-node-ncmd-subscribe` (`Sparkplug_5_Operational_Behavior.adoc:158`) and `-device-dcmd-subscribe` (`:403`), both chapter 5, so both owned by Story 4.3 — and `topics-ncmd-topic` / `-dcmd-topic` stay `gap` because a subscriber must construct the topic form too. The unimplemented command path stays visible.
- [x] [Review][Decision] **NDATA is `n/a` under reasoning the same chapter rejects for DDEATH** — NDATA: "the bridge carries no node-level measurement and never emits NDATA … the topic machinery supports it; nothing calls it". DDEATH, two sections later: "**Gap, not n/a, and the distinction is deliberate.** A device *can* die while its node lives; with one meter it simply never has, which is a deployment fact rather than a role we do not play." Emitting NDATA is likewise a role the bridge could play and does not. 6 clauses turn on this; moving them to `gap` would also reduce the `n/a` excess flagged under failure mode 2. — **RESOLVED: verdicts stand; the missing *criterion* gets written down.** The test is whether the bridge holds the data or event the message type exists to carry. DDEATH: yes — meter unreachability is already detected and drives the stale/bad quality verdict, so we have the event and do not emit → `gap`. NDATA: no — the node's only metric is `Contract/Version`, a session-lifetime constant (`sparkplug_publisher.rs:243,251`), so nothing could ever change → `n/a`, and consistent with chapter 4's existing `topics-ndata-*` `n/a` rows. Falsification condition goes in the row: if the node ever gains a mutable metric, the 6 NDATA clauses become `gap`.
- [x] [Review][Decision] **Is an ADR owed now for the DDATA / re-declaring-DBIRTH timestamp deviation, or is #29 sufficient ownership?** — the document says "[#29] carries the decision, and it **likely** needs an ADR: this is an architectural position, not a bug", and the Findings table repeats "Likely needs an ADR". The quality-code deviation of identical weight got ADR 0012. No story in Epic 4 owns writing this one. CLAUDE.md's rule against deferring a decision to an artifact that does not exist is adjacent, if not squarely hit. — **RESOLVED: write the ADR now.** The position is taken, implemented and contrary to two MUSTs; ADR 0012 is the identical precedent; and "likely needs an ADR" is the exact shape CLAUDE.md names as having already cost (AR13 deferred a decision to an artifact that did not exist for all of Epic 1). Nothing to decide, only to record.
- [x] [Review][Decision] **`gap` is doing two incompatible jobs and the split flatters the deviation count** — the new AC3 defines `gap` as "satisfied by construction but exercised by no named test", but 6 of the 11 gaps are the opposite: clauses the bridge actively fails (`-ndeath-will-message-qos`, MUST QoS 1 vs a will at QoS 0; `-nbirth-rebirth-req`; three DDEATH clauses; `-ddeath-timestamp`). A reader of "every gap carries an owning story, epic or issue" cannot tell an untested-but-correct row from a broken one. Splitting the bucket would touch chapter 4 too. — **RESOLVED: sub-label as `gap (unproven)` / `gap (unimplemented)` in both chapters.** The inherited definition already covers both cases ("We do not do it, **or** nothing proves that we do"), so this is a labelling change, not a verdict change: no ADR, no re-audit, no number moves.

**Patches** (fix is unambiguous):

- [x] [Review][Patch] `payloads-propertyset-quality-value-type` is `conformant` on a test that asserts the same expression as production — the clause names a literal (`Sparkplug_6_Payloads.adoc:631-632`, "a value of **3**"); production is `r#type: Some(DataType::Int32.code())` (`encode.rs:292`) and the cited proof is `assert_eq!(props.values[0].r#type, Some(DataType::Int32.code()))` (`encode.rs:435-437`). `codes_match_the_specification_numbering` (`datatype.rs:66-74`) pins 1, 4, 8-13, 17 — **`Int32` is absent**. Change `Int32 = 3` (`datatype.rs:23`) to any other discriminant and the whole suite stays green while the wire violates a MUST. This is one of the 8 rows Task 1 required be re-verified rather than trusted, and it is the same self-consistency shape that shipped contract v1. → `gap`, fifth entry on #30. It also undercuts the `-propertyvalue-type-value` row, whose rationale credits "the `Int32` half is asserted". [docs/sparkplug-conformance.md]
- [x] [Review][Patch] `payloads-metric-propertyvalue-type-req` is `conformant` on the evidence the adjacent row uses to justify a `gap` — the `-type-value` row states "delete the `r#type` line from `string_property` (`encode.rs:300`) and the suite stays green". That mutation *is* a falsification of `-type-req`: with the line gone, `string_property` no longer "always sets `type`", and nothing went red. It is also the case the diff's own new preamble forbids ("a field we happen to always set … the verdict is `gap` until a test says otherwise"). Secondary: the clause is MUST for **NBIRTH/DBIRTH** (`:593-594`) while the cited test asserts on a DATA payload, bridged by code-shape reasoning. [docs/sparkplug-conformance.md]
- [x] [Review][Patch] `payloads-metric-timestamp-in-UTC` is `conformant` on proofs the document itself says do not exist — the row cites "same proofs" as the payload-level row, i.e. two smart-me date-*parsing* tests. Three lines below, `-name-birth-data-requirement` is honestly a `gap` because "**every timestamp assertion in the tree is payload-level**; nothing reads an encoded metric's `timestamp` field", and the story's own mutation confirms it (drop `encode_metric`'s timestamp → **green**). If nothing reads the field, no test witnesses that it is UTC. → `gap`. (`payloads-timestamp-in-UTC` is weaker for the same reason but is at least read by `chaos_sigterm_no_lie`; consider qualifying it.) [docs/sparkplug-conformance.md]
- [x] [Review][Patch] `payloads-nbirth-timestamp` is `conformant` on a presence check — the clause requires a timestamp "that denotes the time at which the message was published" (`:1064-1066`). `chaos_sigterm_no_lie.rs:274` only unwraps the field; `:331-332` bounds it from above via `death_stamp > birth_stamp`. `cold_start_birth_declares_tags_with_no_value_and_stale_quality` asserts nothing about it. Replace `clock.wall()` at `mqtt_driver.rs:178` with a small constant and everything stays green — indeed passes *more* easily. → `gap`, or qualify the row. [docs/sparkplug-conformance.md]
- [x] [Review][Patch] `payloads-ndeath-will-message-retain` cites two witnesses the document elsewhere says do not observe the will — the Findings table says `every_edge_node_message_is_qos_zero_and_never_retained` is "false for the will", and the `-will-message-publisher` row says `chaos_sigterm_no_lie` proves the *explicit* death rather than the will. The one test the document credits with observing the broker-published will, `chaos_stale_on_death` (SIGKILL), is not cited here. Read that test: if it asserts retain, cite it; if not, the row is a `gap`. [docs/sparkplug-conformance.md]
- [x] [Review][Patch] The mutation count is wrong in three places, and the summary drops the one result that complicates it — the closing paragraph says "**Five** verdicts … were checked by mutation" with a tidy red=conformant / green=gap symmetry, but the Property-sets table records a **sixth** whose result is red *for a `gap` row*; `sprint-status.yaml` in the same diff says "**six** mutations run". The summary also says the unpaired PropertySet value "**were removed**" where the table says it was *appended*. [docs/sparkplug-conformance.md, sprint-status.yaml]
- [x] [Review][Patch] A sixth `deviation` verdict renders in the chapter while the tally says five — the scope-limit row's Verdict column reads `**deviation**` and is correctly excluded (it is not a `tck-id` row), but unlike the prose-only table it is never *declared* out-of-tally. A reader counting rendered verdicts gets 6 and concludes the arithmetic does not close. One sentence fixes it. [docs/sparkplug-conformance.md]
- [x] [Review][Patch] "34 conformant" double-counts the phantom clause, and the 109/108 caveat lives two sections from the number it qualifies — the document establishes that `-sequence-num-req-nbirth` / `-zero-nbirth` are one clause with two spellings ("**108 distinct requirements**"), then gives both a `conformant` row and states "`34 + 5 + 11 + 59 = 109` … with no remainder" without annotation. 34 conformant is 33 distinct. Annotate at the tally. [docs/sparkplug-conformance.md]
- [x] [Review][Patch] "The coverage check below" is not below, or anywhere — the last line of the file reads "The coverage check below was likewise armed against `HEAD` … it reported 101 missing ids there and 0 here". The enumeration `grep` is shown; the set-difference that produced 101/0 is not, and `epics.md` points elsewhere for it. The single strongest completeness claim is the one asserted rather than shown, in a document whose thesis is that assertion is the failure mode. (The claim is *true* — this review reproduced 0/0 independently — so show the command, or reword.) [docs/sparkplug-conformance.md]
- [x] [Review][Patch] Chapter 4's assurance line was removed and reinstated only under chapter 6 — the diff deletes "Every `conformant` row names a test. No row is asserted from reading the code alone." from beneath the chapter-4 tally and restores it, in amended schema-witness form, beneath chapter 6's. Chapter 4 is `done` and now carries no such claim. [docs/sparkplug-conformance.md]
- [x] [Review][Patch] Off-by-one citation: `seq: None,` is at `encode.rs:219`; `:220` is `uuid: None,`. Sole miss among ~30 code citations, but the document's authority rests on citations being checkable. [docs/sparkplug-conformance.md]
- [x] [Review][Patch] The `-ncmd-seq` receive-side note says the obligation has "no `tck-id` of its own" — it has one: `tck-id-payloads-ncmd-seq` (`:1417-1418`), the same clause read from the receiving side, which is the id the table files as `n/a` three lines earlier. The sentence contradicts its own table entry. [docs/sparkplug-conformance.md]
- [x] [Review][Patch] `epics.md` AC3 is missing a clause the story file carries, while the story asserts they are identical — the story ends AC3 with "**And** the Status table row for chapter 6 is updated"; the `epics.md` text added by this diff stops one line earlier. The story says "if you find them disagreeing, the epic wins", so the governing document is the one missing the obligation. The work satisfied it anyway. [_bmad-output/planning-artifacts/epics.md]
- [x] [Review][Patch] The confounded first attempt at the sixth mutation appears only in the story file — the Dev Agent Record records that the first run deleted `values.push(string_property(unit))` and went red for the wrong reason (index-out-of-bounds in the `unit_of` helper, not a length assertion). The delivered matrix shows only the clean re-run. CLAUDE.md's "state how it could pass wrongly" ethos, which this document otherwise applies meticulously, wants the near-miss where the matrix's reader can see it. [docs/sparkplug-conformance.md]
- [x] [Review][Patch] The carried-forward finding credits `every_edge_node_message_is_qos_zero_and_never_retained` with a per-type discrimination it does not have — `qos_for(_message: MessageType)` ignores its argument (`mqtt_driver.rs:123-125`), so the six-type loop is one assertion repeated six times. The verdicts still hold (both call sites derive from that function, so mutating it goes red), but if `qos_for` ever grows a real `match`, five of the six retain verdicts revert to unproven with no test change to signal it. Worth a clause in the finding row. [docs/sparkplug-conformance.md]
- [x] [Review][Patch] The `sprint-status.yaml` comment has an ambiguous referent — "the file above said 'ready-for-dev' while this line said 'in-progress'" reads, in a YAML file, as pointing at something above in that file. Name the story file. [_bmad-output/implementation-artifacts/sprint-status.yaml]

**Deferred** (pre-existing, not caused by this change):

- [x] [Review][Defer] **Chapter 4 is marked `done` but fails the completeness standard this pass invents** — deferred, pre-existing (Story 4.1). Applying the same two mechanical checks to `Sparkplug_4_Topics.adoc`: it holds **70** `tck-id`s, of which **27 have no row and appear in no collective block** (`topics-nbirth-metrics`, `-nbirth-seq-num`, `-nbirth-timestamp`, `topics-ndeath-payload`, `-ndeath-seq`, `topics-ddata-seq-num`, three `host-topic-phid-death-payload-timestamp-*`, …), and the stated tally "17 conformant · 0 deviations · 8 gaps · 21 n/a" does not match the rows, which count **14 · 0 · 8 · 19**. Most pointedly, `tck-id-topics-nbirth-bdseq-increment` is unrecorded — chapter 4's own id for the per-CONNECT `bdSeq` increment, i.e. the exact Story-4.10 deviation this chapter-6 pass presents as a discovery. Not introduced here, but this diff reaffirms `chapter 4 | 4.1 | **done**` and adds a general completeness AC that chapter 4 fails.

**Dismissed** (7): the `-nbirth-bdseq-repeat` / `-disconnect-mqtt311` vacuity asymmetry (the MQTT 3.1.1 antecedent genuinely never fires — verified against `:1531` — so the two cases are not alike); matrix "done" vs sprint "review" (normal workflow); AC3 having been written alongside the work it governs; "roughly 50 / the 9 excess" as spurious precision; the `-nbirth-edge-node-descriptor` example allegedly not demonstrating independent reading (mode 4 was defeated elsewhere, on NCMD/DCMD); Task 1's per-task budget shifting 7/9 → 5/11 (disclosed at length, total reconciles); the `Contract/Version` and `engUnit` notes delivered as prose beneath the table rather than inside the row (content complete).

## Dev Notes

### What this story is, and is not

It is an **audit that records**. It does not fix. Every defect it finds becomes a row plus an issue or an owning story — the fixes are Stories 4.6, 4.7, 4.17, Epic 3. Resist turning a `gap` into a `conformant` by writing the missing test inside this story: that is scope creep into stories that already exist, and it hides the size of the gap the audit is measuring.

**No production code changes.** `git diff -- crates/*/src/` is empty at the end. If the audit finds something that genuinely cannot wait, raise it and stop — do not fold a fix into a documentation commit.

### How this audit could pass wrongly

`CLAUDE.md` requires every human-run gate to state what *else* could make it pass. An audit conducted by an agent is exactly such a gate, and its failure mode is not a red test — it is a document that looks complete. Five ways this pass reports success without having done the work:

1. **A `conformant` naming a test that does not exercise the clause.** The precedent is already in the matrix: `every_edge_node_message_is_qos_zero_and_never_retained` is cited for six message types and, per the findings table, over-claims for the will. Citing it for `payloads-ncmd-qos` would be worse still — it cannot prove anything about a message the bridge never publishes. For each `conformant`, ask what the named test would do if the behaviour were removed. If the answer is "still pass", the row is a `gap`.
2. **`n/a` used as a dustbin.** `n/a` means the role, message, or feature genuinely does not apply — not that the clause is awkward. Expect roughly **50 legitimate `n/a`** in chapter 6 (36 scope-limit + 7 `state-*` + 6 `ndata-*` + `-disconnect-mqtt50`). A count materially above that is a smell; justify each extra one in the row.
3. **A collective block that hides its members.** Task 8's diff must compare **ids**, not headings. A block reading "the 26 template clauses" satisfies a reader and fails the check; a block that lists its 26 ids satisfies both.
4. **A verdict copied from its chapter-4 twin.** `payloads-nbirth-qos` and `topics-nbirth-mqtt` say similar things about the same message, and `payloads-nbirth-edge-node-descriptor` restates a chapter-4 clause outright — but similar is not identical, and the twin's verdict was reached against a different clause text. Read chapter 6's wording each time.
5. **The 8 existing rows "re-verified" by re-reading the matrix.** Re-verification means going back to the specification and the code. Reading our own previous conclusion and agreeing with it is the self-consistency trap the Epic 1 retrospective named.

### Scope boundary against Story 4.3 — decided here

Chapter 6 contains clauses about sequencing, ordering and lifecycle (`payloads-sequence-*`, `-nbirth-seq`, `-ddata-seq-inc`, `-dbirth-order`…) that read like Story 4.3's subject matter. The split is **by chapter, not by theme**:

- **4.2 owns every `tck-id-payloads-*` clause**, including the lifecycle-flavoured ones. The matrix is organised by chapter; a chapter-6 id filed under a chapter-2/5 heading is unfindable.
- **4.3 owns chapters 2 and 5**, and the cross-chapter *synthesis* of the lifecycle. Where it needs a chapter-6 clause it cross-references this pass rather than re-deciding it.

The Status table in the matrix already implies this split. Making it explicit here is what stops the two stories from either double-covering or leaving a hole between them.

### Verdict rules — inherited, non-negotiable

From Story 4.1, already encoded in the matrix's "How to read this":

| Verdict | Condition |
| --- | --- |
| `conformant` | We do what the clause requires **and a named test proves it** |
| `deviation` | We knowingly do otherwise; row carries rationale + ADR or deferred-work link |
| `gap` | We do not do it, or nothing proves that we do |
| `n/a` | The clause addresses a role we do not play, a message we do not emit, or a feature we do not use |

Two clarifications this pass needs:

1. **A `conformant` with no named test is a `gap`.** This will bite: several behaviours are correct by construction and asserted by nothing (the property-set array lengths are the likely case). Mark them `gap` and say "correct by construction, unproven" in the row. Downgrading honestly is the entire point — contract v1 shipped because 148 green tests all agreed with each other.
2. **Gap ownership.** Story 4.1's AC says "every `gap` row carries an issue number", but 4.1 in practice pointed gaps at an owning story or epic (`Story 4.6`, `Epic 3`) and raised an issue only for the *unowned* one (#27). Keep that: **an owning story or epic if one exists, otherwise a new issue.** An unowned, unnumbered gap is not acceptable.

### A defect found while drafting this story — `tck-id-payloads-metric-datatype-not-req`

> *"The datatype SHOULD NOT be included with metric definitions in NDATA, NCMD, DDATA, and DCMD messages."* — `Sparkplug_6_Payloads.adoc:491`

`encode_metric` sets `datatype: Some(metric.value.datatype().code())` **unconditionally** (`encode.rs:243`). There is one metric encoder for every message type, so DDATA — the bridge's highest-volume message — carries a datatype the specification says it should not. `payloads-metric-datatype-req` (the MUST, for NBIRTH/DBIRTH) is satisfied by the same line; the two clauses pull in opposite directions and the code only knows about one of them.

This is a **SHOULD NOT**, not a MUST NOT, so it is a `deviation` if we choose to keep it and a `gap` if we do not. Record it, with an issue, and let the decision belong to whoever picks up the fix — it needs a message-type-aware encoder, which is a code change and therefore not this story. Note the cost while you are there: a datatype field on every metric of every DDATA is wire overhead on the one message that repeats forever.

Found by reading the norm rather than the code, which is the whole argument for the vendored specification. Do not let it dissolve into a generic "datatype clauses: conformant" row.

### `is_null` has no `tck-id` — a ruling, not an omission

AC1 asks for `is_null` semantics rows, but chapter 6 describes `is_null` **only in prose** (lines 305, 503, 515, 595, 600) and attaches no `tck-testable` identifier to it. There is nothing to key a row on.

Do **not** invent an id, and do not silently drop the requirement. Record it as a **clearly-labelled non-`tck` row** — `(no tck-id — prose, ch. 6 §Metric)` in the id column — carrying the same five fields as every other row. CLAUDE.md says cite the `tck-id`, not prose; where the specification itself offers no id, saying so *is* the citation. The tally counts `tck-id` rows only, so this row sits outside the 109 and must be stated as such, or the arithmetic in Task 7 will not close.

The same treatment applies to any other prose-only obligation this pass turns up (`is_historical` and `is_transient` are the likely candidates — both are hard-coded `None` at `encode.rs:244-245`).

### Current state of the code being audited

Read these three files completely before writing a single row. They are small and there is no excuse for auditing them from memory.

**`crates/sparkplug-b/src/encode.rs`** (543 lines) — payload builders and the session lifecycle.
- `encode_metric` (`:234`) sets `name`, `timestamp`, `datatype`, `properties`, and `value`-or-`is_null`. It hard-codes `alias: None`, `is_historical: None`, `is_transient: None`, `metadata: None`.
- `encode_properties` (`:273`) returns `None` when there are no properties — "an empty property set says something different from no property set at all".
- `death_payload` (`:215`) omits `seq` deliberately and carries only `bdSeq`.
- `build_birth` (`:178`) resets the counter, prepends the `bdSeq` metric, takes `seq = 0`.
- Device messages share the node's single `seq` counter (`:142-163`).

**`crates/sparkplug-b/src/model.rs`** (261 lines) — `Quality` (with the spec-mandated `192/500/0`), `MetricValue` (deliberately no `Float32` variant), `Metric` with `with_quality` / `with_quality_code` / `with_engineering_unit`. `ENG_UNIT_KEY = "engUnit"` (`:147`) is a **convention, not a specification clause** — check whether chapter 6 says anything about it and, if it does not, say so rather than implying the spec blesses it.

**`crates/sparkplug-b/src/datatype.rs`** (77 lines) — `#[repr(u32)]` enum whose discriminants *are* the wire codes, so `code()` is a cast and cannot drift. Note the gaps in the range: `16` (DataSet) and `18+` are absent, which is itself part of the datatype-clause answer.

**Bridge side, for the naming and property rows:** `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs` — `CONTRACT_VERSION: i64 = 2` (`:39`), `ignition_quality_code` (`:62`), metric names `Contract/Version`, `Power` (kW), `Energy` (kWh) (`:76-84`).

### The Ignition quality deviation — already recorded, verify rather than rewrite

The matrix already carries both quality rows and calls `payloads-propertyset-quality-value-value` a **deviation** backed by ADR 0012. The generic crate publishes `192/500/0`; only the bridge deviates, via `Metric::with_quality_code`. Tests hold both sides apart: `sparkplug_publisher.rs::no_non_good_quality_can_be_mistaken_for_good_by_ignition` and `::the_generic_crate_still_publishes_the_specified_codes`. AC2 asks you to make the *provenance* explicit — that the Ignition codes came from `quality_code_probe` measuring a real host, not from a table someone read. Add that sentence; do not re-litigate the deviation.

### Traps specific to this story

- **Do not add bridge or Ignition context to `crates/sparkplug-b/`.** `tests/no_context_leak.rs` fails if `smartme` / `ignition` / `SMARTME_` appears in that crate's sources, and NFR19 wants its "Conformance scope" written for a stranger. `docs/sparkplug-conformance.md` is a bridge-repo document and names Ignition freely; the crate must not link to it.
- **The 8 pre-existing rows were written before the chapter was walked.** Re-verify, do not inherit.
- **`payloads-nbirth-bdseq-repeat` and `payloads-nbirth-bdseq`** interact with the *known* `bdSeq`-per-CONNECT deviation (fixed for a client's lifetime, `mqtt_driver.rs:30`, Story 4.10). Record the deviation here with its owner; do not describe the behaviour as conforming.
- **If you write any test at all**, CLAUDE.md's falsification rule applies without exception: break the code, watch it go red, record the falsification next to the test. But see "Scope boundaries" — you probably should not be writing one.
- **The manual** (`docs/manual/`) must track real behaviour. This story changes no behaviour, so it likely needs no edit — but if the audit changes what the manual *claims*, fix the manual in the same commit.

### Project Structure Notes

- Output is a single existing file: `docs/sparkplug-conformance.md`. Extend the "Chapter 6" section in place; keep 4.1's table shape (`tck-id | Level | Our behaviour | Proof | Verdict`) so the two chapters read as one document.
- Sub-headings within chapter 6, mirroring 4.1's `### Namespace structure` / `### Edge-node messages` style: metric identity · property sets · timestamps · per-message-type · scope limit (aliases/templates/DataSets) · Host Application STATE.
- Issues go on `guycorbaz/smartme_mqtt` via `gh issue create`.
- No `project-context.md` exists in this repo; `CLAUDE.md` is the standing rule set and outranks anything here that contradicts it.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md#Story 4.2`] — the two ACs, verbatim above
- [Source: `_bmad-output/planning-artifacts/epics.md:750`] — Stories 4.1–4.3 are the audit; the rest of Epic 4 may be reshaped by their findings
- [Source: `docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_6_Payloads.adoc`] — the clause set; v3.0.0, EPL-2.0
- [Source: `docs/sparkplug-conformance.md#Chapter 6`] — the 8 rows already present, and the verdict definitions
- [Source: `docs/adr/0012-quality-codes-spec-versus-host.md`] — the quality deviation
- [Source: `docs/adr/0011-graceful-shutdown-requires-both-deaths.md`] — bears on the `will-message-publisher-disconnect-*` clauses
- [Source: `_bmad-output/implementation-artifacts/deferred-work.md#code review of 1-8-sparkplug-b`] — the aliases/templates/DataSets scope limit
- [Source: `_bmad-output/implementation-artifacts/1-15-tier-3-ignition-contract-test-manual-runbook.md`] — `quality_code_probe`, and how the run nearly reported a false pass
- [Source: `_bmad-output/implementation-artifacts/epic-1-retro-2026-07-26.md#Self-consistency is not conformance`] — why a `conformant` needs an external witness
- [Source: `CLAUDE.md`] — read the norm first; cite `tck-id`s, not prose; falsify before trusting; ADR + issue for anything that moves a requirement
- [Source: `_bmad-output/planning-artifacts/architecture.md#Documentation`] — contract↔code drift is the worst-case lie; NFR19 conformance scope

## Dev Agent Record

### Agent Model Used

claude-opus-5[1m] (Claude Opus 5, 1M context)

### Debug Log References

**The chapter-6 section already existed, uncommitted, when this session started.** It was written on
2026-07-27 at 13:24 — after this story file was created at 11:53 — by an earlier session that never
checked a box or moved the status. So this run was not a drafting pass but a **verification pass**,
and the story's own "How this audit could pass wrongly" made that the right shape: a document that
looks complete is precisely the failure mode, and failure mode 5 forbids re-reading our own
conclusions and agreeing with them. Nothing below was accepted because the matrix asserted it.

| Check | Method | Result |
| --- | --- | --- |
| Clause enumeration | `grep` over `Sparkplug_6_Payloads.adoc` | 109 ids; boundary re-verified — only that file carries `payloads-*`, and the whole vendored tree yields the same 109 |
| Coverage (Task 8) | **Python set difference, not `comm`** | 0 missing, 0 invented |
| Coverage check armed | same check against `git show HEAD:` | **101 missing** — it discriminates |
| Tally | parsed every chapter-6 table row mechanically | 55 rows + 54 collective ids = 109; 34 conformant · 5 deviation · 11 gap · 59 n/a; no duplicate id |
| Cited tests exist | `grep "fn <name>"` over `crates/` | **34 of 34 exist** |
| Cited issues exist | anonymous GitHub REST API | #23, #26–#30 all open, titles match |
| Spec claims | read the vendored norm directly | both editorial defects, NCMD/DCMD wording, bdSeq increment, `-disconnect-mqtt311` conditional, quality codes — all confirmed verbatim |
| Code claims | read the cited lines | `alias: None`, unconditional `datatype`, `seq: None` in the will, `millis(value_date)` on DDATA, rumqttc 0.25 with no `v5` import — all confirmed |
| Five verdicts | **mutation testing** | see below |
| No production code | `git diff -- crates/` | empty |
| Regression | `./scripts/ci-local.sh --fast` + `cargo deny check` | green (`advisories ok, bans ok, licenses ok, sources ok`) |

**`comm` was abandoned mid-run.** The story's Task 8 command uses `comm`, and under this locale it
emitted *"le fichier 2 n'est pas dans l'ordre trié"* — a `comm` on mis-sorted input can report an
empty difference that means nothing. The check was redone as an order-independent set difference in
Python. **The story's shell command should not be trusted as written**; the Python form is recorded
here and belongs in any re-run.

**Mutation results** — five verdicts were checked by breaking the code and watching the suite, per
`CLAUDE.md`:

| Mutation | Expected | Observed |
| --- | --- | --- |
| `encode_metric` drops `name` | red (`-name-requirement` is conformant) | **red** ✅ |
| `int_property` drops `r#type` | red (`-propertyvalue-type-req` is conformant) | **red** ✅ |
| `encode_metric` drops `timestamp` | green (`-name-birth-data-requirement` is a gap) | **green** ✅ |
| `string_property` drops `r#type` | green (`-propertyvalue-type-value` is a gap) | **green** ✅ |
| unpaired PropertySet **value** | green (`-values-array-size` is a gap) | **green** ✅ |
| unpaired PropertySet **key** | green expected | **red** — see the finding below |

The sixth was run twice. The first attempt deleted `values.push(string_property(unit))`, which went
red — but for a confounded reason: it also removes the engineering unit, and the failure was an
index-out-of-bounds inside the test helper `unit_of`, not a length assertion. Re-run cleanly, by
appending a surplus key *after* the pairs so every existing lookup still resolves, it still went
red, this time because `a_birth_is_self_describing` pins `keys == ["Quality", "engUnit"]` exactly.
Sources were restored and `git diff -- crates/` confirmed empty after every mutation.

### Completion Notes List

- **All three ACs are satisfied.** AC1: rows exist for metric naming, datatype codes, `is_null`,
  property sets, timestamps and `Quality`, and the aliases/templates/DataSets limit is one named
  `deviation` with 36 `n/a`s pointing at it. AC2: the quality row states the codes were established
  by `quality_code_probe` measuring a real Ignition 8.3.7, not read from a table. AC3: all 109
  clauses are accounted for, the arithmetic `34 + 5 + 11 + 59 = 109` is stated and was re-derived
  mechanically, every `gap` carries an owning story, epic or issue, and the chapter-6 Status row
  reads **done**.
- **One correction made to the matrix, and it is the only one the verification forced.** The tally
  closed with *"nothing would notice if they broke"* about all four unproven encoder invariants.
  Mutation testing shows that is true of three but not of the fourth: a surplus PropertySet **key**
  turns the suite red. Not because anything asserts the clause — `a_birth_is_self_describing` merely
  pins one scenario's exact key list — but the blanket claim was stronger than the evidence. Both
  rows stay `gap` (neither clause has a test that proves it), the two proof cells now distinguish
  the keys side from the values side, and a mutation table records what was actually observed. This
  is the same over-claim the matrix legislates against in failure mode 1, pointing the other way.
- **The `n/a` count was scrutinised as the matrix invites.** 59 against a scoped expectation of ~50;
  the 9 excess are the NCMD/DCMD clauses. The reclassification is sound: chapter 6 states outright
  that *"NCMD messages are used by Host Applications to write to Edge Node outputs"* (`:1411`) and
  the same for DCMD (`:1455`), so these clauses bind a publisher the bridge never is. The
  unimplemented command path stays visible three times over, under Stories 4.6/4.7.
- **Both specification editorial defects were confirmed in the norm, not taken on trust.** At
  `:426` the AsciiDoc anchor reads `-sequence-num-req-nbirth` while the rendered id on the same line
  reads `-zero-nbirth`; and `-name-birth-data-requirement` / `-name-cmd-requirement` both hang off
  the `* *timestamp*` bullet and govern the timestamp despite their `name` ids.
- **`-metric-datatype-not-req` is a real deviation**, confirmed at `:491`: SHOULD NOT for DDATA,
  while `encode_metric` sets `datatype` unconditionally. Issue #28 exists and is open.
- **No production code changed and no test was written**, as the story requires. The audit records;
  the fixes belong to Stories 4.6, 4.7, 4.10, 4.17, Epic 3 and issues #28–#30.
- **The manual needs no edit.** It documents the `ValueDate`-based freshness behaviour that this
  audit records as a deviation from `-ddata-timestamp` — it describes real behaviour, which is what
  the standing order requires, and nothing in it is contradicted.
- **No new issue was opened by this pass.** The one finding refines an existing row and belongs to
  #30, which already asks for exactly these assertions.

### File List

- `docs/sparkplug-conformance.md` — modified (chapter 6; the two array-size proof cells, a mutation
  table under Property sets, and the tally's closing paragraph)
- `_bmad-output/implementation-artifacts/4-2-conformance-matrix-payloads-metrics-datatypes.md` —
  modified (tasks, Dev Agent Record, File List, Change Log, Status)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified (status → review)
- `_bmad-output/planning-artifacts/epics.md` — modified by the earlier session (AC3 amendment);
  carried into this story's commit unchanged

## Change Log

| Date | Change |
| --- | --- |
| 2026-07-27 | Chapter-6 audit drafted (earlier session, uncommitted, status never advanced) |
| 2026-07-28 | Verification pass: coverage re-derived order-independently, 34 cited tests and 6 issues confirmed to exist, spec and code claims re-read against the norm, six mutations run. One correction applied to the keys/values array-size rows and the tally's closing claim. Status → review |
| 2026-07-28 | Code review (3 adversarial layers). 22 findings kept, 7 dismissed. **Chapter-6 tally moved `34·5·11·59` → `30·5·15·59`** — four `conformant` rows downgraded to `gap (unproven)`, none a behaviour change. Chapter 4 recounted (`17·0·8·21` → `14·0·6·21`) and its Status changed from `done` to `audited, not complete`. ADR 0013 and ADR 0014 written. `gap` split into `(unproven)` / `(unimplemented)` across both chapters. Status → done |
