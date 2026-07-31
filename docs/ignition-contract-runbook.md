# Tier-3 Ignition contract test — runbook

**What this is.** The only check in the project that a *real* Sparkplug host accepts our
hand-rolled protobuf. Everything else validates the codec against itself — round-trips
through our own decoder, property tests over our own invariants — which proves
self-consistency and nothing about conformance. A codec can be perfectly self-consistent and
still be rejected by Ignition, or worse, silently misread by it.

**What it is not.** An automated test. There is no assertion here worth the name: the
assertion is a human looking at the tag browser. It is `#[ignore]`d and will never run as a
side effect of `cargo test`.

**When to run it.** Before a release, and after any change to the encoder, the topic grammar,
the metric set or the session lifecycle. This first pass is the early-discovery run, while
there is still budget to fix the codec; the release-gate re-run belongs to Epic 8.

---

## Before you start

You need:

- Ignition running with the **MQTT Engine** module, connected to the same broker.
- The Designer open, with the **MQTT Engine** tag provider selected in the Tag Browser.
- A **disposable group name**. The test refuses to publish into `Site`.

> **Read this before running.** A Sparkplug host *persists what it discovers*. The group you
> name becomes a folder in Ignition's tag tree that outlives the test and has to be deleted by
> hand. If your broker is production — and if you have only one broker, it is — pick a group
> name that is obviously disposable, and do the clean-up at the end. It is part of the
> procedure, not an afterthought.

Record the **Ignition version** you are testing against. A pass is only meaningful against a
stated version.

## Running it

```bash
SPARKPLUG_CONTRACT_BROKER=<host>:1883 \
SPARKPLUG_CONTRACT_GROUP=ContractTest \
  cargo test -p sparkplug-b --test ignition_contract -- --ignored --nocapture
```

`--nocapture` is **not optional**. Without it you see none of the prompts, and the test looks
like it has hung when it is in fact waiting for you.

The broker address is deployment-specific: keep it in your shell or your local `.env`, never
in a committed file.

The test walks five steps, printing a checklist and waiting for **Enter** at each one. Take as
long as you need — nothing times out.

---

## What each step proves

### Step 1 — birth, cold start

Publishes NBIRTH, then a device BIRTH declaring both tags with **no value** and quality
`STALE`.

This is the bridge's honest first message: before the first successful fetch there is nothing
to report, and a placeholder zero would be indistinguishable from a meter genuinely reading
zero. What is under test is whether Ignition accepts a **null metric that still declares its
datatype** — if it rejects it, or invents a `0`, the cold-start design does not survive
contact with the host.

Also verify the device folder is named by the **serial** (`30000001`), not by a friendly name.

### Step 2 — first reading

`Power = 1.234 kW`, `Energy = 5678.9 kWh`, both `GOOD`.

Deliberately awkward numbers: nothing round, nothing that could be a default or a placeholder.
Check them **exactly**. A unit-scaling bug (W vs kW, Wh vs kWh) shows up here as a factor of
1000 and nowhere else.

Check the tag timestamps too: they must be the values' own acquisition time, not the moment
Ignition received them. Freshness travelling in the payload is what makes a lost message read
as *old data* rather than as *current data*.

### Step 3 — the values update

`Power = 2.345`, `Energy = 5679.1`.

Confirms updates flow without a rebirth or a reconnect, and that the energy counter moves
**up**. A counter that goes backwards in a historian corrupts every derived rate.

### Step 4 — STALE while the node stays online ← the critical one

Republishes the same values with quality `STALE`, node still connected.

**This is the failure mode the whole project exists to prevent.** The cloud has stopped
answering while the bridge is perfectly healthy on MQTT. The node is alive, so the transport
mechanism (NDEATH) will not fire — the only thing marking the data untrustworthy is the
quality property.

If Ignition still shows these as good, the guarantee fails here, and it fails silently. This
step is the one worth being slow and suspicious about.

### Step 5 — death, two certificates

Publishes the explicit NDEATH, then drops the socket so the broker's last will fires as well.

Per [ADR 0011](adr/0011-graceful-shutdown-requires-both-deaths.md) a graceful stop produces
**two NDEATH messages** carrying the same `bdSeq`. That is by design — the explicit
certificate is immediate, the will is the fallback for a hard death — but no broker-level test
can tell us how a *consumer* reacts to it.

So this step asks the question directly: does Ignition treat the second death as a harmless
repeat, or does it log an error, complain about a duplicate session, or otherwise misbehave?
Check the Ignition logs, not just the tag values.

