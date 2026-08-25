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
SPARKPLUG_CONTRACT_GROUP=ContractV10 \
  cargo test -p smartme-bridge --test ignition_contract -- --ignored --nocapture
```

> **Name the group after the contract you are attesting to, and NEVER REUSE ONE.** `ContractV3`
> was the 2026-08-03 run, `ContractV10` the 2026-08-21 one; reusing a name puts two attestations'
> evidence in one Ignition folder, and the tag tree outlives the test.
>
> **A second pass on the same day needs a second name.** On 2026-08-21 the session ran twice on
> `ContractV10`, and the second pass therefore began against tags Ignition had already garnished
> from the first — which left a residual in the session's own finding about the `Cause` property
> ([#107]). A host persists what it discovers; a fresh question needs a folder it has never seen.

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

**The bridge gate installs a `tracing` subscriber and its log reaches you** — INFO by default,
`RUST_LOG` overriding. This paragraph said the opposite until 2026-08-21, and the correction is
worth keeping in view: for the whole of August the checklist items phrased *"the bridge's log
shows…"* — `Rebirth Request accepted`, `node re-announced on a Rebirth Request`,
`reason=Retained`, `reason=NameOnlyNearly`, `reason=ValueNotTrue` — **could not fire at all**,
because the only subscriber in the crate is built in `main.rs`, which an integration test does
not run, and `tracing` with no subscriber discards every event regardless of `RUST_LOG`. The
operator saw silence and had no way to tell it from a failure. Found during the 2026-08-03 run
([#44](https://github.com/guycorbaz/smartme_mqtt/issues/44)), repaired in the gate, and
**observed working on 2026-08-21**: both rebirth events printed, as distinct lines, during a real
session.

It was the same shape as the Epic 4 acceptance criteria written in terms of trace levels that sat
below the default filter. The rule it leaves behind: **silence is not evidence**, so before
trusting any log-shaped checklist item, confirm the log speaks at all.

Each gate prints a checklist and waits for **Enter** at each step. Take as long as you need —
nothing times out. **Every checklist item is followed by what else could make that step pass
wrongly**; read those, because this gate has already come within one step of returning a false
pass twice.

---

## What changed since the last run — v3 → v10, and what this run can and cannot attest

*Written 2026-08-12, before the v6 run, so that the run's scope is decided in advance rather than
claimed afterwards. The heading and the count move WITH the table (v10 appended 2026-08-15 —
the story 3.5 review caught them one version apart, which is exactly the attestation-drift this
section exists to prevent).*

The last complete run was 2026-08-03, contract v3. Seven versions have shipped since:

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

Also verify the device folder is named by the meter's **short name** (`contract-meter` for this
gate), and that the serial `30000001` is **not** a folder anywhere in the tree.

> **This reversed at contract v13** ([ADR 0049](adr/0049-the-device-is-named-by-its-measuring-point-and-vouched-for-by-its-serial.md)).
> The step used to require the opposite — *named by the serial, not by a friendly name* — for a good
> reason: the serial was the one identifier the bridge reads off the device itself. What decided
> against it is what a supervisor historises: a measuring point, not a box. A device id built on the
> serial makes a replaced meter into a new device and breaks the series at the moment nothing changed
> for the operator.
>
> **And then check what replaces the guarantee that gave up.** In the tag properties of `Power`,
> the property **`serial` must read `30000001`**. Without it, nothing on the wire says which physical
> meter is behind the name, and a swapped configuration line would publish one flat's measurements
> under another's with every value still plausible. A missing `serial` here is a FAILED step, not a
> cosmetic one.

> **Where to read the datatype, added 2026-08-22.** Nowhere an operator would look. The Tag Browser
> lists tag *properties* and `DataType` is not one of them; the **Tag Editor does not show it either**
> for an MQTT Engine tag, because the module owns the type. Use the Designer's **Script Console**:
> `system.tag.browse("[MQTT Engine]", {"recursive": True, "tagType": "AtomicTag", "name": "Power"})`
> and print each result's `dataType` — expected **`Float8`**. Without scripting, the Tag Editor's
> **`Numeric`** category is weaker evidence of the same thing: Ignition composes it only for a
> numeric tag, so its presence rules out a typeless or string tag.

### Step 2 — first reading

`Power = 1.234 kW`, `Energy = 5678.9 kWh`, both `GOOD`.

> **The metric names stayed English at contract v13, and that was a decision**
> ([ADR 0050](adr/0050-the-metric-names-stay-english.md)). §16.9.5 of the site's report asks for
> `puissance` and `energie`; the rename was implemented on 2026-08-25 and reversed the same day,
> before anything shipped, so the site's anomaly `A38` stays open on the disagreement. If a session
> ever finds French names here, the contract has moved and this runbook has not — stop and establish
> which version you are looking at.

Deliberately awkward numbers: nothing round, nothing that could be a default or a placeholder.
Check them **exactly**. A unit-scaling bug (W vs kW, Wh vs kWh) shows up here as a factor of
1000 and nowhere else.

> **The browser rounds, so "exactly" is not what you are reading.** Ignition's `FormatString`
> defaults to `#,##0.##`: `1.234` displays as `1,23` and `2.345` as `2,35`. The factor of 1000
> still shows; a third-decimal discrepancy does not. On 2026-08-21 a `Power` display one refresh
> behind read as a metric that had not updated at all, and cost ten minutes of the session.
>
> **And `EngHigh` defaults to `100`**, which `Energy = 5678.9` exceeds fifty-six times. A tag with
> scaling enabled would clamp it to `100` and look exactly like a unit bug in the bridge. `Power`
> stays under 100 and does not run that risk — comparing the two tells you which you are looking
> at.

