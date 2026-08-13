# Story 4.3: Conformance matrix — session lifecycle and host interaction

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As the maintainer,
I want the lifecycle and host-facing clauses audited,
so that the gaps we suspect are counted and the ones we do not suspect are found.

## Acceptance Criteria

**AC1 — the clause rows exist**

**Given** the specification's session and host-interaction clauses
**When** they are walked against the implementation
**Then** the matrix gains rows for: birth/death ordering, `seq` numbering and wrap, `bdSeq` per CONNECT, NDEATH via will and explicit publish, NCMD/`Node Control/Rebirth`, DDEATH, and the primary-host STATE mechanism
**And** the two gaps already known — NCMD/Rebirth unimplemented, STATE never considered — appear as `gap` rows pointing at Stories 4.4–4.8.

**AC2 — no gap is left unassigned**

**Given** the completed matrix
**When** Epic 4's remaining stories are reviewed against it
**Then** any newly discovered `gap` is either scheduled into this epic or recorded with an issue and an owning epic — no gap is left unassigned.

**AC3 — the pass closes the specification, not just a chapter**

**Given** the **124 clauses** that no story owns after 4.1 (chapter 4) and 4.2 (chapter 6) — chapters **1, 2, 3, 5 and 10** — verified by mechanical enumeration to be the exact remainder of the pinned specification's **303** ids
**When** the pass ends
**Then** every one of them is accounted for by a row or by a collective block that **names its member ids**, and the arithmetic `conformant + deviation + gap + n/a = 124` is stated in the matrix
**And** the matrix states the whole-specification total — `70 + 109 + 124 = 303` — so a reader can tell audited-in-full from audited-in-part
**And** a clause satisfied by construction but exercised by no named test is recorded as `gap (unproven)`, not `conformant`
**And** every `gap` carries an owning story, epic or issue
**And** the Status table rows for chapters 1, 2, 3, 5 and 10 exist and are updated.

*AC3 added 2026-07-28 while contexting the story; the epic amendment carries the same text. See "The scope decision" below — the epic scoped 4.3 to chapters 2 and 5 (103 clauses), which would have left **21 clauses owned by nobody**.*

## Tasks / Subtasks

- [x] **Task 1 — enumerate mechanically, and confirm the remainder is exactly 124** (AC: 3)
  - [x] Run this and keep the output as the worklist. Do not hand-read the chapters; the count is the contract.
    ```bash
    for f in 1_Introduction 2_Principles 3_Components 5_Operational_Behavior 10_Conformance; do
      grep -oE 'tck-id-[A-Za-z0-9-]+' \
        docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_$f.adoc | sed 's/-$//' | sort -u
    done | sort -u
    ```
    Expected: **124 ids.**
  - [x] **The chapter partition was verified, not assumed, and it is clean.** Per-chapter distinct counts are 8 · 4 · 1 · 70 · 99 · 109 · 12 (chapters 1, 2, 3, 4, 5, 6, 10) and they sum to **303**, which equals `grep -rhoE 'tck-id-[A-Za-z0-9-]+' docs/spec/sparkplug-b-3.0.0/ | sed 's/-$//' | sort -u | wc -l`. Because the sum equals the total, **no id appears in two chapters** — the same property 4.2 checked for chapter 6, established here for the whole tree. Chapters 7, 8, 9 and both appendices carry **zero** ids. Re-run both if you doubt the worklist.
  - [x] **Clause budget — the 124 are allocated with no overlap and no remainder.** Check your worklist against it before starting; if it does not reconcile, the enumeration is wrong, not the budget.

    | Task | Clauses | Families |
    | --- | ---: | --- |
    | 2 — edge-node session lifecycle | 31 | `operational-behavior-edge-*` 12, `message-flow-edge-node-*` 19 |
    | 3 — data behaviour and RBE | 24 | `operational-behavior-data-*` 24 |
    | 4 — device lifecycle | 9 | `operational-behavior-device-*` 1, `message-flow-device-birth-*` 7, `message-flow-device-dcmd-*` 1 |
    | 5 — Host Application and primary host | 34 | `operational-behavior-host-*` 22, `-primary-*` 4, `message-flow-phid-sparkplug-*` 6, `-hid-sparkplug-*` 1, `components-ph-state` 1 |
    | 6 — identifiers and case sensitivity | 10 | `intro-*` 8, `case-sensitivity-*` 2 |
    | 7 — principles | 4 | `principles-*` 4 |
    | 8 — conformance profiles | 12 | `conformance-*` 12 |
    | **Total** | **124** | + 70 (ch. 4) + 109 (ch. 6) = **303** |

- [x] **Task 2 — edge-node session lifecycle: births, deaths, reconnect, `seq`, `bdSeq`** (AC: 1)
  - [x] `operational-behavior-edge-*` (12) and `message-flow-edge-node-*` (19), against `crates/sparkplug-b/src/encode.rs`, `seq.rs`, and `crates/smartme-bridge/src/app/mqtt_driver.rs`.
  - [x] **`tck-id-message-flow-edge-node-ncmd-subscribe` (`Sparkplug_5_Operational_Behavior.adoc:158`) is this story's most important single row.** The 4.2 code review moved the publish-side NCMD/DCMD clauses to `n/a` in *both* chapters 4 and 6, precisely because they bind a Host Application publisher. **This is the clause that carries the real "we do not subscribe" gap**, and until this pass files it, that obligation is recorded nowhere as a first-class row. `gap (unimplemented)`, owner **Story 4.6**. Its DCMD twin is `-device-dcmd-subscribe` (`:403`) — note it requires **QoS 1** on the subscription, which Story 4.6 must honour.
  - [x] Cross-reference rather than re-decide: chapter 6's `payloads-nbirth-*`, `-ndeath-*` and `payloads-sequence-num-*` rows already rule on `seq`/`bdSeq` content. This pass rules on the **flow** — when a birth must be sent, what a reconnect owes. Where a chapter-5 clause restates a chapter-6 one, both get a row (the matrix is keyed by `tck-id`) and the later one cross-references the proof.
  - [x] **`bdSeq` per CONNECT is already a known deviation with an owner** — chapter 6 records it at `payloads-nbirth-bdseq-repeat` (Story 4.10): the will is serialised into `MqttOptions` once and `rumqttc` rebuilds every reconnect's CONNECT from that snapshot (`mqtt_driver.rs:29-30, 156-163`), so the number never increments. Any chapter-5 clause demanding a per-CONNECT increment is the *same* defect; point it at Story 4.10, do not open a second issue.
  - [x] **Chapter 4 has an unrecorded id for this exact requirement**: `tck-id-topics-nbirth-bdseq-increment`. It is one of the **29** chapter-4 clauses recorded nowhere, and they now belong to **Story 4.19** — so leave it alone. Cross-reference it if useful; do not absorb it.

