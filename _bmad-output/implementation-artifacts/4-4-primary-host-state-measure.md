# Story 4.4: Primary Host / STATE — measure what the host actually does

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As the maintainer,
I want to observe the real primary-host mechanism before designing for it,
so that the decision rests on this deployment's behaviour rather than on a reading of the spec.

## Acceptance Criteria

**AC1 — the observation is made and recorded**

**Given** the author's broker, which carries live `spBv1.0/STATE/…` topics
**When** a read-only observer records the STATE traffic — **topic, raw payload, retain flag and QoS** — across an Ignition restart
**Then** the findings are recorded: whether a primary host ID is configured, what the host publishes on going online and offline, and whether it expects edge nodes to react.

**AC2 — the cost of ignoring STATE is stated plainly**

**Given** the observation
**When** it is written up
**Then** it states plainly what an edge node that ignores STATE — which is what the bridge does today — actually loses **in this deployment**.

**AC3 — the record says how it could have passed wrongly**

**Given** this is a human-run gate
**When** the runbook is written
**Then** **every observation step states what *else* could produce the same result** — `CLAUDE.md` requires it, and the Tier-3 contract test nearly returned a false pass because two of its five steps were confounded.

**AC4 — the eleven clauses Story 4.3 opened are answerable from the findings**

**Given** the eleven `gap (unimplemented)` rows the conformance audit filed against Stories 4.4–4.6
**When** the write-up is finished
**Then** it says, for each, whether this deployment's behaviour makes the clause **relevant, irrelevant, or still undetermined** — 4.5 decides, but it must not have to re-measure.

*AC3 and AC4 added 2026-07-28 at story creation. AC3 applies a standing `CLAUDE.md` rule to the first human-run gate since Story 1.15. AC4 exists because Story 4.3 produced a concrete list this story is the input to, and without it 4.5 would inherit findings it cannot map onto the clauses it must rule on.*

## Tasks / Subtasks

- [x] **Task 1 — build the observer, and do not reuse the one that already exists** (AC: 1)
  - [x] New file `crates/smartme-bridge/tests/observe_primary_host_state.rs`, `#[tokio::test]`, `#[ignore = "…"]`, gated on an env var with **no default**. Mirror `chaos_sigterm_no_lie_against_an_external_broker` (`chaos_sigterm_no_lie.rs:189-209`) for the gating shape.
  - [x] **DO NOT use `common::named_subscriber_on`.** Read the trap in Dev Notes first — it will silently show you nothing and you will conclude the host publishes no STATE.
  - [x] Capture per message: **topic · raw payload bytes · `retain` · `qos` · local receive time**. `rumqttc::Publish` carries `retain` and `qos` directly. Print the payload as UTF-8 if it is valid UTF-8, hex otherwise — **do not attempt a Sparkplug decode**.
  - [x] Subscribe to `spBv1.0/STATE/#` **at QoS 1** and, separately, note whether a broader `spBv1.0/#` shows STATE arriving on any other topic shape.
  - [x] **Publish nothing.** No will, no birth, no probe message. See "Scope boundaries".

- [x] **Task 2 — arm the run against the production broker, safely** (AC: 1)
  - [x] Broker address comes from the environment only. **Nothing deployment-specific may be committed** — the repository is public. `.env` holds the real values; committed files carry placeholders.
  - [x] Use a client id that is unmistakably an observer and unique (e.g. `state-observer-4-4`). A broker **evicts the older session when a client id reconnects**, so a colliding id would silently unplug something.
  - [x] Set **clean session true** so the run leaves no persistent session queueing messages on a production broker after it ends.
  - [x] Confirm with Guy before the restart step. The broker is production and Ignition is live on it.

- [x] **Task 3 — observe the steady state** (AC: 1, 3)
  - [x] Record every retained STATE message delivered **on subscribe**. These are a snapshot of current state, not transitions — the `retain` flag is how you tell, and it is why Task 1 must capture it.
  - [x] Record the **host id** — the last topic token — and whether more than one host publishes STATE.
  - [x] Decode the payload as **JSON**, and record the literal keys and value types actually present, not the ones the specification predicts.
  - [x] For each observation, write what else could have produced it (AC3). Starters in Dev Notes.

- [x] **Task 4 — observe an Ignition restart** (AC: 1, 3)
  - [x] With the observer already connected and recording, have Guy restart Ignition.
  - [x] Record, in order and with timestamps: the offline STATE (if any), the reconnect, the online STATE. Note `retain` and `qos` on each.
  - [x] **Record whether an offline STATE appears at all.** A graceful stop may publish a death and then send a DISCONNECT — which instructs the broker to discard the will. A crash would produce the will instead. These are different observations and the write-up must say which one was made.
  - [x] Compare the `timestamp` values across the offline/online pair against your own receive times. The specification expects birth and will timestamps to **match each other** and to be the CONNECT-time value — so a payload timestamp is not a publish time.

