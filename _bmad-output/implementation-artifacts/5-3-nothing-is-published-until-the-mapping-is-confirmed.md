# Story 5.3: Nothing is published until a human has confirmed the mapping

Status: review

## Story

As the operator,
I want to see the meter→topic mapping and confirm it before anything is published,
so that a mapping which is well-formed but wrong does not create tag history I cannot delete.

## Why this exists, and why validation is not enough

**FR25**, and the PRD says exactly what it is for: *"Before publishing, a mapping confirmation
('are these 4 meter→topic mappings correct?') — **the only guard against a mis-map the machine
cannot detect**"* (`prd.md:136`).

Story 5.1's validation catches what a machine can catch: a serial that cannot be a topic level, two
meters colliding on one topic, a missing field. It cannot catch `meter_id = "garage"` pointed at the
cellar's `device_id`. That configuration is well-formed, unique, complete — and wrong, and only a
human looking at it can say so.

**And the cost of not catching it is not a restart.** A Sparkplug host *persists what it discovers*:
the tag folder outlives the process, has to be deleted by hand, and deleting it discards the alarm
and history configuration of everything under it. This is the same argument that removed the
defaults from `group_id` and `node_id` on 2026-07-31, applied one level down.

**[ADR 0023](../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md)
raised the stakes rather than lowering them.** Before it, a mapping arrived through eleven
environment variables that somebody had typed into a deployment. Now the whole configuration arrives
through a form, and one click would otherwise be the entire distance between an empty file and an
irreversible namespace.

## Acceptance Criteria

**AC1 — an unconfirmed mapping publishes nothing at all**

**Given** a `config.toml` that is present, valid, and **not confirmed**
**When** the bridge starts
**Then** it comes up and stays up, serving the web UI
**And** it opens **no MQTT session**: no CONNECT, no will registered, no NBIRTH, no DBIRTH
**And** it says why, at a level visible under the **default** filter, distinguishing this state from
the *no configuration at all* state of Story 5.2.

> **No session, not merely no DDATA.** An NBIRTH alone creates the node folder in the host's tag
> tree — the very thing that cannot be undone by restarting. Withholding only the data would leave
> the irreversible half already done.
>
> **Three states now, not two**, and they must stay distinguishable in the code as 5.2's two do:
> *absent* (first run) · *present, valid, unconfirmed* (this story) · *ready*. A fourth already
> exists and is a refusal: *present and invalid*.
>
> The absence assertion here is the load-bearing one and it falsifies the wrong way — "no NBIRTH
> appeared" holds over a broker nothing ever connected to. Prove the harness sees a CONNECT for a
> **confirmed** configuration first, then prove it sees none for an unconfirmed one. The harness for
> this already exists in `unconfigured_start.rs` and `chaos_device_certificates.rs`.

**AC2 — confirmation is recorded in the file, and a headless bring-up can give it**

**Given** [FR23 as rescoped by ADR 0023](../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md)
— *"a headless bring-up writes `config.toml` by hand"*
**When** an operator writes the file themselves
**Then** the confirmation is a key they can set in it
**And** setting it is a legitimate confirmation, because writing the mapping out by hand **is**
having looked at it.

> **Decided at drafting, not deferred: a boolean, `mapping_confirmed`, not a fingerprint.**
>
> A fingerprint of the meter tuples would be self-invalidating and is the shape this project usually
> prefers — the mechanism enforcing the rule rather than somebody remembering it. It was rejected
> for one reason: **it cannot be written by hand.** A headless operator would have to compute a hash
> to bring the bridge up, and FR23 promises the opposite. A guard that makes the documented
> bring-up path impossible is not a guard, it is a second bug.
>
> What replaces the fingerprint's self-invalidation is AC3, which is enforceable in the model.

**AC3 — changing the mapping withdraws the confirmation, and the MODEL does it**

**Given** a confirmed configuration
**When** the meter set changes in any way that alters what is published — a meter added, removed,
enabled, disabled, or its `meter_id`, `device_id` or `serial` edited
**Then** `mapping_confirmed` returns to `false`
**And** this happens in `app::store` / `app::config`, **not in the screen that saved it**.

