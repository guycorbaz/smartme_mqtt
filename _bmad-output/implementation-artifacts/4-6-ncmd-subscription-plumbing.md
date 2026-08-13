# Story 4.6: NCMD subscription — plumbing that ignores safely

Status: done

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

- [x] **Task 1 — add `NCmd` to `MessageType` in the published crate** (AC: 1)
  - [x] `crates/sparkplug-b/src/topic.rs`: add `NCmd` to the `MessageType` enum with token `"NCMD"`.
        It is **node-level**, so `is_device_level` returns `false` for it.
  - [x] **Do NOT add `DCmd`.** The matrix rules `message-flow-device-dcmd-subscribe` conditional on
        *"if the Device supports writing to outputs"* (`:403-407`), which this bridge does not.
        Adding an unused variant would invite a subscription nothing needs.
  - [x] `cargo test -p sparkplug-b --test no_context_leak` — the guard forbids `smartme`, `ignition`
        and `SMARTME_` in that crate's `src/`. Nothing here should trip it; run it anyway.
  - [x] Extend the topic-grammar tests to cover the NCMD topic shape.

- [x] **Task 2 — do NOT put `NCmd` in the QoS-0 publish list** (AC: 1, 4)
  - [x] `every_edge_node_message_is_qos_zero_and_never_retained` (`mqtt_driver.rs:293-311`)
        enumerates the message types **the edge node publishes**. NCMD is **inbound**; the bridge
        never publishes one. Adding the new variant to that list mechanically would assert a rule
        about a message we do not send — read the trap in Dev Notes before touching this test.
  - [x] Add a comment at that list saying why `NCmd` is absent, so the next person does not "fix" it.

- [x] **Task 3 — subscribe before birthing** (AC: 1)
  - [x] In `mqtt_driver.rs`, the `Transport::Connected` arm (`:175-189`) currently publishes the
        BIRTH first thing. Issue the SUBSCRIBE **before** the birth publish, on the same arm, so the
        ordering holds on every reconnect.
  - [x] **Decide and record: do not block on the SubAck before birthing.** MQTT delivers packets on
        one connection in order, so a SUBSCRIBE written before the PUBLISH satisfies the clause;
        awaiting the SubAck would delay every birth on an unknown latency and could hang a session
        that is otherwise healthy. The SubAck is *checked when it arrives* (Task 4), not waited on.
        This is a decision taken at drafting time on purpose — `CLAUDE.md` forbids deferring it to an
        artifact that does not exist.
  - [x] Handle the `Result` from `subscribe`. A failure must be traced and must **not** abort the
        session or skip the birth.

- [x] **Task 4 — read the SubAck return codes** (AC: 2)
  - [x] `pump_transport` (`:254-272`) currently matches `Ok(_) => {}`, which swallows SubAck.
        Forward it.
  - [x] `rumqttc::SubAck` carries `return_codes: Vec<SubscribeReasonCode>`, whose variants are
        `Success(QoS)` and `Failure` (verified in `rumqttc-0.25.1/src/mqttbytes/v4/suback.rs:66-69`).
        Trace `Failure` at ERROR, `Success(q)` with `q != AtLeastOnce` at WARN.
  - [x] **Read the note in Dev Notes on why this AC exists** before deciding it is ceremony.

- [x] **Task 5 — receive and ignore** (AC: 3, 4)
  - [x] `pump_transport`'s `Ok(_) => {}` also swallows `Packet::Publish`. Forward incoming publishes
        whose topic matches the NCMD topic.
  - [x] **`Transport` is `#[derive(Copy)]`** (`:128`). A variant carrying a payload cannot be `Copy`;
        drop the derive or carry the payload another way. Expect this to be the first compile error.
  - [x] **Decide the back-pressure policy before writing the send** — see the channel-stall note in
        Dev Notes. A blocking `send` on the 8-slot transport channel can stall the EventLoop and cost
        the session its keep-alive.
  - [x] Decode with `sparkplug_b::decode` (`encode.rs:208`), which returns
        `Result<Payload, prost::DecodeError>`. **Never `.expect()` it** — a malformed payload from
        the network is an expected input, not a bug.
  - [x] Recognised commands: **none yet.** Every NCMD is unrecognised in this story. Trace the metric
        names at INFO and drop it. `Node Control/Rebirth` is Story 4.7 and must not be implemented
        here.
  - [x] Trace the metric **names**, not the payload — a name list is diagnostic, a full payload dump
        is noise and may carry values.