Check that the tag timestamps **moved**. That is all this gate can establish.

> **The acquisition-versus-reception clause is NOT measurable here, and said so on 2026-08-22.**
> This paragraph used to ask the operator to confirm the timestamp was the value's own acquisition
> time rather than the moment Ignition received it. The gate stamps its readings with
> `clock.wall()` (`ignition_contract.rs:373` → `value_date: now`), so the two are the same second
> and the check passes identically either way — a step that can only pass measures nothing. The
> property still matters: freshness travelling in the payload is what makes a lost message read as
> *old data* rather than as *current data*. **The repair, not yet made: stamp step 2's reading ~90 s
> in the past**, and the tag timestamp must then read visibly behind the wall clock.

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
still **online**, and `Death Count = 0`, before believing it.

**Where to look for the frozen value, added 2026-08-21.** The step asks you to confirm the values
are *unchanged* rather than blanked — the bridge publishes the last known reading and marks it
untrustworthy, because a stale reading is true history while a blank is not. **The browser's Value
column cannot answer that question**: it renders the quality string for any non-good tag, so a
frozen value and a blanked one look identical there, and at step 1 that same column reads
`Bad_Stale` precisely because there is no value. The number survives in the tag's own **`value`
row**. On 2026-08-21 it showed `2,35` and `5 679,1` in red italics beside `Quality = Bad_Stale`.
The item is answerable; it was simply asked in the wrong place ([#108]).

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

- `Death Count`, before and after — **and then read it a second time a few seconds later.** The two
  certificates are ~2 s apart and this field is read in between more easily than it looks: on
  2026-08-22 it gave `1`, which was about to be recorded as a divergence from the 0 → 2 of every
  other run. It settled at `2`;
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

> ### ⚠ THE ATTESTATION IS OWED — v13 shipped on 2026-08-25 and no session has run
>
> **The registre and the binary parted company again**, deliberately and with the reason written
> down. `CONTRACT_VERSION` is **13**; the table below stops at v12. Action H7 of the epic-8
> retrospective allows exactly this — *a bump earns an attestation, or records what it is waiting
> for and when that arrives* — and this is the record: **v13 is waiting for a Tier-3 session
> against Ignition, and nothing goes to production before it.**
>
> v13 carries half of the site's Sparkplug nomenclature (SCADA technical report v0.10 §16.9): the
> device identity moves ([ADR 0049](adr/0049-the-device-is-named-by-its-measuring-point-and-vouched-for-by-its-serial.md)),
> the metric names do not ([ADR 0050](adr/0050-the-metric-names-stay-english.md) — proposed,
> implemented and reversed on 2026-08-25). **Three things to check that the six steps did not ask
> for before:**
>
> - **Step 1 — the device folder is the meter's SHORT NAME**, `contract-meter` for this gate, and
>   the serial `30000001` is **not** a folder anywhere in the tree. This is the half that breaks a
>   consumer silently: a host watching the old device id simply stops receiving.
> - **Step 1 — `serial` reads `30000001` in the PROPERTIES of `Power`.** It is what replaces the
>   guarantee the serial gave by being the device id, and a person in front of the browser is who
>   it is for. Absent here, the rename has taken something and given nothing back.
> - **Step 1 — `Contract/Version` reads 13**, and `Node Control/Rebirth` is untouched.
>
> **The metric names are UNCHANGED** — `Power`, `Energy`, `Cause/Power`, `Cause/Energy` — so a v13
> tag tree looks exactly like a v12 one on that point. French names mean you are looking at
> something other than this version.
>
> **Use a fresh group.** The device rename means the old tag tree cannot be reused without leaving
> orphans that look like the new tree's neighbours — and a residual group has already cost this gate a
> false reading once (2026-08-21).
>
> ### ~~✅ THE ATTESTATION WAS CURRENT — v12, 2026-08-22 at 20:53~~ *(superseded by v13)*
>
> **Registre and binary coincide for the first time since 2026-08-03.** Three sessions ran that
> day, at v10, v11 and v12, and the third is the one that stands: `CONTRACT_VERSION` is 12 and
> v12 is attested on all six steps.
>
> The two boxes below are kept because they are the record of how that took three passes — a
> version attested while its own headline change did not work (v11), and a remedy chosen from the
> half of a measurement that had not been taken (ADR 0043). Neither is a live instruction.
>
> ### ~~⚠ SUPERSEDED — the v11 session ran, and its headline change FAILED~~ *(historical)*
>
> The box below asked for a v11 session and listed what it should check. **It was run at 20:22 and
> it answered: the property is written by a BIRTH and by nothing else.** ADR 0044 moved the cause
> to a metric and `CONTRACT_VERSION` to 12, and the v12 session at 20:53 confirmed the repair.
>
> ### ~~⚠ THE 2026-08-22 (MORNING) ATTESTATION NO LONGER COVERS WHAT THE BRIDGE EMITS~~ *(historical)*
>
> **ADR 0043 moved `CONTRACT_VERSION` to 11, breaking, on the afternoon of the same day.** Every
> metric now carries the `Cause` property — a `Good` one included, where it reads `no-cause` —
> and the cold-start BIRTH declares a new cause, `no-reading-yet`. This table's promise is that
> *two runs sharing a version number attest to the same tag set*, and the v10 row cannot speak
> for v11.
>
> **A Tier-3 session is owed before production.** It is short, and it is not only a re-attestation:
> **it is the experiment that says whether the repair worked.** Two additions to the six steps:
>
> - **Step 1** — the cold-start DBIRTH must now show a **`Cause` row reading `no-reading-yet`**
>   on both metrics. Its presence is what proves Ignition materialised the property at all; every
>   later step depends on it. If it is absent here, nothing after it can be believed.
> - **Step 4** — the `Cause` row must now read **`reading-too-old`**, without folding and
>   unfolding the tag and without a rebirth. That is the whole of [#107]: the property changing
>   value in a DDATA, on a property the BIRTH declared.
> - **Step 2/3** — the same row must read **`no-cause`** while the values are `Good`. This is the
>   half that is easy to skip and it is not decoration: it is what says the property does not go
>   on displaying a fault the meter has recovered from.
>
> And `Contract/Version` reads **11**, not 10.

| Date | Ignition | MQTT Engine | Contract | Artifact | Result |
| --- | --- | --- | --- | --- | --- |
| 2026-08-22 (20:53) | 8.3.7 **Maker Edition** | 5.0.0-rc1 | **v12** | **the bridge binary** | **Pass — all six steps, and the repair CONFIRMED rather than assumed.** The cause reached the operator for the first time since contract v4 invented it, eight versions ago: `no-reading-yet` at birth, `no-cause` when the values went good, `reading-too-old` when they degraded — **three transitions in DDATA, not one rebirth**. [#100] confirmed on the wire a second time. Below |
| 2026-08-22 (20:22) | 8.3.7 **Maker Edition** | 5.0.0-rc1 | **v11** | **the bridge binary** | **Pass — all six steps — AND THE VERSION'S OWN HEADLINE CHANGE DOES NOT REACH THE OPERATOR.** Read both halves of that sentence. The guarantee holds and the contract is conformant on the wire; the `Cause` property that v11 was cut for is written by a BIRTH and never updated by a DDATA, so it stands frozen at `no-reading-yet` on healthy metrics. [#100] confirmed on the wire the same evening: `bd_seq=0`. Below |
| 2026-08-22 (morning) | 8.3.7 **Maker Edition** | 5.0.0-rc1 | **v10** | **the bridge binary** | **Pass — all six steps.** The second complete run in this table, and the one that **re-attests NFR17 at v10**. `Offline DateTime` tracks the **will**, confirming the 2026-07-31 probe; [#107] measured on a virgin group and found `Cause` absent. Below |
| 2026-08-21 | 8.3.7 **Maker Edition** | 5.0.0-rc1 | **v10** | **the bridge binary** | **Partial — five steps of six. STEP 6 WAS NOT PERFORMED**, so NFR17 is *not* re-attested at v10. Steps 1–5 passed, and the session's own finding is that contract v4's `Cause` does not reach the operator at all unless a BIRTH declares it. Below |
| 2026-08-03 | 8.3.7 | 5.0.0-rc1 | v3 | **the bridge binary** | **Pass — all six steps.** The first complete run of the bridge gate that exists, and the run that closes NFR17. What it establishes, and the two guards that were inert, are below |
| 2026-07-31 | 8.3.7 | 5.0.0-rc1 | v3 | **the bridge binary** | **Partial — targeted probe, NOT the five-step gate.** Steps 2–4 were never exercised: the run published no `Good` value at all. What it did establish is below |
| 2026-07-26 | 8.3.7 | *(not recorded)* | v2 | `sparkplug-b` scripted session | **Pass**, all five steps. ⚠️ **This row attests to an artifact state that no longer exists** — see the drift note below |
| 2026-07-26 | 8.3.7 | *(not recorded)* | v1 | `sparkplug-b` scripted session | **Fail at step 4** — quality `STALE` displayed as `Good(500)`; see [#22](https://github.com/guycorbaz/smartme_mqtt/issues/22) |

A pass is only meaningful against a stated version, so add a row rather than editing one. The
**MQTT Engine module** column was added 2026-07-31: it is the component that decodes Sparkplug, so it
governs conformance more directly than the Ignition platform version, and the note below had been
asking for it since the table was written.

### What the 2026-08-22 20:53 session established — the repair, measured

Ignition **8.3.7 Maker Edition**, MQTT Engine **5.0.0-rc1**, contract **v12**, group
`ContractV12`. **Six steps, all passed.** NFR17 is attested at the version the bridge actually
emits, which had not been true since 2026-08-03.

#### The three transitions, and why step 2 was the decisive one

The cause travels as two metrics from v12 (ADR 0044), and what had to be shown was not that a
BIRTH can declare them — v11 already showed a BIRTH can declare anything — but that **a DDATA can
change one**.

| Step | `Cause/Power` and `Cause/Energy` read | What it proved |
| --- | --- | --- |
| 1 — cold start | `no-reading-yet` | the tags exist; the BIRTH declared them |
| 2 — first good reading | **`no-cause`** | **a DDATA changes the value** — the one thing v11 could not do |
| 3 — values update | `no-cause`, unchanged | a tag that updates does not update *arbitrarily* |
| 4 — STALE, node online | **`reading-too-old`** | [#107] repaired |
| 5 — after a rebirth | `reading-too-old`, **unchanged** | the repair does not depend on a rebirth any more |

**Step 5's line is the one worth keeping.** Under v11 a rebirth was the *only* way to move the
property; under v12 it changes nothing, because the DDATA had already done it. A rebirth that
still mattered would have said the dependency was not gone.

**And the cause tag is `Good` while its measurement is `Bad_Stale`** — checked at step 4, and it is
the check nobody thinks to make, because one reads the value of a tag one has just found. A tag
that explains a fault must not be marked untrustworthy during the fault.

#### The rest of the run

- **`Contract/Version` = 12** read by the host.
- **Step 4** — `Bad_Stale` on both, node **online**, `Death Count = 0`, values **frozen** at `2,35`
  and `5 679,1`.
- **Step 5** — one write, **one NBIRTH and one DBIRTH**, `bdSeq unchanged at 0 ✓`. **[#100]
  confirmed on the wire for the second time that evening**: this bridge had no persisted state and
  was born under zero.
- **Step 6** — two certificates **2.002 s apart** (20:53:04.128 and 20:53:06.130), both `INFO`, and
  the Engine module logged **four lines in the whole export** — the two death pairs of the evening
  and nothing else, no `WARN`, no `ERROR`, for the third session running. `Death Count` 0 → 2.
  `Offline DateTime = 20:53:06`, **the will**, which is now the fourth run to agree. Window noise:
  **121 lines, 38 `WARN`, 9 `ERROR`**, none ours.
- The cause tags **keep `reading-too-old` through the node's death**, which is the honest answer:
  it is the last thing known, and nothing new is being asserted.

> **A query can pair the wrong two deaths.** The first pass over this export reported *1848 s*
> between the certificates — it had matched v11's 20:22 death with v12's 20:53 one, because the
> filter named the node and not the group, and three groups ran that evening. Recomputed on the
> v12 pair alone: 2.002 s. **Filter by group, not by node**, when a day has carried more than one.

### What the 2026-08-22 20:22 session established

Ignition **8.3.7 Maker Edition**, MQTT Engine **5.0.0-rc1**, contract **v11**, group
`ContractV11`, node `BridgeContractNode`, meter `30000001`. **Six steps, all passed** — and the
change v11 was made for does not work.

#### A metric property is written by a BIRTH and by nothing else

Three observations, one session, one virgin group:

| Gesture | What the host did |
| --- | --- |
| the cold-start BIRTH declares `Cause = no-reading-yet` | the property **appears**, with that value |
| a DDATA carries `Cause = reading-too-old` | **ignored** — the row does not move |
| a rebirth's BIRTH re-declares it | the host **takes** the new value |

**And the pairing that makes it airtight was read on the wire at the same instant**, with
`observe_cause_property` subscribed to `spBv1.0/ContractV11/#` while the operator watched:

```
spBv1.0/ContractV11/DDATA/BridgeContractNode/30000001
  Power    quality 2147484164 (Bad_Stale)   · Cause = reading-too-old
  Energy   quality 2147484164 (Bad_Stale)   · Cause = reading-too-old
```

| | On the wire | In the Designer |
| --- | --- | --- |
| quality | `2147484164` (Bad_Stale) | **`Bad_Stale`** — updated |
| `Cause` | **`reading-too-old`** | **`no-reading-yet`** — frozen |

**The quality updates from a DDATA and the property does not.** That is the sharpest form of the
finding, and it is not "properties do not cross": they cross exactly once, at BIRTH, and never
move again. Our bytes were established by reading them, not by trusting the unit tests — the
instrument that read them refuses to conclude when it sees nothing, and it did refuse twice
before the window was synchronised with the operator.

**So ADR 0043's remedy is wrong** — it measured *declare at BIRTH or it does not exist*, which is
true, and inferred that declaring would be enough, which nothing established. ADR 0044 carries the
cause as a **metric** instead. The workaround does exist — force a rebirth on every cause change —
and it is impracticable: a rebirth republishes the node's whole tree to move a twenty-character
string.

**And v11 as shipped is actively harmful, which is why it is being replaced rather than left.**
Under v10 the operator saw nothing; under v11 they see `no-reading-yet` beside a healthy meter,
for ever. Silence is uninformative, a stale cause is false.

#### [#100], confirmed against a real host without being looked for

`bd_seq=0` in the rebirth's log line and in the gate's own verdict — `bdSeq unchanged at 0 ✓`.
This bridge had no persisted state, and it was born under zero as
`tck-id-topics-nbirth-bdseq-increment` requires. That morning it would have been born under 1.

#### The rest of the run

- **Steps 1–3** — cold start with `Contract/Version = 11` read by the host; `1,23` / `5 678,9`
  then `2,35` / `5 679,1`, all `Good`.
- **Step 4** — `Bad_Stale` on both with the node **online** and `Death Count = 0`, values **frozen**
  at `2,35` and `5 679,1`. The guarantee holds at v11 exactly as it held at v10.
- **Step 5** — one write, **one NBIRTH and one DBIRTH gained**, `bdSeq unchanged at 0 ✓`.
- **Step 6** — two certificates **2.063 s apart** (20:22:17.440 and 20:22:19.503), both `INFO`, and
  the Engine module logged **nothing else in the whole export**. `Death Count` 0 → 2.
  `Offline DateTime = 20:22:19` — **the will**, which is now the third run to agree. Window noise:
  **123 lines, 40 `WARN`, 9 `ERROR`**, none of them ours.

### What the 2026-08-22 morning session established

Ignition **8.3.7 Maker Edition**, MQTT Engine **5.0.0-rc1**, contract **v10**, group
`ContractV10b`, node `BridgeContractNode`, single meter `30000001`. **One pass, six steps, all
passed.** NFR17 is re-attested at v10; R3 falls and milestone 3 is reached.

The group was new, which is what the 2026-08-21 session's own finding asked for. Clean-up done.

#### The deaths, measured in Ignition's log rather than in its browser

The Gateway log was exported and queried — 3 h 10 of it, 13:50 to 17:00. The **whole export
contains exactly two lines from the Engine module**, and both are `INFO`:

```
16:59:29.187  INFO  SparkplugPayloadHandler  Handling LWT message for Edge Node
                                             ContractV10b/BridgeContractNode
16:59:31.188  INFO  (idem)
```

**2.001 s apart** — the explicit NDEATH, then the will. Three things follow, and none of them was
available from the tag browser:

- **Both certificates were processed**, matching `Death Count` 0 → 2 and the 2026-08-03 run.
- **Ignition did not complain.** Not a `WARN`, not an `ERROR`, nothing about a duplicate session,
  in three hours of log. The second death is treated as a harmless repeat — which is the question
  this step exists to ask, and it now has a direct answer rather than an inference from a counter.
- **`Offline DateTime` = 16:59:31, the SECOND death — the will.** This **confirms the 2026-07-31
  probe** and settles what the 2026-08-03 run left unobserved: ADR 0011's claimed benefit, *"the
  explicit certificate is immediate"*, is **not observable from the host side on this field**. The
  host keeps the last certificate, not the first.

Both of the step's false-pass paths were closed by the same query rather than by eye: a keep-alive
timeout takes ~30 s and these are instant and paired to a certificate; and the log names
`ContractV10b/BridgeContractNode`, so it is not another node going offline.

**And the noise justifies the method, measured this time rather than asserted.** In the window
16:59:20–16:59:45 the log carries **109 lines — 9 `ERROR` and 32 `WARN`** — all from a
`TransmissionClient` that cannot reach `tcp://localhost` and from Modbus timeouts. **The two lines
that matter are 2 of 109.** An operator scrolling would have seen nine `ERROR`s at exactly the
right moment and reported a failure that has nothing to do with the death.

#### A trap this session found: `Death Count` read between the two deaths

The first reading taken was **`Death Count` = 1**, and it was about to be recorded as a divergence
from the 2026-08-03 run's 0 → 2. It was not a divergence: the field had been read in the two-second
gap between the explicit certificate and the will. **Read it, then read it again a few seconds
later.** A single early reading of this field manufactures a discrepancy with the only other
complete run in this table.

#### [#107], measured on a virgin group — and what the measurement still cannot say

Step 4, after folding and unfolding the tag: **no `Cause` line**, and the tag editor's `Custom`
category carries nothing. The 2026-08-21 observation is confirmed without the confound of a reused
group, so **the operator does not see why a value is not good**. That is the fact, and it is firm.

**The measurement has no positive control, and that limits what it proves.** `Cause` is the only
custom metric property this bridge publishes (`METRIC_PROPERTY_CAUSE`,
`sparkplug_publisher.rs:194`); `EngUnit` and the quality are properties Engine maps onto native tag
properties, so they prove nothing about the custom path. Two mechanisms remain compatible with the
observation:

- **(a)** Engine materialises a property only when a BIRTH declared it — what [#107]'s title asserts;
- **(b)** Engine never surfaces custom metric properties to the Designer at all.

The distinction decides the repair: under (a) the fix is to declare `Cause` at BIRTH; under (b)
declaring changes nothing and the cause must travel as a **metric**. One manoeuvre answers both —
publish a BIRTH declaring `Cause` with a neutral value and look — and it is the same manoeuvre as
the arbitration story 2.1's task 3 has been blocking.

#### Two steps that cannot measure what they ask, both found by trying

- **Step 1's datatype is not readable where an operator would look.** The Tag Browser lists tag
  properties and `DataType` is not one; the **Tag Editor does not carry it either** for an Engine
  tag, because the module owns the type and does not offer it for editing. What answers is the
  Designer's **Script Console**:

  ```python
  for r in system.tag.browse("[MQTT Engine]", {"recursive": True, "tagType": "AtomicTag",
                                               "name": "Power"}).getResults():
      print(r['fullPath'], "->", r['dataType'])
  ```

  It returned **`Float8`**, so the null metric was accepted with its declared datatype. The editor's
  presence of a **`Numeric`** category is a weaker form of the same evidence, available without
  scripting.

- **Step 2's timestamp clause is not measurable by this gate at all.** The prose asks the operator
  to confirm the timestamp is the value's acquisition time rather than the moment Ignition received
  it — but the gate stamps its readings with `clock.wall()`, the current instant
  (`ignition_contract.rs:373` → `value_date: now`). Acquisition, publication and reception fall in
  the same second, so the check passes identically whether the bridge stamps at acquisition or at
  reception. **A step that can only pass measures nothing.** The repair is small and is not yet
  made: publish step 2's reading stamped ~90 s in the past, and the tag timestamp must then read
  visibly behind the wall clock. Until then this clause is **not attested** and the gate's own
  printed checklist is the honest one — it asks only that the timestamp *moved*.

#### The rest of the run, in one line each

- **Step 1** — folder named by the serial `30000001`, both metrics created null with `Float8`,
  `EngUnit` = `kW` / `kWh`, no invented `0`.
- **Step 2** — `1.234` / `5678.9`, both `Good`. **The `EngHigh = 100` clamp trap was not armed on
  this installation**: the tags carry `Scale Mode = Off` and `No_Clamp`, so `Energy` displayed
  `5 678,9` in full. Worth knowing before treating a clamped `100` as a unit bug elsewhere.
- **Step 3** — `2.345` / `5679.1`, counter up, no rebirth and no reconnect needed.
- **Step 4** — `Bad_Stale` on both with the node **online** and `Death Count = 0`, and **the values
  frozen rather than blanked**: the `value` row showed `2,35` and `5 679,1`. The [#108] repair works
  — the question is now asked where it can be answered.
- **Step 5** — the operator wrote `true` **twice**. Both requests were accepted from
  `spBv1.0/ContractV10b/NCMD/BridgeContractNode`, each answered by its own re-announcement (57 ms
  and 0.4 ms later), and the gate counted **2 NBIRTHs and 2 DBIRTHs gained** — one of each per
  request, so the ratio is what the step asks for even though the absolute count is not.
  `bdSeq unchanged at 1 ✓`, which on its own excludes a reconnect. No `reason=Retained`, no
  near-miss `WARN`.

### What the 2026-08-21 session established, and what it left owed

Ignition **8.3.7 Maker Edition**, MQTT Engine **5.0.0-rc1**, contract **v10**, group
`ContractV10`, node `BridgeContractNode`, single meter `30000001`. **Two passes of the bridge gate
on the same group**, an hour apart; neither reached step 6.

**Read the negative first.** Step 6 was never performed, so the two death certificates were not
observed and **NFR17 is not re-attested at v10**. The 2026-08-03 row remains the only complete
run in this table, and it attests to v3. R3 stays *avérée* and milestone 3 stays unreached.

The MQTT Engine version was asked for twice during the session and not supplied on the day —
but **it was already on record, and this document is where it is recorded**: `5.0.0-rc1`, given
by Guy on 2026-07-28 during story 4.4's task 4, and carried by the **2026-08-03 row of this very
table, two lines above**. It is also in `docs/primary-host-state-observation.md` and in ADR 0011.

Nothing was owed here. The cell is filled from the record, and **the question is closed** —
arbitrated by Guy on 2026-08-22, who had supplied the version and pointed out that it was held.
The *Maker Edition* wording on this row and not on the 2026-08-03 one describes the same
installation, and is not evidence the module moved.

> **Corrected 2026-08-22.** Until this date the paragraph read *"that is the third row in this
> table carrying `(not recorded)`"* and named the gap the finding worth keeping. That was false:
> the first two runs recorded the module version, and the correction is Guy's — he had supplied it
> and said so. The lapse is instructive in its own right, and it is the one `CLAUDE.md` already
> names for the smart-me specification: **the claim was carried over from the session's own notes
> instead of being checked against the register printed directly beneath it.** A document that
> holds the answer is not consulted merely by being open.

#### What passed

- **The guarantee held, and this is the point of the exercise.** Steps 2–3 published `Good`
  (`1.234` / `5678.9`, then `2.345` / `5679.1`); step 4 republished the same values as `Stale`
  with the node still **online** and `Death Count = 0`. Ignition displayed **`Bad_Stale`** on
  both metrics. The transition is the evidence, not the state: a tag Ignition has just created
  reads `Bad_Stale` on its own, which is why step 1 proves nothing about quality and step 4
  proves everything.
- **The values FROZE rather than blanking**, and this is now observable where it was thought not
  to be: the tag's `value` row showed `2,35` and `5 679,1` in red italics beside
  `Quality = Bad_Stale`. At step 1 the same row showed `Bad_Stale` because there was no value at
  all. The two states are distinguishable — the runbook should say where to look, which it did
  not.
- **The cold start survived contact**: null metrics accepted with their datatype, no invented
  `0`, `EngUnit` = `kW` / `kWh`, device folder named by the serial `30000001`.
- **`Contract/Version = 10` was read by a real host** for the first time.
- **The bridge answered IGNITION**, in both passes: NCMD received on
  `spBv1.0/ContractV10/NCMD/BridgeContractNode`, metric `Node Control/Rebirth`, written from the
  Designer. Exactly one NBIRTH and one DBIRTH gained, `bdSeq unchanged at 1 ✓`.
- **[#44]'s two log guards fired**, which settles by observation what had been settled by reading
  the code: `Rebirth Request accepted` and `node re-announced on a Rebirth Request` both reached
  the operator, as distinct events. No `reason=Retained`, no near-miss WARN.
- **The Rebirth control lives at** `Edge Nodes/<group>/BridgeContractNode/Node Control/Rebirth` —
  a metric name containing `/` becomes a folder, as this document already noted for
  `Contract/Version`.

#### The finding: contract v4's `Cause` does not cross the host unless a BIRTH declares it

| When | On the wire | In the tag browser |
|---|---|---|
| step 1, cold-start DBIRTH | `cold_start_metrics` attaches **no property** | — |
| step 4, DDATA `Stale` | `metrics_for` attaches `Cause = reading-too-old` (`sparkplug_publisher.rs:721`, pinned by its own unit tests) | **no `Cause` row at all** |
| step 5, rebirth DBIRTH | metrics re-announced **with** their properties | **`Cause = reading-too-old`** |

Observed twice, and the second time within a single pass: absent at 15:59 after step 4, present
at 17:14 after the rebirth, with the wire showing exactly one NBIRTH and one DBIRTH gained in
between. `Power`'s `Timestamp` stayed at 15:56:39 across the rebirth, which is consistent — a
`Stale` metric is republished with its own `ValueDate`.

**What it costs.** Contract v4 exists so that an operator sees *why* a value is not trustworthy.
A meter that is healthy at its BIRTH and degrades later publishes its cause in DDATA only, so
**nobody will ever see it** — unless a rebirth happens to intervene. The property is on the wire
and stops at the host.

**This answers [#68]**, and it unblocks the arbitration story 2.1's task 3 has been waiting on
since 2026-08-10: whether to declare `Cause` at BIRTH with a neutral value, at the cost of
contradicting *"a good metric carries no cause"*. That decision now has its measurement.

**The residual, named rather than buried.** The group was **reused** across the two passes rather
than being fresh, so the second pass began against tags Ignition had already garnished — and the
sound experiment has one variable, not two. Against it: at 15:59 the pane had rendered step 4's
quality, value and timestamp, so it was not stale as a whole; for it: `Node Control/Rebirth` kept
its 15:56:26 timestamp across a NBIRTH that re-declared it, which shows this pane does not
refresh everything. **To seal it**: one pass on a group Ignition has never seen, collapsing and
re-expanding the tag at step 4 before reading. Ten minutes, and it should be done before any
contract change is decided on this basis.

#### Two defects in the gate itself, found by running it

- **Step 1's printed checklist still says `Contract/Version is present and reads 3`**, and the
  line below it explains a `2`. The bridge emits **10**. An operator following the list to the
  letter records a failure where there is a success — the same shape as [#44]'s reconnect
  wording: a printed instruction left false by a contract bump.
- **Step 4's item *"the value blanked instead of freezing"* is not checkable where it is asked.**
  The Value column renders the quality string for any non-good tag, so frozen and blanked look
  identical there. The `value` sub-row is where the number survives, and the checklist should say
  so.

#### One observation about the instrument, for whoever runs this next

Ignition's `FormatString` defaults to `#,##0.##`, so the tag browser **rounds to two decimals**:
`1.234` reads `1,23` and `2.345` reads `2,35`. Step 2 and step 3 ask the operator to check the
values *exactly*. A factor of 1000 still shows; a discrepancy in the third decimal does not.
Mid-session this produced a false alarm — a `Power` display one refresh behind read as a metric
that had not updated — which cost ten minutes and nearly cost the step.

And `EngHigh = 100` / `EngLow = 0` are Ignition's defaults: `Energy = 5678.9` is fifty-six times
`EngHigh`, so a tag with scaling enabled would clamp it to `100` and look exactly like a unit bug
in the bridge. It did not happen here; it is one refresh setting away from happening.

#### `bdSeq` starts at 1, on the wire, against a real host

The gate runs on a fresh state directory, so its first published `bdSeq` is the first this node
has ever published — and both passes printed `bdSeq = Some(1)`. That is [#100]: chapter 4
requires the first session to be numbered zero. The issue was opened 2026-08-19 from the code;
this is the same fact observed on the wire during a Tier-3 session.

[#44]: https://github.com/guycorbaz/smartme_mqtt/issues/44
[#100]: https://github.com/guycorbaz/smartme_mqtt/issues/100
[#107]: https://github.com/guycorbaz/smartme_mqtt/issues/107
[#108]: https://github.com/guycorbaz/smartme_mqtt/issues/108

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

> **These rows attest to contract v3, and the contract is now v10.** Story 2.1 (2026-08-10)
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

> **Settled on 2026-08-22, and it tracks the will.** The 16:59:31 reading is the second of two
> certificates 2.001 s apart, timed against Ignition's own log. Two independent runs now agree, so
> ADR 0011's benefit is real on the wire and invisible on this field. See the 2026-08-22 section.

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