- [x] **Task 3 — data behaviour, and the RBE clause the bridge does not satisfy** (AC: 1, 2)
  - [x] `operational-behavior-data-*` (24) against `crates/smartme-bridge/src/app/poll_publish.rs` and `core/state_machine.rs`.
  - [x] **See "A defect found while drafting" below — `tck-id-principles-rbe-recommended` is filed under Task 7 by id, but this task owns the behaviour it judges.** Rule on it once, in one place, and cross-reference from the other.

- [x] **Task 4 — device lifecycle** (AC: 1)
  - [x] `operational-behavior-device-*` (1), `message-flow-device-birth-*` (7), `message-flow-device-dcmd-*` (1).
  - [x] DDEATH is `gap (unimplemented)` owned by **Epic 3**, consistent with chapters 4 and 6. Record precisely what chapter 6 recorded: the **crate side is conformant and tested** (`LiveSession::device_death`, `encode.rs:155-163`, asserted by `device_messages_share_the_edge_node_numbering` and `a_device_death_carries_no_bdseq`); the gap is entirely in the bridge, which never calls it. Epic 3's work is a caller, not an encoder.

- [x] **Task 5 — Host Application and primary host: the biggest `n/a` block, and the one place it must not be reflexive** (AC: 1)
  - [x] `operational-behavior-host-*` (22), `-primary-*` (4), `message-flow-phid-sparkplug-*` (6), `-hid-sparkplug-*` (1), `components-ph-state` (1) = **34**.
  - [x] Most of these bind a Host Application and are `n/a` for the same reason chapter 4's `host-topic-phid-*` and chapter 6's `payloads-state-*` are. **List them collectively, by id, once.**
  - [x] **But `operational-behavior-primary-*` (4) and any clause that binds an *Edge Node's reaction* to a primary host are NOT `n/a`.** They are the substance of Stories 4.4–4.5 and must be `gap` rows pointing there. This is the distinction the whole STATE blind spot turns on: *what a Host publishes* is not our clause; *what an Edge Node must do when the Host goes offline* is. Read each of the 34 for which side it binds — do not classify by prefix.
  - [x] `tck-id-intro-sparkplug-host-state` (chapter 1, filed under Task 6 by id) is the same subject; cross-reference rather than deciding it twice.