- [x] **Task 6 — tests, each falsified before it is trusted** (AC: 1, 2, 3, 4)
  - [x] The subscription ordering is the property worth proving: **the SUBSCRIBE reaches the broker
        before the NBIRTH.** The chaos harness already starts a real Mosquitto via testcontainers
        (`chaos_*` tests) — that is the shape to reuse; a unit test cannot observe packet order.
  - [x] **Falsify each new assertion against deliberately broken code and record the falsification
        next to the test** — `CLAUDE.md`. For the ordering test, moving the subscribe *after* the
        birth must turn it red. If it stays green it is not a test.
  - [x] A malformed-payload test: feed bytes that fail `decode` and assert the task survives and
        traces. Falsify by replacing the graceful path with `.expect()`.
  - [x] `cargo test -p smartme-bridge --test arch_purity` must pass **unchanged** — do not edit the
        guard to accommodate this story.

- [x] **Task 7 — amend every document this story falsifies** (AC: 5)
  - [x] Work the list in *The eleven sentences this story falsifies* below, file by file, and report
        each as amended or as confirmed still-true with the reason.
  - [x] `docs/sparkplug-conformance.md:353` — the `-ncmd-subscribe` row. The document calls it *"the
        most consequential row in this chapter"* (`:412`). Move it off `gap (unimplemented)` and name
        the evidence. **Leave `-rebirth-action-1/2/3` as gaps** — they are Story 4.7.
  - [x] `docs/manual/` — chapter 5's *Known limitations* bullet and chapter 2's capability table both
        state the absence flatly. The manual documents implemented behaviour, so both change.
        `latexmk` must exit 0.
  - [x] `docs/primary-host-state-observation.md` and `docs/adr/0016-…md` use "no subscription" as
        **evidence in an argument**. Amend the sentence *and* re-check that the argument still holds
        — this is the exact step the project keeps skipping.
  - [x] `./scripts/ci-local.sh` (not `--fast`, and never piped — read the `EXIT=` line out of a log
        at an **absolute** path).

### Review Findings

Code review 2026-07-29 — three adversarial layers (Blind Hunter, Edge Case Hunter, Acceptance
Auditor), each in a fresh context, all read-only (verified: 97 file hashes identical before and
after). 27 findings survived triage; 3 were dismissed, and one of those dismissals matters — see the
bottom of this section.

**Decisions needed (2) — both resolved by Guy on 2026-07-29**

- **D1 — the default trace filter is `ERROR`, so AC3 is entirely invisible and AC2 half-invisible in a real deployment.** `main.rs:21` is `tracing_subscriber::fmt::init()`; with the `env-filter` feature on (workspace `Cargo.toml:44`) that resolves to `EnvFilter::from_default_env()`, whose default directive is `LevelFilter::ERROR` (`tracing-subscriber-0.3.23/src/filter/env/mod.rs:289-293`). Without `RUST_LOG`, the WARN for a granted QoS below 1 (`mqtt_driver.rs:349`) and **all three** AC3 traces — unrecognised (INFO), undecodable (WARN), no-metrics (INFO) — never reach the operator. AC3's word is *loudly*; AC2 says *"visible in the logs without needing broker access"*. Only the refusal ERROR survives the default filter. The chaos test passes because it sets `RUST_LOG=info` itself, with a comment explaining why — the product does not. → **Resolved: default directive INFO, `RUST_LOG` still honoured. Now a patch below.**
- **D2 — no inbound packet-size limit, and this story is what makes that reachable.** `rumqttc` defaults `max_incoming_packet_size` to 10 KiB (`rumqttc-0.25.1/src/lib.rs:516`) and rejects a larger frame in `mqttbytes/mod.rs:181-183` — *before* the topic guard at `mqtt_driver.rs:508` is evaluated. The session drops ungracefully, the broker fires the will, and the host is told the node died while it was alive. Before this story no PUBLISH could reach the socket at all, so the subscription creates the path. → **Resolved: recorded, not patched. Now a deferred item below, with the reasoning.**

**Patches (18)**