- [x] **Task 5 — write the findings** (AC: 1, 2, 3, 4)
  - [x] New file `docs/primary-host-state-observation.md`, structured like `docs/ignition-contract-runbook.md`: *Before you start · Running it · What each step proves (and what else could produce it) · Record of runs · Interpreting the result · Clean-up*.
  - [x] State what an edge node ignoring STATE loses **here** (AC2) — not in general. Anchor it in what was observed.
  - [x] Answer AC4 against the eleven clauses listed in Dev Notes: relevant · irrelevant · undetermined, one line each.
  - [x] Record the **MQTT Engine module version**, not only the Ignition platform version. Story 1.15 left that gap open and it governs Sparkplug behaviour more directly than the platform version does.

- [x] **Task 6 — hand off cleanly** (AC: 4)
  - [x] Update the eleven conformance-matrix rows **only** to point at the findings document. **Do not change their verdicts** — 4.5 decides.
  - [x] *(Already done at story creation — verify it still holds.)* `epics.md` Story 4.5 said *"0015 is next as of 2026-07-28"*; 0015 was consumed hours later by the 4.3 review. Amended to **0016**, with the second occurrence recorded rather than the digit quietly bumped. **Check `docs/adr/` rather than trusting any number in `epics.md`** — this has now happened twice. **Re-verified 2026-07-29 against `docs/adr/`: the highest is `0015-language-type-invariants-as-conformance-evidence.md`, so 0016 is still free and the note still holds.**
  - [x] `./scripts/ci-local.sh --fast`. This story adds a test that never runs unattended; the run is a regression check.

## Dev Notes

### What this story is, and is not

It **measures**. It does not decide, does not implement, and does not write an ADR — that is Story 4.5,
whose acceptance criteria depend on this one's output. Resist designing the STATE handler here; the
whole point of splitting 4.4 from 4.5 was the Epic 1 retrospective rule that *an acceptance criterion
may not defer its decision to an artifact that does not yet exist*.

**No production code changes.** `git diff -- crates/*/src/` is empty at the end. The only Rust added
is one `#[ignore]`d test under `crates/smartme-bridge/tests/`.

### ⚠️ The trap that will cost you the story if you miss it

`crates/smartme-bridge/tests/common/mod.rs` already has a subscriber that looks perfect for this —
`named_subscriber_on(host, port, client_id)`, used by the chaos tests against a real broker. **It is
the wrong tool and it fails silently.**

```rust
// common/mod.rs:128-137
Ok(Event::Incoming(Packet::Publish(p))) => {
    if let Ok(payload) = sparkplug_b::decode(&p.payload) {   // ← protobuf
        let seen = Seen { topic: p.topic.clone(), payload };
        ...
    }
    // no else: a payload that does not decode is DISCARDED
}
```

Three separate failures if you reuse it:

1. **STATE payloads are JSON UTF-8, not protobuf.** `sparkplug_b::decode` will usually fail on them
   and the message is dropped with no log line. You would observe **silence on a topic that is
   actually busy** and report "the host publishes no STATE" — a false negative, and precisely the
   class of error this project keeps paying for.
2. **Protobuf decoding is permissive.** Some JSON byte sequences decode into a *garbage* `Payload`
   rather than erroring, so the failure mode is not reliably silence — it can be plausible nonsense.
3. **`Seen` carries neither `retain` nor `qos`.** AC1 requires both. They exist on `rumqttc::Publish`
   (`p.retain`, `p.qos`) and are thrown away before `Seen` is built.

Write a fresh, small observer. Do not "fix" `common/mod.rs` — the chaos tests depend on its current
shape and this story must not touch them.

### Why the retain flag decides whether your restart observation means anything

The specification requires a Host Application's STATE to be published **retained**
(`tck-id-operational-behavior-host-application-connect-will-retained`, `-connect-birth-retained`,
`Sparkplug_5_Operational_Behavior.adoc:767-768`, `:786-787`) and at **QoS 1** (`:765-766`, `:784-785`).

Retained means the broker replays the last STATE to **every new subscriber immediately on
subscribe**. So the first `online: true` you see is almost certainly a *stored snapshot*, not a live
transition — and if you do not record `retain`, an Ignition restart's fresh publish is
indistinguishable from the retained one you were handed at connect time. **That single flag is the
difference between observing a restart and merely observing that a broker has a memory.**

### What the specification predicts, so you can record where reality differs

Quoted so the write-up can compare rather than assume. **Where the observation and the specification
disagree, the observation wins and the disagreement is the finding.**

- **Topic** — `spBv1.0/STATE/sparkplug_host_id` (`operational-behavior-host-application-connect-will-topic`, `:757-759`).
- **Payload** — *"MUST be JSON UTF-8 data. It MUST include two key/value pairs where one key MUST be
  'online' and it's value is a boolean … The other key MUST be 'timestamp'"*
  (`-connect-will-payload`, `:760-764`). The timestamp is *"a numeric value representing the current
  UTC time in milliseconds since Epoch"* (`-death-payload`, `:808-812`).
- **Birth and will timestamps match each other** — the birth's timestamp *"MUST match the timestamp
  value that was used in the immediately prior MQTT CONNECT packet Will Message payload"*
  (`-connect-birth-payload`, `:779-783`). So neither is a publish time.