> **This is the whole reason the boolean is acceptable.** A boolean the UI is trusted to clear is a
> boolean that survives the one edit somebody makes through a different path — a hand-edited file, a
> future API, a migration. The clearing has to live where every writer must pass.
>
> **Note which changes do NOT withdraw it**: the publish period, the broker, the log settings,
> ~~the Sparkplug identity. That last one deserves a second look and is deliberately excluded:
> `group_id` and `node_id` change the *namespace*, not the *mapping*, and they already cost a new
> session under [Story 5.2's AC4 table](5-2-config-persists-and-reloads-without-a-restart.md).~~
> If a review decides a namespace change also needs re-confirming, that is a change to this AC and
> not a bug in it.
>
> **AMENDED 2026-08-06, recorded here 2026-08-08 by the closing review.** The review the last
> sentence invited happened, decided the other way, and changed the code and the manual without
> coming back for the AC — so for two days this criterion stated, in the present tense, the
> opposite of what ships. **`group_id` and `node_id` DO withdraw the confirmation**, and
> `store::same_mapping` (`app/store.rs:441`) compares them before it looks at a single meter.
>
> The reason is at `app/store.rs:420`–`:436` and is not a preference: `ui::screens`'
> `mapping_fingerprint` — the value binding the operator's click to what the screen showed them —
> had **always** included both identifiers. Two rules answering one question, disagreeing for a
> month, and the gap was reachable by the path the manual recommends: confirm the mapping, then
> correct the node id. `save` carried the confirmation over, `classify` called it a new session,
> the screen honestly said *"waiting for a restart"*, and the bridge came back publishing into a
> namespace no human had ever seen. That is the harm FR25 exists to prevent, reached from the
> inside.
>
> The argument the struck text made — *"the namespace is not the mapping"* — is answered by what
> a topic is: every identifier here appears in every topic the bridge publishes, so changing one
> moves every device. Asserted at `app/store.rs:744`
> (`changing_the_node_identity_withdraws_the_confirmation`, both identifiers, separately),
> falsified 2026-08-06 by restoring the meters-only comparison, and documented at
> `docs/manual/chapters/09-appendix-config-reference.tex:71`.

**AC4 — the confirmation is presentable without HTML**

**Given** Epic 5 owns the model and Epic 6 owns the screens
([ADR 0021](../../docs/adr/0021-configuration-is-editable-from-the-ui.md))
**When** something needs to show the operator what they are confirming
**Then** the model can produce it: per meter, the **exact topic** that will be published and the
`device_id` behind it
**And** it is asserted against literal strings, not against the code's own expression of the rule.

> The mis-map this story guards against is invisible unless the operator sees **the serial beside
> the meter name and the topic** — `prd.md:135` says exactly that: *"serial beside each so he can't
> cross-wire"*. A confirmation screen that showed only names would be a click that proves nothing.
>
> "Asserted against literal strings" is not pedantry: the Story 4.2 review found a `conformant` row
> whose evidence was a test asserting production's own expression against itself.

**AC5 — a confirmed configuration is not re-confirmed on every start**

**Given** a confirmed configuration and a container restart or image update
**When** the bridge starts
**Then** it publishes, with no human present
**And** the confirmation survives the schema-version rules of Story 5.2's AC5.

> Journey 3 is `docker compose pull && docker compose up -d`. A guard that demanded a human after
> every update would be a guard nobody keeps — it would be switched off, and the switching-off would
> be the real configuration.

## Tasks / Subtasks

- [x] **Task 1 — read before writing**
  - [x] `app/store.rs` — `exists` is the existing seam between *absent* and *present*; this story
        adds a second distinction *inside* present, and must not blur the first.
  - [x] `lib.rs::run_unconfigured` — it already does most of AC1. Decide whether the unconfirmed
        state reuses it or needs its own, and say why in the code.
  - [x] Story 5.2's `unconfigured_start.rs` — the harness and its guards are reusable verbatim.
  - [x] `app/reconfigure.rs` — AC3 is a change classification, and that module already exists to
        answer "what did this change cost". Do not start a second one.

- [x] **Task 2 — the model** (AC: 2, 3)
  - [x] `mapping_confirmed` in `StoredConfig`, schema version bumped, defaulting to **`false`** for
        a file that predates it — the safe direction, and the one an operator can undo in one click.
  - [x] The withdrawal in AC3, at the boundary every writer passes.
  - [x] A test that the withdrawal survives a path the UI does not own: change the meter set by
        writing the file directly, and confirm it comes back unconfirmed.

- [x] **Task 3 — the startup state** (AC: 1, 5)
  - [x] Three states distinguishable in code, one trace each, all at a level the default filter
        shows.
  - [x] The image smoke tests in `scripts/docker-smoke.sh` gain the unconfirmed case — that file is
        where a change to how the bridge starts is now caught, and this is one.

- [x] **Task 4 — what the operator is shown** (AC: 4)
  - [x] A function on the model returning per-meter (meter id, serial, device id, full topic).
  - [x] Asserted against literal topic strings.

- [x] **Task 5 — falsification** (AC: all)
  - [x] Remove the confirmation check and confirm an unconfirmed configuration publishes — against
        a **real broker**, and prove the harness sees the CONNECT for a confirmed one first.
  - [x] Make the withdrawal a no-op and confirm a changed mapping stays confirmed.
  - [x] Default `mapping_confirmed` to `true` for an old file and confirm the test catches it.
  - [x] `./scripts/ci-local.sh`, **full** — not `--fast`; this story changes how the binary starts,
        and `--fast` does not build the image.

- [x] **Task 6 — the consequences** (see the standing rule)
  - [x] `docs/manual/chapters/04-configuration.tex`: the three states, and the new key.
  - [x] `docs/manual/chapters/09-appendix-config-reference.tex`: `mapping_confirmed`.
  - [x] `.env.example`'s "what moved into config.toml" list.
  - [x] Epic 5's FR list: FR25 stops being outstanding.

## What is done, verified 2026-08-08 (recorded by the closing review, not by the implementing session)

**Every box above was ticked with no artefact named beside it, and every one of them is true.**
That is worth stating plainly, because it is the opposite of what the same check found on stories
3.1, 5.2 and 6.1 — there, ticks stood for work that was partly or wholly absent. Here the work
exists and only the evidence was missing, which is a documentation defect and not a false claim.
The anchors below are what the review had to reconstruct; a future reader gets them for free.

- **Task 2, the model.** The field is `app/store.rs:132`; the schema history is at `:50`
  (`mapping_confirmed` arrived at version 3, `ui_port` took it to 4). The withdrawal is `save` at
  `:551`, and the caller's value is discarded rather than trusted — the doc comment at `:531` says
  why, in the AC's own terms. `confirm` at `:585` is deliberately **not** routed through `save`,
  and the asymmetry is argued rather than left to be discovered: sending it through `save` would
  clear the flag it exists to set.
- **Task 2's third box — the withdrawal survives a path the UI does not own.** Four tests, and
  they are not variations on one: `:984` (a caller that changes the mapping *and* asserts
  confirmation in the same write, falsified 2026-08-04, and the two neighbouring tests stayed
  green under that mutation — which is what makes it worth having alone); `:1016` (a duplicated
  meter must not turn the comparison into a subset test — **found by review**, against code that
  had survived three falsifications, none of which had targeted `same_mapping`); `:1047` and
  `:1064` for the other direction, a write that must **not** cost a second click.
- **Task 3, the startup states.** `app/phase.rs` — four `Decision` variants, the unconfirmed arm
  checked *before* the publish arm and the ordering explained at `:77`. Falsified at `:85` with
  five mutations, one per property, **each asserting its own text changed before anything ran** —
  a discipline that exists because on 2026-08-04 `rustfmt` had reflowed a target and the test
  stayed green. Mutation B is the instructive one and the file says so: the obvious mutation makes
  the test fail at its *precondition*, which would have proved nothing.
- **Task 3's second box.** `scripts/docker-smoke.sh:120`–`:165` covers the unconfirmed image, and
  it asserts the thing that actually matters: not only that the container says *"has NOT been
  confirmed"* and stays up, but that it does **not** say *"no configuration yet"*. The negative
  assertion is written as an explicit `if`, with the reason in the comment — under `set -e` an
  and-list makes the exit code describe something other than what was measured.
- **Task 4, AC4.** `app/config.rs:783` (`mapping_preview`), test at `:959`. Literal strings, as the
  AC demanded: `spBv1.0/Plant/DDATA/Bridge01/9202685`, and the device UUID asserted in full,
  because *"it is the half that is easy to cross-wire"*.
- **Task 5, the falsification.** `tests/unconfirmed_publishes_nothing.rs`, and it is the best
  absence test in the repository. Three tests, the **premise first**: a confirmed mapping is shown
  to produce an NBIRTH on this very harness, so that the silence asserted next means something.
  `wait_until_it_says` (`:97`) exists because deleting one `.env("SMARTME_CLIENT_ID", "x")` once
  made the binary exit on its first turn while *"nothing reached the broker"* still held. The
  falsification output is copied at `:169` and names the topic that reached the wire — an
  **NBIRTH**, not a DDATA, which is the whole distinction AC1 was written around.
- **Task 6, the consequences.** All four: `04-configuration.tex:53` and `:76`,
  `09-appendix-config-reference.tex:71`, `.env.example:30` and `:122`, and `epics.md:286` records
  **FR25 met by 5.3**. This is the only one of the seven `review` stories whose consequence sweep
  is complete — story 3.2's reached the manual and not `epics.md`.

**Two things this review could not verify, and one it changed.**

- **The `ci-local.sh` box cannot be checked by reading.** Same position as story 5.2's, with the
  same answer: it has been run since, over the same tree, for later stories, and all three GitHub
  workflows are green on every commit after this one. That is the stronger claim, but it is not
  the claim the box makes.
- **AC3 was stating the opposite of the shipped behaviour** and is now amended in place, above.
  This is the eighth occurrence of the recurring shape here: the code and the manual get corrected
  together, and the criterion stating the consequence does not.
- **`### The original note, kept for the reasoning` (below) is now a duplicate.** It repeats the
  FR24 paragraph immediately above it, which was written to supersede it, and it opens with two
  blank lines. Left in place rather than deleted — the section says it is kept for the reasoning —
  but a reader reaching it after the settled version will read the open question twice and cannot
  tell which is current from the text alone.

## Dev Notes

### What this story does NOT do

**No screen.** Per ADR 0021 the confirmation *screen* is Epic 6. This story owns the state, the
withdrawal rule, and the data a screen would render — all testable without a line of HTML, which is
the whole point of the split.

**No re-confirmation on a namespace change.** Excluded deliberately, and stated in AC3 so that a
reviewer disagreeing with it is disagreeing with a decision rather than finding an omission.

### The state this story adds is a fourth, and the count matters

Story 5.2 established: *absent* → comes up, no session; *present and invalid* → refuses to start.
This adds *present, valid, unconfirmed* → comes up, no session, **different message**. If the two
"comes up, no session" states share a trace line, an operator cannot tell "I have not configured it"
from "I have not confirmed it", and the second is one click from being fixed while the first is not.

### The trap in AC1's test, again

Every absence assertion in this repository has to be paired with a presence one. `unconfigured_start.rs`
does it by requiring the specific log line before it will believe the process is alive on purpose;
`chaos_device_certificates.rs` does it by counting NBIRTHs rather than asserting none. Reuse
whichever fits — do not write a third pattern.

### FR24 — SETTLED, and recorded as met

The mapping **scheme** is fixed and defaulted: the topic is
`spBv1.0/{group}/{type}/{node}/{serial}` and the metrics are `Power` and `Energy` under the meter
name. What the operator configures is the identity, not a template — and `mapping_preview` now
renders exactly that, per meter, for the confirmation this story adds. **FR24 is met.** A per-meter
editable topic *template* was never in the requirement and is not implied by it; if one is ever
wanted it is a new FR, not an unfinished one.

### The original note, kept for the reasoning



FR24 — *"configure the meter→topic/tag mapping, with sensible defaults"* — is worth settling in this
story rather than leaving to drift. The mapping **scheme** is fixed and defaulted: the topic is
`spBv1.0/{group}/{type}/{node}/{serial}` and the metrics are `Power` and `Energy` under the meter
name. What the operator configures is the identity, not a template. **If that is the intended
reading, FR24 is met and should be recorded as met**; if the requirement meant a per-meter editable
topic template, then it is not started and needs its own story. Decide it here — an FR that nobody
ever declares met is an FR that is discovered open at the end of the epic.

### What is left in Epic 5 after this

**FR43 and NFR16** — the broker connection secured with TLS and/or authentication — and they are
**blocked upstream**, not merely unstarted: `rumqttc`'s default TLS stack pins a `rustls-webpki`
carrying unfixed advisories that cannot be upgraded past its own requirement, so it ships disabled.
The variable names are reserved. That blocker is a dependency's release, not a task anyone here can
schedule, so it is recorded as **[#50](https://github.com/guycorbaz/smartme_mqtt/issues/50)** rather
than as a story nobody can start. Epic 5 should carry FR43 and NFR16 as *blocked upstream*.