> **⚠️ "Check the Ignition logs" is not performable by scrolling, and on this installation it is not
> performable at all without a filter.** Measured 2026-07-31: an unrelated `MQTT Transmission` client
> retrying a connection it cannot make produces **8–10 lines every 3 seconds** — roughly 200 lines a
> minute, including an `ERROR` on every cycle — alongside continuous Modbus timeouts. Three separate
> attempts to page back to a two-second window failed to reach it.
>
> Two ways that makes this step return a wrong verdict. A careful operator sees `ERROR` lines around
> the right time and reports a failure that has nothing to do with the death. A hurried one concludes
> the log is useless and stops reading it, which is the same as not running the step.
>
> **So the step is: export the log and QUERY it. Do not scroll, and do not rely on the viewer's
> search either.** The Gateway's log export is an `.idb` file, which is a plain SQLite database with a
> `logging_event` table (`timestmp` in epoch millis, `formatted_message`, `logger_name`,
> `level_string`). Two SQL queries answered what three pages of scrolling could not:
>
> ```sql
> -- everything about the node, anywhere in the export
> SELECT timestmp, level_string, logger_name, formatted_message FROM logging_event
>  WHERE formatted_message LIKE '%<NODE_ID>%' ORDER BY timestmp;
>
> -- and whether the Engine module ever complained, about anything
> SELECT level_string, COUNT(*) FROM logging_event
>  WHERE logger_name LIKE '%mqtt.engine%' GROUP BY 1;
> ```
>
> **Zero hits on the first query is a valid pass** — Engine's `SparkplugPayloadHandler` logs *nothing*
> at INFO for births, rebirths or data; only deaths surface. The second query is what separates a
> result from background: run it and record the installation's baseline before the run, so a chronic
> unrelated `ERROR` is recognisable as noise.
>
> The tag-side evidence does not depend on any of this: `Node Info → Death Count` moving from 0 to 2
> already establishes that both certificates were processed, and the node's `Online`/`Offline DateTime`
> establish the transition.

---

## Record of runs

