# Tier-3 Ignition contract test — runbook

**What this is.** The only checks in the project that a *real* Sparkplug host accepts what we
publish. Everything else validates our bytes against our own expectations — round-trips
through our own decoder, property tests over our own invariants — which proves
self-consistency and nothing about conformance. A codec can be perfectly self-consistent and
still be rejected by Ignition, or worse, silently misread by it.

**There are two of them since Story 4.8**, and which one you run matters: one publishes the
crate's bytes, the other the product's, and since ADR 0012 those differ. See *There are TWO
gates* below before running anything.

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

## There are TWO gates, and they attest to different things

Story 4.8 added a second one, because the first had stopped testing the product. Run the
**bridge** gate to answer NFR17; run the **crate** gate only for the narrow purpose stated below.

| Gate | Publishes | Answers |
| --- | --- | --- |
| **`smartme-bridge`** ← the one that matters | the **product's** bytes, by driving `mqtt_driver::run` | *Does what `smartme_mqtt` puts on the wire conform to what MQTT Engine accepts?* — which is NFR17 |
| `sparkplug-b` | a session scripted from crate primitives, with the **specification's** quality codes | *Does the crate's codec conform?* — plus a standing demonstration that Ignition displays the specified `Stale = 500` as `Good(500)` |

**Why the split exists.** Since `d28bb02` ([ADR 0012](adr/0012-quality-codes-spec-versus-host.md))
the crate returns the specification's quality codes and the bridge deviates to Ignition's. The two
publish **different bytes**. The crate gate's `v2 | Pass` row was obtained before that split, so it
attests to an artifact state that no longer exists — see the drift note under *Record of runs*, and
[#40](https://github.com/guycorbaz/smartme_mqtt/issues/40).

## Running the bridge gate (NFR17)

```bash
SPARKPLUG_CONTRACT_BROKER=<host>:1883 \
SPARKPLUG_CONTRACT_GROUP=ContractV6 \
  cargo test -p smartme-bridge --test ignition_contract -- --ignored --nocapture
```

> **Name the group after the contract you are attesting to.** `ContractV3` was the 2026-08-03 run;
> reusing it would put two contracts' evidence in one Ignition folder, and the tag tree outlives
> the test.

Six steps. Steps 1–4 and 6 mirror the crate gate's; **step 5 is new and cannot be automated at
all** — you trigger a rebirth *from the Ignition Designer*, and the gate reads the `bdSeq` off the
wire before and after and prints its own verdict. A rebirth you publish yourself proves only that
the bridge answers *us*, which every automated test already proves.

The gate refuses to start if it cannot reach the broker within 20 s, and says why. It also refuses
a group named `Site`.

## Running the crate gate

```bash
SPARKPLUG_CONTRACT_BROKER=<host>:1883 \
SPARKPLUG_CONTRACT_GROUP=ContractTest \
  cargo test -p sparkplug-b --test ignition_contract -- --ignored --nocapture
```

**Its step 4 is expected to show `Good(500)`.** That is not a failure of the gate; it is the
demonstration the gate now exists for. See *Step 4* below.

## Both gates

`--nocapture` is **not optional**. Without it you see none of the prompts, and the test looks
like it has hung when it is in fact waiting for you.

The broker address is deployment-specific: keep it in your shell or your local `.env`, never
in a committed file.

**Neither gate installs a `tracing` subscriber, so the bridge prints no log at all.** The
subscriber is built in `main.rs`, which these tests do not run; without one, `tracing` discards
every event and `RUST_LOG` changes nothing. Any checklist item phrased as *"the bridge's log
shows…"* — `Rebirth Request accepted`, `node re-announced on a Rebirth Request`,
`reason=Retained`, `reason=NameOnlyNearly`, `reason=ValueNotTrue` — **cannot fire**, and the
operator sees silence rather than a failure. Discovered during the 2026-08-03 run;
[#44](https://github.com/guycorbaz/smartme_mqtt/issues/44). Until it is fixed, treat those items as
absent, not as passed: silence is not evidence. It is the same shape as the Epic 4 acceptance
criteria written in terms of trace levels that sat below the default filter.

Each gate prints a checklist and waits for **Enter** at each step. Take as long as you need —
nothing times out. **Every checklist item is followed by what else could make that step pass
wrongly**; read those, because this gate has already come within one step of returning a false
pass twice.

---

## What changed since the last run — v3 → v9, and what this run can and cannot attest

*Written 2026-08-12, before the v6 run, so that the run's scope is decided in advance rather than
claimed afterwards.*

The last complete run was 2026-08-03, contract v3. Three versions have shipped since:

| | What it changed | What the operator sees that they did not see at v3 |
|---|---|---|
| **v4** | every non-good metric may carry a `Cause` property naming the oracle that refused it | a second property beside `Quality` in the tag browser |
| **v5** | `counter-went-backwards` joins the cause vocabulary | nothing, unless a counter resets during the run |
| **v6** | **breaking** — a verdict belongs to a METRIC, so a refused energy index no longer nulls the power beside it | nothing, unless one metric alone is refused |
| **v7** | additive — four causes name what `value-unusable` used to say at once, and a field the bridge could not read degrades that field alone | a more precise `Cause` string, and only on the field at fault |
| **v8** | additive — `source-refused` splits into three refusals an operator repairs in different places, and a `429` gets its own cause | a degraded meter says whether the fault is a credential, the configuration, the wrong meter, or a rate limit |
| **v9** | additive — `feed-not-advancing`: the cloud handed back the same `Date` header twice | a meter can now be refused because the CLOUD stopped answering afresh, not because the meter went quiet — the two send an operator to different places |
| **v10** | additive — `device-not-in-account` splits out of `configuration-contradicted`, and a device the account no longer has ends with a DDEATH | the tag browser shows the device DEAD (stale, last values kept) instead of an endlessly refreshed `Bad`, and the cause names the row or the account rather than the file |

### What this run attests

Everything the v3 run attested, re-observed against the shipped contract: the cold-start birth, the
first reading, the update, the honest `Bad_Stale` while the node stays online, the rebirth issued
by Ignition, and the two death certificates. **That is the whole of NFR17 and it is the point of
the exercise** — a v3 attestation says nothing about v6 bytes, and the gap is [#72]'s sibling risk
R3 in the project register.

**Plus one new observation, at step 4: does the `Cause` property reach the tag browser at all?**
The step publishes `Verdict::stale(Cause::ReadingTooOld)`, so both metrics carry
`Cause = "reading-too-old"`. If Ignition shows it, the operator gains the *reason* a value is not
good, which is the entire purpose of v4. **How this passes wrongly:** the property may be visible
only in the tag's *properties* pane and not in the browser column, which is a display setting and
not a contract failure — check both before recording an absence.

### What this run does NOT attest, and both gaps are structural rather than oversights

**1. The per-metric verdict — v6's breaking change — is not exercised.** Every step publishes one
verdict for the whole reading, so `Verdicts::uniform` is what reaches the wire and the v6 code path
that stamps metrics separately never diverges from the v5 one. Provoking it needs a meter whose
energy index drops while its power is current — a counter reset, which cannot be staged on real
hardware. **A run that passes therefore says nothing about v6's headline change**, and recording it
as "v6 attested" without this sentence would be the drift [#40] is about, one version later.

**2. [#68] is not answered by this gate.** The question is what Ignition does with a property it
never saw declared at BIRTH.

> **CORRECTED 2026-08-12, hours after it was written.** This paragraph first claimed the gate
> exercises the *declared* case, on the reasoning that step 1's cold-start DBIRTH is `Stale` and
> therefore carries a `Cause`. **It does not.** `cold_start_metrics`
> (`adapters/sparkplug_publisher.rs:611`) sets the quality code and attaches **no property at all** —
> it bypasses `metrics_for` entirely. Story 2.1's own review recorded that gap and this document
> contradicted it the same day. So the gate's DBIRTH declares nothing, exactly like production's
> `Good` births, and **both are the undeclared case**: whatever separates them, it is not this.

That leaves the difference unexplained, and an unexplained difference is not a finding. What was
actually observed on 2026-08-12, and it is little: the operator saw no `Cause` on the gate's node
and reported seeing one on production's — while adding, correctly, that they were not sure they had
looked in the right place in the Designer. **An observation nobody can locate is not a
measurement**, and nothing was recorded from it.

**The half that belongs to us has its own instrument now.**
`tests/observe_cause_property.rs` subscribes to the real broker, publishes nothing, and prints
every metric with its quality and its properties. It separates *what the bridge emits* from *what a
host displays*, which is the confusion this whole question kept collapsing into. Its first run
(2026-08-12, 75 s) answered **INCONCLUSIVE by construction and said so**: all three meters were
`Good`, so no `Cause` was owed and its absence proved nothing.

**To settle it, the observation must happen while a meter is degraded** — the cloud going quiet is
enough, and it did for ten hours on 2026-08-10. Run the observer then, and check the tag browser in
the same minutes. **How that passes wrongly:** if Ignition *retains* a property it once saw, a
`Cause` visible on a currently-`Good` tag is a stale reason rather than a live one — which would be
a hazard of its own, and is worth checking before concluding anything from a tag that looks
right.

Until then, story 2.1's task 3 stays UNMET and the arbitration it blocks — whether to declare
`Cause` at BIRTH with a neutral value, contradicting *"a good metric carries no cause"* — cannot be
taken.

[#40]: https://github.com/guycorbaz/smartme_mqtt/issues/40
[#68]: https://github.com/guycorbaz/smartme_mqtt/issues/68
[#72]: https://github.com/guycorbaz/smartme_mqtt/issues/72

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

**The two gates expect OPPOSITE outcomes here, and that is the point.**

- **Bridge gate — expect a NON-good quality** (`Bad_Stale`). It publishes
  `ignition_quality_code`, and this step is the only check anywhere that the host *agrees* the
  code means "not good". No automated test can do it: our tests compare our bytes to our own
  expectations, and the whole v1 failure was a code we were confident about and the host read
  as good. If this shows good, the guarantee fails silently and the deviation has stopped
  working.
- **Crate gate — expect `Good(500)`.** It publishes the specification's `Stale = 500`. Ignition
  reads the quality *level* from the top bits of a 32-bit code, so 500 lands in the good band
  with 500 as a subcode. This is a demonstration of the defect
  [ADR 0012](adr/0012-quality-codes-spec-versus-host.md) exists to work around, kept
  deliberately as the standing external evidence that the deviation was necessary. A non-good
  quality *here* would be the surprise, and worth an issue.

Whichever gate you are running, this step is the one worth being slow and suspicious about —
and it is the step that came closest to a false pass on the v2 run, because a tag can read
"not good" for reasons that have nothing to do with the quality property. Check the node is
still **online** before believing it.

### Step 5 (bridge gate only) — a rebirth issued BY IGNITION ← the one nothing else can do

You trigger it from the Designer, by writing `true` to the node's `Node Control/Rebirth` tag.
**Do not publish it yourself.** A rebirth the gate publishes proves the bridge answers *us*,
which `chaos_ncmd_rebirth` already proves automatically and for free. The only thing this step
adds — the only thing no test in this repository can supply — is that the bridge answers
*Ignition*.

What to confirm, and the gate prints the numbers so you do not have to judge by eye:

- exactly **one** NBIRTH gained, and one DBIRTH per meter;
- **`bdSeq` unchanged** across the rebirth. The gate reads it off the wire before and after and
  prints its own verdict.

**Record where the control appears in the Designer.** The next person will not find it, and if
it is *absent* that absence is the measurement: it would mean MQTT Engine offers the control
only for a node that declared the metric, and that the flow ADR 0016 described had never
occurred.

**Two ways this step passes wrongly, both of which have caught this project before:**

- a **reconnect** produced the birth. ⚠️ **This warning was written before Story 4.10 and is no
  longer true of the bridge** — the gate's own printed checklist still carries the old wording.
  A reconnect *used to* publish an NBIRTH under the same `bdSeq`, because the session number was
  frozen for a client's lifetime. Since 4.10 the driver rebuilds the client per session and *one
  iteration = one CONNECT = one `bdSeq`* (`crates/smartme-bridge/src/app/mqtt_driver.rs:908`), so a
  reconnect mints a **new** number. **The gate's `bdSeq`-unchanged verdict therefore excludes a
  reconnect by itself**, which is just as well: the log check this bullet asks for cannot be
  performed at all (see *Both gates*).
- a **retained** NCMD was replayed at subscribe time rather than a request anyone sent
  ([ADR 0017](adr/0017-a-retained-ncmd-is-a-replay-not-a-request.md)). The bridge refuses those
  now and logs `reason=Retained`.

**If no birth follows, look for the near-miss WARN before concluding anything.**
`reason=NameOnlyNearly` means Engine sent a different spelling — the norm contradicts itself
here, `Sparkplug_5_Operational_Behavior.adoc:950` says *"Node Control/Refresh"* where every
tck-id says `Rebirth`. `reason=ValueNotTrue` means a different encoding. Each has a different
repair, and the log is what tells them apart.

### Step 5 (crate gate) / Step 6 (bridge gate) — death, two certificates

Publishes the explicit NDEATH, then drops the socket so the broker's last will fires as well.

Per [ADR 0011](adr/0011-graceful-shutdown-requires-both-deaths.md) a graceful stop produces
**two NDEATH messages** carrying the same `bdSeq`. That is by design — the explicit
certificate is immediate, the will is the fallback for a hard death — but no broker-level test
can tell us how a *consumer* reacts to it.

So this step asks the question directly: does Ignition treat the second death as a harmless
repeat, or does it log an error, complain about a duplicate session, or otherwise misbehave?
Check the Ignition logs, not just the tag values.

**Record, from `Node Info`, without reading ahead:**

- `Death Count`, before and after;
- the **date and time** carried by `Offline DateTime` — both, not the time alone.

**Write those down before reading the next paragraph or the findings below.** The two deaths are
about two seconds apart, so the second field discriminates which one the host kept — and this
runbook already contains a prior answer to that question, in a section an operator can easily have
read first. On 2026-08-03 the operator was told the expected outcome *in the briefing for this step*
and then reported that outcome, quoting a timestamp that turned out to be the previous run's, from a
node deleted before the value could be checked. Nothing was recorded, which was the right call, but
the measurement was lost for that run.

That is the plainest form of the failure this document is built around: a step that announces its
own answer cannot measure anything. Steps 1–5 each carry a *what else could make this pass wrongly*
list; this one needs the opposite discipline — **ask, then look, then compare.**

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
| 2026-08-03 | 8.3.7 | 5.0.0-rc1 | v3 | **the bridge binary** | **Pass — all six steps.** The first complete run of the bridge gate that exists, and the run that closes NFR17. What it establishes, and the two guards that were inert, are below |
| 2026-07-31 | 8.3.7 | 5.0.0-rc1 | v3 | **the bridge binary** | **Partial — targeted probe, NOT the five-step gate.** Steps 2–4 were never exercised: the run published no `Good` value at all. What it did establish is below |
| 2026-07-26 | 8.3.7 | *(not recorded)* | v2 | `sparkplug-b` scripted session | **Pass**, all five steps. ⚠️ **This row attests to an artifact state that no longer exists** — see the drift note below |
| 2026-07-26 | 8.3.7 | *(not recorded)* | v1 | `sparkplug-b` scripted session | **Fail at step 4** — quality `STALE` displayed as `Good(500)`; see [#22](https://github.com/guycorbaz/smartme_mqtt/issues/22) |

A pass is only meaningful against a stated version, so add a row rather than editing one. The
**MQTT Engine module** column was added 2026-07-31: it is the component that decodes Sparkplug, so it
governs conformance more directly than the Ignition platform version, and the note below had been
asking for it since the table was written.

### What the 2026-08-03 run established — the run that closes NFR17

Ignition 8.3.7, MQTT Engine 5.0.0-rc1, contract v3, group `ContractV3`, node `ContractNodeV3`.
Six steps, all passed. **Every previous row in this table is either partial or attests to an
artifact that no longer exists**, so these are first measurements, not confirmations.

- **ADR 0012's deviation is verified on a TRANSITION.** Steps 2–3 published `Good`
  (`Power = 1.234 kW`, `Energy = 5678.9 kWh`, then `2.345` / `5679.1`), and step 4 republished the
  same values as `Stale` with the node still `online` and `Death Count = 0`. Ignition displayed
  **`Bad_Stale`**. This is the one thing no automated test in the workspace can establish, and the
  exact failure that contract v1 shipped: a quality code we were confident about that the host read
  as good.
- **Step 1's `Bad_Stale` is NOT part of that evidence, and the runbook should say so.** A tag
  Ignition has just created and never received a value for reads `Bad_Stale` on its own. At step 1,
  *"the host honoured our `STALE`"* and *"the host defaults a valueless tag to `Bad_Stale`"* are
  indistinguishable. Only the step-2→4 transition separates them — which is precisely why the
  2026-07-31 probe, which never published a `Good` value, established nothing about quality.
- **The cold start survives contact with the host.** The null metric was accepted with its datatype;
  Ignition did **not** invent a `0`. `EngUnit` came through as `kW` / `kWh`, and the device folder
  is named by the serial `30000001`.
- **`Contract/Version = 3` was read by a real host** for the first time.

> **These rows attest to contract v3, and the contract is now v9.** Story 2.1 (2026-08-10)
> added a `Cause` property to every non-good metric, which is a change to the tag set (v4);
> story 2.2, the same day, added the `counter-went-backwards` cause to that property's
> vocabulary (v5); story 2.3 (2026-08-11) made a verdict belong to a METRIC rather than to the
> reading, so a refused energy index no longer nulls the power value beside it (v6, **breaking**).
> So a v3 run does **not** attest to what a consumer sees today, and **no run has happened against
> v4 through v9.** What the v3 rows still establish is everything independent
> of that property: the quality codes on a `Good`→`Stale` transition, the rebirth flow, and the
> double NDEATH. Since v4 the binding is machine-checked by `tests/contract_golden.rs`, so a
> future drift between this table and the code cannot be silent — but a missing run is still a
> missing run.
>
> **The run became possible on 2026-08-12.** Until then the deployment spoke v3, so there was
> nothing to point the gate at: panoramix ran `v0.4.0-rc2`, three contract versions behind its own
> repository. It now runs `v0.4.0-rc3` at contract 6, observed on `/healthz`. A missing run is
> still a missing run — but it is now a missing run rather than an impossible one, and the two
> observations owed here (the `Cause` property's fate in the tag browser, [#68], and
> `Rebirth (Last) Cause`) are both takeable.
>
> *Corrected 2026-08-11: this block said "the contract is now v4" while `CONTRACT_VERSION` had
> already moved to 5. Story 2.1 instituted a mechanical grep for the stale number and story 2.2
> did not re-run it — the check exists precisely because this block's job is to tell an operator
> that the recorded run no longer attests to the shipped contract, which it cannot do while
> naming the wrong shipped contract.*
- **The bridge answers a rebirth issued by IGNITION.** Two writes to
  `ContractNodeV3/Node Control/Rebirth` produced **2 NBIRTHs and 2 DBIRTHs, `bdSeq` unchanged at 1**
  — the gate read both off the wire. Everything else in the repository proves only that the bridge
  answers *us*.
- **Engine did not resend of its own accord** — two writes, two requests. Second independent
  measurement after 2026-07-31. So the *"Ignition resends"* premise behind Story 4.7's
  no-rate-limit decision is still **unmeasured**; what is measured, twice, is that bursts would be
  operator-driven.
- **Both death certificates were processed:** `Node Info → Death Count` moved 0 → 2, per ADR 0011.

**Two guards were inert, and one of them is now unnecessary.**

- The **reconnect** false-pass at step 5 could not be checked the way the checklist asks — see the
  tracing note under *Both gates* — but it no longer needs to be. Since Story 4.10 the driver owns
  its session loop, *one iteration = one CONNECT = one `bdSeq`*
  (`mqtt_driver.rs:908`), so a reconnect mints a **new** number. **`bdSeq` unchanged now excludes a
  reconnect on its own.** The wording under *Step 5* has been corrected accordingly.
- The **retained-NCMD** false-pass (ADR 0017) was excluded by timing rather than by log: a retained
  message is replayed at subscribe time, which is step 1, not ten minutes later at the instant of an
  operator's click.

**Not observed, and the reason is worth more than the measurement.** Which timestamp
`Offline DateTime` retained — the explicit certificate's, or the will's two seconds later — was not
established. The 2026-07-31 probe found it tracked the **will**, which would make ADR 0011's claimed
benefit (*"the explicit certificate is immediate"*) unobservable from the host side.

The step-6 briefing **stated that prior finding before asking for the reading**. The operator then
reported *"it's the will"* with the timestamp `8:34:58 PM` — which is verbatim the 2026-07-31 value
recorded further down this page, and impossible for a run that ended around midday. By the time the
coincidence was caught, `ContractNodeV3` had been deleted in the clean-up and the value was gone.

Nothing was recorded, and that is the only reason this is a lost measurement rather than a false
one. Step 6 now asks for the date *and* the time and says to write them down before reading on.

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

> **⚠️ Superseded 2026-08-03 — v3 now has a complete run**, the top row of the table. The note below
> is kept because its reasoning about *why the version was bumped* is still the reason this table
> works. What is no longer true is its opening claim.
>
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
Edge Nodes/<your group>/BridgeContractNode  ← bridge gate
Edge Nodes/<your group>/ContractNode        ← crate gate
```

**The two gates use different node ids** — `BridgeContractNode` and `ContractNode`, from each test's
own `NODE_ID`. This section named only the crate gate's until 2026-08-03, so following it after a
bridge run left the folder behind. **The bridge gate's id was `ContractNodeV3` until 2026-08-12**;
runs recorded before that date left a folder under the old name, and the record-of-runs entries
below still name it because that is what they created.

**Delete only that folder.** Removing MQTT Engine tags also discards their alarm and history
configuration, and your real edge nodes live under the same parent.

The node never republishes, so a plain delete sticks. If tags reappear, Cirrus Link's
documented sequence is: disable MQTT Engine → delete the tags → re-enable MQTT Engine.

The broker itself needs no clean-up: every message is published with `retain = false`, so
nothing is left waiting for a future subscriber.
