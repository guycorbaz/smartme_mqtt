# Story 4.7: `Node Control/Rebirth` — answer with a fresh birth (FR19)

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As the SCADA,
I want a rebirth request answered with a complete re-announcement,
so that I can resynchronise without waiting for the bridge to reconnect on its own.

## Acceptance Criteria

The epic states three (`epics.md:915-928`). **Two of them are amended and three are added**, for
reasons recorded under *Dev Notes → What the epic gets wrong, and what it leaves out*. Read that
section before implementing: one epic clause, taken literally, would force the bridge to **delete a
value it can honestly account for**, and would turn an existing green test red for the right reason.

**AC1 — the NBIRTH declares the command, per the five MUSTs that require it**

**Given** any NBIRTH this bridge publishes — first birth, reconnect birth, or rebirth answer
**When** its payload is decoded
**Then** it carries a metric named exactly `Node Control/Rebirth`
**And** that metric has datatype `Boolean` (code 11) and value `false`
**And** it carries **no alias**
**And** the same metric is present on every NBIRTH, not only the first.

> Five clauses, in three chapters, all MUST, all currently unmet:
> `tck-id-topics-nbirth-rebirth-metric` (`Sparkplug_4_Topics.adoc:215-217`),
> `tck-id-payloads-nbirth-rebirth-req` (`Sparkplug_6_Payloads.adoc:1082-1084`),
> `tck-id-operational-behavior-data-commands-rebirth-name`, `-rebirth-datatype` and `-rebirth-value`
> (`Sparkplug_5_Operational_Behavior.adoc:955-965`). The no-alias clause
> (`-rebirth-name-aliases`, `:957-961`) is `n/a` here and **must stay** `n/a` — see Dev Notes.

**AC2 — a Rebirth Request is recognised and answered with a complete BIRTH sequence**

**Given** an NCMD on this node's command topic carrying a metric named `Node Control/Rebirth` with
boolean value `true`
**When** the driver handles it
**Then** it republishes the **complete** sequence: one NBIRTH followed by one DBIRTH per declared
meter
**And** the NBIRTH carries `seq = 0` and the DBIRTHs continue `1, 2, …` in publication order
**And** the answer is traced at **INFO**, naming the topic — visible under the default log filter,
with no `RUST_LOG` set.

> `tck-id-operational-behavior-data-commands-rebirth-action-2` (`:981-983`): *"it MUST send a
> complete BIRTH sequence including the NBIRTH and DBIRTH(s) if applicable"*.

**AC3 — DATA stops on receipt and does not resume until the BIRTH sequence is out**

**Given** a Rebirth Request has been taken off the command channel
**When** the driver acts on it
**Then** **no** DATA message is published between that moment and the last DBIRTH of the answer
**And** the property is asserted by a test that would go red if a DATA could interleave — not merely
argued from the shape of the `select!` loop.

> `tck-id-operational-behavior-data-commands-rebirth-action-1` (`:979-980`): *"it MUST immediately
> stop sending DATA messages"*. The bridge satisfies this **by construction** today (one task, one
> sequential handler) — which is exactly why it needs an assertion: a refactor that spawns the answer,
> or defers it behind a flag, breaks the clause with nothing to notice.

**AC4 — the answer re-announces what is known, and never invents a reading**

**Given** a rebirth answered while a meter has **never** produced a reading
**When** its DBIRTH is decoded
**Then** the metrics are valueless (`Null(Double)`) with quality `Stale` and the payload timestamp is
the birth's own — identical to cold start.

**Given** a rebirth answered while a meter **has** a reading, however old
**When** its DBIRTH is decoded
**Then** the reading is re-declared with its **own `ValueDate`** as the payload timestamp, never
`now`, and its quality is degraded, never upgraded — `Good` becomes `Stale`, and `Stale`/`Bad` stay
where they are.

> **This amends the epic**, which said a rebirth during a cloud outage yields metrics *"with no value
> and quality `Stale`, exactly as at cold start"*. That is right for a meter with no reading and
> **wrong** for one with a reading: blanking a value the bridge can account for destroys true history
> and contradicts `a_rebirth_redeclares_what_is_known_instead_of_blanking_it`
> (`sparkplug_publisher.rs:698`), which pins the opposite. See Dev Notes.

**AC5 — a rebirth re-announces a session, it does not open one**

**Given** a rebirth is answered
**When** the NBIRTH's `bdSeq` metric is compared to the one in the will registered at CONNECT
**Then** it is unchanged
**And** the assertion is made **through the NCMD path**, not through the reconnect path that already
has coverage.

> `tck-id-operational-behavior-data-commands-rebirth-action-3` (`:984-987`): *"The NBIRTH MUST include
> the same bdSeq metric with the same value it had included in the Will Message of the previous MQTT
> CONNECT packet … Because a new MQTT Session is not being established, there is no reason to update
> the bdSeq number."* Concretely: **`new_session()` must not be called on this path.**

**AC6 — the norm's reading, and a trace that records what actually arrived**

**Given** an NCMD carrying a metric named `Node Control/Rebirth` with boolean value `true`
**When** it arrives
**Then** it is answered, per AC2.

**Given** an NCMD carrying `Node Control/Rebirth` whose value is boolean `false`, non-boolean, or
absent
**When** it arrives
**Then** it is **not** answered — `-ncmd-rebirth-value` defines the request as carrying `true`, and
this bridge implements the norm's reading
**And** it is traced **distinctly** from both an unrecognised command and an answered one
**And** the trace records the metric's **datatype and value exactly as received**.

**Given** any other NCMD
**When** it arrives
**Then** every Story 4.6 ignore path behaves exactly as before: undecodable → WARN, no-metrics →
INFO, unrecognised names → INFO, none panicking, none applying a partial effect
**And** an alias-addressed metric with no name is **not** treated as a Rebirth Request.

> `tck-id-operational-behavior-data-commands-ncmd-rebirth-value` (`:974-975`): *"A Rebirth Request
> MUST include a metric value of true."*
>
> **The datatype-and-value trace is load-bearing, not decoration.** A strict matcher's failure mode is
> that it never fires, silently, if a live host encodes the request differently from our reading. The
> trace is what makes that visible instead of invisible: a near-miss is recorded with the exact bytes
> that missed, so the pre-production Ignition run diagnoses itself. See *Dev Notes → The strict
> matcher and its one failure mode*.

**AC7 — every document and every test that says the bridge answers no command is corrected**

**Given** this story makes *"nothing acts on any command"* false
**When** it lands
**Then** each passage in *Dev Notes → The passages this story falsifies* is amended **or explicitly
confirmed still-true with its reason**, reported as a **per-passage table**, not as narrative
**And** the **seven** existing conformance rows this story owns move off `gap (unimplemented)` with
their evidence named, and the three affected tallies are recomputed — the eighth clause,
`topics-nbirth-rebirth-metric`, **has no row yet** and belongs to Story 4.19
**And** `chaos_ncmd_subscription`'s three inverted assertions are re-aimed rather than deleted, and
the file's *"ways this could pass wrongly"* list is updated with what changed.

*AC1, AC3 and AC6 added at story creation 2026-07-30; AC4 and AC5 amended. AC7 is the sixth
consecutive instance of the same guard: five times this project has corrected a claim and left the
sentences describing its consequences untouched, and the fifth happened **inside the fix for the
fourth**. The mechanism that works is the itemised table, not the intention.*

## Tasks / Subtasks

- [x] **Task 1 — put `Node Control/Rebirth` in the NBIRTH** (AC: 1)
  - [x] `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs`: the node BIRTH currently carries
        one metric, `contract_metric` (`:243`, `:252`). Add the Rebirth declaration alongside it, in
        **both** arms — `Session::Pending` (first birth) and `Session::Live` (rebirth/reconnect). Two
        call sites, one omission away from a clause that holds on the first birth and fails on every
        later one.
  - [x] Value: `MetricValue::Boolean(false)` (`model.rs:87`), which encodes as `DataType::Boolean`
        = code 11 (`datatype.rs:40`). **Do not** use `Null(DataType::Boolean)`: the clause says value
        `false`, and a null metric is the absence of a value.
  - [x] Quality: match `contract_metric`'s reasoning — it is a fact about the running software, always
        known, never stale. Say so in a comment rather than copying the call.
  - [x] No alias: `encode_metric` hard-codes `alias: None` (`encode.rs:240`), so this is satisfied by
        construction. **Confirm it deliberately** and keep `-rebirth-name-aliases` at `n/a`, matching
        the three chapter-6 alias clauses.
  - [x] **`CONTRACT_VERSION` 2 → 3.** Decided at drafting time; the reasoning and the rejected
        alternative are in Dev Notes. Update the doc comment on the constant
        (`sparkplug_publisher.rs:29-39`) with a version-3 bullet in the same shape as the others.

- [x] **Task 2 — recognise a Rebirth Request** (AC: 2, 6)
  - [x] `mqtt_driver.rs`: extend `Inbound` (`:253-261`) with a `Rebirth` variant. The enum's doc
        comment says *"The bridge recognises NO command yet — `Node Control/Rebirth` is Story 4.7"*;
        that sentence is now false and is on the AC7 list.
  - [x] Match on **name and value**: name `== "Node Control/Rebirth"` **and** boolean `true` →
        `Rebirth`. The same name with any other value — boolean `false`, non-boolean, or absent — gets
        its **own** classification and its own trace, distinct from both `Unrecognised` and `Rebirth`.
        See *Dev Notes → The strict matcher and its one failure mode*.
  - [x] The generated variant is `protobuf::payload::metric::Value::BooleanValue(bool)` (proto
        `bool boolean_value` in the `Metric` message) — **verify the generated name against
        `target/…/org.eclipse.tahu.protobuf.rs` rather than trusting this line.**
  - [x] **The trace carries the datatype and the value as received**, on every rebirth-named metric,
        answered or not. **This is the near-miss detector and it is not optional** — it is the whole
        mitigation for the strict matcher's one failure mode. A trace that only says *"answered a
        rebirth"* discards exactly the bytes needed to diagnose a request that missed.
  - [x] Name-only matching, deliberately: `metric_label` (`:275-281`) already renders an
        alias-addressed metric as `<alias N>`. **A `<alias N>` never matches.** This bridge publishes
        `alias: None` on everything, so no host can legitimately know an alias for our metrics, and
        `-rebirth-name-aliases` exists precisely so a host never needs one. Record the reasoning in a
        comment; the alternative (matching a configured alias) is rejected in Dev Notes.
  - [x] A payload carrying **several** metrics of which one is the request is still a request. Do not
        require it to be alone.

- [x] **Task 3 — answer it, and share the code path with the connect birth** (AC: 2, 3, 5)
  - [x] The `Transport::Connected` arm (`:452-472`) already builds a `Queue`, calls
        `publisher.birth(clock.wall(), &meters, &mut queue)` and drains it. The rebirth answer is the
        **same three lines**. Extract them into one function and call it from both arms.
  - [x] **Do NOT call `publisher.new_session()`** (`sparkplug_publisher.rs:189`). It advances `bdSeq`,
        which `-rebirth-action-3` forbids on this path. Note that nothing calls it today; it is
        reachable and wrong, and the compiler will not stop you.
  - [x] Handle the `Err` arm the way the connect arm does — trace at ERROR and publish nothing. A
        half-emitted birth is the failure `birth()`'s validate-everything-first design exists to
        prevent.
  - [x] The INFO trace must be distinguishable from the connect birth's `"session born"`
        (`:466`): an operator reading a log needs to know whether the node re-announced because it
        reconnected or because a host asked.
  - [x] **AC3 is satisfied by keeping the answer inline and synchronous.** Do not `tokio::spawn` it,
        do not set a flag consumed later, do not send it through a channel. Write the reason in a
        comment at the call site: the `select!` loop handles one branch to completion, so a DATA from
        `inbox` cannot interleave — and that is a property of the shape, which is why Task 5 asserts
        it rather than trusting it.

- [x] **Task 4 — decide the rate question, do not defer it** (AC: 2)
  - [x] A host may send several Rebirth Requests in a burst; Ignition resends. The command channel is
        8 slots with a traced drop (`COMMAND_QUEUE`, `:193`), so a burst degrades to some answers plus
        traced drops — never a stall.
  - [x] **Decision: no rate limit, no coalescing.** Recorded here rather than deferred to a chaos test
        that does not exist (`CLAUDE.md`). Grounds: a birth here is 1 NBIRTH + N DBIRTHs for N=1
        configured meter; suppressing a rebirth request is the exact failure this story fixes; and a
        suppressed answer is invisible to the host, which cannot distinguish it from a node that never
        heard. Put this in a comment. If a burst ever proves costly, Story 4.13 (chaos broker
        recovery) is where it is **measured**, not guessed.

