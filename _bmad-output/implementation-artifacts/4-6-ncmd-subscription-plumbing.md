# Story 4.6: NCMD subscription — plumbing that ignores safely

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As the bridge,
I want to receive node commands without acting on ones I do not understand,
so that an unknown command is never mistaken for a known one.

## Acceptance Criteria

**AC1 — the subscription exists, at QoS 1, and BEFORE the birth**

**Given** a connected session
**When** the driver handles the CONNACK
**Then** it subscribes to `spBv1.0/<group>/NCMD/<node>` **at QoS 1**
**And** the SUBSCRIBE is issued **before** the NBIRTH is published — not merely in the same sequence
**And** the subscription is re-established on **every** reconnect, not only the first.

> The clause is `tck-id-message-flow-edge-node-ncmd-subscribe`
> (`Sparkplug_5_Operational_Behavior.adoc:158-163`): *"The MQTT client associated with the Edge Node
> MUST subscribe to a topic of the form 'spBv1.0/group_id/NCMD/edge_node_id' … It MUST subscribe on
> this topic with a QoS of 1."* Its section preamble (`:155-156`) is the ordering requirement:
> ***"Prior to sending an NBIRTH message**, the MQTT client associated with the Edge Node must
> subscribe to receive NCMD messages"*.

**AC2 — the broker's answer to the SUBSCRIBE is read, not discarded**

**Given** the SubAck arrives
**When** the driver handles it
**Then** a **refused** subscription (return code `0x80`) is traced at **ERROR**, naming the topic
**And** a **granted QoS lower than 1** is traced at **WARN**, naming the granted value
**And** neither aborts the session: the bridge keeps publishing, because publishing without command
capability is strictly better than not publishing
**And** the outcome is visible in the logs without needing broker access.

**AC3 — an unrecognised NCMD is ignored, loudly and safely**

**Given** an NCMD payload the bridge does not recognise
**When** it arrives
**Then** it is traced at **INFO** with the metric **names** it carried, and otherwise ignored
**And** a payload that fails to decode is traced at **WARN** and ignored
**And** a payload that decodes but carries **no metrics** is traced too, not silently dropped
**And** neither path panics, and neither applies a partial effect.

**AC4 — the driver still decides no truth**

**Given** the mqtt driver task
**When** an NCMD is handled
**Then** no quality or staleness decision is taken there
**And** `arch_purity.rs` still passes unchanged — the mqtt task may not name `state_machine`,
`Policy` or `State::`, and may not call `.step(`.

**AC5 — every document that says the bridge subscribes to nothing is corrected**

**Given** this story makes "the bridge issues no MQTT subscription of any kind" false
**When** it lands
**Then** each of the passages listed in *Dev Notes → The eleven sentences this story falsifies* is
amended or explicitly confirmed still-true, and the list is reported as checked
**And** the conformance matrix row for `-ncmd-subscribe` moves off `gap (unimplemented)` with its
evidence named.

*AC2 and AC5 added at story creation 2026-07-29. AC2 exists because the Story 4.4 review found the
observer discarding exactly these return codes, which made a refused subscription indistinguishable
from a quiet topic — the same byte, the same mistake, one file away. AC5 exists because this project
has now paid four times for correcting a claim while leaving the sentences describing its
consequences unexamined; here the affected sentences are known in advance, so there is no excuse.*

## Tasks / Subtasks

- [ ] **Task 1 — add `NCmd` to `MessageType` in the published crate** (AC: 1)
  - [ ] `crates/sparkplug-b/src/topic.rs`: add `NCmd` to the `MessageType` enum with token `"NCMD"`.
        It is **node-level**, so `is_device_level` returns `false` for it.
  - [ ] **Do NOT add `DCmd`.** The matrix rules `message-flow-device-dcmd-subscribe` conditional on
        *"if the Device supports writing to outputs"* (`:403-407`), which this bridge does not.
        Adding an unused variant would invite a subscription nothing needs.
  - [ ] `cargo test -p sparkplug-b --test no_context_leak` — the guard forbids `smartme`, `ignition`
        and `SMARTME_` in that crate's `src/`. Nothing here should trip it; run it anyway.
  - [ ] Extend the topic-grammar tests to cover the NCMD topic shape.

- [ ] **Task 2 — do NOT put `NCmd` in the QoS-0 publish list** (AC: 1, 4)
  - [ ] `every_edge_node_message_is_qos_zero_and_never_retained` (`mqtt_driver.rs:293-311`)
        enumerates the message types **the edge node publishes**. NCMD is **inbound**; the bridge
        never publishes one. Adding the new variant to that list mechanically would assert a rule
        about a message we do not send — read the trap in Dev Notes before touching this test.
  - [ ] Add a comment at that list saying why `NCmd` is absent, so the next person does not "fix" it.

