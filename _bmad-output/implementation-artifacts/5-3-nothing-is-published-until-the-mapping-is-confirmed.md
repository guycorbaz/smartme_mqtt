# Story 5.3: Nothing is published until a human has confirmed the mapping

Status: ready-for-dev

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
> **Note which changes do NOT withdraw it**: the publish period, the broker, the log settings, the
> Sparkplug identity. That last one deserves a second look and is deliberately excluded: `group_id`
> and `node_id` change the *namespace*, not the *mapping*, and they already cost a new session under
> [Story 5.2's AC4 table](5-2-config-persists-and-reloads-without-a-restart.md). If a review decides
> a namespace change also needs re-confirming, that is a change to this AC and not a bug in it.

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

- [ ] **Task 1 — read before writing**
  - [ ] `app/store.rs` — `exists` is the existing seam between *absent* and *present*; this story
        adds a second distinction *inside* present, and must not blur the first.
  - [ ] `lib.rs::run_unconfigured` — it already does most of AC1. Decide whether the unconfirmed
        state reuses it or needs its own, and say why in the code.
  - [ ] Story 5.2's `unconfigured_start.rs` — the harness and its guards are reusable verbatim.
  - [ ] `app/reconfigure.rs` — AC3 is a change classification, and that module already exists to
        answer "what did this change cost". Do not start a second one.

- [ ] **Task 2 — the model** (AC: 2, 3)
  - [ ] `mapping_confirmed` in `StoredConfig`, schema version bumped, defaulting to **`false`** for
        a file that predates it — the safe direction, and the one an operator can undo in one click.
  - [ ] The withdrawal in AC3, at the boundary every writer passes.
  - [ ] A test that the withdrawal survives a path the UI does not own: change the meter set by
        writing the file directly, and confirm it comes back unconfirmed.

- [ ] **Task 3 — the startup state** (AC: 1, 5)
  - [ ] Three states distinguishable in code, one trace each, all at a level the default filter
        shows.
  - [ ] The image smoke tests in `scripts/docker-smoke.sh` gain the unconfirmed case — that file is
        where a change to how the bridge starts is now caught, and this is one.

- [ ] **Task 4 — what the operator is shown** (AC: 4)
  - [ ] A function on the model returning per-meter (meter id, serial, device id, full topic).
  - [ ] Asserted against literal topic strings.

- [ ] **Task 5 — falsification** (AC: all)
  - [ ] Remove the confirmation check and confirm an unconfirmed configuration publishes — against
        a **real broker**, and prove the harness sees the CONNECT for a confirmed one first.
  - [ ] Make the withdrawal a no-op and confirm a changed mapping stays confirmed.
  - [ ] Default `mapping_confirmed` to `true` for an old file and confirm the test catches it.
  - [ ] `./scripts/ci-local.sh`, **full** — not `--fast`; this story changes how the binary starts,
        and `--fast` does not build the image.

- [ ] **Task 6 — the consequences** (see the standing rule)
  - [ ] `docs/manual/chapters/04-configuration.tex`: the three states, and the new key.
  - [ ] `docs/manual/chapters/09-appendix-config-reference.tex`: `mapping_confirmed`.
  - [ ] `.env.example`'s "what moved into config.toml" list.
  - [ ] Epic 5's FR list: FR25 stops being outstanding.

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

### FR24, and whether it is already met

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