- [x] **Task 5 — tests, each falsified before it is trusted** (AC: 1, 2, 3, 4, 5, 6)
  - [x] **Unit, `sparkplug_publisher.rs`:** every NBIRTH carries the Rebirth metric with datatype
        Boolean, value `false`, no alias — asserted on **both** the first birth and a rebirth, against
        the decoded payload, not against the builder's own expression. *(The Story 4.3 review found a
        row scored `conformant` on a test comparing production's expression with itself; #30 lists
        eight such. Decode the bytes.)*
  - [x] **Unit, `mqtt_driver.rs`:** `classify` returns `Rebirth` for boolean `true`, and **not**
        `Rebirth` for boolean `false`, for a missing value, for a non-boolean value, or for an
        alias-addressed metric with no name. Falsify by dropping the value check — if the `false` case
        still passes, the assertion is not testing what it says.
  - [x] **Assert the near-miss trace itself**, against captured output: a `Node Control/Rebirth` that
        was NOT answered must leave its datatype and value in the log. Falsify by removing those two
        fields from the trace — the classification tests stay green, which is exactly why this needs
        its own assertion.
  - [x] **Unit, `mqtt_driver.rs`:** the trace arms, against captured output. The Story 4.6 review's
        sharpest finding was that swapping two arms' *bodies* left every test green: the log line IS
        the observable AC2 and AC6 are written in.
  - [x] **Chaos, end to end:** publish a real Rebirth Request from an independent client and assert
        the **complete** sequence arrives — NBIRTH with `seq = 0`, then one DBIRTH — with the `bdSeq`
        unchanged from the birth observed before the request. Reuse the `chaos_ncmd_subscription`
        harness shape (testcontainers Mosquitto, `start_verbose_broker`, `wait_for`).
  - [x] **AC3's assertion is the hard one.** Assert on the observed message stream that **no
        `/DDATA/` appears between the request and the last DBIRTH**. Falsify it by deferring the
        answer (e.g. `tokio::spawn` the birth, or handle it on the next loop iteration) — with a 5 s
        poll interval you may need to shorten the interval, or send the request while a DATA is
        already in flight, to make the window real. **If the mutation cannot be made to fail, say so
        in the story rather than claiming the AC** — `CLAUDE.md`: if it cannot be made to fail, it is
        not yet a test.
  - [x] **Record every falsification next to the test**, in the table in this file's Dev Agent Record.
  - [x] `cargo test -p smartme-bridge --test arch_purity` must pass **unchanged**. The guard is not to
        be edited: the handler transports and traces, it decides no truth.
  - [x] `cargo test -p sparkplug-b --test no_context_leak` — nothing here should reach the published
        crate at all, which is itself worth confirming.

- [x] **Task 6 — re-aim `chaos_ncmd_subscription`, do not delete it** (AC: 7)
  - [x] Three of its assertions now assert the opposite of correct behaviour:
        `"unrecognised NCMD ignored"` (`:366-377`), the metric-name needle (`:379-384`), and
        `rebirth.is_none()` (`:410-420`). Its comment at `:407-409` says so in advance.
  - [x] **⚠️ THE TRAP, and it is live under the strict matcher.** Its `command_payload` helper
        (`:148-163`) builds metrics with `..Default::default()` — **value `None`**. Under AC6 a
        valueless `Node Control/Rebirth` is **not** a request and must not be answered. So
        `rebirth.is_none()` **stays green** after a completely correct implementation, and after a
        completely broken one. It confirms nothing. **A dev who reads the green suite as validation
        will ship an unanswered rebirth.**
  - [x] Give the helper a **value parameter** and republish the rebirth case as boolean `true`, then
        invert the assertion. Keep the valueless payload as a separate AC6 case, asserting the
        near-miss trace rather than the absence of a birth.
  - [x] The other two Story 4.6 paths — undecodable, and decoded-but-empty — are unchanged and their
        assertions must stay exactly as they are.
  - [x] Update the file's module-doc list of *"every way this test could pass wrongly"* with the
        no-value trap above, so the next reader inherits it.