- [x] [Review][Patch] **The default trace filter is `ERROR`, so AC3 is invisible and AC2 half-invisible without `RUST_LOG`** — set an explicit default directive of INFO while still honouring `RUST_LOG` [`crates/smartme-bridge/src/main.rs:21`]. Note this also makes the transport-error WARN visible, which is what carries the D2 diagnosis.
- [x] [Review][Patch] `-rebirth-action-1`'s evidence cell says *"nothing receives a Rebirth Request"* — now false — and prose **added by this story** certifies it as correct [`docs/sparkplug-conformance.md:507`, `:435-437`]
- [x] [Review][Patch] *"only the NCMD subscription and handler are missing"* is false, in the live *Findings carried forward* table, two rows below one that **was** amended [`docs/sparkplug-conformance.md:1245`]
- [x] [Review][Patch] *"either one alone is sufficient"* is a false entailment: the unretained NBIRTH alone does **not** imply "no host-initiated repair" — a Rebirth handler would repair precisely because births are unretained. Only the missing handler is load-bearing, which is what the justification beneath it actually argues [`docs/primary-host-state-observation.md:406-412`; repeated at this file's Completion Notes and `sprint-status.yaml:227`. **ADR 0016's own wording is correct** — it says the conclusion *"needs both halves"* — so amend the two copies, not the ADR]
- [x] [Review][Patch] AC1's third clause — *"re-established on every reconnect, not only the first"* — is asserted by **no test**; the chaos test observes one connect. The behaviour was verified out-of-band by an independent broker restart and it holds, but a refactor hoisting the subscribe out of the `Transport::Connected` arm would leave the whole suite green [`crates/smartme-bridge/tests/chaos_ncmd_subscription.rs`]
- [x] [Review][Patch] `granted()` reads only `return_codes[0]`, never compares `ack.pkid`, and the trace stamps the NCMD topic on unconditionally — so any SubAck is attributed to it. Latent until Story 4.5 adds the STATE subscription, at which point a refused STATE subscription is reported as a refused **NCMD** one [`crates/smartme-bridge/src/app/mqtt_driver.rs:205-212`, `:343`]
- [x] [Review][Patch] The chaos test's AC3 assertion greps for `"Node Control/Rebirth"` — the metric **name** — not for the ignore trace. A Story 4.7 handler logging the same name keeps it green while asserting the opposite of its own failure message. The file's *"ways this could pass wrongly"* list claims immunity here and is wrong about this one assertion [`chaos_ncmd_subscription.rs:307-311`]
- [x] [Review][Patch] The four SubAck **trace arms** are unfalsified. Mutation 3 collapsed `granted()`, which the unit test calls directly; swapping the *bodies* of the `Refused` and `AsRequired` arms — so a refusal logs *"granted at QoS 1"* — leaves both the unit test and the chaos test green. AC2's actual observable has no test [`mqtt_driver.rs:348-376`]
- [x] [Review][Patch] The chaos test's liveness assertion cannot detect a dead driver task: `supervisor.rs:139` only awaits the mqtt task **after** `shutdown`, so a panicked driver leaves `try_wait().is_none()` true and `rebirth.is_none()` trivially true — two assertions satisfied by the failure they exist to exclude [`chaos_ncmd_subscription.rs:382-391`]
- [x] [Review][Patch] The boot order gained a sixth step in the code and the manual but not in the canonical architectural statements, which still read `bdSeq → NDEATH → LWT → connect → NBIRTH` [`_bmad-output/planning-artifacts/epics.md:140` (AR10), `_bmad-output/planning-artifacts/architecture.md:85`]
- [x] [Review][Patch] `classify`'s `<unnamed>` fallback is reachable from a conformant host — Sparkplug permits a metric carried by `alias` with no `name` — and is untested; both `command_payload` helpers always set a name. A host sending `Node Control/Rebirth` by alias would produce `names=["<unnamed>"]` and Story 4.7's name-matching handler would silently never fire [`mqtt_driver.rs:243-249`]
- [x] [Review][Patch] *"the Rebirth path needs an NCMD subscription that does not exist (Story 4.6)"* — present tense, now false, in *Alternatives considered*; line 59 of the same file **was** amended [`docs/adr/0016-rebirth-before-primary-host-wait.md:125`]
- [x] [Review][Patch] The `-state-subs` verdict cell names **Story 4.6** as an owner of a still-unmet gap, while the prose in the same cell says 4.6 added no STATE handling and the other ten clauses of the family read *"Stories 4.4–4.5"* [`docs/sparkplug-conformance.md:697` vs `:367-374`]
- [x] [Review][Patch] A `try_subscribe` failure births anyway — correct, and argued in the function's docs — but the `conformant` row and the manual's new section both state the subscription happens before every birth with no caveat for that path; the manual's only caveat covers the *broker's* refusal, which is a different path [`docs/sparkplug-conformance.md:353`, `docs/manual/chapters/05-mqtt-sparkplug-contract.tex:239-263`, `mqtt_driver.rs:554-566`]
- [x] [Review][Patch] The module docs state *"`rumqttc` connects with a clean session"* as settled fact, while this same commit's matrix records `principles-persistence-clean-session-311` as **gap (unproven)** — *"`set_clean_session` is never called and no test asserts the flag"*. Benign in effect (re-subscribing is right either way), but it converts an open gap into a premise [`mqtt_driver.rs` module docs; `docs/sparkplug-conformance.md`, issue #35]
- [x] [Review][Patch] *"There is no command path to lose: the bridge accepts no NCMD/DCMD"* — defensible only if *accepts* means *acts on*; it now receives them [`docs/primary-host-state-observation.md:304-305`]
- [x] [Review][Patch] The Completion Notes' summary says the AC5 list was *"one short"*; the section it points at says eleven became thirteen, and the Change Log says *"two short"*. 11 → 13 is two [this file]
- [x] [Review][Patch] Task 7's obligation — *"report each as amended or as confirmed still-true with the reason"* — was narrated, not itemised. No per-passage table exists. Had it existed, the two surviving false sentences above would very likely have surfaced: the itemisation is the mechanism, not the ceremony [this file]

