# Story 4.4: Primary Host / STATE — measure what the host actually does

Status: ready-for-dev

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

- [ ] **Task 1 — build the observer, and do not reuse the one that already exists** (AC: 1)
  - [ ] New file `crates/smartme-bridge/tests/observe_primary_host_state.rs`, `#[tokio::test]`, `#[ignore = "…"]`, gated on an env var with **no default**. Mirror `chaos_sigterm_no_lie_against_an_external_broker` (`chaos_sigterm_no_lie.rs:189-209`) for the gating shape.
  - [ ] **DO NOT use `common::named_subscriber_on`.** Read the trap in Dev Notes first — it will silently show you nothing and you will conclude the host publishes no STATE.
  - [ ] Capture per message: **topic · raw payload bytes · `retain` · `qos` · local receive time**. `rumqttc::Publish` carries `retain` and `qos` directly. Print the payload as UTF-8 if it is valid UTF-8, hex otherwise — **do not attempt a Sparkplug decode**.
  - [ ] Subscribe to `spBv1.0/STATE/#` **at QoS 1** and, separately, note whether a broader `spBv1.0/#` shows STATE arriving on any other topic shape.
  - [ ] **Publish nothing.** No will, no birth, no probe message. See "Scope boundaries".

- [ ] **Task 2 — arm the run against the production broker, safely** (AC: 1)
  - [ ] Broker address comes from the environment only. **Nothing deployment-specific may be committed** — the repository is public. `.env` holds the real values; committed files carry placeholders.
  - [ ] Use a client id that is unmistakably an observer and unique (e.g. `state-observer-4-4`). A broker **evicts the older session when a client id reconnects**, so a colliding id would silently unplug something.
  - [ ] Set **clean session true** so the run leaves no persistent session queueing messages on a production broker after it ends.
  - [ ] Confirm with Guy before the restart step. The broker is production and Ignition is live on it.

- [ ] **Task 3 — observe the steady state** (AC: 1, 3)
  - [ ] Record every retained STATE message delivered **on subscribe**. These are a snapshot of current state, not transitions — the `retain` flag is how you tell, and it is why Task 1 must capture it.
  - [ ] Record the **host id** — the last topic token — and whether more than one host publishes STATE.
  - [ ] Decode the payload as **JSON**, and record the literal keys and value types actually present, not the ones the specification predicts.
  - [ ] For each observation, write what else could have produced it (AC3). Starters in Dev Notes.

- [ ] **Task 4 — observe an Ignition restart** (AC: 1, 3)
  - [ ] With the observer already connected and recording, have Guy restart Ignition.
  - [ ] Record, in order and with timestamps: the offline STATE (if any), the reconnect, the online STATE. Note `retain` and `qos` on each.
  - [ ] **Record whether an offline STATE appears at all.** A graceful stop may publish a death and then send a DISCONNECT — which instructs the broker to discard the will. A crash would produce the will instead. These are different observations and the write-up must say which one was made.
  - [ ] Compare the `timestamp` values across the offline/online pair against your own receive times. The specification expects birth and will timestamps to **match each other** and to be the CONNECT-time value — so a payload timestamp is not a publish time.

- [ ] **Task 5 — write the findings** (AC: 1, 2, 3, 4)
  - [ ] New file `docs/primary-host-state-observation.md`, structured like `docs/ignition-contract-runbook.md`: *Before you start · Running it · What each step proves (and what else could produce it) · Record of runs · Interpreting the result · Clean-up*.
  - [ ] State what an edge node ignoring STATE loses **here** (AC2) — not in general. Anchor it in what was observed.
  - [ ] Answer AC4 against the eleven clauses listed in Dev Notes: relevant · irrelevant · undetermined, one line each.
  - [ ] Record the **MQTT Engine module version**, not only the Ignition platform version. Story 1.15 left that gap open and it governs Sparkplug behaviour more directly than the platform version does.

- [ ] **Task 6 — hand off cleanly** (AC: 4)
  - [ ] Update the eleven conformance-matrix rows **only** to point at the findings document. **Do not change their verdicts** — 4.5 decides.
  - [ ] *(Already done at story creation — verify it still holds.)* `epics.md` Story 4.5 said *"0015 is next as of 2026-07-28"*; 0015 was consumed hours later by the 4.3 review. Amended to **0016**, with the second occurrence recorded rather than the digit quietly bumped. **Check `docs/adr/` rather than trusting any number in `epics.md`** — this has now happened twice.
  - [ ] `./scripts/ci-local.sh --fast`. This story adds a test that never runs unattended; the run is a regression check.

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

### Debug Log References

### Completion Notes List

### File List