- [x] **Task 6 — identifiers and case sensitivity** (AC: 1)
  - [x] `intro-*` (8) and `case-sensitivity-*` (2), against `crates/sparkplug-b/src/topic.rs::check_identifier` and the `EdgeNode`/`device_topic` constructors.
  - [x] Chapter 4 already rules on `topic-structure-namespace-valid-*` and the two uniqueness clauses. The `intro-*` ids are the **character and string** rules underneath them. Expect several to be discharged by the same `wildcards_and_separators_are_refused_in_every_element` test — but check the character sets **clause by clause**: the chapter-1 rules and chapter 4's `+`/`/`/`#` rejection are not obviously the same set, and assuming they are is failure mode 4.
  - [x] `tck-id-intro-edge-node-id-uniqueness` is very likely the same requirement as `topic-structure-namespace-unique-edge-node-descriptor` — already `gap (unimplemented)` under **[#27](https://github.com/guycorbaz/smartme_mqtt/issues/27)**. Point at #27; do not open a second issue for one requirement stated twice. (Chapter 6's `payloads-nbirth-edge-node-descriptor` is the third statement of it.)
  - [x] `case-sensitivity-metric-names` bears on the fixed names `Power` / `Energy` / `Contract/Version` (`sparkplug_publisher.rs:76-84`); `-sparkplug-ids` on group/node/device ids.

- [x] **Task 7 — principles: four clauses, and two of them bite** (AC: 1, 2)
  - [x] All four are quoted in full under "Two defects found while drafting" and "The clean-session clause" below. Rule on them against those readings, not from memory.
  - [x] `tck-id-principles-birth-certificates-order` — *"Birth Certificates MUST be the first MQTT messages published by any Edge Node"* (`:97-99`). `publish()` refuses before the birth (`Published::DroppedBeforeBirth`), asserted by `a_drop_before_the_birth_is_reported_not_silent` and observed on a real broker by `chaos_sigterm_no_lie`. Likely `conformant` — but ask what the named test would do if the guard were removed.
  - [x] `tck-id-principles-persistence-clean-session-311` — see below; expect `gap (unproven)`.
  - [x] `tck-id-principles-persistence-clean-session-50` — `n/a`, the bridge speaks MQTT 3.1.1. The evidence is already established and re-checkable: `rumqttc = "0.25"` (`Cargo.toml:42`) and the driver imports `rumqttc::{AsyncClient, …}`, **not** `rumqttc::v5::*` (`mqtt_driver.rs:43,156`).
  - [x] `tck-id-principles-rbe-recommended` — see below; a `deviation` if we keep the behaviour, a `gap` if we do not.

- [x] **Task 8 — conformance profiles: read them, do not assume `n/a`** (AC: 1)
  - [x] `conformance-*` (12). The chapter defines four profiles: **Sparkplug Edge Node** (`:35`), **Sparkplug Host Application** (`:42`), **Sparkplug Compliant MQTT Server** (`:53`), **Sparkplug Aware MQTT Server** (`:71`).
  - [x] **Verified while drafting: the Edge Node profile section carries no `tck-id` of its own** (`:35-41` is prose). The 12 ids sit under the Host Application profile (`conformance-primary-host`, 1) and the two MQTT Server profiles (`conformance-mqtt-*` and `conformance-mqtt-aware-*`, 11). So a blanket `n/a` is probably right — **and that is exactly why it must be justified per profile rather than waved through.** `n/a` for "we are not an MQTT Server" is legitimate; `n/a` because a clause is awkward is the dustbin failure.
  - [x] Record the consequence plainly: **the specification's Edge Node conformance profile imposes no additional testable clause beyond those already audited.** That is a useful finding for NFR19's "documented conformance scope" and belongs in the matrix, not only in this story.

- [x] **Task 9 — findings, tally, status** (AC: 2, 3)
  - [x] Extend **Findings carried forward** with everything this pass turns up, using the existing table's `Finding | Chapter | Where` shape. Anything that is a defect rather than a gap gets an issue (`gh issue create`) per CLAUDE.md.
  - [x] Add a **per-chapter tally** for each of 1, 2, 3, 5 and 10 in the shape of chapters 4 and 6, plus a **whole-specification total** discharging AC3's `70 + 109 + 124 = 303`.
  - [x] Add Status table rows for chapters 1, 2, 3 and 10 (they do not exist today — the table lists only 4, 6 and "2, 5"). Split the "2, 5" row.
  - [x] **AC2 is a real task, not a formality.** Walk Epic 4's remaining stories (4.4–4.18) against every new `gap`. Each one is either scheduled into an existing story, or gets an issue **and** an owning epic. An unowned, unnumbered gap is not acceptable.
  - [x] Reconcile: `conformant + deviation + gap + n/a` must equal **124**. State the arithmetic. An audit whose numbers do not add up has not been completed.

- [x] **Task 10 — verification** (AC: 3)
  - [x] Diff the enumerated 124 ids against those present in `docs/sparkplug-conformance.md`, **in both directions** — missing *and* invented. Use the Python form now printed in the matrix's chapter-6 tally, changing only the spec file list. **Do not use `comm`**: under this locale it emitted *"le fichier 2 n'est pas dans l'ordre trié"* during 4.2, and a `comm` over mis-sorted input can report an empty difference that means nothing.
  - [x] **Arm the check before trusting it.** Run it against `git show HEAD:docs/sparkplug-conformance.md` first and confirm it reports **124 missing**. A check that has not been seen red is not a check. (This is how 4.2's was validated: 101 missing at `HEAD`, 0 after.)
  - [x] Re-read **"How this audit could pass wrongly"** and check the finished sections against all six modes.
  - [x] `./scripts/ci-local.sh --fast` — this story should change no Rust, so the run is a regression check, not a gate on new behaviour.
  - [x] `git diff -- crates/*/src/` must be empty. See "Scope boundaries".

### Review Findings

**Code review 2026-07-28** — three adversarial layers in fresh contexts (Blind Hunter: diff only;
Edge Case Hunter: diff + repository; Acceptance Auditor: diff + spec + context docs). All three ACs
were judged **PASS** by the Auditor, which independently reproduced the enumeration, the coverage
check, every tally, all 22 cited tests and the four GitHub issues. The findings below are defects in
**evidence and description**, not in the verdicts — with two exceptions marked `[Decision]`.

**Two findings converged across independent layers** (Blind + Auditor), which is the strongest signal
in the run: the chapter-1 `-string` proof cells, and the undisclosed chaos-test caveat.

- [x] [Review][Decision] **The five `-phid-*` clauses: `gap` or `n/a`?** — The specification says
  plainly that the capability is optional: *"Specifying a Primary Host is not required for an Edge
  Node"* (`Sparkplug_5:190-191`) and *"It is not required that an Edge Node must have a Primary Host
  configured"* (`Sparkplug_1:285-286`). Neither sentence is quoted in the matrix. Structurally these
  are identical to `message-flow-device-dcmd-subscribe`, which this same pass ruled **`n/a`** on the
  grounds that its antecedent is an absent capability. Two absent capabilities, two opposite
  verdicts. **Against changing:** AC1 requires the STATE blind spot to appear as `gap` rows pointing
  at Stories 4.4–4.8, and a distinction does exist — DCMD's antecedent is a fact about the *meter*
  (no writable output exists), the Primary Host antecedent is a fact about *our software* (we never
  built the option), and Guy's broker carries live `spBv1.0/STATE` topics. Moving them would make
  chapter 5 `22/2/21/54` and the whole-specification totals `70/8/47/149`.
- [x] [Review][Decision] **Chapter 1's three `-string` rows extend the admissible-witness class
  without an ADR** — `intro-group-id-string`, `-edge-node-id-string`, `-device-id-string` are
  `conformant` because a Rust `String` is UTF-8 by construction. The document's ratified rule
  ([ADR 0014](../../docs/adr/0014-schema-as-conformance-evidence.md)) admits exactly one non-test
  witness, the pinned protobuf schema, and says that where the guarantee comes from *our own code
  shape* the verdict is `gap (unproven)` — the reading this same pass applied to downgrade
  `-single-server`. This is the one place the pass **loosened** a rule. Options: (a) amend ADR 0014
  to admit language-level type invariants, with an issue, keeping `conformant`; (b) downgrade all
  three to `gap (unproven)`, making chapter 1 `0/0/7/1` and the story total `23/3/34/64`. Note the
  distinction that favours (a): `String`'s invariant is the *language's* and cannot be changed by
  us, unlike `MqttConfig`'s shape.

- [x] [Review][Patch] **`chaos_stale_on_death` does not SIGKILL anything** — the row claims *"the
  bridge is SIGKILLed"*; the test calls `bridge.abort()` on a tokio task in-process, with no signal
  and no separate process [docs/sparkplug-conformance.md `-birth-publish-will-message` row; `crates/smartme-bridge/tests/chaos_stale_on_death.rs:68`]
- [x] [Review][Patch] **"The NBIRTH carries one metric" is false — it carries two** — `build_birth`
  prepends `bd_seq_metric` before `Contract/Version` (`encode.rs:180-182`). Stated four times, and
  it inverts the analysis: `-nbirth-order` is live **today** over two metrics, not latent [docs/sparkplug-conformance.md, data-behaviour section and Findings]
- [x] [Review][Patch] **`chaos_sigterm_no_lie` does not observe the NBIRTH "before anything else"** —
  `wait_for` silently discards non-matching messages, so a pre-birth DDATA would be dropped and the
  test would still pass [docs/sparkplug-conformance.md `principles-birth-certificates-order`; `tests/common/mod.rs:157-161`]
- [x] [Review][Patch] **The manual still over-claims specification backing** — no QoS clause exists
  for an *explicitly published* NDEATH; the only NDEATH QoS clause governs the will. The edit fixed
  the will half and left the published-NDEATH half [docs/manual/chapters/04-mqtt-sparkplug-contract.tex:163-165]
- [x] [Review][Patch] **The manual minimises the will-QoS violation with an unsupported claim** —
  *"a broker holding a will delivers it once regardless, so the observable effect is small"*: at QoS 0
  there is no acknowledgement, retry or queueing, so the certificate can be lost outright. Cited by
  two layers independently [docs/manual/chapters/04-mqtt-sparkplug-contract.tex:174-176]
- [x] [Review][Patch] **The U+0000 finding is described as reaching the wire; the probe showed a
  constructed string** — no broker, no publish call. Reword, and give the probe as a re-runnable
  command so the evidence is not a deleted artefact [docs/sparkplug-conformance.md, chapter 1]
- [x] [Review][Patch] **The coverage check proves an id is *mentioned*, not that it carries a
  verdict** — 88 of 124 sit in a verdict row; the other 36 rest on hand-counted collective blocks.
  The document's own anomaly (`-ncmd-subscribe` cited at HEAD) demonstrates the hole [docs/sparkplug-conformance.md, coverage check]
- [x] [Review][Patch] **The whole-specification gap split is wrong: 39/13 claimed, 38/14 actual** —
  recomputed from each chapter's own stated numbers [docs/sparkplug-conformance.md, Whole-specification total]
- [x] [Review][Patch] **A mutation claim in the `-nbirth-qos` proof cell was inherited from chapter 6
  and never run** — now run (`qos_for` → `AtLeastOnce`): **red**. Add as the ninth mutation and
  correct the count [docs/sparkplug-conformance.md `-nbirth-qos`]
- [x] [Review][Patch] **The chaos-tests-were-not-run caveat appears only in the Debug Log** — three
  chapter-5 `conformant` rows name a chaos test as their only proof, and the durable artifact carries
  no qualification. Mitigating and worth stating: CI runs them on `ubuntu-latest` [docs/sparkplug-conformance.md, chapter 5]
- [x] [Review][Patch] **"15 rows added to Findings carried forward" — the diff adds 13** [story File List]
- [x] [Review][Patch] **The *Gap ownership* Primary-Host row itemises 10 clauses under a count of
  11** — `-termination-host-offline-timestamp` is missing from the breakdown [docs/sparkplug-conformance.md, Gap ownership]
- [x] [Review][Patch] **"45 of the 99 … 33 host and 12 command clauses" reconciles to nothing** — the
  `-data-commands-*` family has 16 rows; 33 + 16 = 49. The 45 was mis-transposed from the story's
  count over a different set [docs/sparkplug-conformance.md, chapter-5 tally]
- [x] [Review][Patch] **`-will-message-topic` cites the chaos test as "matching the topic off the
  wire"** — it only tests `.contains("/NDEATH/")`; `foo/NDEATH/bar` would satisfy it. Chapter 6
  describes the same test accurately, so this row contradicts it [docs/sparkplug-conformance.md]
- [x] [Review][Patch] **`-will-message-will-retained` and `-nbirth-qos` treat the same derivation
  oppositely** — both flow from `qos_for`'s single return, accepted as proof in one row and rejected
  in the other. Conservative, but state the convention instead of leaving it implicit [docs/sparkplug-conformance.md]
- [x] [Review][Patch] **`-single-server`'s rationale names Story 4.5; its owner is Story 4.10** — on
  the document's own reasoning the guard could land after the change it guards [docs/sparkplug-conformance.md]
- [x] [Review][Patch] **"Chapters 7, 8, 9 and both appendices carry zero ids — verified" cites a
  command that does not grep them** — the claim is true (I re-checked: all zero), the citation is not [docs/sparkplug-conformance.md, Status]
- [x] [Review][Patch] **18 host-application `n/a` ids are dismissed by bucket name with no
  quotation** — the verdicts are right (the Auditor spot-read 11 of 30 and all bind a Host), but the
  showing is thin in the one family where this pass proved prefix is unreliable [docs/sparkplug-conformance.md, Host section]
- [x] [Review][Patch] **The mutation narrative claims both a clean prediction record and a surprise
  from the same run** — M7 went green as predicted; what changed was the row's rationale. Say that [docs/sparkplug-conformance.md, mutation table]
- [x] [Review][Patch] **The `prd.md` / `architecture.md` RBE corrections read as this commit's work**
  — they were an earlier commit; date them as prior [docs/sparkplug-conformance.md, chapter 2]
- [x] [Review][Patch] **Task 10 required confirming 123→"124 missing" and got 123; the box was ticked
  and the shortfall turned into a compliment** — record it as unmet-as-written per CLAUDE.md [story Task 10]
- [x] [Review][Patch] **The six failure modes are discharged in substance but never recorded per
  mode** — the story demands the check; the record does not show it [story / matrix]
- [x] [Review][Patch] **Vacuity is scored three ways in one table without stating the selecting
  principle** — `-publish-nbirth` conformant, `-nbirth-change` n/a, `-nbirth-order` gap (unproven).
  The rule (does the clause bind a message we emit? if so, is it proven?) is sound but unstated [docs/sparkplug-conformance.md, data behaviour]
- [x] [Review][Patch] **Change Log writes `26 · 3 · 31 · 64 = 124`**, which reads as multiplication in
  a document asserting that audits whose numbers do not add up are incomplete [story Change Log]
- [x] [Review][Patch] **`-dcmd-subscribe`'s quotation takes its punctuation from `:399` (prose), not
  the cited `:403-407`** — cosmetic, verdict unaffected [docs/sparkplug-conformance.md]

- [x] [Review][Defer] **Chapter 6's `payloads-nbirth-bdseq-repeat` may be a wrong verdict against its
  own clause text** [docs/sparkplug-conformance.md] — deferred, pre-existing. The increment
  requirement appears in chapter 6 only as a non-normative sub-bullet (`:1521-1522`), so the clause
  at `:1075` states the same satisfied requirement as chapter 5's `-nbirth-payload-bdSeq`. Re-deciding
  chapter-6 rows is outside this story's scope; hand to **Story 4.19** with `topics-dcmd-topic`
- [x] [Review][Defer] **The "SIGKILLed" misdescription originates in chapter 6's row** [docs/sparkplug-conformance.md] — deferred, pre-existing. This story propagated it into chapter 5; fix here, and correct chapter 6 under Story 4.19
- [x] [Review][Defer] **No falsification was aimed at the `n/a` column** [docs/sparkplug-conformance.md] — deferred, pre-existing method gap. All nine mutations target `conformant` or `gap (unproven)` rows; `n/a` is 64 of 124 and was checked only by re-reading
- [x] [Review][Defer] **11 untracked dotfiles sit at the repo root** (`.bashrc`, `.gitconfig`, `.idea`, `.mcp.json`, …) [repo root] — deferred, pre-existing environment artefacts. A live hazard given CLAUDE.md's "never `git add <directory>`" rule

**Dismissed as noise (2).** *"Nothing establishes that #34/#35 exist"* — correct from the diff alone,
refuted by the Auditor's `gh issue view`. *"The `deviation` / `gap` boundary is undefined"* — the
verdict table defines it ("we knowingly do otherwise" versus "we do not do the thing").


## Dev Notes

### What this story is, and is not

It is an **audit that records**. It does not fix. Every defect becomes a row plus an issue or an owning story — the fixes are Stories 4.4–4.10, 4.17, Epic 3. Resist turning a `gap` into a `conformant` by writing the missing test inside this story: that is scope creep into stories that already exist, and it hides the size of the gap the audit is measuring.

**No production code changes.** `git diff -- crates/*/src/` is empty at the end. If the audit finds something that genuinely cannot wait, raise it and stop.

### The scope decision — taken here, not deferred

The epic scopes 4.3 to *"chapters 2 and 5"*. Mechanically that is **103** clauses (4 + 99), and the pinned specification holds **303**. Chapters 4 and 6 account for 179. **That leaves 21 clauses — chapters 1 (8), 3 (1) and 10 (12) — owned by no story in any epic.**

They are not filler. Chapter 1 carries the identifier character and uniqueness rules that sit underneath chapter 4's topic grammar; chapter 10 carries the **conformance profiles** — the specification's own statement of what it means to claim conformance, which is the direct input to NFR19's "documented conformance scope".

**Decision: 4.3 owns all 124** (Guy, 2026-07-28; two split proposals were weighed and declined — by chapter into 103+21, and by role into 78+46). The alternative — a new story for the 21 — was rejected because the audit's whole value is a countable, closeable set, and a matrix that tallies each chapter while silently omitting three of them reproduces the exact defect the 4.2 code review found in chapter 4: per-chapter arithmetic that closes, over a clause set that does not. `epics.md` is amended with the same text; no ADR, because this adds a completeness obligation and reverses no position.

### Two defects found while drafting, and one clause that passes by luck

Read the norm and the code before ruling on these — they are recorded here so they cannot dissolve into a generic "lifecycle: conformant" row, not so they can be copied.

#### 1. `tck-id-principles-rbe-recommended` — the bridge publishes periodically, by design

> *"Because of the stateful nature of Sparkplug sessions, data SHOULD NOT be published from Edge Nodes on a periodic basis and instead SHOULD be published using a RBE based approach."* — `Sparkplug_2_Principles.adoc:50-52`

`poll_publish.rs:143` is `tokio::time::interval(config.interval)` (5 s default, `supervisor.rs:191`), and `step_once` publishes the outcome of **every** tick. `StateMachine::step` (`core/state_machine.rs:82`) returns `(State, Quality)` — a verdict, not a publish decision. **There is no change detection anywhere in the tree.** Every tick publishes, changed or not.

This is a `SHOULD NOT`, so it is a `deviation` if we keep the behaviour and a `gap` if we do not — the same shape as `-metric-datatype-not-req` in chapter 6.

**And it interacts with the PRD in a way that must be stated rather than resolved here.** `prd.md:212` describes the downstream publisher as *"NDATA/DDATA (report-by-exception)"*. **No FR requires it** — the FR list (FR17–FR22) covers units, serial binding, timestamps, rebirth, delivery and outage policy, and none mentions RBE. So the accurate finding is narrow: *the PRD's prose says report-by-exception, no requirement obliges it, and the implementation does not do it.* Record that; do not upgrade it to "FR unmet", and do not quietly amend the PRD from inside an audit.

**The verdict is decided: `deviation`, and the row must say that RBE is *blocked*, not rejected.** Guy took this on 2026-07-28. Three facts carry it, and the row should carry all three:

1. **For active meters RBE would suppress almost nothing.** `crates/smart-me-client/fixtures/smartme_sample.json` publishes `CounterReading` to six decimals (`4843.822`, `6330.412207`). At the fixture's `0.754 kW`, energy advances ~`0.001 kWh` per 5-second poll — visible at the published precision. The values genuinely change every tick.
2. **For a dead meter it suppresses everything, and that case is live.** Fixture meter `30000003` reads `0.0 kW` with a `ValueDate` of `2026-04-20` — the physically unplugged meter. The bridge republishes byte-identical content for it roughly **17 000 times a day**, indefinitely. This is precisely the case the clause addresses.
3. **RBE cannot land before `Node Control/Rebirth`.** Sparkplug assumes a late-joining consumer issues a Rebirth to relearn state. The bridge does not answer one (Stories 4.6/4.7), so the periodic publish is currently *substituting* for the missing Rebirth. Implementing RBE first would mean a new consumer never learns the unplugged meter's value — a functional regression wearing conformance as a costume.

So: `deviation` owned by **[#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)**, rationale as above, and an explicit revisit condition — **when Story 4.7 lands, this deviation must be re-examined.** The PRD was corrected on 2026-07-28 (`prd.md:149`, `:212`) and `architecture.md:211` with it; all three claimed report-by-exception, which the bridge has never done. `docs/manual/chapters/04-mqtt-sparkplug-contract.tex:237` already recorded it correctly — the manual was right and the planning artifacts were wrong, which is worth noticing about which documents get maintained.

#### 2. `tck-id-principles-persistence-clean-session-311` — satisfied by a dependency default we never set

> *"If the MQTT client is using MQTT v3.1.1, the Edge Node's MQTT CONNECT packet MUST set the 'Clean Session' flag to true."* — `Sparkplug_2_Principles.adoc:133-135`

**The flag is true, and nothing in this repository says so.** `mqtt_driver.rs:156` calls `MqttOptions::new(...)` and **never calls `set_clean_session`**. It is true only because rumqttc 0.25.1 defaults it that way (`rumqttc-0.25.1/src/lib.rs:513`, `mqttbytes/v4/connect.rs:27`). No test asserts it; a rumqttc upgrade that flips the default would violate a MUST in total silence.

This is the textbook `gap (unproven)` under the taxonomy adopted on 2026-07-28: *we do the thing, nothing proves we do*. It is also the first one whose guarantee comes from **outside** our code, which the row should say — [ADR 0014](../../docs/adr/0014-schema-as-conformance-evidence.md) admits the pinned schema as a witness precisely because the compiler enforces it, and a dependency's runtime default has none of that property.

Suggested owner: a new issue, or fold into **Story 4.10** (which already owns the CONNECT packet). Decide in the pass; do not leave it unowned.

#### 3. `tck-id-message-flow-edge-node-ncmd-subscribe` — the clause the last review moved *into* this story

Not a defect found here, but the reason Task 2 flags it: the 4.2 code review reclassified chapter 4's `topics-ncmd-mqtt` and chapter 6's `payloads-ncmd-qos`/`-retain` to `n/a`, because they bind an NCMD *publisher* and that is always a Host Application. Correct — but it means **the obligation we actually fail now rests entirely on this chapter-5 clause**. If this pass files it loosely, the unimplemented command path loses its only first-class row.

### How this audit could pass wrongly

`CLAUDE.md` requires every human-run gate to state what *else* could make it pass. An audit conducted by an agent is exactly such a gate, and its failure mode is not a red test — it is a document that looks complete. **Six** ways this pass reports success without doing the work. The first five are inherited from 4.2 and all five were live; the sixth is new and specific to this story.

1. **A `conformant` naming a test that does not exercise the clause.** The 4.2 code review downgraded four rows for exactly this, and the worst was a test asserting production's own expression against itself. For each `conformant`, ask what the named test would do if the behaviour were removed. If the answer is "still pass", the row is `gap (unproven)`.
2. **`n/a` used as a dustbin.** This story's `n/a` risk is far larger than 4.2's: **34 Host-Application clauses in Task 5 and 11 MQTT-Server clauses in Task 8 are plausibly `n/a` by prefix.** That is 45 of 124 — over a third — dismissible without reading a word. Read each for which role it binds. Task 5's `operational-behavior-primary-*` are the ones most likely to be wrongly swept in, and they are the substance of Stories 4.4–4.5.
3. **A collective block that hides its members.** Task 10's diff compares **ids**, not headings. A block reading "the 22 host clauses" satisfies a reader and fails the check; a block listing its 22 ids satisfies both.
4. **A verdict copied from a chapter-4 or chapter-6 twin.** This chapter restates lifecycle requirements the other two already rule on — `bdSeq`, `seq`, births, deaths, NCMD. Similar is not identical, and the twin's verdict was reached against different clause text. Read chapter 5's wording each time. When they genuinely are the same requirement, say so and point at the *same* owner (as with #27 and Story 4.10) rather than inventing a second.
5. **Re-verifying by re-reading our own conclusions.** The matrix contains 80 clause rows across two chapters, and they are persuasive. None of them is evidence about chapters 1, 2, 3, 5 or 10.
6. **NEW — declaring the audit complete on 103 clauses.** The epic says "chapters 2 and 5". If the pass follows the epic instead of AC3, it will produce a document whose per-chapter tallies all close and whose whole-specification claim is false by 21 clauses. That is precisely the defect the 4.2 review found in chapter 4. The `303` reconciliation in AC3 is the guard; state it explicitly or the guard does not exist.

### Verdict rules — inherited, and one of them is new

From the matrix's "How to read this", as amended on 2026-07-28:

| Verdict | Condition |
| --- | --- |
| `conformant` | We do what the clause requires **and a named test proves it** — or the pinned schema makes the violation unrepresentable ([ADR 0014](../../docs/adr/0014-schema-as-conformance-evidence.md), **field types only, never values**) |
| `deviation` | We knowingly do otherwise; row carries rationale + ADR or deferred-work link |
| `gap (unimplemented)` | We do not do the thing the clause requires |
| `gap (unproven)` | We do it; nothing proves that we do |
| `n/a` | The clause addresses a role we do not play, a message we do not emit, or a feature we do not use |

**The `gap` split is new** (2026-07-28 code review) and this is the first pass drafted against it. It changes no verdict and no count — but every `gap` row you write must carry one of the two labels, and chapters 4 and 6 are already converted, so a bare `gap` in your output is a defect.

**A `conformant` with no named test is a `gap (unproven)`.** This will bite: several lifecycle behaviours are correct by construction or by a dependency default (the clean-session clause above is one) and asserted by nothing. Mark them `gap (unproven)` and say "correct by construction, unproven" in the row. Downgrading honestly is the entire point — contract v1 shipped because 148 green tests all agreed with each other, and the 4.2 code review found the same shape inside the audit meant to prevent it.

**Gap ownership:** an owning story or epic if one exists, otherwise a new issue. An unowned, unnumbered gap is not acceptable.

### Current state of the code being audited

Read these before writing rows. The lifecycle lives in three places and the split matters.

**`crates/sparkplug-b/src/encode.rs`** (543 lines) — the session type-state. `build_birth` (`:178`) resets the counter, prepends `bdSeq`, takes `seq = 0`. `death_payload` (`:215`) omits `seq` (`:219`) and carries only `bdSeq`. `LiveSession::device_death` (`:155-163`) takes the next sequence number. Device messages share the node's single `seq` counter (`:142-163`).

**`crates/sparkplug-b/src/seq.rs`** — `SeqCounter` is a `u8` advancing with `wrapping_add` (`:14-35`); 0–255 is a type invariant, not a check.

**`crates/smartme-bridge/src/app/mqtt_driver.rs`** — the CONNECT packet and the reconnect loop. `MqttOptions::new` (`:156`), will registered at `:158-159`, explicit NDEATH at `:240`, death flush at `:243`, and the reconnect backoff at `:268-269` (*"rumqttc has no internal backoff: this sleep IS the backoff"*). `qos_for(_message: MessageType)` (`:123-125`) **ignores its argument** and returns `(AtMostOnce, false)` for everything — recorded in the matrix's findings, and the reason five retain verdicts would revert to unproven if it ever grew a real `match`.

**`crates/smartme-bridge/src/app/poll_publish.rs`** — the publish cadence. `tokio::time::interval` at `:143` with `MissedTickBehavior::Delay`; `step_once` (`:89`) touches the heartbeat first, fetches under `config.fetch_timeout`, then calls `policy.step` and publishes.

**`crates/smartme-bridge/src/core/state_machine.rs`** — `step` (`:82`) is pure and returns `(State, Quality)`. Note the ordering comments: `Failed` latches before the boot-sanity guard; `Bad` is judged before the timestamp guards. Those orderings are load-bearing and several chapter-5 data clauses will touch them.

**`crates/sparkplug-b/src/topic.rs`** — `check_identifier`, `EdgeNode::new`, `node_topic`, `device_topic`. The chapter-1 identifier clauses land here.

### Scope boundaries

- **Do not add bridge or Ignition context to `crates/sparkplug-b/`.** `tests/no_context_leak.rs` fails if `smartme` / `ignition` / `SMARTME_` appears in that crate's sources. `docs/sparkplug-conformance.md` is a bridge-repo document and names Ignition freely; the crate must not link to it.
- **Do not re-decide chapter 4 or 6 rows.** Cross-reference them. The one exception is `tck-id-topics-nbirth-bdseq-increment` (Task 2), and only because it is a chapter-4 clause with no row at all.
- **Chapter 4's 29 missing clauses belong to Story 4.19.** Completing them is not this story; if you touch chapter 4, touch only what Task 2 authorises.
- **If you write any test at all**, CLAUDE.md's falsification rule applies without exception. But you probably should not be writing one.
- **The manual** (`docs/manual/`) must track real behaviour. This story changes no behaviour, so it likely needs no edit — but if the audit changes what the manual *claims*, fix the manual in the same commit.

### Project Structure Notes

- Output is the existing `docs/sparkplug-conformance.md`. Add chapter sections in the shape of chapters 4 and 6 (`tck-id | Level | Our behaviour | Proof | Verdict`), ordered by chapter number so the document reads as one audit.
- Suggested sub-headings, mirroring the established style: edge-node session flow · data behaviour · device lifecycle · Host Application and primary host · identifiers and case sensitivity · principles · conformance profiles.
- Issues go on `guycorbaz/smartme_mqtt` via `gh issue create`. **`gh` needs the sandbox disabled** — its token is in the OS keyring, and inside the sandbox it reports a misleading "token is invalid".
- No `project-context.md` exists in this repo; `CLAUDE.md` is the standing rule set and outranks anything here that contradicts it.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md#Story 4.3`] — AC1 and AC2 verbatim; AC3 added by the amendment note there
- [Source: `_bmad-output/planning-artifacts/epics.md:750`] — Stories 4.1–4.3 are the audit; the rest of Epic 4 may be reshaped by their findings
- [Source: `docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_5_Operational_Behavior.adoc`] — 99 ids; `:158` NCMD subscribe, `:403` DCMD subscribe (QoS 1)
- [Source: `docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_2_Principles.adoc`] — 4 ids; `:50-52` RBE, `:97-99` birth order, `:133-138` clean session
- [Source: `docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_10_Conformance.adoc`] — 12 ids; profiles at `:35`, `:42`, `:53`, `:71`
- [Source: `docs/sparkplug-conformance.md`] — the 96 existing rows, the verdict rules, and the chapter-4/6 tallies to mirror
- [Source: `_bmad-output/implementation-artifacts/4-2-conformance-matrix-payloads-metrics-datatypes.md#Review Findings`] — the 22 findings of the 4.2 code review; failure modes 1 and 2 were both live
- [Source: `docs/adr/0013-payload-timestamp-is-acquisition-time.md`] — the timestamp deviation, for cross-referencing chapter-5 data clauses
- [Source: `docs/adr/0014-schema-as-conformance-evidence.md`] — the schema witness and its bound
- [Source: `docs/adr/0011-graceful-shutdown-requires-both-deaths.md`] — bears on the edge-node death clauses
- [Source: `_bmad-output/planning-artifacts/prd.md:212`] — "NDATA/DDATA (report-by-exception)", and the FR list at `:269-306` that does not require it
- [Source: `_bmad-output/planning-artifacts/architecture.md:101`] — NFR19 conformance scope; the crate's rustdoc must state which subset it covers
- [Source: `CLAUDE.md`] — read the norm first; cite `tck-id`s, not prose; falsify before trusting; ADR + issue for anything that moves a requirement

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-07-28.

### Debug Log References

**Mechanical enumeration (Task 1).** The worklist is exactly **124** ids, and the chapter partition
was verified rather than assumed: per-chapter distinct counts are `8 · 4 · 1 · 70 · 99 · 109 · 12`
summing to **303**, which equals the whole-tree total — so no id appears in two chapters. The clause
budget in Task 1 reconciled with **zero** unmatched ids across all seven families.

**Coverage check, armed before trusted (Task 10) — and the task's own criterion was NOT met as
written.** Task 10 says: *"confirm it reports **124 missing**. A check that has not been seen red is
not a check."* It reports **123**. The intent was met — the check was seen red and it
discriminates — but the letter was not, and the first draft of this record turned the shortfall into
a compliment about cross-chapter pointers instead of recording it. Per `CLAUDE.md`, recorded here as
**unmet as written**: exactly one of the 124,
`tck-id-message-flow-edge-node-ncmd-subscribe`, was already cited at HEAD in the chapter-4 and
chapter-6 forward references, so the reachable red figure was always 123. No issue opened — the
criterion was mis-predicted at drafting time, not missed at implementation time, and the guard it
exists to provide functioned.

Against the finished file: **0 missing / 0 invented**. A **second** check was added at the code
review, because the first proves only that an id is *mentioned* somewhere — not that it carries a
verdict. It reports 88 clauses in a verdict row and 36 in a named collective block, `88 + 36 = 124`,
which is what AC3 actually asks for.

**The check caught failure mode 3 on its first run.** It reported 26 missing ids because three
collective blocks used abbreviated forms (`` `-death-qos` ``, `` `-reordering-start` ``). Rewritten
with full ids. A block that satisfies a reader and fails the diff is exactly what the mode predicts.

**Eight mutations run against production code** (all reverted; `git diff -- crates/*/src/` is empty).
Six expected red went red; two expected green went green. Tabulated in the matrix under the
chapter-5 tally. The seventh **corrected the row it tested** — the ordering clauses were drafted as
"vacuous, payloads too small to be out of order", and stamping `Energy` 60 s earlier than `Power`
produced a genuinely mis-ordered payload with all 69 tests green. Not vacuous; unwitnessed.

**One measurement, not a mutation.** A throwaway probe against `EdgeNode::new` (added under
`crates/sparkplug-b/tests/`, run, removed) showed `U+0000` passing `check_identifier` and reaching
the topic `"spBv1.0/a\0b/NBIRTH/node"`. This is the evidence behind the three chapter-1 `-chars`
gaps and [#34](https://github.com/guycorbaz/smartme_mqtt/issues/34).

**`rumqttc`'s clean-session default verified in the registry source**, not from the story's summary:
`rumqttc-0.25.1/src/lib.rs:513` and `mqttbytes/v4/connect.rs:27` both default `clean_session: true`.
The story's claim held.

**`./scripts/ci-local.sh --fast`** — first run exited 1 at `cargo-deny` (*"failed to obtain lock file
… read-only path"*), the known sandbox trap; re-run with the sandbox disabled exits **0**, all steps
reproduced. Chaos tests skipped: **no Docker daemon on this machine**, so no chapter-5 verdict rests
on a chaos test this session re-ran — the chaos citations are read from the tests' own assertions.

**The manual was rebuilt** (`latexmk`, 23 pages, clean).

### Completion Notes List

**AC1 — satisfied.** Rows exist for birth/death ordering, `seq` numbering and wrap, `bdSeq` per
CONNECT, NDEATH via will and explicit publish, NCMD/`Node Control/Rebirth`, DDEATH and the
primary-host STATE mechanism. Both known gaps appear as `gap` rows pointing at Stories 4.4–4.8:
NCMD/Rebirth as seven clauses (`-ncmd-subscribe` plus six `-rebirth-*`), STATE as eleven.

**AC2 — satisfied, and it was a task rather than a formality.** All 31 gaps were walked against
Stories 4.4–4.18; the walk is tabulated in the matrix under *Gap ownership*. Outcome: **2 new issues**
([#34](https://github.com/guycorbaz/smartme_mqtt/issues/34) identifier character set → Epic 3,
[#35](https://github.com/guycorbaz/smartme_mqtt/issues/35) clean-session and single-server →
Story 4.10) and **2 widened** (comments posted on
[#27](https://github.com/guycorbaz/smartme_mqtt/issues/27) and
[#30](https://github.com/guycorbaz/smartme_mqtt/issues/30) — the matrix claims they were widened, so
they were actually widened). No gap is unowned. **Epic 4 needed no reshaping**: 4.4–4.7 and 4.10
already covered the two missing mechanisms.

**AC3 — satisfied.** All **124** clauses carry a row or a collective block naming its member ids.
The arithmetic is stated and closes: **`26 conformant + 3 deviations + 31 gaps + 64 n/a = 124`**, and
the whole-specification total **`70 + 109 + 124 = 303`** is stated with a per-chapter table. Status
rows now exist for chapters 1, 2, 3, 5 and 10 — the old "2, 5" row is split. Every `conformant` with
no named test was recorded as `gap (unproven)`; every gap carries an owner.

**What the pass found that was not suspected.**

1. **A measured defect, not a paperwork one.** `check_identifier` implements chapter 4's
   `+`/`/`/`#` rule, and chapter 1 defers its character set to the *MQTT* specification. A `U+0000`
   reaches a published topic. Assuming the two sets were equal — the natural reading, and the one
   this pass nearly took — would have produced three false `conformant` rows.
2. **The audit's reach has a limit worth stating.** This repository keeps **no copy** of the MQTT specification,
   so those three clauses cannot be audited in full against their own norm. #34 should begin by
   pinning it.
3. **Six `operational-behavior-edge-node-*` ids bind Host Applications despite their prefix**, and
   four `operational-behavior-primary-*` ids split three/one between Edge Node and Host. Classifying
   either family by prefix would have moved three real gaps into `n/a`.
4. **The chapter-10 finding is an absence**: the Edge Node conformance profile carries no `tck-id`,
   so an Edge Node's conformance claim is exactly the union of chapters 1–6. Direct input to NFR19.

**Judgements a reviewer should attack first**, stated with their alternatives rather than buried:

- **The five `-phid-*` clauses are `gap`, not `n/a`.** They are conditional on a Primary Host
  configuration the bridge does not have. Reading the condition as "false, therefore vacuous" is
  available and was rejected: the condition is a *capability* the bridge lacks, not a deployment
  fact, and AC1 requires the STATE blind spot to stay visible. This is the most reversible call here.
- **`-device-dcmd-subscribe` is `n/a` while `-ncmd-subscribe` is a `gap`.** Decided by the matrix's
  own hold-the-datum criterion (no writable output exists; every node has a rebirthable session).
  The story text implied 4.6 should own the DCMD one.
- **`-nbirth-payload-bdSeq` is `conformant` where chapter 6's `-nbirth-bdseq-repeat` is a
  `deviation`.** Not the chapters disagreeing: chapter 5 gives the increment its own id
  (`-will-message-payload-bdSeq`, marked `deviation`), chapter 6 does not. One defect, one owner
  (Story 4.10), one row per chapter.

**One inconsistency found and deliberately NOT fixed.** Chapter 4's `topics-dcmd-topic` is a `gap`
where chapter 5's `-device-dcmd-subscribe` is `n/a`. Re-deciding chapter-4 rows is outside this
story's scope boundary, so it is recorded in the findings for **Story 4.19** rather than changed.

**Manual updated, because the audit changed what it claims.**
`docs/manual/chapters/04-mqtt-sparkplug-contract.tex:163` said every message is QoS 0 *"as the
Sparkplug specification requires"* — false for the registered will, which two clauses require at
QoS 1. Corrected, with the violation added to *Known limitations* along with the absent Primary Host
support. This is the same over-claim already recorded against
`every_edge_node_message_is_qos_zero_and_never_retained`; it had propagated into the manual.

**No production code changed.** `git diff -- crates/*/src/` is empty. No test was added — this story
audits and records; the fixes are Stories 4.4–4.10, 4.17, 4.19 and Epic 3.

### File List

- `docs/sparkplug-conformance.md` — modified: Status table split per chapter; new chapter 1, 2, 3, 5
  and 10 sections; **13** rows added to *Findings carried forward*; tallies for chapters 1, 2, 3, 5
  and 10; *Whole-specification total*; *Gap ownership* (the AC2 walk); the chapter-5 mutation table;
  the armed coverage check for chapters 1, 2, 3, 5 and 10
- `docs/manual/chapters/04-mqtt-sparkplug-contract.tex` — modified: corrected the QoS-0 over-claim
  under *Delivery semantics*; added the will-QoS violation and the absent Primary Host support to
  *Known limitations*
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified: story status
- `_bmad-output/implementation-artifacts/4-3-conformance-matrix-lifecycle-and-host-interaction.md` —
  modified: this record

**Created outside the repository** — [#34](https://github.com/guycorbaz/smartme_mqtt/issues/34),
[#35](https://github.com/guycorbaz/smartme_mqtt/issues/35); scope comments on
[#27](https://github.com/guycorbaz/smartme_mqtt/issues/27) and
[#30](https://github.com/guycorbaz/smartme_mqtt/issues/30).

## Change Log

| Date | Change |
| --- | --- |
| 2026-07-28 | Story 4.3 implemented. 124 clauses audited across chapters 1, 2, 3, 5 and 10; `26 + 3 + 31 + 64 = 124`. Coverage check armed (123 missing at HEAD) and green (0/0). Eight mutations run, one of which corrected the row it tested. Issues #34 and #35 opened; #27 and #30 widened. Manual corrected. No production code changed. Status → review |
| 2026-07-28 | Code review (3 adversarial layers, fresh contexts). All three ACs PASS — the Auditor independently reproduced the enumeration, the coverage check, every tally, all 22 cited tests and the four issues. **27 patches applied; the defects were in evidence and description, not in verdicts.** Four factual errors corrected: `chaos_stale_on_death` does not SIGKILL (it aborts a tokio task), the NBIRTH carries **two** metrics not one (so both ordering clauses are live today, not latent), `chaos_sigterm_no_lie` does not observe the NBIRTH "before anything else" (`wait_for` discards non-matching messages), and the U+0000 probe showed a constructed topic rather than a published one. Six counts rectified, the largest being the gap split `39/13 → 38/14`. A ninth mutation added: the `-nbirth-qos` proof cell had **inherited** a mutation result from chapter 6 without running it — now run, red. A second coverage check added because the first proves an id is *mentioned*, not that it carries a verdict (88 rows + 36 block members = 124). **ADR 0015** ([#36](https://github.com/guycorbaz/smartme_mqtt/issues/36)) ratifies the language-type-invariant witness the pass had used without deciding it. The manual's two remaining over-claims removed. Verdicts and tallies unchanged |