**Deferred (6)**

- [x] [Review][Defer] **An inbound packet above 10 KiB tears down the session and fires the will** [`mqtt_driver.rs:293` (no `set_max_packet_size`), reaching `:508`] — deferred by decision, 2026-07-29. **Reason: the vector is strictly weaker than a capability the unauthenticated broker already offers, and the correct control is broker-side, not bridge-side.** Four grounds, recorded so this is not re-litigated from scratch: (a) it is a *disruption*, not a lie — the session genuinely dies, the death certificate is genuinely correct, and the bridge re-births ~1 s later; (b) no legitimate NCMD for this bridge approaches 10 KiB (a `Node Control/Rebirth` is tens of bytes and this bridge has no writable outputs), so the default is correctly sized and raising it only moves the cliff at the cost of the bounded memory AC-LEAK-01 protects; (c) the bridge *cannot* drop the packet and keep the session — that is rumqttc's deliberate behaviour (*"Don't let rogue connections attack with huge payloads"*), so no code-level fix changes the outcome; (d) **on a broker with no authentication, any client can already publish a forged NDEATH on `spBv1.0/<group>/NDEATH/<node>`** — which lies immediately and needs no provocation. The proper mitigations are Mosquitto's `message_size_limit` (rejects the oversized PUBLISH before it reaches any subscriber, and protects Ignition too) and broker ACLs, both deployment decisions belonging to Epics 5/7. **Not measured:** a sustained attack would produce death/birth churn at roughly 1 Hz on the host; whether that is tolerable is unobserved and belongs to Story 4.13 (chaos broker recovery), which can watch it rather than reason about it.
- [x] [Review][Defer] A SubAck that never arrives is indistinguishable from a granted subscription — no deadline, no pending-subscribe state, no absence check; the log is silent, which is also what a healthy grant looks like below INFO [`mqtt_driver.rs:342-377`] — deferred, needs its own mechanism and interacts with the deliberate do-not-block-on-SubAck decision
- [x] [Review][Defer] `Transport::Subscribed` is delivered by a blocking `send().await` on `pump_transport`, the task the same file says must never block — the rule is applied to the command channel and not to the transport channel four lines above [`mqtt_driver.rs:498-506` vs `:511-515`] — deferred, pre-existing shape shared with `Connected`/`Lost`; this adds one more instance
- [x] [Review][Defer] The fixed two-second settle guards a **diagnostic** property — that a late SUBSCRIBE is reported as late rather than absent — and was validated by one re-run. Under a loaded CI (`jobs=2` plus testcontainers) an ordering regression would panic with *"the broker never received a SUBSCRIBE"*, sending the reader to the harness rather than the bug. No false green is possible [`chaos_ncmd_subscription.rs`] — deferred, diagnostic quality not correctness
- [x] [Review][Defer] An inbound publish whose topic does not match `ncmd_topic` is discarded with no trace, in a driver where every other drop is traced [`mqtt_driver.rs:508-510`, `:521`] — deferred, unreachable until a second subscription exists
- [x] [Review][Defer] A failed subscribe is never retried for the life of the session [`mqtt_driver.rs:554-566`] — deferred, pairs with the decision item above

**Dismissed (3), and one of them is worth recording**

