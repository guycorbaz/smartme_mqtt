# Story 4.8: Re-aim the Tier-3 gate at the bridge, and extend it to `Node Control/Rebirth` — close NFR17

Status: done

> **Closed 2026-08-03 by the run, not by the code.** The gate was delivered 2026-08-01 and the story
> then sat in `review` for the only reason it could: NFR17 closes on a human running it against a
> real Ignition. That run happened — **Ignition 8.3.7, MQTT Engine 5.0.0-rc1, contract v3, six steps,
> pass**. The header said `ready-for-dev` throughout the review period; it was stale, and
> `sprint-status.yaml` was the accurate record.

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As the maintainer,
I want the manual contract test to exercise the bridge's own bytes against a real Ignition, including a rebirth the host itself issues,
so that NFR17 is verified by an artifact that actually publishes what the product publishes.

## Acceptance Criteria

The epic states two (`epics.md:973-982`). **Both are amended and four are added**, for reasons in
*Dev Notes → What the epic gets wrong*. Read that section first: the epic's AC1 names an artifact
that **cannot answer a rebirth**, and the gate it names has silently stopped testing the product.

**AC1 — the gate publishes the BRIDGE's bytes**

**Given** the Tier-3 contract test
**When** it runs
**Then** every message on the wire is produced by `SparkplugPublisher` — so the quality codes are
`ignition_quality_code`'s (`Bad_Stale` = `0x8000_0000 | 516`), the NBIRTH declares
`Contract/Version = 3` **and** `Node Control/Rebirth`, and the topic grammar is the bridge's
**And** no step publishes a payload assembled by the test itself.

> The whole point of NFR17 is *"Sparkplug B output conforms to what Ignition MQTT Engine accepts"*
> (`prd.md:360`). The output that matters is the product's.

**AC2 — the quality-code drift is recorded as a defect, not quietly fixed**

**Given** `crates/sparkplug-b/tests/ignition_contract.rs` has published the *specification's*
`Stale = 500` since `d28bb02` (ADR 0012), while the run table's `v2 | Pass` row was obtained when it
published Ignition's code
**When** this story lands
**Then** the run table's v2 row is **annotated in place** as attesting to an artifact state that no
longer exists — not edited, not deleted; the runbook's own rule is *"add a row rather than editing
one"*
**And** a GitHub issue records the drift, its window (`fce148f` → `d28bb02`), and the fact that
**step 4 of that test is today guaranteed to fail**
**And** the crate's test is dispositioned explicitly — see AC6.

**AC3 — the rebirth is issued BY IGNITION, not by the test**

**Given** the bridge is live in the gate's session
**When** the operator triggers a rebirth from the Ignition Designer
**Then** the step confirms a complete re-announcement — NBIRTH then one DBIRTH per meter — with the
`bdSeq` **unchanged** from the birth observed at the start of the run
**And** the checklist states what else could make the step appear to pass.

> `tck-id-operational-behavior-data-commands-rebirth-action-2` and `-action-3`. A rebirth published
> by the test would prove only that the bridge answers *us*; the gate exists to prove it answers
> *Ignition*, which is the one thing no test in this repository can do.

**AC4 — the run answers whether MQTT Engine offers a Rebirth control at all**

**Given** MQTT Engine may render a Rebirth control only for a node that *declared* the metric, and
this bridge declared none until Story 4.7
**When** the operator looks for the control
**Then** the run records **whether it is offered, and where it appears**, as a measurement
**And** if it is not offered, that is recorded as the finding rather than worked around — it would
mean ADR 0016's *"every one is answered with a log line"* described a flow that never occurred.

> This is the input **Story 4.5 is waiting on**. Its remaining question is *"is a host-initiated
> repair path sufficient, given a host that may not ask?"*, and ADR 0016's sequencing argument is
> spent. 4.8's measurement is what re-weighs it.

**AC5 — the run captures WHICH spelling MQTT Engine sends**

**Given** the specification contradicts itself — `Sparkplug_5_Operational_Behavior.adoc:950` says a
host sends a Rebirth Request *"using the 'Node Control/Refresh' metric"* while `-rebirth-name`
(`:956`) and `-ncmd-rebirth-name` (`:973`) both say `Node Control/Rebirth`
**When** Ignition issues the request
**Then** the run records the metric name, datatype and value **exactly as received**
**And** the step names the **near-miss WARN** as the instrument: if the request is not answered, the
bridge logs `reason=NameOnlyNearly` with the bytes, and that line is the measurement.