- **QoS 1, retain true**, on both birth and death.
- **Per-server timestamps** — with multiple brokers the host keeps a STATE timestamp per server
  (`-multi-server-timestamp`, `:792-796`). One broker here, so expect this to be unobservable.

### The eleven clauses this story is the input to (AC4)

Story 4.3 filed these as `gap (unimplemented)` pointing at Stories 4.4–4.6. Story 4.5 rules on them;
this story must leave each answerable. All are in `docs/sparkplug-conformance.md`, chapter 5.

| Clause | What the observation must settle |
| --- | --- |
| `message-flow-edge-node-birth-publish-phid-wait` | Would waiting for the host before birthing change anything here, or does the host tolerate a node that births first? |
| `message-flow-edge-node-birth-publish-phid-wait-id` | Is there a host id an edge node could match against? |
| `message-flow-edge-node-birth-publish-phid-wait-online` | Is the `online` key present and boolean, as the spec requires? |
| `message-flow-edge-node-birth-publish-phid-wait-timestamp` | Do successive STATE timestamps move monotonically enough for the anti-replay rule to be usable? |
| `message-flow-edge-node-birth-publish-phid-offline` | Does an offline STATE actually appear on a restart — and would disconnecting on it be right or catastrophic here? |
| `operational-behavior-edge-node-birth-sequence-wait` | Same question as `-phid-wait`, from the birth-sequence side |
| `operational-behavior-edge-node-termination-host-offline` | Ditto, for disconnecting |
| `operational-behavior-edge-node-termination-host-offline-reconnect` | There is one broker — is "walk to the next server" meaningful at all here? |
| `operational-behavior-edge-node-termination-host-offline-timestamp` | Is a stale offline STATE (older timestamp) something this deployment actually produces? |
| `operational-behavior-primary-application-state-with-multiple-servers-state-subs` | Does the host publish a STATE birth certificate as the clause assumes? |
| `operational-behavior-primary-application-state-with-multiple-servers-walk` | Single broker — relevant or not? |

**A likely and legitimate outcome: several are `irrelevant` because there is one broker.** Say so
explicitly rather than leaving them open; "one broker, so server-walking cannot arise" is a finding.

### How this observation could pass wrongly — AC3 starters

Not exhaustive; the runbook must extend these per step.

- **You see no STATE.** Could be: no host configured · the protobuf-decode trap above · a topic
  filter that does not match · broker ACLs hiding the topic from your client · the host publishes on
  a non-standard topic. Distinguish by subscribing to `spBv1.0/#` and `#` and comparing.
- **You see `online: true` and call it a birth.** Could be the retained snapshot from a session
  established days ago. Only `retain: false` proves a live publish.
- **You see no offline STATE on restart.** Could be: a graceful stop that published a death you
  missed by connecting late · a clean DISCONNECT that made the broker discard the will · the host
  genuinely not implementing it. These have different consequences for 4.5 and must not be conflated.
- **Timestamps look wrong.** They are CONNECT-time values by design, not publish times. Comparing
  them to your receive clock and concluding "the host's clock is skewed" would repeat the mistake
  Story 1.1 had to settle for `ValueDate`.
- **One run looks conclusive.** A single restart is one sample. Say how many were observed.

### Deployment facts that constrain this story

- **The broker is production and it is the only one available.** Ignition is live on it. Memory of
  this project: never aim a test at it unasked. This story is read-only, which is why it is
  allowed at all — but the *restart* in Task 4 is a production action and needs Guy's explicit go.
- **The repository is PUBLIC.** No broker address, no host id, no topic containing a real site name
  may be committed. `.env` holds real values; committed files carry empty placeholders. If the
  observed host id is identifying, redact it in the findings and say that you did.
- **Guy runs Ignition 8.3.7.** Cirrus Link's module documentation targets 8.1 — trust the
  measurement over their tables. That mismatch is what misled the original quality codes.

### Existing code you should read before writing anything

- `crates/smartme-bridge/tests/common/mod.rs` — the subscriber to **not** reuse, and the source of
  the client-id-eviction warning (`:93-97`).
- `crates/smartme-bridge/tests/chaos_sigterm_no_lie.rs:150-210` — the arming pattern for a real
  broker: `#[ignore]` with a reason naming the env vars, `expect` messages that say *"there is
  deliberately no default"*, and a refusal to run against the default production group.
- `docs/ignition-contract-runbook.md` — the runbook shape to mirror, including its *Record of runs*
  table and its mandatory *Clean-up* section.
- `crates/smartme-bridge/src/app/mqtt_driver.rs:156-169` — how the bridge builds its own MQTT
  options today. You are **not** changing it; read it so the findings can say precisely what would
  have to change in 4.5.

### No new dependencies are needed — do not add any

Everything this story requires is already a dev-dependency of `smartme-bridge`
(`crates/smartme-bridge/Cargo.toml:20-25`):