- *"At least two `mqtt_driver.rs` line citations are arithmetically impossible."* **Verified false.** `:50-61` is the complete 12-line *"Session identity, and a recorded deviation"* doc block and `:293-301` is exactly the `MqttOptions` + `set_last_will` construction. The finding was derived from hunk offsets without reading the file, and the doc block had itself grown in this diff, so a uniform shift did not apply. **The citations are correct** — that claim in the Completion Notes stands, and was independently spot-checked at 16 and 10 sites by the other two layers.
- *"~200 lines inserted understates the change (+414 net)."* True but harmless — a narrative approximation in a dated record.
- *"`-ncmd-subscribe` is scored `conformant` while a MUST in the same block is unmet."* The spec was read: the bullet *"This subscription is mandatory as Edge Nodes MUST be able to respond to 'rebirth requests'"* sits **outside** the `[tck-id-…]` markup, which closes at `*#` on the preceding line (`Sparkplug_5_Operational_Behavior.adoc:158-164`). Excluding it is exactly `CLAUDE.md`'s *cite the tck-id, not prose*, the row carries the *"for the subscription only"* qualification, and the unmet half is scored as gaps at `-rebirth-action-1/2/3`. The document already answers the objection.

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
`_bmad-output/planning-artifacts/`, excluding the pinned spec. **Every one of these is true today
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

Falsification runs only; no debugging session was needed. Every mutation below was applied to
production code, run, and reverted — the reverts are verified by `git diff` and by the suite
returning green after each.

| # | Mutation | Test that went RED |
| ---: | --- | --- |
| 1 | `MessageType::NCmd` token spelled `"NCOMMAND"` | `the_ncmd_topic_follows_the_namespace_grammar` |
| 2 | `NCmd` added to `is_device_level`'s `matches!` | both new `topic.rs` tests |
| 3 | `granted()` collapsed to always return `AsRequired` | `the_brokers_answer_to_the_subscription_is_read_not_assumed` (3 of 4 assertions) |
| 4 | `classify`'s `Err` arm replaced with a panic | `a_command_that_does_not_decode_is_classified_never_unwrapped` |
| 5 | `classify`'s `metrics.is_empty()` arm deleted | `a_command_carrying_no_metric_is_not_silently_dropped` |
| 6 | every decoded payload classified `NoMetrics` | `a_recognisable_looking_command_is_still_unrecognised_in_this_story` |
| 7 | `subscribe_to_commands` moved **after** the birth publish | `chaos_ncmd_subscription` — ordering assertion |
| 8 | `Packet::SubAck` arm deleted (back to `Ok(_) => {}`) | `chaos_ncmd_subscription` — AC2 assertion |
| 9 | inbound-publish guard forced to `false` | `chaos_ncmd_subscription` — AC3 assertions |

### Completion Notes List

**All five ACs met.** Nine mutations run; nine went red. The two findings worth reading are the
first one below (a test that failed for the wrong reason) and the last (the AC5 list was **two**
short — eleven became thirteen; the summary line said "one" until the code review added it up).

*Read the `### Review Findings` section above before this one. The review found that AC2 and AC3 were
both dark in a default deployment — the traces they are written in terms of sat below the default log
filter — and that two of the sentences this story set out to correct survived the sweep anyway, one of
them blessed by prose this story added. Both are fixed; the claims below are otherwise upheld.*

**AC1 — subscribe before birth, QoS 1, every reconnect.** `subscribe_to_commands` is called in the
`Transport::Connected` arm, which fires on every `ConnAck`, immediately before the birth publish.
`try_subscribe` and `try_publish` feed the same request channel, so FIFO ordering into the socket is
what makes *"prior to sending an NBIRTH"* true — the driver does **not** wait for the SubAck, decided
at drafting time and recorded in the module docs. Proof is taken from the **broker's** verbose log:
one MQTT client cannot observe another's SUBSCRIBE, so `mosquitto -v` is the only external oracle
available. A `start_verbose_broker` helper was added rather than switching `-v` on globally.

> **Mutation 7 initially went red for the wrong reason, and that is the most useful thing this story
> found.** With the subscribe moved after the birth, the test reported *"the broker never received a
> SUBSCRIBE"* — because the broker log was read the instant the NBIRTH arrived, before the late
> SUBSCRIBE had landed in it. A genuinely late subscription was therefore indistinguishable from an
> absent one, and the message would have sent the next reader hunting the wrong bug. A two-second
> settle was added *because of the falsification run*, and mutation 7 was re-run to confirm the
> message now names the real cause. A test that fails is not automatically a test that tells you what
> broke — `CLAUDE.md` requires the falsification, and this is what it bought.