- [ ] **Task 3 — subscribe before birthing** (AC: 1)
  - [ ] In `mqtt_driver.rs`, the `Transport::Connected` arm (`:175-189`) currently publishes the
        BIRTH first thing. Issue the SUBSCRIBE **before** the birth publish, on the same arm, so the
        ordering holds on every reconnect.
  - [ ] **Decide and record: do not block on the SubAck before birthing.** MQTT delivers packets on
        one connection in order, so a SUBSCRIBE written before the PUBLISH satisfies the clause;
        awaiting the SubAck would delay every birth on an unknown latency and could hang a session
        that is otherwise healthy. The SubAck is *checked when it arrives* (Task 4), not waited on.
        This is a decision taken at drafting time on purpose — `CLAUDE.md` forbids deferring it to an
        artifact that does not exist.
  - [ ] Handle the `Result` from `subscribe`. A failure must be traced and must **not** abort the
        session or skip the birth.

- [ ] **Task 4 — read the SubAck return codes** (AC: 2)
  - [ ] `pump_transport` (`:254-272`) currently matches `Ok(_) => {}`, which swallows SubAck.
        Forward it.
  - [ ] `rumqttc::SubAck` carries `return_codes: Vec<SubscribeReasonCode>`, whose variants are
        `Success(QoS)` and `Failure` (verified in `rumqttc-0.25.1/src/mqttbytes/v4/suback.rs:66-69`).
        Trace `Failure` at ERROR, `Success(q)` with `q != AtLeastOnce` at WARN.
  - [ ] **Read the note in Dev Notes on why this AC exists** before deciding it is ceremony.

- [ ] **Task 5 — receive and ignore** (AC: 3, 4)
  - [ ] `pump_transport`'s `Ok(_) => {}` also swallows `Packet::Publish`. Forward incoming publishes
        whose topic matches the NCMD topic.
  - [ ] **`Transport` is `#[derive(Copy)]`** (`:128`). A variant carrying a payload cannot be `Copy`;
        drop the derive or carry the payload another way. Expect this to be the first compile error.
  - [ ] **Decide the back-pressure policy before writing the send** — see the channel-stall note in
        Dev Notes. A blocking `send` on the 8-slot transport channel can stall the EventLoop and cost
        the session its keep-alive.
  - [ ] Decode with `sparkplug_b::decode` (`encode.rs:208`), which returns
        `Result<Payload, prost::DecodeError>`. **Never `.expect()` it** — a malformed payload from
        the network is an expected input, not a bug.
  - [ ] Recognised commands: **none yet.** Every NCMD is unrecognised in this story. Trace the metric
        names at INFO and drop it. `Node Control/Rebirth` is Story 4.7 and must not be implemented
        here.
  - [ ] Trace the metric **names**, not the payload — a name list is diagnostic, a full payload dump
        is noise and may carry values.

- [ ] **Task 6 — tests, each falsified before it is trusted** (AC: 1, 2, 3, 4)
  - [ ] The subscription ordering is the property worth proving: **the SUBSCRIBE reaches the broker
        before the NBIRTH.** The chaos harness already starts a real Mosquitto via testcontainers
        (`chaos_*` tests) — that is the shape to reuse; a unit test cannot observe packet order.
  - [ ] **Falsify each new assertion against deliberately broken code and record the falsification
        next to the test** — `CLAUDE.md`. For the ordering test, moving the subscribe *after* the
        birth must turn it red. If it stays green it is not a test.
  - [ ] A malformed-payload test: feed bytes that fail `decode` and assert the task survives and
        traces. Falsify by replacing the graceful path with `.expect()`.
  - [ ] `cargo test -p smartme-bridge --test arch_purity` must pass **unchanged** — do not edit the
        guard to accommodate this story.