- **`rumqttc`** (workspace, 0.25) — the MQTT client. Use `rumqttc::{AsyncClient, MqttOptions, Event,
  Packet, QoS}`, **not** `rumqttc::v5::*`; the bridge speaks MQTT 3.1.1 and the observer should match
  what the deployment actually uses.
- **`serde_json`** (workspace, 1) — already present, for parsing the STATE payload.
- **`tokio`** with `test-util` — for the async test.

`dev-story` HALTs on "additional dependencies beyond story specifications". There are none here, so
that HALT should not fire. If you find yourself reaching for one, you are probably solving the wrong
problem — re-read the trap section.

### HALT conditions specific to this story

This is the first story since 1.15 that cannot be completed by the agent alone.

- **Guy is unavailable, or declines the Ignition restart.** Do Tasks 1–3 (build the observer, record
  the steady state), write up what *was* observed, and mark **Task 4 and the parts of AC1 that depend
  on it as unmet, with an issue** — `CLAUDE.md` requires unmet criteria to be recorded as unmet. Do
  **not** infer the restart behaviour from the steady state, and do not mark AC1 satisfied.
- **The broker rejects the observer, or an ACL hides the STATE topic.** Stop and report. Do not
  work around it by publishing anything.
- **No STATE traffic exists at all.** That is a legitimate and important finding — but rule out the
  protobuf-decode trap, the topic filter and the ACL first, and say in the write-up which of those
  you eliminated and how.

### Project Structure Notes

- Observer: `crates/smartme-bridge/tests/observe_primary_host_state.rs` — **not** in
  `crates/sparkplug-b/`. That crate is published and `tests/no_context_leak.rs` guards `src/` against
  bridge context; more importantly, a deployment observation is not a Sparkplug-library concern.
- Findings: `docs/primary-host-state-observation.md`, alongside the Tier-3 runbook.
- The manual (`docs/manual/`) documents implemented behaviour. This story implements nothing, so it
  needs no manual edit — **unless** the findings contradict something the manual currently claims.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md#Story 4.4`] — AC1 and AC2 verbatim; the
  read-only note at `:845`
- [Source: `_bmad-output/planning-artifacts/epics.md#Story 4.5`] — the decision story this feeds, and
  its ADR-numbering note at `:865`
- [Source: `docs/sparkplug-conformance.md`, chapter 5, *Host Application and Primary Host*] — the
  eleven clauses, and why they are `gap` rather than `n/a`
- [Source: `docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_5_Operational_Behavior.adoc:753-796`] —
  the Host Application connect/birth/will clauses quoted above
- [Source: `docs/ignition-contract-runbook.md`] — the runbook pattern for a human-run gate
- [Source: `_bmad-output/implementation-artifacts/1-15-tier-3-ignition-contract-test-manual-runbook.md`] —
  the precedent: what a manual gate produced, and how two of its five steps were confounded
- [Source: `_bmad-output/implementation-artifacts/epic-1-retro-2026-07-26.md`] — the retrospective
  that found STATE was absent from every planning artifact
- [Source: `CLAUDE.md`] — read the norm first; manual steps must state how they could pass wrongly;
  nothing deployment-specific in a public repo

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-07-28.

### Debug Log References

**The story's central premise was measured, not assumed.** The whole design rests on the claim that
reusing `common::named_subscriber_on` would silently drop STATE messages. A throwaway probe
(added under `crates/smartme-bridge/tests/`, run, removed) fed real-shaped STATE payloads to the
decode call that helper uses:

```
rejected -> DROPPED by the old helper: {"online":true,"timestamp":1784984793000}
   -> failed to decode Protobuf message: buffer underflow
rejected -> DROPPED by the old helper: {"online":false,"timestamp":1784984793000}
rejected -> DROPPED by the old helper: {"timestamp":1784984793000,"online":true}
```

All three rejected, so the old helper's `if let Ok(...)` with no `else` would discard every one. The
premise holds and the failure mode is **silence**, not garbage — which is the more dangerous of the
two, because an empty transcript reads as an answer.

**The observer publishes nothing, and that is checked mechanically rather than asserted:**
`grep -cE "\.publish\(|try_publish|set_last_will"` over the new file returns **0**.

**`clean_session` is set explicitly** rather than inherited from rumqttc's default. #35 records that
relying on that default is how a MUST went unasserted; on a production broker an accidental
persistent session would queue messages for a client id that never returns.

**No deployment specifics committed.** The only host-shaped string in the file is the literal
placeholder `host:1883` inside a doc comment. The broker comes from `SMARTME_STATE_BROKER` with
**no default** — deliberately a different variable from the bridge's own `SMARTME_BROKER_HOST`, so
that pointing an observer at production is always an explicit act rather than something `.env`
does for you.

**`./scripts/ci-local.sh --fast` exits 0**; `cargo clippy -p smartme-bridge --tests -- -D warnings`
is clean; `cargo fmt` applied.

**Re-run 2026-07-29 after Tasks 5–6: `./scripts/ci-local.sh --fast` exits 0** — lock file committed,
fmt, `clippy -D warnings` and cargo-deny all green; **148 tests pass, 3 ignored**, with the
Docker-dependent chaos tests excluded by `--fast` as usual. **Not piped**: the script was redirected
to a log file and its exit code read directly, because piping it hands the exit status to the last
command in the pipe and a red run reads as green. Tasks 5–6 changed no Rust, so this is a pure
regression check.