> The bridge answers the tck-id spelling only. If Engine sends `Refresh`, the answer is silence plus
> a WARN — and the WARN is the difference between a diagnosable result and an invisible one.

**AC6 — the crate's Tier-3 test is dispositioned, not orphaned**

**Given** `crates/sparkplug-b/tests/ignition_contract.rs` publishes the specification's quality codes
**When** this story lands
**Then** its module docs and the runbook state what it does and does **not** attest — it is evidence
about the **crate**, and its step 4 documents a deviation rather than testing a guarantee
**And** it is either re-scoped or retired, with the reason written down; it is **not** left in place
with a checklist that cannot be satisfied.

**AC7 — every step says what else could make it pass**

**Given** `CLAUDE.md`: *for a human-run gate, every step must say what else could make it pass
wrongly*
**When** the steps are written
**Then** each carries its own false-pass list
**And** the rebirth step's list includes the two that this project has already been caught by: a
**reconnect** birth mistaken for an answer (a reconnect produces an NBIRTH under the same `bdSeq` —
the presence of an NBIRTH cannot distinguish them), and a **retained** message replayed at subscribe
time rather than a request anyone sent (ADR 0017).

**AC8 — NFR17 is closed against a stated version**

**Given** `epics.md:114` still says *"the NCMD/Rebirth half is Epic 4"*
**When** the run has been performed
**Then** that note records the Ignition version, the **MQTT Engine module version**, and the contract
version it was verified against
**And** the run table gains a row for contract **v3**, with the module version in it — the runbook
already flags that its absence is a defect.

> **MET, 2026-08-03.** Ignition **8.3.7**, MQTT Engine **5.0.0-rc1**, contract **v3**. The `v3 · the
> bridge binary · Pass` row is the top of the run table, `epics.md:114-116` no longer says the
> NCMD/Rebirth half is outstanding, and the full findings sit under *What the 2026-08-03 run
> established* in `docs/ignition-contract-runbook.md`.

*AC1, AC2, AC6 and AC7 added at story creation 2026-07-31; AC3 and AC5 amended. The scope decision
— re-aim rather than extend — was taken by Guy on 2026-07-31 before drafting.*

## Tasks / Subtasks

- [ ] **Task 1 — read before writing anything** (AC: all)
  - [ ] `crates/sparkplug-b/tests/ignition_contract.rs` — the whole file. It is the shape to keep
        (`checkpoint()`, the refusal to guess a broker or a group, the clean-up contract) and the
        publishing half to replace.
  - [ ] `crates/smartme-bridge/tests/chaos_ncmd_rebirth.rs` — **the harness to reuse.** Story 4.7
        built exactly what this story needs: it drives `mqtt_driver::run` in-process, feeds it
        `MeterUpdate`s over an mpsc channel, and observes from an independent subscriber. Point it at
        a real broker and replace the assertions with `checkpoint()` and you have the gate.
        **Do not build a second harness.**
  - [ ] `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs:89-101` (`ignition_quality_code`)
        and `:387` (`node_metrics`) — what the bridge actually puts on the wire.
  - [ ] `docs/ignition-contract-runbook.md` — the five steps, the run table, the clean-up section.
  - [ ] `docs/adr/0012-quality-codes-spec-versus-host.md` — the split this story's finding 2 is a
        consequence of.

- [ ] **Task 2 — build the gate on the bridge** (AC: 1)
  - [ ] New `#[ignore]`d manual test, `crates/smartme-bridge/tests/ignition_contract.rs`. Same
        refusal to guess: no default broker, no default group, and it must refuse a group called
        `Site` exactly as the crate test does. **Copy that guard rather than re-deriving it** — a
        Sparkplug host persists what it discovers, and the folder outlives the run.
  - [ ] Drive `mqtt_driver::run` with scripted `MeterUpdate`s, using a `SystemClock`. The five
        existing steps map onto reading qualities: cold start (no reading yet), `Good`, `Good`
        updated, `Stale`, then shutdown.
  - [ ] **Step 4 is the one that changes meaning.** It now exercises `ignition_quality_code`, which
        is the whole reason ADR 0012 exists and the one thing no automated test can check. Say so in
        the step.
  - [ ] `bd_seq_path` must point at a scratch directory the test owns, not at the deployment's.