- [ ] **Task 7 — amend every document this story falsifies** (AC: 5)
  - [ ] Work the list in *The eleven sentences this story falsifies* below, file by file, and report
        each as amended or as confirmed still-true with the reason.
  - [ ] `docs/sparkplug-conformance.md:353` — the `-ncmd-subscribe` row. The document calls it *"the
        most consequential row in this chapter"* (`:412`). Move it off `gap (unimplemented)` and name
        the evidence. **Leave `-rebirth-action-1/2/3` as gaps** — they are Story 4.7.
  - [ ] `docs/manual/` — chapter 5's *Known limitations* bullet and chapter 2's capability table both
        state the absence flatly. The manual documents implemented behaviour, so both change.
        `latexmk` must exit 0.
  - [ ] `docs/primary-host-state-observation.md` and `docs/adr/0016-…md` use "no subscription" as
        **evidence in an argument**. Amend the sentence *and* re-check that the argument still holds
        — this is the exact step the project keeps skipping.
  - [ ] `./scripts/ci-local.sh` (not `--fast`, and never piped — read the `EXIT=` line out of a log
        at an **absolute** path).

## Dev Notes

### What this story is, and is not

It builds **plumbing**. It receives NCMD and throws every one away, on purpose. `Node
Control/Rebirth` is **Story 4.7** and implementing it here would merge two stories whose evidence
differs — ADR 0016 sequenced them deliberately.

The value of a story that ignores everything is that the *ignoring* is the dangerous part. A
subscription that silently fails, a decode that panics, or a command half-applied are all worse than
no subscription at all, and none of them are visible without the diagnostics this story adds.

### ⚠️ The tck-id in `epics.md` is wrong, and the ordering is stronger than it says

The epic's AC1 cites `tck-id-...-edge-node-subscribe-ncmd`. **That id does not exist.** The real one
reverses the last two words: **`tck-id-message-flow-edge-node-ncmd-subscribe`**
(`Sparkplug_5_Operational_Behavior.adoc:158-163`). Cite it in full, per `CLAUDE.md`.

The epic also says the subscription happens *"as part of the same post-CONNACK sequence that
publishes NBIRTH"*. The specification says something stricter — the section preamble at `:155-156`:

> *"**Prior to sending an NBIRTH message**, the MQTT client associated with the Edge Node must
> subscribe to receive NCMD messages with the following rules."*

"In the same sequence" permits birth-then-subscribe. The norm does not. AC1 is written to the norm.
There is also a plain reason: a host that receives an NBIRTH may immediately send a command, and a
node that has not yet subscribed will never see it.

### Why AC2 exists — one file away, this exact byte was thrown away yesterday

Story 4.4's observer matched `Packet::SubAck(_)` and discarded `return_codes`. A broker that
**refuses** a subscription answers with return code `0x80` — not an error, not a disconnect. The
observer reported "ready", waited its whole window, received nothing, and printed a diagnostic asking
the operator to rule out a broker ACL **by hand** — the very question the discarded byte had already
answered. The Story 4.4 review called it the instrument's sharpest false-negative surface.

The bridge is about to make the same subscription against the same broker. Do not repeat it.

Note also that a granted QoS is not necessarily the requested one. A downgrade to 0 is silent, and it
matters here because the clause is a MUST on QoS 1: if the broker grants 0, the bridge is not
conformant and only the SubAck says so.

### The QoS-1 subscription does NOT contradict the QoS-0 publish rule

Expect this to be questioned in review; the answer belongs in a comment.

`qos_for` returns `(AtMostOnce, false)` for every message and
`every_edge_node_message_is_qos_zero_and_never_retained` pins it. That rule is about **publishing**.
The NCMD subscription is a **subscribe** QoS, a different field in a different packet travelling the
other way. `tck-id-message-flow-edge-node-ncmd-subscribe` mandates 1 for it. No conflict.