**AC2 — the SubAck is read.** `pump_transport` forwards `Packet::SubAck`'s `return_codes`;
`granted()` distils them into four outcomes traced at ERROR (refused, or an empty answer), WARN (a
QoS other than 1) or INFO (granted). None aborts the session. The two bad branches are unit-tested;
the happy branch is observed end-to-end in the chaos test, so the mechanism is proven to run and not
merely to be correct in isolation.

**AC3 — ignored, loudly and safely.** Three shapes, three distinct traces: unrecognised (metric
**names** only), undecodable, and decoded-but-empty. `classify` never unwraps a decode — a malformed
payload from the network is an ordinary input, and eleven bytes from any client would otherwise stop
the bridge. The load-bearing assertion is not a log line: the chaos test publishes a real
`Node Control/Rebirth` and asserts **no second NBIRTH follows**. A bridge that answered it would have
implemented Story 4.7 by accident, with none of the evidence that story owes.

**AC4 — the driver still decides no truth.** `arch_purity` passes **unchanged** — verified by
`git diff --quiet` on the guard, not by reading it. `classify` and `granted` classify bytes for a log
line; neither is a quality or staleness verdict.

**Back-pressure, decided and recorded.** Inbound commands get their **own** channel with `try_send`
and a traced drop, never `send().await`. `pump_transport` is the task that answers PINGREQ: blocking
it on a full queue would cost the session its keep-alive and the broker would disconnect a bridge
that was otherwise healthy. This is the rule `publish()` already documents, applied to the first
externally-driven path into the driver. An unbounded channel was rejected — it trades a stall for
unbounded memory, which AC-LEAK-01 (Story 4.15) exists to prevent.

**`Transport` lost its `Copy` derive**, as the story predicted, because the SubAck carries a `Vec`.

**Two conformance verdicts moved, in two chapters.** `-ncmd-subscribe` (ch. 5) and
`topics-ncmd-topic` (ch. 4) both went from `gap (unimplemented)` to `conformant`. Chapter 5's tally
moved `22·2·26·49 → 23·2·25·49`, chapter 4's `14·0·6·21 → 15·0·5·21`, and the whole-specification
total `70·8·52·144 → 72·8·50·144`. **This is the first story to move a verdict in that matrix rather
than record one**; 4.1–4.4 only ever described the code.

**`DCmd` was deliberately not added.** `tck-id-message-flow-device-dcmd-subscribe` (`:403-407`) is
conditional on *"if the Device supports writing to outputs"* and no device here does. Consequence
followed through: `topics-dcmd-topic` had named Story 4.6 as its owner, which is now wrong — re-owned
to **Story 4.19**, where the matrix already records that the row probably belongs at `n/a`.

**AC5 — the eleven sentences were twelve, and then thirteen.** Re-running the story's own grep found
a passage the story's table had missed (`docs/sparkplug-conformance.md:674`, the `-state-subs` row).
Reading around it found a thirteenth the grep could never have found: chapter 4's `topics-ncmd-topic`
said *"not implemented"*, not *"no subscription"*. **A mechanical list is a floor, not a ceiling** —
and it is worth saying that the story was right to include the list anyway: without it the four
`primary-host-state-observation.md` passages would not have been looked at at all.

Two further consequences the list did not name, both found by following the change rather than the
grep:

- **Every `mqtt_driver.rs` line citation in the matrix went stale.** Inserting ~200 lines into `mqtt_driver.rs` invalidated every
  `mqtt_driver.rs:NNN` reference in the conformance matrix. All were recomputed and re-verified
  against the file. Citations in `_bmad-output/implementation-artifacts/` were left alone: those are
  dated records of completed work, and rewriting them would falsify the record.
- **A false sentence in the manual that a previous correction had missed.** Chapter 2's warning still
  said the tag definitions cannot be restored *"until \prog itself is restarted"*. The Story 4.4
  review corrected exactly that sentence — in chapter 5 — and this copy survived by one chapter. It
  is now corrected, and the correction says so rather than quietly overwriting it.

**The two load-bearing passages were re-argued, not just re-worded**, which is the step AC5 exists to
force. `primary-host-state-observation.md` gains *"What Story 4.6 changed in this argument"* and
ADR 0016 gains *"What Story 4.6 changed"*. Both reach the same conclusion: the cost of ignoring STATE
rested on three facts and Story 4.6 removed one of them, but the missing Rebirth handler alone is
sufficient (**corrected by the review: this originally said "either survivor alone is sufficient",
which is false — the unretained NBIRTH does not on its own imply the absence of a repair path**) — a
Rebirth that arrives and is ignored repairs exactly as much as one that never arrives. So the
decision stands, and **the case for Story 4.7 preceding 4.5 is stronger than before**: what was a
two-part absent mechanism is now a single missing handler behind a live subscription.