**Red-green-refactor does not apply to this task and was not faked.** The deliverable is an
observation instrument, and there is no assertion available that would not simply compare our own
expectation to itself — the same position Story 1.15 reached for the Tier-3 contract test. The
falsification that *was* available is the premise probe above, and it was run.

### Completion Notes List

**Tasks 1 and 2 complete. Tasks 3–6 are blocked on a production action and are NOT claimed.**

- **Task 1 — done.** `crates/smartme-bridge/tests/observe_primary_host_state.rs`: `#[ignore]`d,
  env-gated, captures topic · raw bytes · `retain` · `qos` · receive time, prints the payload as
  UTF-8 or hex, reports the JSON *shape actually present* rather than the one the specification
  predicts, and never decodes as Sparkplug. It also prints a diagnostic block when the transcript is
  empty, listing the four things to rule out before "the host publishes no STATE" counts as a
  finding — AC3's discipline built into the tool rather than left to the runbook.
- **Task 2 — done.** Gating mirrors `chaos_sigterm_no_lie_against_an_external_broker`: an env var
  with no default and an `expect` message that says so. Client id `state-observer-4-4`, clean session
  true.

**Task 3 — done, and it found the thing the story existed to find.**

**The deployment does not publish STATE where Sparkplug 3.0.0 requires it.** Three passes:

| Filter | Window | Result |
| --- | --- | --- |
| `spBv1.0/STATE/#` | 25 s | **0 messages** |
| `#` | 20 s | 78 messages, 61 distinct topics, **not one `spBv1.0/…` topic** |
| `STATE/#` | 8 s | **4 retained messages** |

The four, verbatim from the transcript:

```
retain=true  qos=AtLeastOnce  STATE/scada     "OFFLINE" (7 bytes)
retain=true  qos=AtLeastOnce  STATE/IamHost   "OFFLINE" (7 bytes)
retain=true  qos=AtLeastOnce  STATE/ignition  "OFFLINE" (7 bytes)
retain=true  qos=AtLeastOnce  STATE/SCADA     "ONLINE"  (6 bytes)
```

**Two departures from the vendored norm, both measured:**

1. **Topic** — `STATE/<host_id>`, with **no `spBv1.0/` namespace element**. The specification requires
   *"'spBv1.0/STATE/sparkplug_host_id'"* (`tck-id-operational-behavior-host-application-connect-will-topic`,
   `Sparkplug_5_Operational_Behavior.adoc:757-759`, quoted verbatim).
2. **Payload** — the bare ASCII literals `ONLINE` / `OFFLINE`, no JSON, no `timestamp`. The
   specification requires *"JSON UTF-8 data … one key MUST be 'online' … The other key MUST be
   'timestamp'"* (`-connect-will-payload`, `:760-764`).

`retain` and `qos` **do** match the specification (true, 1).

> ### ⚠️ THE CONCLUSION DRAWN HERE WAS WRONG, AND TASK 4 REFUTED IT
>
> This section originally read: *"an Edge Node implementing Sparkplug 3.0.0 literally would never see
> this host's STATE … a bridge built to wait for an online STATE before birthing would therefore
> **never birth**."*
>
> **That was false as a claim about the deployment.** It was a true description of what was on the
> broker at that instant, generalised into a claim about what the host *does* — and the Ignition
> restart showed the host publishes the **fully conformant 3.0 form** as well. See Task 4 below.
>
> It is left standing rather than deleted because the mistake is the most useful thing this story
> produced: **three passes of careful, honest measurement supported a conclusion that one state
> transition destroyed.** A snapshot of retained messages describes the broker's memory, not the
> host's behaviour. This is why AC1 says *"across an Ignition restart"* and not *"observe the STATE
> topics"* — and had the story been scoped to a snapshot, Story 4.5 would have been handed a
> confident, evidenced, wrong premise.