- [ ] **Task 3 — the rebirth step** (AC: 3, 4, 5, 7)
  - [ ] The operator issues the rebirth **from Ignition**. Write down where the control is, because
        the next person will not find it — and if it is absent, that absence is AC4's measurement.
  - [ ] Confirm NBIRTH + one DBIRTH per meter, and that `bdSeq` is unchanged. **Record both values**;
        do not ask the operator to confirm "it looks the same".
  - [ ] Print the bridge's own log lines for the step: `"Rebirth Request accepted"` (classification)
        and `"node re-announced on a Rebirth Request"` (the answer). They are different events —
        the first fires before the birth is attempted. The Story 4.7 review found a test resting on
        the first and calling it proof of the second.
  - [ ] If no birth follows, **look for the near-miss WARN before concluding anything** (AC5).
        `reason=NameOnlyNearly` means Engine sent a different spelling; `reason=ValueNotTrue` means a
        different encoding; `reason=Retained` means the broker replayed something. Each has a
        different repair, and the log distinguishes them.

- [ ] **Task 4 — decide the crate test's fate, at drafting time** (AC: 6)
  - [ ] Its step 4 asks the operator to confirm *"Both tags now show quality STALE / uncertain"* and
        warns *"If Ignition still shows these as good, the whole guarantee fails here"*. The crate
        publishes `500`. Ignition shows `500` as `Good`. **The step cannot pass, and it fails for a
        reason ADR 0012 chose.**
  - [x] **DECIDED by Guy, 2026-07-31: keep it.** Re-scope its module docs to *"the crate's codec
        against the specification, and a demonstration that the specification's quality codes are
        misread by Ignition"*, and rewrite step 4's checklist to **expect `Good(500)`** — turning a
        broken gate into the standing external evidence that ADR 0012's deviation was necessary.
        Retiring it would lose the only external proof of that. Recorded on
        [#40](https://github.com/guycorbaz/smartme_mqtt/issues/40).
  - [ ] Whatever is chosen, `CLAUDE.md` forbids leaving it undecided.

- [ ] **Task 5 — the record** (AC: 2, 8)
  - [ ] Annotate the v2 row **in place**; add a v3 row after the run. Include the **MQTT Engine
        module version** — the runbook already records its absence as a defect.
  - [x] **[#40](https://github.com/guycorbaz/smartme_mqtt/issues/40) opened 2026-07-31** — the
        drift, its window (`fce148f` → `d28bb02`), and the guaranteed step-4 failure.
  - [ ] `epics.md:114` (NFR17) and Story 4.8's own entry: carry the amended ACs back, as 4.6 and 4.7
        did.
  - [ ] `docs/sparkplug-conformance.md` — check whether any row cites the Tier-3 gate as evidence.
        If one does, its evidence just moved artifacts.

- [ ] **Task 6 — run it, with Guy, against production** (AC: 3, 4, 5, 8)
  - [ ] **Do not run this unasked.** The broker is unauthenticated Mosquitto on the LAN with a live
        Ignition on it. The run publishes a disposable node into a real tag tree.
  - [ ] Clean-up is part of the procedure: delete `Edge Nodes/<group>/<node>` under the MQTT Engine
        provider, and **only** that folder.
  - [ ] `./scripts/ci-local.sh` before pushing — not `--fast`, never piped, log to an absolute path,
        and read the `EXIT=` line out of the file.

## Dev Notes

### ⚠️ What the epic gets wrong, and why this story is bigger than it looks

**1. The epic's AC1 is unimplementable as written.** It says to add a rebirth step to
`crates/sparkplug-b/tests/ignition_contract.rs`. That test:

- **never subscribes** — `grep subscribe` over the file returns nothing;
- never declares `Node Control/Rebirth`;
- lives in a crate whose own `lib.rs` states it supplies *"neither the metric nor the command path"*.

A publish-only scripted session cannot answer a rebirth. The step could only ever be faked.

**2. The gate stopped testing the product, and re-running it today would reproduce the v1 failure.**
This is the serious one. From `git log`, all on 2026-07-26:

| Commit | Effect |
| --- | --- |
| `43912b5` | the Tier-3 test is created (Story 1.15) |
| `57914bf` | **the last commit that touches it** |
| `fce148f` | the crate's quality codes move to Ignition's — the v1→v2 fix. The `v2 \| Pass` run happens after this |
| `d28bb02` | **ADR 0012: the crate returns to the specification's `0/192/500`, the bridge deviates.** The test is not touched |

The test calls `with_quality(quality)` → the crate's `Quality::code()` (`model.rs:63-69`) → **`Stale
= 500`**. The bridge publishes `ignition_quality_code` → `0x8000_0000 | 516`. Different bytes. And
`500` is the exact code this project *proved* Ignition displays as `Good(500)`.

So, today:

- **Step 4 — the one marked *"← the critical one"* — is guaranteed to fail.** Its checklist asks for
  *"quality STALE / uncertain"* and warns *"If Ignition still shows these as good, the whole
  guarantee fails here"*.
- The `2026-07-26 | 8.3.7 | v2 | Pass, all five steps` row attests to an artifact state that no
  longer exists.
- **ADR 0012's deviation is verified by nothing.** It is the single most consequential thing in the
  wire contract and the only external oracle that ever covered it has drifted off it.

ADR 0012 records the Tier-3 run as the *discovery* and never recorded that the gate itself had to be
re-pointed. That is [[amend-the-consequences-not-just-the-claim]] in its most expensive form: not a
sentence left stale, but an **instrument** left aimed at the wrong target.

### The harness already exists — do not build a second one

`chaos_ncmd_rebirth.rs` (Story 4.7) drives `mqtt_driver::run` in-process, feeds it judged readings
over an mpsc channel, and observes from an independent subscriber. That is precisely the shape this
gate needs; only three things change:

| | chaos test | Tier-3 gate |
| --- | --- | --- |
| broker | testcontainers Mosquitto | the real one, from an env var, no default |
| oracle | `assert!` on a transcript | `checkpoint()` — a human in the Designer |
| readings | a 20 ms feeder | five scripted steps with chosen qualities |

Keep the crate test's `checkpoint()` verbatim: printing a checklist and waiting on Enter is the right
shape and it has been used in anger.

### What this run is positioned to settle, and nothing else can

**Hypothesis A — does MQTT Engine offer a Rebirth control at all?** It may render one only for a node
that *declared* the metric, and this bridge declared none until Story 4.7. If so, ADR 0016's *"a real
Host Application sends Rebirth requests \[and\] every one of them is answered with a log line"*
describes a flow that **has never occurred**, and the hazard Story 4.6 was said to create was
theoretical. Reasoning, not measurement — until this run.

**Hypothesis B — which spelling does it send?** The norm contradicts itself (`:950` vs `:956`/`:973`).
The bridge answers `Node Control/Rebirth` only, and traces anything that misses. **The near-miss WARN
is the instrument**, and it exists because the Story 4.7 review widened detection past the matcher.
Without it, a wrong spelling would present as silence.

**Story 4.5 is waiting on hypothesis A.** ADR 0016's sequencing argument is spent; 4.5's remaining
question is *"is a host-initiated repair path sufficient, given a host that may not ask?"* If Engine
does not offer the control, the answer leans hard toward implementing STATE. Do not answer it here —
just measure it, and say so.

### What 4.7 does and does not claim, and why that matters here

Story 4.7 claims **conformance to the norm, not compatibility with Ignition**. No real MQTT Engine
request has ever been observed or answered. This story is where that changes, and the completion
notes must not blur the two — if the run does not happen, the story is not done, however good the
test file is.

### Deployment facts that constrain this story

- **The broker is unauthenticated Mosquitto on the LAN, with a LIVE Ignition** (8.3.7, MQTT Engine
  v5.0.0-rc1). Never aim anything at it unasked.
- **A Sparkplug host persists what it discovers.** The group name becomes a folder that outlives the
  run. Clean-up is a required step, and it must delete only the node's folder — removing MQTT Engine
  tags discards their alarm and history configuration, and real edge nodes live under the same
  parent.
- **Contract version is v3** and no run has been recorded against it.
- The **MQTT Engine module version** governs Sparkplug conformance more directly than the platform
  version. It is missing from every row of the run table. Capture it.
- **Cirrus Link's published docs target Ignition 8.1** and are what misled the original quality
  codes. Measure; do not read their tables.

### Testing standards

- The gate is `#[ignore]`d and manual. It never runs as a side effect of `cargo test`.
- **Every step states what else could make it pass.** The Tier-3 test nearly returned a false pass
  once because two of its five steps showed a non-good quality for reasons unrelated to the property
  under test.
- No raw time outside `core/clock.rs`; `arch_purity` enforces it, inline test modules included.
- `./scripts/ci-local.sh` before pushing, then `gh run list`.

### Project Structure Notes

- The new gate belongs in `crates/smartme-bridge/tests/`, because it drives the bridge. Putting it in
  `sparkplug-b` would reintroduce the exact defect this story fixes, and `no_context_leak` guards
  that boundary.
- **Tier-3 re-arm automation is deferred post-MVP** (`architecture.md:51`, `:191`). Do not automate
  the human step.
- The published `sparkplug-b` crate should need no change.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md#Story 4.8`] — the two original ACs
- [Source: `_bmad-output/planning-artifacts/epics.md:114`] — NFR17's coverage note, to close
- [Source: `_bmad-output/planning-artifacts/prd.md:360`] — NFR17
- [Source: `docs/ignition-contract-runbook.md`] — the five steps, the run table, clean-up
- [Source: `docs/adr/0012-quality-codes-spec-versus-host.md`] — the split that caused finding 2
- [Source: `docs/adr/0016-rebirth-before-primary-host-wait.md`] — the spent argument, and hypothesis A
- [Source: `docs/adr/0017-a-retained-ncmd-is-a-replay-not-a-request.md`] — the retained-replay
  false-pass in AC7
- [Source: `docs/spec/…/Sparkplug_5_Operational_Behavior.adoc:950, 956, 973, 979-987`] — the
  self-contradiction, and the action clauses
- [Source: `crates/smartme-bridge/tests/chaos_ncmd_rebirth.rs`] — the harness to reuse
- [Source: `_bmad-output/implementation-artifacts/4-7-node-control-rebirth-answer.md`] — the review
  that produced hypotheses A and B, and the near-miss instrument
- [Source: `CLAUDE.md`] — read the norm first; manual steps must state how they could pass wrongly;
  decide at drafting time

## Field measurements already taken (2026-07-31)

A targeted probe was run against the live Ignition **before** this story was implemented, because
Guy was at the Designer and the two hypotheses were cheap to settle. It ran the **bridge binary** on a
disposable group (`RebirthProbe/ProbeNode`), with `SMARTME_API_BASE` pointed at an unroutable address
so no reading was ever fetched. **It is not a Tier-3 pass** and the runbook records it as *Partial*.

**Settled — these ACs now have field evidence, and the story's job is to make them repeatable:**

| | Result |
| --- | --- |
| **AC4** (does Engine offer the control?) | **Yes.** A `Node Control` folder holding a writable `Rebirth` tag appears for a node that declares the metric. |
| **AC5** (which spelling?) | **The tck-id spelling.** The matcher requires name + `BooleanValue(true)` + `retain = false` *simultaneously*, and it fired — so all three held. **No near-miss WARN anywhere in the run.** `Sparkplug_5:950`'s `Node Control/Refresh` is a defect in the norm's prose, not what Engine sends. |
| **AC3** (complete re-announcement, `bdSeq` unchanged) | **Confirmed from the host's own counters**, not just ours: `bdSeq = 1`, `Birth Count = 3`, `Rebirth Count = 2`, `seq = 1` after two rebirths. |
| **AC1** (the gate must publish the bridge's bytes) | **Vindicated.** Ignition displayed the bridge's quality as **`Bad_Stale`** — ADR 0012's deviation verified against a real host for the first time since the drift. The crate's test would have shown `Good`. |
| Step 5 / ADR 0011 | **Ignition tolerates the double NDEATH.** `Death Count 0 → 2`; two INFO lines, no Sparkplug-side WARN or ERROR in three hours of logs. Recorded in ADR 0011. |

**Also measured, and worth carrying into the implementation:**

- **`Rebirth (Last) Cause: Triggered by user`.** Engine classifies rebirths by cause, which implies
  automatic causes exist (`tck-id-operational-behavior-host-reordering-rebirth`). **Inference, not
  measurement** — no automatic rebirth was observed. It is nonetheless the first concrete sign that
  Engine implements that path, and it is the input **Story 4.5** is waiting on.
- **Two writes produced exactly two requests.** Engine did not resend on its own. The *"Ignition
  resends"* premise behind Story 4.7's no-rate-limit decision is still **unmeasured**; what is
  measured is that bursts would be operator-driven.
- **A metric name containing `/` becomes a folder.** `Contract/Version` and `Node Control/Rebirth`
  appear as folders holding `Version` and `Rebirth`.
- **Engine calls both deaths an "LWT message"**, through one handler — it does not distinguish the
  explicit certificate from the will. See ADR 0011's amended consequences.
- **Engine's Sparkplug handler logs nothing at INFO for births, rebirths or data** — only deaths. The
  Gateway log is not a general observation instrument for the Ignition side.

**NOT settled, and the reason it looked settled:**

> **Step 4 was never exercised, and it appeared to pass.** The probe published no `Good` value at all,
> so `Power` and `Energy` read `Bad_Stale` throughout — before the rebirth and after the death. A
> reader ticking *"are the tags untrustworthy after the node dies?"* would learn nothing: *became*
> untrustworthy and *always was* are indistinguishable here. **This is the false-pass shape the
> runbook exists to name.** The gate this story builds must publish `Good`, degrade to `Stale`, then
> die — in that order.

**A correction to record, because the reasoning was wrong and the conclusion survived by luck.** On
seeing an empty `Edge Nodes` folder before the run, the inference drawn was *"Engine had no node, so
it can never have sent this bridge a Rebirth request, so ADR 0016's claim describes a flow that never
occurred."* **False.** The tree was empty because Guy had cleaned it after earlier tests — a snapshot
at rest, read as a history. Exactly the error `docs/primary-host-state-observation.md` was written
about. The ADR 0016 question remains open, and `Rebirth (Last) Cause` is now the better lead on it.

**Procedural finding that changes Task 3.** *"Check the Ignition logs"* is not performable by
scrolling on this installation: an unrelated MQTT **Transmission** client retrying a connection it
cannot make emits 8–10 lines every 3 seconds, `ERROR` on every cycle. Three attempts to page back to a
two-second window failed. **Export the log and query it** — the `.idb` is plain SQLite with a
`logging_event` table. Two queries answered everything. The runbook now carries the queries.

*(Unrelated to this project, noted as a courtesy: 384 of the Engine module's log lines are
`JsonPayloadHandler :: Error handling payload`, firing every 30 seconds for hours before the probe
began. Engine's JSON namespace, not Sparkplug.)*

## The batched pre-production question list

**Why it is a list.** Every question below needs an Ignition restart or a live production session, and
Guy declined to restart on 2026-07-31 — *"je risque la perte de données"*. That is the right call, and
the right response is to **batch**: the 2026-07-28 restart produced a large amount of information
precisely because it was a transition, and one interruption with six questions prepared beats six
interruptions. Written now, while the questions are fresh, so the run does not have to re-derive them.

**None of these blocks a decision.** ADR 0018 (Primary Host / STATE) was taken without them, on
grounds that do not depend on any answer here — `CLAUDE.md` forbids deferring a decision to an
artifact that does not exist, and AR13 sat unmade for a whole epic for exactly that reason. What these
answers refine is a **revisit condition** and the gate's own coverage.

| # | Question | Why it matters | How to tell |
| ---: | --- | --- | --- |
| 1 | **Does MQTT Engine request a rebirth on its OWN** when it receives DATA from a node whose BIRTH it never saw? | **ADR 0018's revisit condition 3.** It is the one inferred link in the host-initiated repair chain; steps 1 and 4 are measured. | Leave the bridge running, restart Ignition, click nothing. The bridge logs `Rebirth Request accepted`; Ignition's `Rebirth (Last) Cause` shows something other than `Triggered by user`. |
| 2 | **What values does `Rebirth (Last) Cause` take?** | The label's existence is the entire evidence for question 1's premise. Its vocabulary is the answer. | Read the tag after each rebirth of any origin. |
| 3 ✅ | **ANSWERED 2026-08-03 — Ignition displayed `Bad_Stale` on a `Good`→`Stale` transition, node still `online`, `Death Count = 0`.** Original entry below. **Step 4 of the gate — `Good` then `Stale`, against the product.** | **Never exercised.** The 2026-07-31 probe published no `Good` value, so `Bad_Stale` after the death was indistinguishable from `Bad_Stale` before it. This is the step that guards against the silent lie. | The gate this story builds: publish `Good`, degrade to `Stale`, then die — in that order. |
| 4 | **Does Engine consume `$sparkplug`?** | A *Sparkplug Aware* MQTT Server stores BIRTHs and replays them retained (`Sparkplug_10:71-83`) — a **third** remedy neither ADR 0016 nor ADR 0018 weighed, because Mosquitto is not one. Story 4.4 noted it is cheap to check. | Subscribe to `$sparkplug/#` and watch, or read Engine's namespace configuration. |
| 5 | **Does an out-of-order `seq` provoke a rebirth?** | `tck-id-operational-behavior-host-reordering-rebirth` (`Sparkplug_5:565-568`) is conditional on the host being *"configured with a 'reordering timeout' parameter"*, and **nothing measured says Ignition is**. | Publish a DDATA with a deliberately skipped `seq` from a disposable node and watch for an NCMD. |
| 6 | **Do the two STATE forms move together across a transition?** | Engine publishes both a legacy `STATE/<id>` = `ONLINE`/`OFFLINE` and the conformant `spBv1.0/STATE/SCADA` JSON (Story 4.4). Whether both track a real transition is unmeasured, and it bears on any future STATE work. | Subscribe to both before the restart; compare. |

**Two rules the run must carry**, both learned the hard way:

- **A snapshot at rest is not behaviour.** Three careful passes concluded this host did not speak
  Sparkplug 3.0; one restart refuted it. Subscribe *before* the transition, never sample after it.
  (The same error was made again on 2026-07-31 — see the correction above.)
- **Export the log and query the `.idb`.** The viewer is unusable here: an unrelated MQTT Transmission
  retry loop emits ~200 lines a minute. The runbook carries the SQL.

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

**The run, 2026-08-03 — Ignition 8.3.7, MQTT Engine 5.0.0-rc1, contract v3, group `ContractV3`,
node `ContractNodeV3`. Six steps, pass.** Findings in full in the runbook; what belongs to this
story:

- **Question 3 of the batched list is ANSWERED** — the one marked *"never exercised"*. Steps 2–3
  published `Good`, step 4 degraded the same values to `Stale` with the node `online` and
  `Death Count = 0`, and Ignition displayed `Bad_Stale`. ADR 0012's deviation is verified on a
  transition, which is the only form of that measurement worth anything.
- **Questions 1 and 2 remain open.** The run never restarted Ignition, so whether Engine requests a
  rebirth on its own — ADR 0018's revisit condition 3 — is still the inferred link it was. Two
  operator writes produced exactly two requests, which is the second measurement that bursts are
  operator-driven and *not* evidence that Engine resends.
- **A false-pass this story's own AC list did not name.** Step 1 shows `Bad_Stale` on a tag Ignition
  has never valued, and it shows it whether or not the host honoured our quality code — the two are
  indistinguishable at rest. Recorded in the runbook so the next operator does not read step 1 as
  quality evidence.
- **Three step-5 guards were inert** — no `tracing` subscriber in either gate, so no bridge log line
  can appear ([#44](https://github.com/guycorbaz/smartme_mqtt/issues/44)). Harmless here: since
  Story 4.10 a reconnect mints a new `bdSeq`, so the gate's own `bdSeq`-unchanged verdict excludes
  the reconnect false-pass without the log, and a retained NCMD would have replayed at subscribe
  time. The gate's printed checklist still carries the pre-4.10 wording; the runbook has been
  corrected.
- **Not observed:** which timestamp `Offline DateTime` retained. The 2026-07-31 probe found it
  tracked the will rather than the explicit certificate; this run did not re-check it.

### File List