- [x] **Task 7 — work the falsification list and the conformance rows** (AC: 7)
  - [x] Produce the **per-passage table** in Completion Notes: file, line, what it said, disposition
        (amended / confirmed still-true, with the reason). Narrative is not a report — the Story 4.6
        review made that case with evidence.
  - [x] `docs/sparkplug-conformance.md`: **seven** rows move. Chapter 5 — `-rebirth-name` (`:514`),
        `-rebirth-datatype` (`:515`), `-rebirth-value` (`:516`), `-rebirth-action-1` (`:518`),
        `-action-2` (`:519`), `-action-3` (`:520`). Chapter 6 — `payloads-nbirth-rebirth-req`
        (`:930`). Recompute chapter 5's and chapter 6's tallies **and** the whole-specification total,
        and state the arithmetic.
  - [x] **Chapter 4's `topics-nbirth-rebirth-metric` has no row at all** — it is one of the 29 clauses
        Story 4.19 owns (`epics.md:1140` instructs 4.19 to point it at Story 4.7). **Do not open the
        row here**; record the evidence in this story's notes so 4.19 can cite it, and leave chapter
        4's tally alone.
  - [x] `crates/sparkplug-b/src/lib.rs:26-33` — the *Conformance scope* lists the `Node Control/Rebirth`
        metric under *"Not implemented — a conformant node must supply these itself"*. **Re-read it
        rather than editing reflexively:** the sentence is about the crate, and the crate still does
        not supply the metric — the bridge does. Confirm or amend, and say which.
  - [x] `docs/manual/`: chapter 5 `:238-239` and `:314`, chapter 2 `:533`, `:644`, `:683-684`, `:699`.
        The manual documents implemented behaviour, so it changes **in this story**. `latexmk` must
        exit 0.
  - [x] `docs/adr/0016-…md:62`, `:98` and `docs/primary-host-state-observation.md:304`, `:417` use
        *"answers no command"* / *"a Rebirth that arrives and is ignored repairs nothing"* as
        **evidence in an argument**. Amend the sentence **and re-check what it was holding up** — and
        note that ADR 0016's argument for sequencing 4.7 before 4.5 **expires here**: the memo says
        only the missing handler was load-bearing, and this story supplies it. **Story 4.5 must be
        re-weighed, not inherited.** Record that as the ADR's own follow-up.
  - [x] `_bmad-output/planning-artifacts/prd.md` — five passages assert the bridge *"responds to
        Ignition NCMD/Rebirth"* (`:68`, `:87`, `:97`, `:107`, `:212`, `:246`, `:360`). These were
        always **specification**, not description, and they now become true. Confirm each as
        still-true rather than amending — and say so, because a confirmation is a disposition too.
  - [x] **`prd.md:149` and the RBE deviation ([#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)).**
        The PRD says the bridge *"cannot safely [do report-by-exception] until it answers NCMD/Rebirth
        (Stories 4.6–4.7)"*. That blocker lifts here. **Do NOT implement RBE** — re-examine the
        `tck-id-principles-rbe-recommended` deviation row, record what changed, and leave the decision
        to its own story. Comment on #32 only if Guy asks: commenting on a public issue is an outward
        action.
  - [x] `epics.md` — carry the amended and added ACs back into Story 4.7's entry, as Story 4.6 did.
        Also check Story 4.8's premise (`:932-947`): it extends the Tier-3 gate to rebirth and its
        NFR17 note (`:114`) says *"the NCMD/Rebirth half is Epic 4"* — 4.8 still owns closing it, so
        confirm rather than amend.
  - [x] **The two DCMD `n/a` verdicts become time-limited.** `-device-dcmd-subscribe` (`:403-407`) and
        chapter 4's `topics-dcmd-topic` are `n/a` on the stated condition *"if the Device supports
        writing to outputs"*. A meter relay command is planned for the pre-production Ignition run, so
        the condition is scheduled to hold. Record the expiry in the matrix cells, citing
        **[#38](https://github.com/guycorbaz/smartme_mqtt/issues/38)**; do **not** re-verdict them and
        do **not** add `DCmd` here.
  - [x] `./scripts/ci-local.sh` — not `--fast`, never piped, log written to an **absolute** path, and
        read the `EXIT=` line out of the file rather than trusting a reported exit code.

### Review Findings

Code review 2026-07-30, three adversarial layers in parallel (Blind Hunter — diff only, no project
access; Edge Case Hunter — diff + project + norm, story withheld; Acceptance Auditor — diff + story +
project + norm). All three layers completed. Every finding below was re-verified against the working
tree by the reviewer before being recorded; the layers' unverified claims were dropped.

**The headline: AC7 failed for the sixth consecutive time, and the falsification record has entries
that cannot be reproduced from the code.** The seven ACs are substantially met — the mechanism works
and the norm is read correctly — but the *consequences* sweep missed a fourth tally, five metric-count
statements, a deleted test's name, and the module doc of the very test file this story rewrote. And
the citation rot the story identified as a recurring defect was fixed in one file and shipped in
another.

#### Decisions taken at review (Guy, 2026-07-30)

All six resolved at review time rather than deferred to an artefact that does not exist
(`CLAUDE.md`). Each became a patch; the resolution is recorded on the item.

1. **Retained NCMD → reject, and trace it as a near miss.** A retained NCMD is by definition not a
   conformant Rebirth Request (`tck-id-payloads-ncmd-retain`), so rejecting it costs no
   compatibility, and routing it through the near-miss WARN makes it diagnosable instead of
   invisible. **This is a position on the inbound wire contract, so it needs an ADR and a GitHub
   issue**, and the manual's Known limitations gains the exposure it closes.
2. **Widen the detection net, not the action.** The action stays byte-exact — the norm is the norm.
   Detection widens: a name differing only by case or surrounding whitespace, and the literal
   `Node Control/Refresh`, go to the near-miss WARN with datatype and value. This is what closes the
   story's own residual-risk argument, which rests entirely on that detector firing.
3. **Make the birth drain all-or-nothing.** Check capacity before draining; if the 1 + N messages do
   not fit, publish nothing and trace at ERROR. This aligns the code with what its own doc already
   claims and with `birth()`'s validate-everything-first design.
4. **Leave `degrade` as it is, and document the consequence.** The direction of the error is safe —
   never a fresh-looking lie — and re-judging would require handing the publisher a clock and a
   `Policy`, which is exactly the purity `arch_purity` protects. The manual records that answering a
   rebirth costs a stale window of up to one poll interval.
5. **Guarantee a DATA is pending at dequeue.** Push a burst into the 64-slot inbox immediately before
   publishing the NCMD, so an `inbox` branch is certainly ready when the command is dequeued. That
   makes AC3's window structural rather than probabilistic — **and mutation 13 must then be re-run
   and its real output recorded**, because the currently recorded RED was obtained against the weak
   window.
6. **The oversized packet is *not* covered by decision 1** — the rejection happens after decode,
   whereas rumqttc tears the session down before it. Handled separately: set an explicit, justified
   `set_max_packet_size` rather than inheriting rumqttc's default, plus the trace patch that names the
   command topic. The 2026-07-29 deferral stands as a decision; what changes is the unmeasured
   residue handed to Story 4.13 — *"1 Hz with an attacker"* becomes *"permanent without one"*, and
   `deferred-work.md` is updated to say so.

#### Patches from the decisions

- [x] [Review][Patch] **Reject a retained NCMD and trace it as a near miss** — read `publish.retain` in `pump_transport`'s inbound arm (it destructures only `topic` and `payload` today) and carry it to `classify`, so a retained payload can never reach `Inbound::Rebirth`. Cite `tck-id-payloads-ncmd-retain` (`Sparkplug_6_Payloads.adoc:1421`) at the check. **Needs an ADR and a GitHub issue** — it is a position on what the bridge accepts on the wire — plus a Known-limitations sentence in the manual and a falsification run (publish a retained conformant request; assert no NBIRTH follows and the near-miss WARN names the retain flag) [crates/smartme-bridge/src/app/mqtt_driver.rs:811]
- [x] [Review][Patch] **Widen near-miss detection to a near-*name* miss, leaving the action byte-exact** — a metric whose name, after trimming and case-folding, equals `Node Control/Rebirth`, or which equals `Node Control/Refresh`, enters `rebirth_named` for *detection* only; the answer still requires the exact name and boolean `true`. Record in a comment that `Sparkplug_5_Operational_Behavior.adoc:950` uses `Node Control/Refresh` in prose while every tck-id in the same section says `Rebirth`, so the norm contradicts itself and the wider net is what makes that visible [crates/smartme-bridge/src/app/mqtt_driver.rs:421]
- [x] [Review][Patch] **Make the birth drain all-or-nothing** — check the client's request-channel capacity against `queue.pending.len()` before draining; on insufficient room publish nothing and trace at ERROR, matching the `birth()` `Err` arm. Only then may the INFO line claim *"complete BIRTH sequence republished"*. Falsify by shrinking the channel below 1 + N [crates/smartme-bridge/src/app/mqtt_driver.rs:900]
- [x] [Review][Patch] **Document the rebirth stale window** — answering a rebirth re-declares every known reading through `degrade`, so all tags read `Bad_Stale` until the next poll (up to 30 s). Decided to keep: the error direction is safe and re-judging would put a truth decision in an adapter. Manual, Known limitations; and note that an unauthenticated client can hold that state by asking repeatedly [docs/manual/chapters/05-mqtt-sparkplug-contract.tex]
- [x] [Review][Patch] **Give AC3's window teeth, then re-run mutation 13** — push a burst into the inbox immediately before publishing the NCMD so a DATA is certainly pending at dequeue, and fix the module doc's inverted claim (20 ms is *slow* relative to per-message work, which is why the inbox is normally empty). The recorded RED for mutation 13 must be replaced with the output of a re-run against the strengthened window [crates/smartme-bridge/tests/chaos_ncmd_rebirth.rs:265]
- [x] [Review][Patch] **Set an explicit incoming packet-size limit, and name the command topic in the transport-error trace** — do not inherit rumqttc's 10 KiB default silently; choose a value, justify it against `AC-LEAK-01`'s bounded-memory requirement, and state it. Separately, the `"transport error"` WARN gives no route from symptom to cause: include the subscribed command topic so an operator can tell an oversized NCMD from an ordinary broker drop. Update `deferred-work.md`'s 2026-07-29 entry: ground (a)'s bound assumed a sustained attacker, and `retain` removes that assumption [crates/smartme-bridge/src/app/mqtt_driver.rs:616]

#### Resolved decision detail

- [x] [Review][Decision] **A retained NCMD is honoured, so one unauthenticated publish makes every future session answer a request nobody sent** — *resolved: reject + near-miss WARN, with an ADR.* — Nothing reads `publish.retain`; the only `retain` reads in `mqtt_driver.rs` are outbound (`:618`, `:961`). Any LAN client publishes once to the NCMD topic with `retain = true` and the broker keeps it; from then on every CONNACK → SUBSCRIBE (`:649`) draws an immediate redelivery and the bridge answers. Self-sustaining with the attacker gone, indistinguishable in the log from a real host request (the INFO line is identical), clearable only by publishing an empty retained payload to that topic. `tck-id-payloads-ncmd-retain` (`Sparkplug_6_Payloads.adoc:1421`): *"NCMD messages MUST be published with the MQTT retain flag set to false"* — a conformant host never sends one, so rejecting a retained NCMD costs nothing and is defensible on the clause. Options: reject + WARN (near-miss shaped); accept but trace the retain flag; accept as-is and record the exposure. Wire-contract position → ADR.
- [x] [Review][Decision] *(resolved: explicit limit + trace patch; deferral stands, 4.13 brief widened)* **The oversized-NCMD deferral loses its bounding argument once `retain` is in play — reopen it, do not re-decide it from scratch** — The oversized-inbound-packet vector is **already deferred by decision (Guy, 2026-07-29)**, recorded in `deferred-work.md` under Story 4.6 with four grounds. Ground (a) is the load-bearing one: *"It is a disruption, not a lie: the session genuinely dies, the death certificate is correct, and the bridge re-births ~1 s later"* — a bound that assumes a **sustained attacker**, and the residue was handed to Story 4.13 as *"a sustained attack would churn death/birth at roughly 1 Hz"*. `retain = true` removes the attacker: one publish of a ≥10 241-byte frame to the command topic is redelivered on every reconnect, so the frame that killed the session is the first thing delivered to the next one, indefinitely, with nobody present. Ground (d) — *"any client can already publish a forged NDEATH"* — does not cover it either: a forged NDEATH is a one-shot lie the bridge's next BIRTH corrects, whereas this makes the bridge its own amplifier. What is genuinely new is therefore the interaction, not the vector; the question is whether it changes the decision or only Story 4.13's brief. Note also that the trace on this path says only `"transport error"` and never names the command topic, so there is no route from symptom to cause — that half is a cheap patch regardless of how the decision lands.
- [x] [Review][Decision] *(resolved: widen detection, keep the action byte-exact)* **The near-miss detector cannot see a near-*name* miss — and the norm's own prose names a metric the matcher rejects** — `classify` filters on byte equality (`mqtt_driver.rs:421-425`), so `"node control/rebirth"`, a trailing space, or `Node Control/Refresh` never enter `rebirth_named`; they fall through to `Inbound::Unrecognised` and are traced at INFO with no datatype and no value, among ordinary unknown commands. **`Sparkplug_5_Operational_Behavior.adoc:950` says: *"it can send a 'Rebirth Request' using the 'Node Control/Refresh' metric"*** — a slip in the specification, in the very section whose tck-ids all say `Rebirth`, and `Refresh` appears nowhere in this repository. The story's entire residual-risk argument is that the near-miss trace makes a never-firing strict matcher diagnosable; that argument does not close while the most plausible real-host miss is the one the detector cannot see. Byte equality is right for the *action*; the *detection* net must be wider than the action.
- [x] [Review][Decision] *(resolved: all-or-nothing drain)* **A half-emitted birth is reachable, `announce`'s doc says it is not, and the log then calls it complete** — `announce` drains its queue message by message (`mqtt_driver.rs:903-905`) and `publish` turns a full 64-slot request channel into a WARN and continues (`:960-969`). So NBIRTH-queued / DBIRTH-dropped is reachable — during the pump's 1 s error-arm sleep, under TCP back-pressure, or under the burst Task 4 deliberately permits. The publisher has already committed `Session::Live` and `self.declared`, so it goes on emitting DDATA for a device the host now regards as undeclared, and the INFO line says *"complete BIRTH sequence republished"*. `announce`'s `# On error, nothing is published` section — *"A half-emitted birth would put an incomplete tag set on the irreversible side of the contract, which is the failure that design exists to prevent"* — covers only the `birth()` `Err` path, not the drain. Options: make the drain all-or-nothing (check capacity first); scope the doc and make the trace conditional on the drain succeeding; accept and record.
- [x] [Review][Decision] *(resolved: keep `degrade`, document the window)* **Answering a rebirth drives every Good tag to `Bad_Stale` for up to one poll interval** — `birth()` re-declares a known reading with `degrade(update.published)` (`sparkplug_publisher.rs:308`) and `degrade` maps `Good → Stale` unconditionally (`:496`). The publisher holds no clock and no `Policy`, so it cannot re-judge; `max_age_ms` is 90 s against a 30 s poll, so the reading it downgrades would have been judged **Good**. Sound and conservative on the reconnect path (outage of unknown length); on the rebirth path the link is healthy, so it is a *false* stale, and an unauthenticated client can hold every tag in the SCADA system at `Bad_Stale` indefinitely by asking every few seconds. **AC4 as written is met** — it requires degradation, never upgrade — so this is a question about AC4's intent, not a violation of its letter. The direction of error is safe (no fresh-looking lie), which is why it is a decision and not a patch.
- [x] [Review][Decision] *(resolved: guarantee a pending DATA, re-run mutation 13)* **AC3's window is ~0 messages wide and the module doc's stated basis is inverted** — `chaos_ncmd_rebirth.rs` feeds a reading every 20 ms and its doc claims *"20 ms is fast relative to everything the driver does per message, so the `inbox` branch of its `select!` is essentially always ready"*. It is the other way round: 20 ms is *slow* relative to the driver's per-message work, so the inbox is almost always empty and the driver sits parked in `select!`. The anti-vacuity guard (`count(seen,"DDATA") >= 3`) proves the stream flowed *before* the request, not that a DATA was pending *during* the window — which is the premise the assertion needs. So the deferral mutations it exists to catch pass whenever they complete inside one tick, and mutation 13's recorded RED is probabilistic rather than structural. A window with teeth needs a reading guaranteed pending at dequeue: push a burst into the 64-slot inbox immediately before publishing the NCMD, or assert the DDATA cadence is unbroken up to the NCMD and resumes after the last DBIRTH.

#### Patches

*Group A — AC7 survivors: passages this story falsified and left standing.*

- [x] [Review][Patch] A **fourth** tally left arithmetically false: *"The 50 gaps split 36 unimplemented / 14 unproven"* while the table 28 lines above now reads 43; a row count returns 29 / 14 [docs/sparkplug-conformance.md:1665]
- [x] [Review][Patch] *"it is the only node-level metric the bridge ever publishes"* — false as of AC1, and it is the behaviour column of a **conformant** verdict [docs/sparkplug-conformance.md:523]
- [x] [Review][Patch] *"the NBIRTH carries **two** metrics … two metrics is enough to be out of order"* — now three; a live `gap (unproven)` row that understates its own exposure [docs/sparkplug-conformance.md:526]
- [x] [Review][Patch] `case-sensitivity-metric-names` (live SHOULD NOT) enumerates *"three constants — `Power`, `Energy`, `Contract/Version`"*; there are now four (`sparkplug_publisher.rs:103,112,114,118`) and the new name was never checked against the clause. The verdict holds — `node control/rebirth` lower-cased collides with nothing — but that has to be *stated*, not assumed [docs/sparkplug-conformance.md:753]
- [x] [Review][Patch] *"The node's only metric is `Contract/Version`"* — false [docs/sparkplug-conformance.md:1001]
- [x] [Review][Patch] *"the NBIRTH's two metrics and the DBIRTH/DDATA's two share a timestamp"* — now three [docs/sparkplug-conformance.md:1289]
- [x] [Review][Patch] *"It still **acts** on none of them, so the six clauses are exactly where they were. Command handling is … inert"* — present tense, two sentences above its own correction *"**Story 4.7 has since landed**"*, in a paragraph this story edited. This is exactly the shape AC7 is the sixth guard against [docs/sparkplug-conformance.md:1422]
- [x] [Review][Patch] A live evidence cell names `a_recognisable_looking_command_is_still_unrecognised_in_this_story`, a test **this diff deletes**. The `seq`-tolerance argument now rests on a witness that cannot be run; the replacement test does still build `seq: None` payloads, so the fix is one word [docs/sparkplug-conformance.md:1074]
- [x] [Review][Patch] The module doc of the test file this story rewrote still says *"Story 4.6 — … every command is thrown away safely"* and *"Three properties"* — there are now five, and property 4 asserts a command **is** answered. The test function is still `chaos_ncmd_subscribed_before_the_birth_and_every_command_ignored` [crates/smartme-bridge/tests/chaos_ncmd_subscription.rs:1]
- [x] [Review][Patch] *"Until 4.7, every one is answered with a log line"* — unamended, in a file this diff edits [_bmad-output/implementation-artifacts/sprint-status.yaml:235]
- [x] [Review][Patch] *"`CONTRACT_VERSION` stays at 2"* — present tense, in the ADR a reader goes to for what the version numbers mean. Historically true of ADR 0012's own change, so the fix is a dated marker, not a rewrite [docs/adr/0012-quality-codes-spec-versus-host.md:47]
- [x] [Review][Patch] The published crate's *confirmed-unchanged* note now states *"moving seven specification clauses"* — a fact about another crate's matrix, restated where nothing can keep it true; the eighth clause to move (Story 4.19) makes it wrong with no signal [crates/sparkplug-b/src/lib.rs:26]

*Group B — citation rot, third recurrence, in the story that claimed to have fixed it.*

- [x] [Review][Patch] Five of the fourteen *"all re-pointed and each verified by printing the line it names"* `mqtt_driver.rs` citations are wrong. `:387` and `:1087` cite the NDEATH publish at `:909` — which is `tracing::info!(bd_seq, "session born")`, **a line this story wrote** (actual: `:771`). `:523` cites `MessageType::NData` at `:943` — that is `fn subscribe_to_commands` (actual: `:993`). `:368` cites `subscribe_to_commands` at both `:554` (a `%topic,` macro fragment) **and** `:943` in the same row, and cites the NCMD topic build at `:270` (blank; actual `:593`, where the neighbouring row correctly points). `:369` cites `Packet::ConnAck` at `:492` (a bare `}`; actual `:797`) [docs/sparkplug-conformance.md:368]
- [x] [Review][Patch] The whole `sparkplug_publisher.rs` citation class — 19 references — was never swept, and this story moved ~90 lines in that file. Verified stale: `:371` cites the NDEATH topic build at `:206` (actual `:242`); `:169` cites `DroppedBeforeBirth` at `:306-308` (actual `:343`); `:527`/`:530` cite `metrics_for` at `:392-399`/`:381` (actual `:472`, and both cited ranges now land inside the doc comments this story added); `:621` cites the held `EdgeNode` at `:157` (a doc line); `:753` cites the metric constants at `:76-84` (now `ignition_quality_code`'s doc comment) [docs/sparkplug-conformance.md]

*Group C — falsification-record integrity (`CLAUDE.md`'s core rule).*

- [x] [Review][Patch] `assert!(log.contains('3'), "the DATATYPE that arrived must be in the log (Int32 is 3)")` — `captured` builds `tracing_subscriber::fmt()` with no `.without_time()`, so every line carries an RFC-3339 timestamp and the needle is one digit. Mutation 8 removed `datatype` and `value` **together**, so its RED is attributable to the `"IntValue(1)"` assertion alone: the datatype half of AC6's trace requirement was never independently falsified. The behaviour is correct — this is a false falsification record, which is the shape of the four Epic-1 tests this project threw away [crates/smartme-bridge/src/app/mqtt_driver.rs:1481]
- [x] [Review][Patch] Two records of the same experiments disagree. `bdSeq 0 → 1` in the test versus `bdSeq **1 → 2**` in the story (mutation 12); and the test quotes `"a DDATA was published inside the birth sequence"`, a string that appears nowhere — the actual assertion reads `"{} DATA message(s) were published inside the birth sequence."`. Row 13 also names two different mutations (`tokio::spawn` versus *"deferred behind a flag consumed one message later"*). Re-run and copy the output; a reconstructed quote is indistinguishable from an experiment that was not run [crates/smartme-bridge/tests/chaos_ncmd_rebirth.rs:73]

*Group D — hardening and test decisiveness.*

- [x] [Review][Patch] The log-flood cap is per **value**, not per line. `describe_value`'s `CAP = 200` bounds one metric; `classify` collects one string per metric with no bound on the count (`:433-443` near-miss, `:445` unrecognised) and `trace_command_outcome` renders the whole vector. ~10 KB of wire becomes ~130 KB of log — at INFO for the unrecognised path, which `main.rs` now makes visible by default — written **synchronously from the same task that publishes DATA**. The doc's *"a hostile payload cannot fill a disk one command at a time"* holds only in the single-metric case. Separately, `format!("{value:?}")` materialises the full attacker-controlled string *before* the cap is applied [crates/smartme-bridge/src/app/mqtt_driver.rs:341]
- [x] [Review][Patch] AC3's window is bounded on the left by the **observer's** receipt of the NCMD, not the bridge's dequeue, while the bridge's DDATA and the commander's NCMD travel different connections. A DDATA the bridge published legitimately can land inside `tail[command_at..=last_dbirth_at]` and fail the test with a message accusing the driver of violating `-rebirth-action-1`. A live flake on a 2-job box, and the conformance row inherits the same window verbatim. Bound the left edge by the answering NBIRTH, or by the driver's own trace [crates/smartme-bridge/tests/chaos_ncmd_rebirth.rs:446]
- [x] [Review][Patch] `chaos_ncmd_rebirth` never checks the rebirth metric on the NBIRTH it treats as the answer. AC1 is asserted on `first_birth` — the `Session::Pending` arm — at `:291-336`; `tail[birth_at]` is checked for `seq` (`:415`) and `bdSeq` (`:431`) only. The clause is *"Every NBIRTH"*, and the arm that runs on every reconnect and every rebirth has no end-to-end witness. One assertion closes it [crates/smartme-bridge/tests/chaos_ncmd_rebirth.rs:414]
- [x] [Review][Patch] `chaos_ncmd_subscription`'s "the answer happened" pair can both hold without the answer happening. `trace_command_outcome` runs **before** `announce` (`mqtt_driver.rs:689-690`), so the `"Rebirth Request accepted"` needle proves classification and stays green for a deleted `Inbound::Rebirth` arm and for `announce` returning `Err`. That leaves `wait_for(NBIRTH, 20 s)`, which accepts any NBIRTH — including a reconnect birth left in `seen` by the deliberate eviction the test just performed. Assert `announce`'s own line (*"node re-announced on a Rebirth Request"*) and drain `seen` before publishing the request [crates/smartme-bridge/tests/chaos_ncmd_subscription.rs:754]
- [x] [Review][Patch] The AC1 unit test cannot witness the norm's exact string — it locates the metric with the same constant the producer uses. The literal *is* witnessed today, but incidentally, by a near-miss **trace** test (`mqtt_driver.rs:1533`) and by the Docker-gated chaos tests; a refactor that stops the trace printing the constant would silently remove the only `--fast` witness of the string three chapters require. Assert the literal in the AC1 test [crates/smartme-bridge/src/adapters/sparkplug_publisher.rs:664]
- [x] [Review][Patch] A rebirth request carried alongside unknown metrics: `classify` returns `Inbound::Rebirth` before building the unrecognised list, so the other metrics are discarded with no log line — while the module advertises *"never silently"*. Same hole on the `RebirthNearMiss` path [crates/smartme-bridge/src/app/mqtt_driver.rs:427]
- [x] [Review][Patch] `describe_value`'s comment claims *"`is_null` and 'no value field at all' both land here"*; the function never reads `is_null`. A metric with `is_null: Some(true)` **and** `value: Some(BooleanValue(true))` — legal on the wire — is classified as a conformant request and answered, which the comment says is not a request [crates/smartme-bridge/src/app/mqtt_driver.rs:347]
- [x] [Review][Patch] `command_payload` derives the declared datatype from the value's *presence* and hard-codes `Boolean`, under a doc claiming *"encoded exactly as a Host Application would send it"*. Any future caller passing a non-boolean silently gets a metric whose declared datatype contradicts its value — the exact near-miss shape this story exists to detect, and the same class as the `..Default::default()` trap the file just spent forty lines documenting [crates/smartme-bridge/tests/chaos_ncmd_subscription.rs:232]
- [x] [Review][Patch] `-rebirth-action-3` is verdicted `conformant` on birth-versus-birth evidence; the clause is birth-versus-**will**. The transitive link exists (`the_will_matches_the_session_before_and_after_the_birth`) but the row names no will-observing witness, so the chain holds via an assumption the row does not state [docs/sparkplug-conformance.md:537]
- [x] [Review][Patch] AC3's title promises DATA *"does not resume until the sequence is out"*; nothing asserts it **resumes**. A driver that stopped publishing DATA permanently after answering a rebirth — the most damaging plausible regression of this change, and a silent one — satisfies every assertion in the file, which ends `feeder.abort(); driver.abort();` immediately after the absence check [crates/smartme-bridge/tests/chaos_ncmd_rebirth.rs:449]
- [x] [Review][Patch] The nine-clause `n/a` block reasons *"these clauses bind the **publisher** of an NCMD/DCMD … there is no behaviour of ours for these clauses to govern"*. The verdict stays right — the bridge publishes no NCMD — but Story 4.7 makes the bridge a **consumer**, and `payloads-ncmd-retain` and `-ncmd-qos` now bear on what it should *accept*. Annotate the reasoning rather than the verdict [docs/sparkplug-conformance.md:1030]

#### Deferred

- [x] [Review][Defer→FIXED] LaTeX table overflow: **confirmed by building it, then fixed.** `Overfull \hbox (17.75223pt too wide)` on chapter 5's NCMD table, under `latexmk` exit 0 — the exact reason it could not be deferred on a reading. Column one was `l`, which does not wrap; both columns are now `p{}`. The rebuild has no overfull box there [docs/manual/chapters/05-mqtt-sparkplug-contract.tex:263]

#### Dismissed as noise

- *"17 falsification mutations, all red"* versus the GREEN recorded in `chaos_ncmd_rebirth.rs:75` — the GREEN is reasoned and correct (*"a name-and-value request still matches"*), and the story's own mutation 6 already records a mixed result. Loose phrasing, not a false record.
- Nine documents written in past tense / closed state while `sprint-status.yaml` marks the story `review` — that is this project's workflow: the dev writes the consequences, the review is what validates them, and it is running now.
- Arithmetic of the three tallies the story *did* recompute — independently reproduced from a mechanical row count by two layers: chapter 5 `29·2·19·49`, chapter 6 `31·5·14·59`, total `79·8·43·144` = 274 of 303. Correct.

## Dev Notes

### What this story is

Story 4.6 built the ear. This builds the voice. The mechanism it needs is **already written and
already tested** — `SparkplugPublisher::birth` re-emits the complete NBIRTH + DBIRTH sequence under
an unchanged `bdSeq`, and the conformance matrix says so at `:552-559`: *"What is missing is only the
trigger … Story 4.7's work is a caller, not an encoder."*

That is true of AC2, AC4 and AC5. It is **not** true of AC1, AC3 and AC6, which is why the epic's
three criteria are not the whole story.

### ⚠️ What the epic gets wrong, and what it leaves out

**1. The epic omits the metric the norm requires in every NBIRTH.** Five MUST clauses in three
chapters require an NBIRTH to declare `Node Control/Rebirth`, boolean, `false`. The bridge's node
BIRTH carries exactly one metric, `Contract/Version` (`sparkplug_publisher.rs:243`, `:252`), plus the
`bdSeq` the crate prepends. The matrix already scores all five as `gap (unimplemented)` **owned by
Story 4.7** (`:514-516`, `:930`), and `epics.md:1140` routes chapter 4's copy here too. The epic's
own acceptance criteria never mention them. Without this metric a host has no declared endpoint to
address, and the bridge fails five MUSTs while advertising that it answers rebirths.

**2. The epic omits `-rebirth-action-1` entirely.** *"When an Edge Node receives a Rebirth Request, it
MUST immediately stop sending DATA messages"* (`:979-980`). The epic's ACs cover the answer and the
`bdSeq`, not the stopping. Today the bridge satisfies it accidentally — one `select!` loop, one
handler run to completion — and an accidental pass is what this project has thrown away four tests
over. AC3 exists to convert the accident into a witnessed property.

**3. The epic's cloud-unreachable clause is wrong for the case that matters.** It reads:

> *"the declared metrics carry no value and quality `Stale`, exactly as at cold start — a rebirth
> never invents a reading."*

The intent is right and the letter is not. `birth()` has two paths (`:265-280`): a meter with **no**
reading gets `cold_start_metrics` — `Null(Double)` + `Stale`, exactly as the epic says. A meter that
**has** a reading gets it re-declared with `degrade(update.published)` and stamped with its own
`ValueDate`. Implementing the epic literally would blank the second case — deleting true history on
the grounds that the *cloud* is currently down, which has no bearing on whether the last reading was
real. It would also turn `a_rebirth_redeclares_what_is_known_instead_of_blanking_it`
(`sparkplug_publisher.rs:698`) red, for the right reason.

AC4 is written to the code's actual, and better, behaviour: **never invent, never upgrade, never
blank.** The rebirth answer is a re-announcement of what is known, degraded.

**4. `seq` "reset per the specification" is already true and worth not re-deriving.**
`LiveSession::rebirth` → `build_birth` calls `self.seq.reset()` (`encode.rs:130-131`, `:178-179`), and
two tests pin it: `rebirth_resets_the_numbering_after_data` and
`prop_rebirth_always_restarts_numbering_at_zero`. AC2 asserts it **through the NCMD path**, which is
new coverage; the encoder needs nothing.

### The three assertions in `chaos_ncmd_subscription` that this story inverts

This is the sharpest hazard in the story, and it is a test, not a document.

`crates/smartme-bridge/tests/chaos_ncmd_subscription.rs` publishes a real `Node Control/Rebirth` and
asserts the bridge **ignores** it, with a failure message that says answering *"would mean Story 4.7
was implemented here by accident"* (`:414-420`). Story 4.7 is not an accident, so three assertions
must be re-aimed:

| Line | Assertion | After this story |
| ---: | --- | --- |
| `:366-377` | log contains `"unrecognised NCMD ignored"` | must no longer fire **for the rebirth**; keep it, aimed at a genuinely unrecognised command |
| `:379-384` | log contains `"Node Control/Rebirth"` | still true, but no longer discriminating — the answer trace names it too |
| `:410-420` | no second NBIRTH follows | **inverted**: a second NBIRTH is now the requirement |

**And the trap inside the trap — the single most dangerous thing in this story.** That test's
`command_payload` helper (`:148-163`) builds metrics with `..Default::default()`, so `value: None`.
Under AC6 a valueless `Node Control/Rebirth` is **not** a conformant Rebirth Request and must not be
answered.

So `rebirth.is_none()` **stays green after a perfect implementation, and after a completely broken
one.** It cannot distinguish them. A dev who runs the suite, sees green, and concludes the rebirth
path works will ship a bridge that answers nothing — and the assertion's own failure message, which
warns about answering *too eagerly*, points the reader in the opposite direction from the bug.

Give the helper a value parameter, republish the rebirth case as boolean `true`, and invert the
assertion. **The same helper exists a second time, inline, at `mqtt_driver.rs:908-923`** — both need
it, and a fix to one is not a fix to the other.

### Decisions taken at drafting time (`CLAUDE.md` forbids deferring them)

**`CONTRACT_VERSION` 2 → 3.** The constant's own rule is unconditional: *"Bump on ANY change to the
topic grammar, to a metric name or unit, or to the meaning of a published quality code"*
(`sparkplug_publisher.rs:29-31`). This adds a metric name that a consumer sees as a new tag in its
browse tree, which is precisely the change the version exists to announce. It is additive — nothing is
removed or renamed — so the alternative (leave it at 2, on the grounds that the norm mandates the
metric and it is therefore not "our" contract) is arguable. **Rejected**, on three grounds: the rule
as written admits no exception; the bridge is not in production with no tag historisation started, so
the bump is free today and will not be later; and **the Tier-3 runbook's run table is indexed by
contract version** (`docs/ignition-contract-runbook.md:118-121`, `| Date | Ignition | Contract |
Result |`), so without a bump two rows both reading `v2` would attest to two different tag sets — the
pre-production run that finally exercises rebirth would be indistinguishable from the 2026-07-26 one.

**Approved by Guy, 2026-07-30.**

**Sharpen the rule while you are in there.** The reason this needed a decision at all is that the
constant's doc comment does not distinguish an **additive** change (a tag appears; nothing a consumer
holds becomes wrong) from a **breaking** one (v1 → v2: a quality a consumer already trusted changed
meaning). Both bump, and both should — but a reader cannot tell from the number which kind they are
looking at. Add that distinction to the comment, so the next person does not re-litigate this from
scratch. One paragraph, in the file the change is already touching.

**Match by name only; a `<alias N>` never matches.** `-rebirth-name-aliases` (`:957-961`) forbids an
NBIRTH from aliasing this metric *"to ensure that any Host Application connecting to the MQTT Server
is capable of requesting a rebirth without knowledge of any potential alias"*. We publish no aliases
at all (`encode.rs:240`), so no host can hold one for our metrics. Matching a configured alias would
add a path no conformant host can exercise and a way for an unrelated numeric alias to trigger a
birth. Rejected.

### The strict matcher and its one failure mode

**Answer only on boolean `true`** — `-ncmd-rebirth-value` (`:974-975`) defines the request that way,
and this repository settles Sparkplug questions by the norm. **Decided by Guy, 2026-07-30**, after the
alternative below was put to him.

*The rejected alternative, recorded because the risk it addressed is real and now has to be covered
another way.* A liberal matcher — treat anything but an explicit boolean `false` as a request — was
proposed on an asymmetry: a strict matcher that is wrong about the encoding **never fires, silently**,
and the bridge then reports FR19 as implemented with nothing observably wrong. That is this project's
signature failure shape (the contract-v1 quality codes; the four Epic 1 tests; the `bdSeq` tautology).
A liberal matcher that is wrong costs one idempotent extra birth. Guy chose the norm's reading, with
the Ignition verification moved to pre-production rather than guessed at now.

**So the residual risk is covered by two things instead, and neither is optional:**

1. **The near-miss trace.** Every metric named `Node Control/Rebirth` is logged with its datatype and
   value **as received**, whether or not it was answered. A strict matcher that does not fire then
   leaves the exact bytes that missed in the log, so the failure is diagnosable in one line instead of
   invisible. This is the part of the liberal proposal that survives, and it is what makes the
   pre-production run self-diagnosing.
2. **Story 4.8, in pre-production, exercising both directions.** North-bound (Ignition reads our
   values) is already covered by the Tier-3 gate; south-bound (Ignition commands us) is what rebirth —
   and later the meter relay — makes testable. Until that run happens, **4.7 claims conformance to the
   norm, not compatibility with Ignition.** Do not let the completion notes blur the two.

**Why a passive capture was not done first.** The plan was to record a real Ignition-issued request off
the production broker before writing the matcher. It does not work: MQTT Engine sends a Rebirth Request
only when it has a reason — DATA from a node whose BIRTH it never saw, or an out-of-order `seq`
(`tck-id-operational-behavior-host-reordering-rebirth`, `:565-568`) — and the bridge does not run
against that broker, so nothing provokes one. A passive window returns an empty transcript, which
`crates/smartme-bridge/tests/observe_primary_host_state.rs` documents at length as worse than no
observation at all.

**A hypothesis for 4.8 to settle, not to record as a finding now.** MQTT Engine may render a Rebirth
control only for a node that *declared* the metric. If so, the absence AC1 fixes is itself why no
request has ever arrived, and ADR 0016's *"every one is answered with a log line"* describes a flow
that has never occurred. Reasoning, not measurement.

**No rate limit on answers.** See Task 4.

**Keep the answer inline and synchronous.** See Task 3. This is what makes AC3 true.

### This handler is a template, and the next command is not a rebirth

Guy's stated plan (2026-07-30): verify against Ignition **in pre-production**, by adding a **meter
relay command** — so the Tier-3 gate can exercise the contract *in both directions* rather than only
north-bound. Two consequences for how this story is built.

**1. The shape you choose here is inherited by a command that switches physical hardware.** A rebirth
is idempotent and harmless; a relay is neither. Keep the three concerns separable — *recognise*
(classify bytes), *trace* (say what arrived), *act* (do the thing) — so the relay path can add an
authorisation or confirmation step between the second and the third without restructuring. Do not fuse
recognition and action into one match arm because there is currently only one command.

**2. A relay is a writable output on a Device, so it is DCMD, not NCMD — and that reopens a deliberate
`n/a`.** Story 4.6 declined to add `MessageType::DCmd` because
`tck-id-message-flow-device-dcmd-subscribe` (`:403-407`) is conditional on *"if the Device supports
writing to outputs"* and no device here does. The matrix records `-device-dcmd-subscribe` and
`topics-dcmd-topic` as `n/a` **on that condition**, not permanently. The condition is now scheduled to
hold.

**Do not add `DCmd` in this story** — that is a separate story with its own subscribe clause, its own
topic grammar and its own safety argument. But the `n/a` verdicts are now **time-limited**, and a
verdict whose stated condition is about to flip should say so rather than being discovered later.
Record that in the matrix as part of Task 7, citing
**[#38](https://github.com/guycorbaz/smartme_mqtt/issues/38)**, which owns the expiry.

### Existing code you must read before writing anything

- `crates/smartme-bridge/src/app/mqtt_driver.rs` — the whole file, but specifically:
  - `:1-72` module docs. The boot order is *"not negotiable"* and lists six steps; a rebirth is a
    **seventh** path that reaches step 6 without steps 1–5. If the docs stay silent about it the next
    reader will believe a BIRTH only ever follows a CONNACK.
  - `:253-261` `Inbound` — extend here; its doc comment is false after this story.
  - `:275-281` `metric_label` — the alias rendering AC6 depends on.
  - `:288-294` `classify` — never `.expect()` a decode; a malformed payload from the network is an
    ordinary input.
  - `:353-383` `trace_command_outcome` — extracted precisely so the trace can be tested.
  - `:452-472` the `Transport::Connected` arm — the birth block to share.
  - `:486-499` the command arm — where the answer goes.
- `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs`
  - `:29-39` `CONTRACT_VERSION` and its bump rule.
  - `:189-192` `new_session` — **the function this story must not call.** Nothing calls it today.
  - `:223-293` `birth()` — both session arms, the validate-everything-first ordering, and the
    known-reading vs cold-start split that AC4 is written from.
  - `:340-347` `contract_metric` — the shape the Rebirth metric follows.
- `crates/sparkplug-b/src/encode.rs:126-132`, `:178-190` — `rebirth` → `build_birth`: `seq` reset,
  `bdSeq` prepended. Read it before assuming anything about numbering.
- `crates/smartme-bridge/tests/arch_purity.rs:90-91` — `NAMING_BANNED_IN_MQTT` and the `.step(` ban.
  Not to be edited.
- `crates/smartme-bridge/tests/chaos_ncmd_subscription.rs` — the harness to reuse and the assertions
  to re-aim.
- `crates/smartme-bridge/src/app/supervisor.rs:129-142` — the shutdown path. Note the Story 4.6
  finding: the mqtt task is only awaited **after** shutdown, so a panicked driver leaves
  `try_wait().is_none()` true. A liveness assertion built on the child process alone cannot see a dead
  driver task.

### The passages this story falsifies (AC7)

Produced mechanically: `grep -rn "no command\|not answer\|nothing acts on\|does not implement\|is ignored"`
over `docs/`, `crates/*/src/`, `_bmad-output/planning-artifacts/`, excluding the pinned spec, then
**reading the neighbourhood of every hit** — which is where the Story 4.6 review found three passages
no keyword search could reach.

| File | Line | What it says | Note |
| --- | ---: | --- | --- |
| `docs/manual/chapters/05-mqtt-sparkplug-contract.tex` | 238 | *"\prog implements no command at all"* | |
| `docs/manual/chapters/05-…tex` | 314 | *"No command is honoured, `Node Control/Rebirth` included"* | Known limitations |
| `docs/manual/chapters/05-…tex` | 334 | the repair *"which \prog does not implement"* | |
| `docs/manual/chapters/02-understanding-sparkplug.tex` | 533 | *"An edge node that does not implement … cannot be repaired"* | generic prose — likely **still true**; confirm |
| `docs/manual/chapters/02-…tex` | 644 | *"on this installation that safety net is absent"* | |
| `docs/manual/chapters/02-…tex` | 683-684 | capability table: `Node Control/Rebirth` **absent** | amended by 4.6 already; changes again |
| `docs/manual/chapters/02-…tex` | 699 | *"reaches \prog but is ignored"* | |
| `docs/manual/chapters/02-…tex` | 677 | Report by exception **absent** | **confirm still-true** — RBE is not implemented here |
| `docs/sparkplug-conformance.md` | 514-516, 518-520 | six chapter-5 rows | verdicts move |
| `docs/sparkplug-conformance.md` | 930 | `payloads-nbirth-rebirth-req` row | verdict moves |
| `docs/sparkplug-conformance.md` | 1027-1030 | *"The unanswered command path is not hidden by any of this"* | |
| `docs/sparkplug-conformance.md` | 1241 | *"there is still no `Node Control/Rebirth` metric and nothing answers a command"* | live *Findings carried forward* table — the exact row shape that survived the 4.6 sweep |
| `docs/sparkplug-conformance.md` | 1352 | *"nothing that acts on what arrives, so `-rebirth-action-1/2/3` are untouched"* | |
| `docs/sparkplug-conformance.md` | 308, 435-448, 552-559 | four prose passages scoping 4.7 | |
| `docs/adr/0016-rebirth-before-primary-host-wait.md` | 62 | *"still answers no command"* | **evidence in a decision** |
| `docs/adr/0016-…md` | 98 | *"A Rebirth that arrives and is ignored repairs exactly as much as one that never arrives"* | **the load-bearing premise — it expires here** |
| `docs/adr/0016-…md` | 107 | *"Until 4.7 lands, every one of them is answered with a log line"* | |
| `docs/primary-host-state-observation.md` | 304-305 | *"There is no command path to lose"* | reworded once already by the 4.6 review |
| `docs/primary-host-state-observation.md` | 417 | *"that arrives and is ignored repairs exactly as little"* | **the same premise, second copy** |
| `crates/smartme-bridge/src/app/mqtt_driver.rs` | 249-252 | *"The bridge recognises NO command yet"* | |
| `crates/smartme-bridge/src/app/mqtt_driver.rs` | 379 | trace text *"this bridge implements no command yet"* | changing it **breaks the chaos test's needle** — do both |
| `crates/smartme-bridge/src/app/mqtt_driver.rs` | 1-72 | module docs: six-step boot order, silent on rebirth | |
| `crates/sparkplug-b/src/lib.rs` | 26-33 | Conformance scope: Rebirth metric *"not implemented"* | about the **crate** — probably confirm, not amend |
| `_bmad-output/planning-artifacts/prd.md` | 68, 87, 97, 107, 212, 246, 360 | *"responds to Ignition NCMD/Rebirth"* | **specification that becomes true** — confirm |
| `_bmad-output/planning-artifacts/prd.md` | 149 | RBE *"cannot safely do so until it answers NCMD/Rebirth (Stories 4.6–4.7)"* | blocker lifts; **do not implement RBE** |
| `_bmad-output/planning-artifacts/epics.md` | 114 | NFR17 note — *"the NCMD/Rebirth half is Epic 4"* | Story 4.8 still owns closing it; confirm |
| `_bmad-output/planning-artifacts/epics.md` | Story 4.7 | the three original ACs | amended, per above |

**Two of these are the same premise in two files** (ADR 0016 `:98` and
`primary-host-state-observation.md:417`). The 4.6 review already had to correct one copy of an
argument in both places; the pair travels together. Amend both, and re-run the argument rather than
re-wording it: **once a Rebirth is answered, the reason for sequencing 4.5 after 4.7 is spent, and
Story 4.5 must be re-decided on its own evidence.**

### Deployment facts that constrain this story

- **The subscription is already live on the production broker**, where MQTT Engine v5.0.0-rc1 sends
  real Rebirth requests that are currently answered with a log line (ADR 0016 `:107`). When this
  lands, the **next** deployment starts answering them for real. That is the intent, and it is also
  the first time this bridge acts on anything an external system sends it.
- **Use the testcontainers broker for every test.** Never aim a chaos test at the LAN broker unasked —
  Ignition is live on it.
- **Not in production, no historisation started**, which is what makes the `CONTRACT_VERSION` bump
  free today.
- **The broker is unauthenticated**, so any client can publish a Rebirth Request. That is not a new
  exposure — the same client can already publish a forged NDEATH, which lies immediately, whereas a
  forged rebirth costs one extra birth (the deferred item in `deferred-work.md`, 2026-07-29). Worth a
  sentence in the manual's Known limitations, not a mechanism.

### Testing standards

- Unit tests inline under `#[cfg(test)]`; integration tests in `crates/smartme-bridge/tests/`.
- No raw time: `Instant::now()` / `SystemTime::now()` live only in `core/clock.rs`, `arch_purity`
  enforces it, inline test modules included.
- **Falsification is mandatory and recorded next to the test.** If a mutation cannot be made to fail,
  say so — do not claim the AC.
- **Assert against decoded bytes, not against the builder's own expression.** #30 lists eight
  invariants currently "proved" by comparing production code with itself.
- **Log-level check:** AC2 and AC6 are written in terms of what an operator sees. `main.rs` now sets
  an explicit default directive of INFO (Story 4.6 review), so INFO traces are visible without
  `RUST_LOG` — **confirm that is still the case** rather than assuming; a test that sets `RUST_LOG`
  itself cannot notice a regression here. See the Story 4.6 review's finding D1.
- `./scripts/ci-local.sh` before pushing, then `gh run list`. Never pipe it; write its log to an
  absolute path and read the `EXIT=` line.

### Project Structure Notes

- The handler belongs in `app/mqtt_driver.rs`; the metric belongs in
  `adapters/sparkplug_publisher.rs`. Nothing belongs in `core/` or `domain/`.
- **The published `sparkplug-b` crate should need no change.** `MessageType::NCmd` exists; the
  Rebirth metric is an application declaration built from the crate's primitives. If you find
  yourself editing the crate, stop and ask whether the change is generic — `no_context_leak` guards
  the boundary but not the judgement.
- The manual documents implemented behaviour, so it changes **in this story**.

### References

- [Source: `docs/spec/…/Sparkplug_5_Operational_Behavior.adoc:943-988`] — the whole Rebirth section:
  the four NBIRTH-metric clauses, the three request clauses, and the three action clauses
- [Source: `docs/spec/…/Sparkplug_4_Topics.adoc:215-219`] — `tck-id-topics-nbirth-rebirth-metric`
- [Source: `docs/spec/…/Sparkplug_6_Payloads.adoc:1082-1084`] — `tck-id-payloads-nbirth-rebirth-req`
- [Source: `docs/sparkplug-conformance.md:514-523, 552-559, 930, 1241`] — the seven rows this story
  owns and the prose that scopes them
- [Source: `_bmad-output/planning-artifacts/epics.md#Story 4.7`] — the three original ACs and the RBE
  note
- [Source: `_bmad-output/planning-artifacts/prd.md:292`] — FR19
- [Source: `docs/adr/0016-rebirth-before-primary-host-wait.md`] — why 4.7 precedes 4.5, and the
  premise that expires when it lands
- [Source: `_bmad-output/implementation-artifacts/4-6-ncmd-subscription-plumbing.md`] — the
  subscription, its review findings, and the AC5 per-passage table this story's AC7 repeats
- [Source: `CLAUDE.md`] — read the norm first and cite the tck-id; falsify before trusting; decide at
  drafting time; amend the PRD, epics and manual together

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-07-30.

### Debug Log References

- Falsification runs: every mutation below was applied to the working tree, the affected test run,
  the output recorded, and the mutation reverted. `git diff --stat` confirmed clean reverts for
  `crates/sparkplug-b/` and `crates/smartme-bridge/tests/arch_purity.rs`, neither of which this
  story edits.
- `./scripts/ci-local.sh` (not `--fast`), log written to an absolute path and the `EXIT=` line read
  from the file rather than from a reported exit code.

  **That precaution paid, twice in this session.** The harness reported the run as *"completed (exit
  code 0)"* on two occasions when the log's own `EXIT=` line read **1** and then **101**. The
  reported code described the trailing `echo`, not the CI. Both were real failures:
  - `EXIT=1` — `cargo fmt --check`. Fixed with `cargo fmt --all`, which then **moved every anchor in
    `mqtt_driver.rs` a second time**, so all 14 conformance-matrix citations had to be re-pointed
    again and re-verified by printing the line each one names.
  - `EXIT=101` — `clippy::doc_lazy_continuation` on a `3b.` doc-list item added to
    `chaos_ncmd_subscription`'s module docs. Renumbered to `4.`/`5.`, which then broke two internal
    *"property 4"* cross-references in the same file; both repaired.

  - `EXIT=101` again — **`no_context_leak` caught my own documentation.** Task 5 asked for that guard
    to be run because *"nothing here should reach the published crate at all, which is itself worth
    confirming"*. It confirmed it by failing: the *confirmed-unchanged* note I added to
    `sparkplug-b/src/lib.rs` named the bridge by its application name, leaking
    `smartme`/`SMARTME_` into a crate that is meant to be publishable standalone. Reworded to
    describe *"a downstream application"* without naming it.

    Worth recording rather than fixing quietly: the leak was in a note whose entire purpose was to
    say the crate had **not** been contaminated by this story. A prose guard cannot catch that; the
    mechanical one did, and it is the only reason the crate boundary held.

  None of these would have been caught by trusting the reported exit code, and the last two would
  have gone to CI red.

### Falsification table — every assertion, run against deliberately broken code

`CLAUDE.md`: *a test written against already-correct code proves nothing by passing.*

| # | Mutation | Target | Result |
| ---: | --- | --- | --- |
| 1 | rebirth metric removed from `node_metrics` | `every_node_birth_declares_the_rebirth_command` | RED — *"NBIRTH #0 … declares no Node Control/Rebirth metric; it carries \["bdSeq", "Contract/Version"\]"* |
| 2 | rebirth metric removed from the `Session::Live` arm ONLY | same | RED — **on NBIRTH #1 only.** The case a single-birth test would have missed, and the reason the test asserts both arms |
| 3 | `Null(DataType::Boolean)` instead of `Boolean(false)` | same | RED — *"-rebirth-value is a MUST on the value false"*, `left: None` |
| 4 | `Boolean(true)` instead of `false` | same | RED — `left: Some(BooleanValue(true))` |
| 5 | `encode_metric` emits `alias: Some(1)` | same | RED — *"-rebirth-name-aliases forbids an alias here"* |
| 6 | value check dropped from `classify` (name alone matches) | `a_rebirth_request_is_the_name_and_the_value_never_the_name_alone` | RED on the `false`, valueless and `IntValue` cases; **GREEN on the `true` case** — the asymmetry is the point |
| 7 | `classify` requires a single-metric payload | `a_rebirth_request_carried_alongside_other_metrics_is_still_a_request` | RED, and only that test |
| 8 | `?received` dropped from the near-miss trace | `the_near_miss_records_the_datatype_and_value_as_received` | RED — and every classification test stays GREEN, which is why this needs its own assertion |
| 9 | `classify` filter widened to `\|\| m.alias.is_some()` | `an_alias_addressed_metric_is_never_a_rebirth_request` | RED, **plus** the Story 4.6 alias trace test — the two guard the same boundary from opposite sides |
| 10 | the three trace arms given one shared message | `an_answered_a_missed_and_an_unknown_command_do_not_read_alike` | RED |
| 11 | the `Inbound::Rebirth` arm deleted (classified, traced, never acted on) | `chaos_ncmd_rebirth` | RED — *"no second NBIRTH followed the Rebirth Request"* |
| 12 | `publisher.new_session()` inserted before the answer | `chaos_ncmd_rebirth` (AC5) | RED — *"the answer opened a NEW session"*, `left: Some(2)` / `right: Some(1)` — bdSeq **1 → 2**. **Re-run 2026-07-31 by the code review**, which found the test file's copy of this row saying `0 → 1`; this value is the one the re-run printed |
| 13 | **the answer deferred behind a flag consumed one message later** | `chaos_ncmd_rebirth` (AC3) | **RED — *"1 DATA message(s) were published inside the birth sequence"*.** The hard one; see below. **The code review found this RED was probabilistic**: the window was ~0 messages wide, so the mutation passed whenever it completed inside one 20 ms tick. The window was rebuilt (feeder stopped, one reading pushed 50 ms after the request) and the limits of what it catches are now stated at the call site |
| 14 | the DDATA feeder stopped (premise mutation) | `chaos_ncmd_rebirth` | RED — *"no DATA was flowing before the request"*: the anti-vacuity guard reports itself instead of the AC passing quietly |
| 15 | `classify` matching on the name alone | `chaos_ncmd_subscription` | RED — *"a Node Control/Rebirth carrying no value was not reported as a near miss"*. **Not the assertion predicted**: the near-miss check runs earlier than the no-NBIRTH one. Recorded as it ran |
| 16 | the `Inbound::Rebirth` arm deleted | `chaos_ncmd_subscription` | RED — *"a conformant Node Control/Rebirth (boolean true) produced no NBIRTH"* |
| 17 | `main.rs` reverted to `tracing_subscriber::fmt::init()` | `chaos_ncmd_subscription` | RED at the FIRST INFO assertion. Before this story the same mutation left the file green |

**AC3 was the assertion at risk of being vacuous, and it took a different test design to avoid it.**
Every other chaos test spawns the binary against TEST-NET-1, so no reading is ever fetched and **no
DDATA is ever published**. An assertion that no DATA interleaves the birth sequence would have held
over an empty stream — green forever, meaning nothing, and green against every mutation that breaks
the clause. `chaos_ncmd_rebirth` therefore drives `mqtt_driver::run` in-process and feeds it a real
reading every 20 ms; the observation is still external (real broker, independent subscriber). It
asserts the stream is flowing **before** it asserts what is absent from it. Mutation 13 confirms the
window is real.

### Falsification table — the code review's own mutations (2026-07-31)

Eleven mutations, each applied to the working tree, the affected test run, the output copied from the
run rather than reconstructed, and the mutation reverted. `sha256sum -c` over all 43 source files
confirmed the tree was byte-identical before and after the three read-only review layers.

| # | Mutation | Target | Result |
| ---: | --- | --- | --- |
| R1 | the `retained` check dropped from `classify`'s `true`-value branch (what shipped) | `a_retained_ncmd_is_a_replay_and_never_a_rebirth_request` | RED — *"a retained NCMD must be reported as a replay, not answered"* |
| R2 | the nearly-name detection branch disabled | `a_name_that_only_nearly_matches_is_a_near_miss_not_an_unknown_command` | RED on the first case — *"`\"node control/rebirth\"` must be DETECTED as a near miss"* |
| R3 | `.take(MAX_TRACED_METRICS)` removed from `as_received` | `a_hostile_metric_count_is_capped_and_the_line_says_it_capped` | RED |
| R4 | *(first attempt)* `datatype` removed inside `as_received` | `the_near_miss_records_the_datatype_and_value_as_received` | **GREEN — and the mutation was wrong, not the test.** That test builds its `Inbound` directly and never calls `as_received`. Recorded because a mutation that misses its target proves nothing, and reporting it as a falsification would be the defect this review exists to find |
| R4b | the near-miss trace rendered as values only (`datatype` and `name` dropped from the line) | same | RED — and its own evidence: the captured line began `2026-07-30T12:00:33.211428Z`, **three `3`s before the message starts**, which is why the shipped `log.contains('3')` needle could not fail |
| R5 | `ignored_alongside` dropped from the `Rebirth` trace arm | `a_rebirth_request_carried_alongside_other_metrics_is_still_a_request` | RED — *"a discarded command must appear in the log even when a recognised one travelled with it"* |
| R6 | the three near-miss clause messages collapsed into one | `an_answered_a_missed_and_an_unknown_command_do_not_read_alike` | RED |
| R7 | the drop count removed from `publish_all` (what shipped) | `a_partly_published_birth_is_counted_and_never_reported_complete` | RED — *"one slot means one message queued and two dropped"*. **Before this test the same mutation left the whole suite green**, which is why `announce` calling a partial sequence *"complete"* survived review-by-reading |
| R8 | `METRIC_NODE_CONTROL_REBIRTH` misspelt as `"Node Control/Reburth"` | `every_node_birth_declares_the_rebirth_command` | RED — `left: "Node Control/Reburth"`, `right: "Node Control/Rebirth"`. Every other assertion in that test stayed green, because they all locate the metric with the same constant the producer uses |
| C1 | the rebirth metric removed from the `Session::Live` arm ONLY | `chaos_ncmd_rebirth` | RED — *"the ANSWERING NBIRTH declares no Node Control/Rebirth metric; it carries \["bdSeq", "Contract/Version"\]"*. Before this review the chaos tier checked only the FIRST birth, while `-nbirth-rebirth-req` binds *"Every NBIRTH"* |
| C2 | DATA publication latched off permanently after the answer | `chaos_ncmd_rebirth` | RED — *"no DATA was published after the birth sequence completed"*. **A total, silent loss of the bridge's purpose, triggerable by any client — and green against every assertion in the file before this review** |
| C3 | the retained guard removed | `chaos_ncmd_subscription` | RED |
| M12 | `publisher.new_session()` before the answer — **re-run**, because the two existing records disagreed | `chaos_ncmd_rebirth` | RED — `left: Some(2)` / `right: Some(1)`, i.e. bdSeq **1 → 2**. The story's table was right; the test file's `0 → 1` was not, and only one experiment had ever been run |

**Two premise failures, both recorded rather than smoothed over.**

1. **The retained-NCMD chaos assertion failed against correct code**, twice, before the test was
   right. Under MQTT 3.1.1 a broker sets the retain flag on *delivery* only when the message answers a
   new subscription; a live delivery to an already-subscribed client carries `retain = 0` whatever the
   publisher asked. So the first version exercised the live path — where the flag is legitimately
   absent — and read the (correct) answer as a defect. The second version snapshotted the answer count
   before that legitimate live answer had landed. The property is now provoked the way the real
   exposure is: publish retained, force a reconnect, let the SUBSCRIBE draw the replay. Both halves are
   asserted, because both are correct behaviour and the asymmetry IS the rule.
2. **The all-or-nothing birth drain the review asked for is not implementable against `rumqttc` 0.25.**
   Refusing to publish unless the whole sequence fits requires knowing how many channel slots are free;
   `AsyncClient` wraps a private `flume::Sender` and exposes no `capacity()`, `EventLoop::requests_tx`
   is `pub(crate)`, and no constructor accepts a receiver we made. Blocking the drain instead would
   hold the driver's `select!` — and its shutdown branch — across an arbitrary broker outage. What
   shipped is the strongest achievable version: every message is attempted, the drops are **counted**,
   and the *"complete BIRTH sequence republished"* line is emitted only when that count is zero. The
   residual gap is stated at the call site rather than hidden: for the sequence not to fit, 63 of 64
   slots must be backed up, which means the broker is not draining and the will is about to fire
   anyway — and a host that gets an NBIRTH without its DBIRTH is exactly the condition that makes it
   send a Rebirth Request, which this bridge now answers.

**The manual's overfull box was confirmed by building it, not by reading.** It was written up as a
deferred item on the grounds that an overfull hbox is a warning and `latexmk` exits 0 regardless. The
build then produced `Overfull \hbox (17.75223pt too wide)` on chapter 5's NCMD table, under exit code
0 — so the item was closed by fixing it rather than deferred. This repository's own rule about exit
codes describing something other than what was measured applied here to a document.

### Completion Notes List

**All seven acceptance criteria are met.** 17 falsification mutations, all red. No test was claimed
without being made to fail first.

- **AC1** — `Node Control/Rebirth`, Boolean, `false`, no alias, on **every** NBIRTH. Built once in
  `node_metrics` and used by both session arms, so the two-call-site omission the task warned about
  is removed structurally rather than tested for; the test still asserts both arms.
  `CONTRACT_VERSION` 2 → 3, and its doc comment now distinguishes **additive** from **breaking**
  bumps, which is what had to be re-derived to make the decision.
- **AC2** — recognised on name **and** boolean `true`; answered with NBIRTH (`seq = 0`) + one DBIRTH
  (`seq = 1`); traced at INFO naming the topic, asserted **with no `RUST_LOG` set**.
- **AC3** — met, and witnessed rather than argued. See the note above.
- **AC4** — met by the existing `birth()` behaviour, which the epic's own criterion would have
  broken. No code change was needed and `a_rebirth_redeclares_what_is_known_instead_of_blanking_it`
  still passes untouched.
- **AC5** — `new_session()` is not called; asserted **through the NCMD path**, comparing the answer's
  `bdSeq` against the first NBIRTH's, both read from the transcript. Never a constant against itself.
- **AC6** — the norm's strict reading, with the near-miss trace carrying datatype and value as
  received. The value rendering is capped at 200 chars **and says when it capped**: the broker is
  unauthenticated, so an unbounded string value is a log-flooding path.
- **AC7** — the per-passage table below.

**What the mechanical grep missed, again.** The story's list was produced by grep plus neighbourhood
reading, and it was still a floor. Three passages it did not contain:

1. **The manual announced `Contract/Version` "Currently 2" in two places** (chapter 5 `:16` and the
   metric table `:83`) — made wrong by this story's own bump, and in the one chapter that is the
   authority on what the product does.
2. **The matrix's RBE ground 3** (`:187`) — *"The bridge answers none (Stories 4.6/4.7)"* — a third
   copy of the premise the story flagged as existing in two places.
3. **The metric table itself** had no row for the new metric. A table of what the BIRTH carries that
   omits what the BIRTH now carries is wrong by silence, which no grep for a false sentence reaches.

**Two things found by following the change rather than the list.**

- **Every `mqtt_driver.rs` line citation in the matrix went stale again**, exactly as the Story 4.6
  review found. This story moved ~600 lines in that file. All 14 citations were re-pointed and each
  was verified by printing the line it now names.
- **`chaos_ncmd_subscription` had a latent defect that only this story's addition exposed.**
  `drop(evictor)` does not stop the eviction: the spawned task still owns the `EventLoop` and
  `rumqttc` reconnects internally, so it kept re-taking the bridge's client id roughly once a second
  for the rest of the run. That was invisible while the test ended immediately afterwards. Adding a
  command at the end made it visible — the bridge's subscription was being torn down before the
  request arrived, and the NBIRTH that showed up was a **reconnect** birth. **The presence of an
  NBIRTH could not distinguish the two; the `"Rebirth Request accepted"` trace assertion is what
  caught it**, which is the argument for asserting the answer by its own trace and not by its wire
  effect. Fixed by aborting the evictor's task.

**A decision NOT taken, deliberately.** RBE is not implemented. The blocker recorded in the PRD, the
epic and the matrix has lifted, and all three now say so — but the verdict is unchanged and its
*reason* moved from "cannot safely be changed" to "has not been decided". Those are different states
and collapsing them would hide a live decision behind a stale excuse. The residual argument is real:
the repair is **host-initiated**, so a consumer that never asks still never learns. #32 owns it.

**Scope held.** No `DCMD`, no relay command, no RBE, no comment on #32 (an outward action). The
published `sparkplug-b` crate is unchanged — verified by `git diff --stat`; only its doc comment
gained a *confirmed-unchanged* note, because "still true" and "nobody re-read this" look identical
in a diff.

**What this story does NOT claim.** Conformance to the norm, not compatibility with Ignition. No
Rebirth request from a real MQTT Engine has been observed or answered. Story 4.8 owns that, and
there is a hypothesis for it to settle: MQTT Engine may render a Rebirth control only for a node
that *declared* the metric — in which case the absence AC1 fixes is itself why no request ever
arrived, and ADR 0016's *"every one is answered with a log line"* describes a flow that has never
occurred. Reasoning, not measurement; recorded in the ADR as such.

### AC7 — the per-passage table

Disposition is **amended**, **confirmed still-true**, or **superseded-in-place** (kept for the
record, marked so it cannot be quoted as live).

| File | Passage | What it said | Disposition |
| --- | --- | --- | --- |
| `docs/manual/chapters/05-…tex` | `:16` | `Contract/Version` *"currently **2**"* | **amended** → 3, plus the additive/breaking distinction. **Not on the story's list** |
| `docs/manual/chapters/05-…tex` | version table | rows for 2 and 1 only | **amended** — row 3 added, `Kind` column added |
| `docs/manual/chapters/05-…tex` | `:83` metric table | no `Node Control/Rebirth` row | **amended** — row added. **Not on the story's list**; wrong by silence |
| `docs/manual/chapters/05-…tex` | `:238` | *"\prog implements no command at all"* | **amended** — one command is honoured; table rewritten with the answered and near-miss rows |
| `docs/manual/chapters/05-…tex` | `:269` | *"session with no command path and without retrying"* | **confirmed still-true** — this is the queue-full-at-CONNACK path, unaffected |
| `docs/manual/chapters/05-…tex` | `:314` | *"No command is honoured"* | **amended** → *"Only one command is honoured"*; a DCMD sentence and the unauthenticated-broker note added |
| `docs/manual/chapters/05-…tex` | `:334` | the repair *"which \prog does not implement"* | **amended, and the conclusion re-run** — the repair now works, but is host-initiated; a host that does not ask still sees uninterpretable DATA |
| `docs/manual/chapters/05-…tex` | `:8-12` | *"implemented and tested as of Epic 1"* | **amended** → Epic 4 |
| `docs/manual/chapters/05-…tex` | "A rebirth never promotes a value" | *"On reconnect the bridge re-declares its tags"* | **amended** — both paths, one code path |
| `docs/manual/chapters/02-…tex` | `:533` warning | *"An edge node that does not implement … cannot be repaired"* | **amended** — the generic sentence is protocol prose and stands; the claim about \prog that followed it did not |
| `docs/manual/chapters/02-…tex` | `:644` | *"on this installation that safety net is absent"* | **confirmed still-true, with an addition** — Mosquitto is still not Aware; the rebirth command now exists but must be exercised by the host |
| `docs/manual/chapters/02-…tex` | `:677` RBE row | Report by exception **absent** | **confirmed still-true** — RBE is not implemented. The stated *blocker* is amended |
| `docs/manual/chapters/02-…tex` | `:683-684` | capability table: `Node Control/Rebirth` **absent** | **amended** → implemented; NCMD row reworded; a DCMD row added |
| `docs/manual/chapters/02-…tex` | `:699` warning | *"the protocol's remaining repair … reaches \prog but is ignored"* | **amended, argument re-run** — one of two gaps closed; the remaining exposure is re-shaped, not removed, and the planning consequence is stated |
| `docs/sparkplug-conformance.md` | `:514-516` | `-rebirth-name`, `-datatype`, `-value` | **amended** — `gap (unimplemented)` → **conformant**, evidence and mutations named |
| `docs/sparkplug-conformance.md` | `:517` | `-rebirth-name-aliases` | **confirmed still-true as `n/a`** — checked, not assumed. The metric exists and still carries no alias; the clause is conditional on aliases being used |
| `docs/sparkplug-conformance.md` | `:518-520` | `-rebirth-action-1/2/3` | **amended** → **conformant** |
| `docs/sparkplug-conformance.md` | `:930` | `payloads-nbirth-rebirth-req` | **amended** → **conformant** |
| `docs/sparkplug-conformance.md` | chapter-5 tally | `23 · 2 · 25 · 49` | **amended** → `29 · 2 · 19 · 49`; `23 + 6 = 29`, `25 − 6 = 19`, sums to 99 |
| `docs/sparkplug-conformance.md` | chapter-6 tally | `30 · 5 · 15 · 59` | **amended** → `31 · 5 · 14 · 59`; sums to 109. *"30 conformant is 29 distinct"* → 31/30 |
| `docs/sparkplug-conformance.md` | total | `72 · 8 · 50 · 144`, *274 of 303* | **amended** → `79 · 8 · 43 · 144`. **274 of 303 is unchanged** — verdicts moved, no row was added |
| `docs/sparkplug-conformance.md` | chapter-4 tally | `15 · 0 · 5 · 21`, *41 of 70* | **confirmed still-true, deliberately untouched** — `topics-nbirth-rebirth-metric` has no row; **Story 4.19** owns it. Evidence recorded for 4.19 to cite |
| `docs/sparkplug-conformance.md` | gap breakdowns | *"25 gaps"* / *"15 gaps"* | **amended** → 19 and 14, with the rebirth clauses removed from both lists |
| `docs/sparkplug-conformance.md` | `:187` RBE ground 3 | *"The bridge answers none (Stories 4.6/4.7)"* | **amended** — ground spent. **Not on the story's list**: a third copy of the premise |
| `docs/sparkplug-conformance.md` | `:192` revisit condition | *"when Story 4.7 lands, this deviation must be re-examined"* | **discharged** — re-examined, RBE not implemented, reason restated, new revisit condition set |
| `docs/sparkplug-conformance.md` | `:353` ncmd-subscribe row | *"the responding is `-rebirth-action-1/2/3`, still `gap`"* | **amended** — all three conformant |
| `docs/sparkplug-conformance.md` | `:450`, `:1394` | *"before Story 4.7 gives any command meaning"* | **amended** — past tense; every other command is still ignored on unchanged paths |
| `docs/sparkplug-conformance.md` | `:552-559` | *"Story 4.7's work is a caller, not an encoder"* | **amended, and its error recorded** — the prediction held for the crate, but the prose implied 4.7 was *only* a caller; five MUST clauses about the NBIRTH metric were also unmet. The clause rows had it right and the summary did not |
| `docs/sparkplug-conformance.md` | `:1027-1030` | *"The unanswered command path is not hidden by any of this"* | **amended** — answered; DCMD expiry recorded |
| `docs/sparkplug-conformance.md` | `:1241`, `:1256`, `:1676` | three *Findings carried forward* rows | **amended** — struck through and closed; the `:1256` row's own error (only the handler was missing) recorded |
| `docs/sparkplug-conformance.md` | `-device-dcmd-subscribe`, `topics-dcmd-topic` | `n/a` / `gap` on the writable-output condition | **annotated ⏳ time-limited**, citing [#38](https://github.com/guycorbaz/smartme_mqtt/issues/38). **Not re-verdicted** — pre-dating a verdict is as wrong as missing one |
| `docs/sparkplug-conformance.md` | 14 `mqtt_driver.rs:NNN` citations | line numbers from before this story | **amended** — all re-pointed and each verified by printing the line it names. **Not on the story's list**; the same rot the 4.6 review found |
| `docs/adr/0016-…md` | `:62` | *"still answers no command"* | **amended** — superseded again by 4.7 |
| `docs/adr/0016-…md` | `:98` | *"A Rebirth that arrives and is ignored repairs exactly as much as one that never arrives"* | **superseded-in-place** — the load-bearing premise. Marked inline so it cannot be quoted as live, and a *What Story 4.7 changed* section states the argument is **spent** and 4.5 must be re-weighed |
| `docs/adr/0016-…md` | `:107` | *"Until 4.7 lands, every one of them is answered with a log line"* | **amended** — and flagged as possibly describing a flow that never occurred (the 4.8 hypothesis) |
| `docs/adr/0016-…md` | Consequences | the #32 instruction | **discharged**, plus a new consequence: this ADR's argument is spent |
| `docs/primary-host-state-observation.md` | `:304` | *"There is no command path to lose"* | **amended** — second rewording; the bridge now acts on one command |
| `docs/primary-host-state-observation.md` | `:331` | *"The bridge implements no Rebirth handling"* | **amended** — leg now false; the measurement itself untouched |
| `docs/primary-host-state-observation.md` | `:341` | *"the two gaps compound"* | **amended to past tense**, shape preserved because it is the reasoning that produced ADR 0016 |
| `docs/primary-host-state-observation.md` | `:413-417` | the three-leg table and *"the third leg alone is sufficient"* | **superseded-in-place with an ⏹ EXPIRED box** — the document predicted this moment and said to read it after 4.7 lands. All three legs resolved; the conclusion does not stand; 4.5 re-weighed |
| `docs/primary-host-state-observation.md` | `:433-436` | *"the argument for that ranking is now stronger"* | **superseded-in-place** — kept as a record of what was believed when the ordering was acted on |
| `crates/sparkplug-b/src/lib.rs` | `:26-33` | Conformance scope: Rebirth metric *"not implemented"* | **confirmed still-true, and said so in the file** — the sentence is about the CRATE, which still supplies neither the metric nor the handler; the bridge does. Story 4.7 changed nothing here |
| `crates/sparkplug-b/src/topic.rs` | `:294` | *"DCMD is a separate, conditional clause this bridge does not implement"* | **confirmed still-true** — no DCMD was added |
| `crates/smartme-bridge/src/app/mqtt_driver.rs` | `:249-252` | *"The bridge recognises NO command yet"* | **amended** — `Inbound` doc rewritten around recognise / trace / act |
| `crates/smartme-bridge/src/app/mqtt_driver.rs` | `:379` trace text | *"this bridge implements no command yet"* | **amended** → *"implements Node Control/Rebirth and no other command"*. **The chaos test's needle `"unrecognised NCMD ignored"` is preserved verbatim** — both were changed together |
| `crates/smartme-bridge/src/app/mqtt_driver.rs` | `:1-72` module docs | six-step boot order, silent on rebirth | **amended** — a *seventh path* section: a BIRTH that follows no CONNACK, under an unchanged `bdSeq` |
| `crates/smartme-bridge/src/app/mqtt_driver.rs` | `:479`, `:596`, `:950` | *"can receive no command"* | **confirmed still-true** — all three are subscription-failure paths, unaffected |
| `docs/manual/README.md` | `:39` | chapter 3 *"documents behaviour `smartme_mqtt` does not implement"* | **confirmed still-true** — generic editorial guidance, still accurate |
| `docs/ignition-contract-runbook.md` | run table | rows indexed by contract version | **amended** — a note that the contract is v3, that the change is additive so the existing rows stand, and that no run has been recorded against v3 |
| `_bmad-output/planning-artifacts/prd.md` | `:68`, `:87`, `:97`, `:107`, `:212`, `:246`, `:360` | *"responds to Ignition NCMD/Rebirth"* | **confirmed still-true — and a confirmation is a disposition.** These were always specification, not description; they are now also description. Nothing amended |
| `_bmad-output/planning-artifacts/prd.md` | `:149` | RBE *"cannot safely do so until it answers NCMD/Rebirth"* | **amended** — blocker lifted, RBE still not implemented, residual host-initiated caveat stated |
| `_bmad-output/planning-artifacts/epics.md` | `:114` NFR17 note | *"the NCMD/Rebirth half is Epic 4"* | **confirmed still-true** — **Story 4.8** owns closing NFR17, and it has not run |
| `_bmad-output/planning-artifacts/epics.md` | Story 4.7 entry | the three original ACs | **amended** — all seven carried back, with the two amendments and four additions marked and reasoned |
| `_bmad-output/planning-artifacts/epics.md` | Story 4.7 RBE note | *"cannot be implemented before this story"* | **amended** — discharged without implementing RBE |
| `_bmad-output/planning-artifacts/epics.md` | Story 4.8 premise `:932-947` | extends the Tier-3 gate to rebirth | **confirmed still-true** — 4.7 supplies exactly what 4.8 assumes |
| `crates/smartme-bridge/tests/chaos_ncmd_subscription.rs` | `:366-377`, `:379-384`, `:410-420` | three inverted assertions | **re-aimed, none deleted** — see below |

**The three `chaos_ncmd_subscription` assertions, and the trap inside them.**

| Assertion | Before | After |
| --- | --- | --- |
| `"unrecognised NCMD ignored"` | fired on the rebirth | kept, aimed at `Node Control/Next Server`, a command genuinely not implemented |
| the `"Node Control/Rebirth"` needle | still true but no longer discriminating — the answer trace names it too | re-aimed at the ignored command's name |
| `rebirth.is_none()` | **confirmed nothing** | **re-aimed and now stronger than it ever was** |

The trap was real and the story was right to call it the most dangerous thing in the file.
`command_payload` built metrics with `..Default::default()`, so `value: None` — and a valueless
`Node Control/Rebirth` is not a Rebirth Request. `rebirth.is_none()` therefore held after a perfect
implementation *and* after a completely broken one, while its failure message warned about answering
too eagerly. The helper now takes a value; the valueless payload is kept as an AC6 near-miss case
that must **not** birth, and a conformant `true` request is sent last and **must** birth. Mutation 15
(match on the name alone) now turns the file red; before this story it left it green.

`RUST_LOG=info` was also **removed** from that run. The Story 4.6 review's finding D1 was that its
INFO assertions were criteria no operator could see, and a test that sets the filter itself can never
notice. It now rides on `main.rs`'s default directive — mutation 17 confirms that dependency is real.

### File List

**Production code**

- `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs` — `METRIC_NODE_CONTROL_REBIRTH`,
  `rebirth_metric`, `node_metrics`; `CONTRACT_VERSION` 2 → 3 with the additive/breaking distinction
- `crates/smartme-bridge/src/app/mqtt_driver.rs` — `Inbound::Rebirth` / `::RebirthNearMiss`,
  `RebirthAsReceived`, `describe_value`, rewritten `classify`, two new trace arms,
  `BirthReason`, `announce` shared by both birth paths, the module's *seventh path* docs
- `crates/sparkplug-b/src/lib.rs` — doc comment only: *confirmed unchanged* note (no code change)

**Tests**

- `crates/smartme-bridge/tests/chaos_ncmd_rebirth.rs` — **new.** AC1/AC2/AC3/AC5 end to end
- `crates/smartme-bridge/tests/chaos_ncmd_subscription.rs` — three assertions re-aimed, valued
  `command_payload`, near-miss case, conformant request, evictor task aborted, `RUST_LOG` removed,
  module docs and falsification table updated

**Documentation**

- `docs/sparkplug-conformance.md` — 7 verdicts, 3 tallies, gap breakdowns, DCMD expiry, 14 citations
- `docs/adr/0016-rebirth-before-primary-host-wait.md` — *What Story 4.7 changed*; argument spent
- `docs/primary-host-state-observation.md` — expiry box; the record's own measurements untouched
- `docs/manual/chapters/05-mqtt-sparkplug-contract.tex`
- `docs/manual/chapters/02-understanding-sparkplug.tex`
- `docs/ignition-contract-runbook.md`
- `_bmad-output/planning-artifacts/prd.md`
- `_bmad-output/planning-artifacts/epics.md`
- `_bmad-output/implementation-artifacts/4-7-node-control-rebirth-answer.md` (this file)
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

**Deliberately NOT changed** — `crates/smartme-bridge/tests/arch_purity.rs` (the guard is not to be
edited), `crates/sparkplug-b/src/` apart from the doc note, and the other Story 4.6 ignore paths.

### Post-review state (2026-07-31)

**33 patches applied, 6 decisions taken and implemented, 1 deferred item closed by fixing it, 3
findings dismissed as noise.** New artefacts: [ADR 0017](../../docs/adr/0017-a-retained-ncmd-is-a-replay-not-a-request.md)
and [#39](https://github.com/guycorbaz/smartme_mqtt/issues/39).

**What the review changed in behaviour**, as opposed to in prose:

- A **retained** NCMD is refused and reported as a replay (ADR 0017). One publish on this
  unauthenticated broker would otherwise have made every future session answer a request nobody was
  sending — self-sustaining, and identical in the log to a real host.
- The near-miss detector is now **wider than the matcher it guards**: a name missing by case, by
  whitespace, or by the specification's own `Node Control/Refresh` slip
  (`Sparkplug_5_Operational_Behavior.adoc:950`, contradicting every tck-id in its own section) is
  reported with its spelling instead of vanishing into the unrecognised path. The story's entire
  residual-risk argument rests on that detector firing.
- A partly-published birth is **counted**, and the *"complete BIRTH sequence republished"* line is
  emitted only when the count is zero.
- The trace cap now bounds the **line**, not just one value: `MAX_TRACED_METRICS` plus the true total,
  and the two unbounded protobuf variants are shortened before they are formatted rather than after.
- A rebirth answered alongside unknown metrics now names them, restoring *"never silently"*.
- `set_max_packet_size` is stated in this repository rather than inherited from the library's `Default`.

**What it changed in evidence:**

- `log.contains('3')` — satisfied by the log's own RFC-3339 timestamp — is gone; the datatype half of
  AC6 is falsified independently for the first time.
- The chaos tier now witnesses AC1 on the **answering** NBIRTH, not only the first, and asserts that
  DATA **resumes**.
- AC3's window no longer depends on a race: the feeder is stopped and the stream allowed to go quiet
  before the request, and one reading is pushed 50 ms after it. What that catches and what it does not
  is stated at the call site rather than implied.
- The AC1 unit test asserts the norm's literal string, which nothing fast did before.
- Both falsification tables were reconciled against a re-run; the disagreeing bdSeq values are settled
  at **1 → 2** by an experiment that actually ran.

**What it changed in documents:** a fourth tally that still read `50 gaps` against a table saying 43;
five statements about the NBIRTH's metric count, two of them in live clause rows; a `case-sensitivity`
row enumerating three constants when there are four; a `Findings carried forward` sentence in the
present tense two sentences above its own correction; an evidence cell naming a test this story
deleted; the module doc of the test file this story rewrote, still announcing *"every command is
thrown away safely"*; and **every** `mqtt_driver.rs` and `sparkplug_publisher.rs` citation in the
matrix — 41 of them, each re-pointed and then verified by printing the line it names.

**Still open, deliberately.** The oversized-inbound-packet deferral (2026-07-29) stands as a decision;
what changed is the brief handed to Story 4.13, because `retain` removes its assumption of a sustained
attacker. And 4.7 still claims **conformance to the norm, not compatibility with Ignition** — Story 4.8
owns that, and now has a second hypothesis to settle: whether MQTT Engine spells the metric the way the
tck-ids do or the way that one sentence of prose does.

### Change Log

| Date | Change |
| --- | --- |
| 2026-07-30 | Story 4.7 implemented. `Node Control/Rebirth` declared in every NBIRTH and answered with a complete BIRTH sequence under an unchanged `bdSeq`. `CONTRACT_VERSION` 2 → 3 (additive). 17 falsification mutations, all red. 7 conformance rows moved, 3 tallies recomputed. AC7 per-passage table above; 3 passages found that the story's mechanical list did not contain, plus 14 stale code citations. ADR 0016's sequencing argument recorded as **spent** — Story 4.5 must be re-weighed, not inherited. |