**What this pass deliberately does NOT claim.** The `ONLINE`/`OFFLINE`-on-`STATE/<id>` form looks
like a pre-3.0 Sparkplug convention, but **only the 3.0.0 specification is vendored here** and it
contains no changelog and no mention of that form. So the deviation from 3.0.0 is established; the
claim "this is the v2.2 form" is not, and is left as a question for Story 4.5 rather than asserted.
Same discipline as [#34](https://github.com/guycorbaz/smartme_mqtt/issues/34), where the MQTT
character set could not be cited because that norm is not vendored either.

**And which client owns which host id is undetermined.** Four ids, three OFFLINE and one ONLINE,
two differing only in case (`scada` / `SCADA`). Retained messages outlive their publisher, so some
are plausibly cruft from long-dead clients. **This makes the Ignition restart more valuable than the
story originally scoped it**: it will show which id Ignition actually owns, and whether it
republishes in the legacy or the 3.0 form.

**Task 4 — done. Guy restarted the Ignition container at ~20:25 local (2026-07-28) with two
observers already listening on distinct client ids.** MQTT Engine module version, from Guy:
**v5.0.0-rc1** — which also closes the residual Story 1.15 left open, since the module governs
Sparkplug behaviour more directly than the platform version does.

**The measured sequence.** `retain=true` is a stored snapshot replayed at subscribe; `retain=false`
is a live publish.

| Observer | Message | retain | Payload |
| --- | --- | --- | --- |
| `spBv1.0/#` | at subscribe | true | 42 bytes — `{"online":false,…}` (the death, already stored) |
| `spBv1.0/#` | **live** | **false** | 41 bytes — `{"online":true,"timestamp":1785263196684}` |
| `STATE/#` | at subscribe | true | `OFFLINE` on all four ids |
| `STATE/#` | **live** | **false** | `ONLINE` on `STATE/SCADA` — **published twice** |

**Finding 1 — MQTT Engine v5.0.0-rc1 publishes BOTH forms, and the 3.0 form is fully conformant.**

```
spBv1.0/STATE/SCADA   {"online":true,"timestamp":1785263196684}   retain=true  qos=1
STATE/SCADA           ONLINE                                       retain=true  qos=1
```

Checked clause by clause against the vendored norm: topic `spBv1.0/STATE/sparkplug_host_id` ✅
(`-connect-will-topic`, `:757-759`); JSON UTF-8 with `online` boolean and `timestamp` number ✅
(`-connect-will-payload`, `:760-764`); retain true ✅ (`-connect-birth-retained`, `:786-787`);
QoS 1 ✅ (`-connect-birth-qos`, `:784-785`). **The bridge can implement the specification as written
against this deployment.**

**Finding 2 — the host id is `SCADA`, and the toggle is what proved it.** Only `STATE/SCADA` and
`spBv1.0/STATE/SCADA` moved. `scada`, `ignition` and `IamHost` stayed frozen at `OFFLINE` throughout,
identifying them as **retained residue from dead clients** — retained messages outlive their
publisher indefinitely. Note `scada` and `SCADA` differ only in case, exactly the hazard
`tck-id-case-sensitivity-sparkplug-ids` warns about.

**Finding 3 — the timestamp is real and the host's clock is sane.** `1785263196684` decodes to
**18:26:36 UTC**, roughly 90 seconds before it was read. It is a genuine epoch-millis value, so the
anti-replay clauses that compare successive STATE timestamps (`-phid-wait-timestamp`,
`-termination-host-offline-timestamp`) are **implementable here** — which the steady-state pass had
concluded they were not, because the legacy payload carries no timestamp at all.

**Finding 4 — a real operational hazard survives the correction.** The retained
`spBv1.0/STATE/SCADA` **did not exist before this restart**: two independent passes over `#` and
`spBv1.0/#` found nothing. The most plausible reading is that Engine was upgraded to v5.x and had
not reconnected since, so its last birth predated 3.0 support — **a hypothesis, not a measurement**.
What *is* measured is that a bridge waiting for `spBv1.0/STATE/<host>` before birthing would have
waited **forever** in the broker state that existed an hour earlier, because the message it waits
for had never been published. Story 4.5 must decide what the bridge does when the retained STATE is
simply absent; "wait for online" is not safe on its own.

**Undetermined, and deliberately not guessed.** Whether the `OFFLINE` was published explicitly by
Ignition on shutdown or delivered by the broker as its will cannot be told from this run — both
produce an identical retained message, and the observer subscribed after the fact. Distinguishing
them needs an observer connected *before* the shutdown begins. Recorded as open for Story 4.5.

**Two defects in the observer, both found by using it against the real thing.**
1. **No connection timeout** — the first run parked forever on an unreachable broker and printed
   nothing. Fixed: bounded 15 s wait with a four-point diagnostic. This would have burned the restart
   window.
2. **A hard-coded client id** — two observers cannot watch different topic shapes at once, because a
   broker evicts the older session when an id reconnects; they would have silently unplugged each
   other. Fixed: `SMARTME_STATE_CLIENT_ID`. Watching both forms simultaneously is what caught the
   `spBv1.0` message appearing.

**AC1 is met.** AC2, AC3 and AC4 remain — they are Task 5's write-up.

**One defect in the observer, found by using it.** The first run against the real broker hung: with
no connection timeout, `ready_rx.await` parked forever and printed nothing. Fixed with a 15-second
bounded wait and a four-point diagnostic. Worth recording because the failure would have burned a
restart window that cannot be repeated cheaply — the tool was written for that window and would have
been useless in it.

### Tasks 5 and 6 — done 2026-07-29, from the committed record, with no second observation

**All four ACs are now met.** No broker was touched: every claim below is drawn from
`docs/primary-host-state-observation.md`, from the vendored specification, or from a grep over
`crates/`.

**AC2 — and it argues *against* the obvious reading of the specification.** The plain answer is that
ignoring STATE costs this deployment **nothing while Ignition is up** — there is no command path to
lose, one broker means no stranding — and costs it **everything after an Ignition restart**. The
chain is written out in the document; its load-bearing links are measured: the bridge's NBIRTH is
QoS 0 / `retain=false` so the broker keeps no copy; the bridge holds **no subscription of any kind**
(`grep -rn "subscribe" crates/smartme-bridge/src/` returns one `tracing_subscriber` initialiser and
two comments); its own broker session is untouched by Ignition restarting, so nothing makes it
re-birth; and the protocol's remaining recovery is an NCMD Rebirth
(`-host-reordering-rebirth`, `:565-568`) the bridge neither implements nor could receive.

> **The finding worth carrying out of this story.** The specification's stated motivation for
> waiting on a Primary Host is that the Edge Node *"store data while the Host Application is
> offline … then send all of its stored data"* (`:191-196`). **The bridge has no store-and-forward.**
> Implementing PHID-wait alone here would not preserve one measurement — it converts silent
> publication into deliberate non-publication. Waiting is worth building for the clean re-birth it
> enables, **not** on the grounds the specification gives, and 4.5 must not justify it that way.
> It also reorders the epic: **Story 4.7 (Rebirth) is the higher priority of the two.**

**The one inference in AC2 is labelled as one.** That Ignition's view of the edge node does not
survive its own restart was *not* observed — no tag state was checked. It is inferred from the
Rebirth mechanism's existence and the unretained NBIRTH. It is falsifiable **without causing a
restart**: look at the bridge's tags after the next Ignition restart, whatever its cause. Recorded
in the document rather than quietly relied on, with a note that the step belongs in
`docs/ignition-contract-runbook.md` when 4.5 or 4.7 next touches it.

**AC3 — restructured to the runbook shape, per step.** The record previously had a single
combined table. It now has *What each step proves — and what else could produce it* with a
subsection per step (the narrow subscribe, the `#` sweep, the legacy retained set, the restart),
each stating its confounds and **which were eliminated and how**. Two remain uneliminated and say
so: the origin of the `OFFLINE`, and the one-sample caveat. Step 1's table is the important one —
it lists five candidate causes of silence, four eliminated, and names the fifth as *the one that was
actually true*, which is what the first pass got wrong.

**AC4 — 9 relevant · 2 irrelevant · 0 wholly undetermined**, four carrying a named undetermined
residue. Three things in that ruling are worth a reviewer's attention:

- **Nine of the eleven clauses are conditional** — *"if the Edge Node is configured to wait for a
  Primary Host Application"*. Nothing forces the bridge to configure one. The ruling therefore says
  explicitly that "relevant" means *the deployment supplies what the clause needs*, not *the bridge
  is in breach today*. Reading them as unconditional obligations would misstate our position.
- **`-phid-wait-id` is load-bearing here, not formal.** Three decoy host ids sit permanently
  retained at `OFFLINE`, one of them (`scada`) differing from the live `SCADA` only in case. A
  case-insensitive match would bind the bridge to a dead client and **it would never birth**.
- **`-birth-sequence-wait` is the one clause whose text and section context disagree.** It carries
  no "if configured" conditional, so read literally it binds every Edge Node — but it sits inside
  § *Primary Host Application STATE in Multiple MQTT Server Topologies* (`:576-577`). With one
  broker the two readings differ **in this deployment**, so 4.5 must choose one explicitly. 4.4
  states the conflict and does not resolve it; that is 4.5's call.

**The two `irrelevant` rulings come with a caveat that is not "no work".** With one server, *"the
next available MQTT Server"* is the same server, so a literal `-host-offline-reconnect` degenerates
into reconnect-to-self — a loop for as long as the host is offline. Recorded so 4.5 does not read
"irrelevant" as "nothing to decide".

**Second restart: not taken, and the reasoning is recorded rather than the conclusion.** A repeat
performed the same way would buy a second timestamp sample and a live `retain=false` death, but it
would **not** settle the question that matters — an explicit death and a broker will are
indistinguishable on the wire whenever you subscribe. The sample should be taken opportunistically
at the next unrelated Ignition restart.

**But the open question turned out to be answerable, and this run had the means.** A Host
Application's birth timestamp MUST *"match the timestamp value that was used in the immediately
prior MQTT CONNECT packet Will Message payload"* (`-connect-birth-payload`, `:779-783`) — a will is
stamped at CONNECT. So: **death timestamp equal to the session's online timestamp → the will; later
→ an explicit publish.** This is the same discriminator `chaos_sigterm_no_lie` uses on our own
NDEATH. It could not be applied retroactively because **the death payload's `timestamp` was never
transcribed** and the retained death has since been overwritten by the birth on the same topic. Its
byte count (42 against the birth's 41, and `false` is one character longer than `true`) confirms a
13-digit epoch-millis field was there. **The record's own gap, written into it as a gap** — the
observer prints full payloads, so this was transcription discipline, not a tool limitation.

**The manual carried a claim these findings refute, and the exception in Project Structure Notes
fired.** That section says this story needs no manual edit *"unless the findings contradict something
the manual currently claims"*. `05-mqtt-sparkplug-contract.tex:253` said the absence of Primary Host
support is **"invisible"** on a single-broker installation with a permanently connected host, and
that it *"matters in a redundant-broker topology"*. AC2 shows the first half is false **in exactly
this deployment**: the cost appears when the *host* restarts, on one broker, with no redundancy
involved. Corrected, with the over-claim named rather than silently replaced, and the
redundant-broker stranding kept as the separate consequence it is. `latexmk` exits 0.

**That is the third instance of the pattern this project keeps paying for** — a limitation recorded
correctly in one place while a descriptive sentence about its *consequences* went unexamined. Same
shape as FR20's QoS-0 over-claim (#33) and the RBE passages. The manual was right about *what* is
missing and wrong about *what it costs*.

**Task 6 — verdicts verified unchanged mechanically, not by inspection.** Every one of the eleven
rows was edited by appending ` · [4.4 measured](…): relevance …` to the existing verdict cell.
`git diff docs/sparkplug-conformance.md`, with everything from the `·` onward stripped, shows each
removed line identical to its added line. The summary-table row at `:1583` carries the same pointer
plus the tally, and repeats **"the verdicts above are unchanged; 4.5 decides"**.

**`./scripts/ci-local.sh --fast` — see the Debug Log.** This story changed no Rust; the run is a
regression check only.

### File List

- `docs/primary-host-state-observation.md` — **new**: the observation record and its interpretation.
  Complete: raw record, per-step confounds (AC3), AC2, the eleven clauses ruled (AC4)
- `docs/sparkplug-conformance.md` — modified: the eleven Primary-Host rows and the gap-summary row
  at `:1583` point at the findings. **Verdicts unchanged** — verified by diff, not by inspection
- `docs/manual/chapters/05-mqtt-sparkplug-contract.tex` — modified: the *Known limitations* bullet on
  Primary Host said the gap is "invisible" on a single broker. AC2 refutes that; corrected, over-claim
  named. `latexmk` exits 0
- `crates/smartme-bridge/tests/observe_primary_host_state.rs` — new: the read-only STATE observer
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified: story status
- `_bmad-output/implementation-artifacts/4-4-primary-host-state-measure.md` — modified: this record

*(`_bmad-output/planning-artifacts/epics.md` — the Story 4.5 ADR-number amendment — was committed at
story creation, `ac1ea16`, and is not part of this implementation's diff.)*

## Change Log

| Date | Change |
| --- | --- |
| 2026-07-28 | Tasks 1–2: read-only STATE observer built, armed and lint-clean. The story's premise — that the existing subscriber would silently drop STATE — was measured (protobuf decode rejects the JSON: "buffer underflow"). Observer publishes nothing (grep-verified 0 call sites). Tasks 3–6 blocked pending Guy's go-ahead on the production broker; AC1/AC2/AC4 unmet and recorded as unmet |
| 2026-07-28 | Tasks 3–4 with Guy at the console: steady state recorded, then an Ignition container restart captured live by two observers on distinct client ids. **AC1 met.** MQTT Engine v5.0.0-rc1 recorded, closing the Story 1.15 residual. **The steady-state conclusion was WRONG and the restart refuted it** — Engine publishes the fully conformant Sparkplug 3.0 form (`spBv1.0/STATE/SCADA`, JSON, retain, QoS 1) *as well as* a legacy `ONLINE`/`OFFLINE` form; the earlier "an edge node would never see this host's STATE" was a snapshot generalised into a claim about behaviour. Host id proven to be `SCADA`; the other three are retained residue. Two observer defects found by using it: no connection timeout, hard-coded client id — both fixed. Record committed to `docs/primary-host-state-observation.md` first, because it cost a production restart and lived only in a temp directory. Tasks 5–6 and AC2/AC3/AC4 remain; story stays `in-progress` |
| 2026-07-29 | Tasks 5–6, from the committed record with **no second observation** and no broker. **All four ACs met; status `review`.** AC2: ignoring STATE costs nothing while Ignition is up and costs total loss of tag recovery after an Ignition restart — and the argument runs *against* the specification's own motivation, because PHID-wait without store-and-forward preserves no measurement, which makes **Story 4.7 (Rebirth) the higher priority**. AC3: restructured to the runbook shape with per-step confounds; step 1 names five candidate causes of silence, four eliminated and the fifth being the one that was actually true. AC4: **9 relevant · 2 irrelevant · 0 wholly undetermined**, four with a named residue; nine of the eleven clauses are *conditional* on configuring a Primary Host, `-phid-wait-id` is load-bearing because three retained decoy ids differ from the live one only in case, and `-birth-sequence-wait` is the one clause whose text and section context disagree in a single-broker deployment. Second restart declined with reasoning: it would buy a sample, not the answer. **But the open question turned out answerable from timestamps** (a will is stamped at CONNECT, `-connect-birth-payload` `:779-783` — the same discriminator `chaos_sigterm_no_lie` uses) and could not be applied because the death payload's timestamp was never transcribed; recorded as the record's own gap. Task 6: eleven matrix rows point at the findings, verdicts verified unchanged by diff. `ci-local.sh --fast` exits 0, 148 tests, no Rust changed |