(Unrelated but nearby: the *will* is registered at QoS 0 where the norm requires 1 — that is a known
deviation, [#26](https://github.com/guycorbaz/smartme_mqtt/issues/26), owned by **Story 4.17**. Do
not fix it here.)

### ⚠️ Routing NCMD through `transport_rx` can stall the whole connection

`transport_rx` is `mpsc::channel(8)` (`mqtt_driver.rs:168`) and `pump_transport` delivers with
`events.send(...).await`. That `await` **blocks the EventLoop task** when the channel is full — and
the EventLoop is what answers PINGREQ. A main loop busy on a slow poll, plus a burst of inbound
NCMDs, and the bridge stops servicing keep-alive and gets disconnected by the broker. Today the
channel only ever carries two rare events, so the bound is invisible; NCMD makes it a live path.

**This project already has the answer, one function away.** `publish()` (`:275-286`) documents it:

> *"A full queue is a traced drop, never a block: a blocked driver stops draining the inbox, and then
> NOTHING is published."*

Apply the same rule. Either give NCMD its own channel or use `try_send` with a traced drop — and say
in a comment which you chose and why. **Do not** add an unbounded channel: that trades a stall for
unbounded memory, and AC-LEAK-01 (Story 4.15) exists because this project cares about that.

### The two `MessageType` call sites behave differently — one will tell you, one will not

- `token()` (`topic.rs:74-83`) is an **exhaustive `match`**. Adding `NCmd` fails to compile until you
  add the arm. The compiler protects you.
- `is_device_level()` (`:87-93`) uses `matches!(self, DBirth | DData | DDeath)`. Adding a variant
  **compiles silently** and `NCmd` returns `false`. That happens to be the right answer — NCMD is
  node-level — but it is right by accident, not by review. Confirm it deliberately and add `NCmd` to
  that function's test coverage so the next variant is not decided by a fall-through.

Everything else that names `MessageType` (12 sites in `sparkplug_publisher.rs`, 7 in
`mqtt_driver.rs`, 1 in `supervisor.rs`) constructs values rather than matching on them, so nothing
else breaks.

### The trap in Task 2, stated plainly

`every_edge_node_message_is_qos_zero_and_never_retained` lists variants literally. Adding `NCmd` to
`MessageType` will **not** break it — and that is the danger. The obvious tidy-up is to add the new
variant to the list "for completeness". Don't: it would assert a publish rule for a message the
bridge never publishes, and the assertion would pass, which makes it exactly the kind of test this
project has already been bitten by four times in Epic 1 — green for a reason unrelated to the
property.

### Existing code you must read before writing anything

- `crates/smartme-bridge/src/app/mqtt_driver.rs` — the whole file. Specifically:
  - `:1-38` module docs: the boot order is *"not negotiable"*. This story inserts a step into it, so
    **the module docs must be updated too**, or the next reader trusts a stale sequence.
  - `:171-198` the `Transport::Connected` arm — where the subscribe goes, before the birth.
  - `:128-134` `enum Transport`, which is `Copy`. See Task 5.
  - `:254-272` `pump_transport` — `Ok(_) => {}` is what currently swallows both SubAck and Publish.
  - `:123-125` `qos_for`, and `:293-311` the test that pins it.
- `crates/smartme-bridge/tests/arch_purity.rs:90-91` — `NAMING_BANNED_IN_MQTT` is
  `["state_machine", "Policy", "State::"]` and `.step(` is banned outside `poll_publish.rs`. A
  handler that logs `State::Something` fails the build. This guard is **not** to be edited.
- `crates/sparkplug-b/src/topic.rs:55-95` — `MessageType`, `token()`, `is_device_level()`.
- `crates/sparkplug-b/src/encode.rs:208` — `decode`.
- `crates/smartme-bridge/tests/observe_primary_host_state.rs` — the corrected SubAck handling and the
  module docs section *"Every way this instrument can lie"*. Same broker, same packet, same lesson.
- `crates/smartme-bridge/tests/chaos_sigterm_no_lie.rs` — the testcontainers pattern for a real
  broker.

### The eleven sentences this story falsifies (AC5)

Produced by `grep -rn "no MQTT subscription\|no subscription\|not subscribed"` over `docs/` and
`_bmad-output/planning-artifacts/`, excluding the vendored spec. **Every one of these is true today
and false once this story lands.** Two of them are load-bearing in an argument, not just
descriptive — amend the claim *and* re-check what it was holding up.

| File | Line | What it says |
| --- | ---: | --- |
| `docs/primary-host-state-observation.md` | 293 | *"it holds **no MQTT subscription of any kind**"* — opens the AC2 cost argument |
| `docs/primary-host-state-observation.md` | 314 | *"The bridge sees nothing — it is not subscribed. **Measured**"* — chain item 3 |
| `docs/primary-host-state-observation.md` | 365 | *"the bridge holds no subscription (step 3, first half)"* — the measured/inferred boundary |
| `docs/primary-host-state-observation.md` | 414 | `-state-subs` ruling: *"the bridge issues no subscription at all"* |
| `docs/manual/chapters/05-mqtt-sparkplug-contract.tex` | 250 | *"\prog issues no MQTT subscription of any kind"* |
| `docs/manual/chapters/02-understanding-sparkplug.tex` | 680 | capability table: `Node Control/Rebirth` **absent** |
| `docs/sparkplug-conformance.md` | 353 | the `-ncmd-subscribe` row itself |
| `docs/sparkplug-conformance.md` | 1200 | *"NCMD/DCMD not implemented — no subscription"* |
| `docs/sparkplug-conformance.md` | 1214 | Primary Host absent *"end to end — no subscription"* |
| `docs/adr/0016-rebirth-before-primary-host-wait.md` | 59 | *"The bridge holds **no MQTT subscription of any kind**"* — **evidence in a decision** |
| `_bmad-output/planning-artifacts/epics.md` | Story 4.6 | the elided tck-id, above |

**Why this table is in the story rather than left to be discovered.** Four times now this project has
corrected a claim and left the sentences describing its *consequences* untouched — FR20's QoS-0
over-claim ([#33](https://github.com/guycorbaz/smartme_mqtt/issues/33)), the RBE passages, the Primary
Host "invisible" bullet, and then the *correction to that bullet*, which asserted the bridge never
re-births when it re-births on every reconnect. The pattern is not carelessness about corrections; it
is that consequence-sentences get checked against the intent of the fix and never against the code.
Here the list is mechanical. Work it.

Note the ADR line especially: ADR 0016's argument for sequencing 4.7 before 4.5 rests partly on the
bridge being unable to *receive* a Rebirth. After this story it can receive one and still not answer
it — which **strengthens** the case for 4.7 rather than weakening it. Say so in the amendment rather
than deleting the sentence.

### Deployment facts that constrain this story

- **The broker is production and Ignition is live on it.** This story publishes nothing new to it,
  but it does open a subscription. Use the testcontainers broker for tests; never aim a chaos test at
  the LAN broker unasked.
- **A live MQTT Engine v5.0.0-rc1 is on that broker** and it is a Host Application that *sends*
  Rebirth requests. Once this subscription exists, real NCMDs may start arriving in production. That
  is precisely why AC3's ignore path must be safe before Story 4.7 gives it meaning.
- **The bridge is not in production yet** and no tag historisation has begun, so a wire-affecting
  change is still cheap. Adding a subscription is not wire-breaking in any case.

### Testing standards

- Unit tests inline under `#[cfg(test)]`; integration tests in `crates/smartme-bridge/tests/`.
- No raw time: `Instant::now()` / `SystemTime::now()` are confined to `core/clock.rs` and
  `arch_purity` enforces it, inline test modules included.
- **Falsification is mandatory and must be recorded next to the test.** Four Epic 1 tests passed for
  the wrong reason — a fake clock that never advanced, a `bdSeq` comparison of a constant against
  itself, a drain where nothing could fail, and a discriminator spanning two clocks.
- `./scripts/ci-local.sh` before pushing; then `gh run list`. Never pipe it, and do not write its log
  to `$TMPDIR` from a sandbox-disabled command — `TMPDIR` is empty there and the redirect fails,
  which looks like a build failure and means the run never started.

### Project Structure Notes

- The `MessageType` change is the only edit to the **published** `sparkplug-b` crate. Keep it free of
  bridge context; `no_context_leak` enforces it.
- The NCMD handler belongs in `app/mqtt_driver.rs`, not in `adapters/` and not in `core/`. It
  transports and traces; it decides nothing. If it starts needing a verdict, that is the signal the
  story has drifted into 4.7.
- The manual (`docs/manual/`) documents implemented behaviour, so it changes **in this story**, not
  later — the standing order in this repo.

### References

- [Source: `docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_5_Operational_Behavior.adoc:155-163`] —
  the subscribe clause and its ordering preamble
- [Source: `docs/spec/…/Sparkplug_5_Operational_Behavior.adoc:403-407`] — `-device-dcmd-subscribe`,
  the conditional clause deliberately not implemented
- [Source: `docs/spec/…/Sparkplug_5_Operational_Behavior.adoc:970-986`] — the Rebirth verb/name/value
  and `-rebirth-action-1/2/3` clauses, all **Story 4.7**
- [Source: `_bmad-output/planning-artifacts/epics.md#Story 4.6`] — the three original ACs
- [Source: `docs/adr/0016-rebirth-before-primary-host-wait.md`] — why 4.6/4.7 precede 4.5
- [Source: `docs/sparkplug-conformance.md:353, :412, :455, :1200`] — the `-ncmd-subscribe` row, why it
  is called the chapter's most consequential, and the DCMD asymmetry
- [Source: `_bmad-output/implementation-artifacts/4-4-primary-host-state-measure.md#Review Findings`]
  — the discarded-SubAck finding that AC2 generalises
- [Source: `CLAUDE.md`] — read the norm first and cite the tck-id; falsify before trusting; decide at
  drafting time; amend the PRD/epics/manual together

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-07-29.

### Debug Log References

### Completion Notes List

### File List