**A hazard this story creates, stated rather than left to be discovered.** The subscription is open on
a broker where a live MQTT Engine v5.0.0-rc1 sends real Rebirth requests. Until Story 4.7 lands,
every one is answered with a log line. That is safe — making the ignoring safe is the whole point of
this story — but the cost grows the longer the state lasts. Recorded in ADR 0016.

**Not done here, and deliberately:** `Node Control/Rebirth` handling (Story 4.7), the will's QoS
(#26, Story 4.17), any STATE handling (Stories 4.4/4.5). Issue
[#23](https://github.com/guycorbaz/smartme_mqtt/issues/23) is now only half true and wants a comment
saying the subscribe half is closed — **not done, because commenting on a public issue is an outward
action I have not been asked to take.**

### AC5 — the per-passage report Task 7 asked for

Added by the code review, 2026-07-29. Task 7 bullet 1 required *"report each as amended or as
confirmed still-true with the reason"*, and the original record gave a narrative instead. The review
found two false sentences that had survived the sweep — and made the case that the itemisation is what
would have caught them, since reconstructing this table is how they were found. So the table exists
now, for the next story that inherits a list like this.

Rows 1–11 are the story's own grep (`no MQTT subscription\|no subscription\|not subscribed` over
`docs/` and `_bmad-output/planning-artifacts/`, excluding the pinned spec). Rows 12–13 were found by
the dev pass. Rows 14–18 were found by the review, after the story had reported AC5 complete.

| # | Passage | Found by | Disposition |
| ---: | --- | --- | --- |
| 1 | `primary-host-state-observation.md:293` — *"no MQTT subscription of any kind"* | grep | **amended** + new re-argument section |
| 2 | `primary-host-state-observation.md:314` — chain item 3, *"not subscribed"* | grep | **amended** to *"not subscribed **to STATE**"* |
| 3 | `primary-host-state-observation.md:365` — the measured/inferred boundary | grep | **amended**; boundary unchanged, sentence qualified |
| 4 | `primary-host-state-observation.md:414` — the `-state-subs` ruling | grep | **amended**; ruling stands, wording precise |
| 5 | `manual/chapters/05-…tex:250` — *"issues no MQTT subscription of any kind"* | grep | **amended** + new *Commands are received and ignored* section |
| 6 | `manual/chapters/02-…tex:680` — capability table, Rebirth absent | grep | **amended** |
| 7 | `sparkplug-conformance.md:353` — the `-ncmd-subscribe` row | grep | **verdict moved** `gap → conformant`, evidence named |
| 8 | `sparkplug-conformance.md:1200` — *"NCMD/DCMD not implemented"* | grep | **amended** (struck through, replaced) |
| 9 | `sparkplug-conformance.md:1214` — Primary Host absent end to end | grep | **amended**; eleven clauses stand |
| 10 | `adr/0016-…md:59` — *"no MQTT subscription of any kind"*, **evidence in a decision** | grep | **amended** + new *What Story 4.6 changed* section |
| 11 | `epics.md` Story 4.6 — the elided tck-id | listed, not grep | **corrected at story creation** |
| 12 | `sparkplug-conformance.md:674` — the `-state-subs` row | dev pass, re-running the grep | **amended** — the story's table had missed it |
| 13 | `sparkplug-conformance.md` ch. 4 `topics-ncmd-topic` — *"not implemented"* | dev pass, reading around | **verdict moved**; the grep could never have found it |
| 14 | `sparkplug-conformance.md:507` — `-rebirth-action-1`, *"nothing receives a Rebirth Request"* | **review** | **amended** — and prose this story *added* had certified it as still-right |
| 15 | `sparkplug-conformance.md:1245` — *"only the NCMD subscription and handler are missing"* | **review** | **amended** — live *Findings carried forward* table, two rows below one that was amended |
| 16 | `primary-host-state-observation.md:304` — *"the bridge accepts no NCMD/DCMD"* | **review** | **reworded** — true only if *accepts* means *acts on* |
| 17 | `adr/0016-…md:125` — *"needs an NCMD subscription that does not exist"* | **review** | **tense corrected**; the rejection never rested on it |
| 18 | `epics.md:140` AR10 + `architecture.md:85` — the five-step boot order | **review** | **amended to six steps** — the code and the manual had moved, these had not |

**What the shape of this table says.** The mechanical grep found 10 of 18. The dev pass found 2 more
by re-running its own grep and by reading around a hit. The review found 5, of which **three were
sentences no keyword search could reach** — they describe the consequence in different words than the
claim (*"nothing receives"*, *"accepts no"*, *"needs a subscription that does not exist"*), and one was
an architectural statement in a different artifact family altogether. A grep is a floor. Reading the
neighbourhood of every hit is the ceiling, and it is what the next story should budget for.

### File List

**Production code**

- `crates/sparkplug-b/src/topic.rs` — `MessageType::NCmd`, its token and level, plus two tests
- `crates/sparkplug-b/src/lib.rs` — conformance-scope docs: NCMD topic form implemented, `DCmd` absent on purpose
- `crates/smartme-bridge/src/app/mqtt_driver.rs` — the subscription, the SubAck check, the inbound-command channel and handler, module docs, four tests

**Tests**

- `crates/smartme-bridge/tests/chaos_ncmd_subscription.rs` — **new**
- `crates/smartme-bridge/tests/common/mod.rs` — `start_verbose_broker`

**Documents**

- `docs/sparkplug-conformance.md` — 2 verdicts, 3 tallies, every `mqtt_driver.rs` line citation, 9 prose passages
- `docs/primary-host-state-observation.md` — 4 passages + a new argument section
- `docs/adr/0016-rebirth-before-primary-host-wait.md` — the premise, and a new section re-checking the decision
- `docs/manual/chapters/05-mqtt-sparkplug-contract.tex` — new *Commands are received and ignored* section, boot order, 2 limitation bullets
- `docs/manual/chapters/02-understanding-sparkplug.tex` — capability table, and the false "until \prog is restarted" sentence
- `_bmad-output/planning-artifacts/epics.md` — AC2 carried back, delivery recorded
- `_bmad-output/implementation-artifacts/4-6-ncmd-subscription-plumbing.md` — this file
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — status

### Change Log

| Date | Change |
| --- | --- |
| 2026-07-29 | **Code review — 3 adversarial layers, 18 patches applied, 6 deferred, 3 dismissed.** The two that mattered: (a) AC2 and AC3 were **dark in a default deployment** — `fmt::init()` resolves to a default filter of ERROR, so every INFO/WARN trace the two ACs are written in terms of was dropped unless `RUST_LOG` was set; the chaos test passed only because it sets it itself. Fixed by an explicit INFO default. (b) Two sentences survived the AC5 sweep, **one of them certified as still-correct by prose this story added** — a fifth instance of the pattern AC5 exists to prevent; the per-passage table Task 7 asked for now exists and is what found them. Also: `granted()` read only `return_codes[0]` (breaks the day Story 4.5 adds a second subscription); the SubAck and ignore trace ARMS were unfalsifiable and are now extracted and unit-tested against captured output; the chaos test's AC3 needle was the metric name rather than the ignore trace, so a Story 4.7 handler would have kept it green; its liveness check could not see a dead driver task behind a live process; AC1's *"every reconnect"* clause had no test and now forces a reconnect by evicting the bridge's client id; an alias-addressed command reported as `<unnamed>`. **7 falsification mutations run, 7 red** — including the two that had left the whole suite green (subscribe hoisted out of the reconnect path; driver task dead behind a live process). `arch_purity` unchanged. `./scripts/ci-local.sh` reports *All CI steps reproduced locally* with `chaos_ncmd_subscription` green inside it; `latexmk` exit 0, 41 pages. One finding deferred by decision with its reasoning recorded: an inbound packet above 10 KiB tears down the session, which is a weaker vector than the forged NDEATH an unauthenticated broker already permits. |
| 2026-07-29 | Story 4.6 implemented. NCMD subscription at QoS 1 before the birth on every connect; SubAck return codes read and traced; inbound commands received, classified and discarded on their own channel with a traced drop. 9 falsification mutations run, all red. `arch_purity` unchanged. Conformance rows `-ncmd-subscribe` and `topics-ncmd-topic` moved to `conformant`; three tallies updated. 13 document passages amended (the story's list of 11 was two short) plus every stale `mqtt_driver.rs` line citation. Manual builds, `latexmk` exit 0. `./scripts/ci-local.sh` reports *All CI steps reproduced locally*. |