| Date | Ignition | MQTT Engine | Contract | Artifact | Result |
| --- | --- | --- | --- | --- | --- |
| 2026-07-31 | 8.3.7 | 5.0.0-rc1 | v3 | **the bridge binary** | **Partial — targeted probe, NOT the five-step gate.** Steps 2–4 were never exercised: the run published no `Good` value at all. What it did establish is below |
| 2026-07-26 | 8.3.7 | *(not recorded)* | v2 | `sparkplug-b` scripted session | **Pass**, all five steps. ⚠️ **This row attests to an artifact state that no longer exists** — see the drift note below |
| 2026-07-26 | 8.3.7 | *(not recorded)* | v1 | `sparkplug-b` scripted session | **Fail at step 4** — quality `STALE` displayed as `Good(500)`; see [#22](https://github.com/guycorbaz/smartme_mqtt/issues/22) |

A pass is only meaningful against a stated version, so add a row rather than editing one. The
**MQTT Engine module** column was added 2026-07-31: it is the component that decodes Sparkplug, so it
governs conformance more directly than the Ignition platform version, and the note below had been
asking for it since the table was written.

### ⚠️ The v2 row attests to an artifact that no longer exists

Recorded 2026-07-31, at Story 4.8's contexting. The row above is **not edited**, per this table's own
rule — but it must not be read as live evidence.

`crates/sparkplug-b/tests/ignition_contract.rs` publishes quality codes through the **crate's**
`Quality::code()`. Three commits, all 2026-07-26 and in this order: `fce148f` moved those codes to
Ignition's encoding (the v1 → v2 fix, and the `v2 | Pass` run happened after it); `57914bf` was the
last commit to touch the test; `d28bb02` — ADR 0012 — moved the crate **back** to the specification's
`0`/`192`/`500` and put the deviation in the bridge, without touching the test.

So that test now publishes `Stale = 500`, which is the exact code this project proved Ignition
displays as `Good(500)`. **Step 4 — the one marked *"← the critical one"* — is today guaranteed to
fail**, and its checklist tells the operator that a `Good` reading there means the whole guarantee has
failed. It has not; the artifact drifted off the product.

Story 4.8 re-aims the gate at the bridge binary for this reason.

### What the 2026-07-31 probe established, and what it did not

**Established, and each is a first:**

- **Ignition displays the bridge's `Stale` as `Bad_Stale`.** ADR 0012's deviation had never once been
  verified against a real Ignition since the drift above. It is now.
- **MQTT Engine renders a `Node Control/Rebirth` control** for a node that declares the metric, and
  writing `true` to it makes it publish a conformant NCMD.
- **The request uses the tck-id spelling.** The bridge's matcher requires the exact name
  `Node Control/Rebirth`, the value `BooleanValue(true)` and `retain = false` simultaneously, and it
  fired — so all three held. No near-miss WARN appeared in the whole run. The specification's own
  prose at `Sparkplug_5_Operational_Behavior.adoc:950`, which says a host requests a rebirth *"using
  the 'Node Control/Refresh' metric"*, is a defect in the norm's wording and not what Engine sends.
- **`-rebirth-action-3` holds from the host's own view:** after two rebirths, Ignition's `Node Info`
  reported `bdSeq = 1`, `Birth Count = 3`, `Rebirth Count = 2`, `seq = 1`.
- **Two writes produced exactly two requests.** Engine did not resend of its own accord here — so the
  *"Ignition resends"* premise behind the no-rate-limit decision (Story 4.7, Task 4) is still
  **unmeasured**; what is measured is that bursts would be operator-driven.
- **`Rebirth (Last) Cause: Triggered by user`.** Engine classifies rebirths by cause. That a label
  exists for the user-triggered case implies other causes exist — the automatic ones the norm permits
  (`tck-id-operational-behavior-host-reordering-rebirth`). **Inference, not measurement:** no
  automatic rebirth was observed. It is nonetheless the first concrete sign that Engine implements
  that path, which is an input Story 4.5 is waiting on.
- **A metric name containing `/` becomes a folder.** `Contract/Version` and `Node Control/Rebirth`
  appear as folders `Contract` and `Node Control` holding `Version` and `Rebirth`. Expected, and
  written down nowhere until now.
- **On SIGTERM, Ignition counted `Death Count = 2`** — both the explicit NDEATH and the will, not
  deduplicated. ADR 0011 left this to be settled in the field and `architecture.md` said so; it is
  settled, **and favourably**. Querying the log export directly: in three hours of logs the Engine
  module emitted exactly two INFO lines and no Sparkplug-side WARN or ERROR at all —
  `Handling LWT message for Edge Node RebirthProbe/ProbeNode` at `20:34:56.751` and `20:34:58.752`,
  one millisecond after each death reached it. **No error, no duplicate-session complaint, nothing.**
  The device was marked offline without any DDEATH, by propagation from the node's death.
- **Engine calls BOTH deaths an "LWT message", through one `SparkplugPayloadHandler`.** It does not
  distinguish an edge node's explicit certificate from the broker's will. ADR 0011's central
  distinction is therefore **invisible to this consumer**: the two seconds of advance notice are real
  on the wire and Engine does process the first at `:56`, but the second overwrites `Offline DateTime`
  with `:58`. The decision stands; the benefit is narrower than the ADR's wording implies.
- **`Offline DateTime` tracked the WILL, not the explicit certificate.** The explicit NDEATH was
  published at 20:34:56.75 and the socket dropped at 20:34:58.75; Ignition recorded `8:34:58 PM`. So
  ADR 0011's claimed benefit — *"the explicit certificate is immediate"* — is not observable in that
  field. Measured, not explained: whether Engine takes the last death or ignored the first is unknown.

**NOT established, and the reason it looks like it was:**

> **Step 4 was not tested, and it appeared to pass.** The probe pointed `SMARTME_API_BASE` at an
> unroutable address on purpose, so **no `Good` value was ever published**. `Power` and `Energy` read
> `Bad_Stale` throughout — before the rebirth, and after the death. A reader checking "are the tags
> untrustworthy after the node dies?" would tick the box, and would have learned nothing: there is no
> way to tell *became* untrustworthy from *always was*.
>
> This is the false-pass shape this runbook exists to name, and the same one that nearly returned a
> wrong verdict on the Story 1.15 run. A real gate must publish `Good`, then degrade to `Stale`, then
> die — in that order — which is what Story 4.8 builds.

> **The contract is now v3 and no run has been recorded against it** (Story 4.7, 2026-07-30). The
> change is **additive**: the NBIRTH declares one new metric, `Node Control/Rebirth` — boolean,
> `false` — which a consumer sees as a new tag in its browse tree. Nothing was removed or renamed, so
> every expectation in the steps above still holds; the two rows below remain valid for what they
> attest.
>
> This is the reason the version was bumped at all rather than left at 2 on the grounds that the norm
> mandates the metric. **This table is indexed by the contract version**, so without the bump two
> rows both reading `v2` would attest to two different tag sets — and the run that finally exercises
> a rebirth (Story 4.8) would be indistinguishable from the 2026-07-26 one.

> The MQTT Engine **module** version is not recorded above and should be, next time: it is the
> component that decodes Sparkplug, so it governs conformance more directly than the Ignition
> platform version does. Note also that Cirrus Link's published documentation for these modules
> is written against Ignition 8.1; the quality-code behaviour in this project was established by
> measurement on 8.3.7, not by reading those tables — which is just as well, since the tables
> are what misled the original implementation.

## Interpreting the result

**Pass** — every box ticked. Record the Ignition version alongside the result.

**Fail on steps 1–3** — a codec or contract problem: datatype, units, naming, timestamps. Fix
`sparkplug-b` and re-run. This is exactly the early discovery this pass exists for.

**Fail on step 4** — the most serious outcome. Either the quality property is not reaching
Ignition in a form it honours, or it is honouring it differently than assumed. Do not paper
over it by changing the test; the "never lies" guarantee depends on this working.

**Fail on step 5** — if the double NDEATH upsets Ignition, ADR 0011 needs revisiting: the
choice to publish explicitly *and* let the will fire would have a cost that was not known when
it was made.

---

## Clean-up — required

In the Designer's Tag Browser, under the **MQTT Engine** provider, delete:

```
Edge Nodes/<your group>/ContractNode
```

**Delete only that folder.** Removing MQTT Engine tags also discards their alarm and history
configuration, and your real edge nodes live under the same parent.

The node never republishes, so a plain delete sticks. If tags reappear, Cirrus Link's
documented sequence is: disable MQTT Engine → delete the tags → re-enable MQTT Engine.

The broker itself needs no clean-up: every message is published with `retain = false`, so
nothing is left waiting for a future subscriber.
