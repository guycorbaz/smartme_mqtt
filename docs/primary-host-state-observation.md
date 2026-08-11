# Primary Host / STATE — what this deployment actually publishes

**Story 4.4.** A read-only observation of the production broker, made 2026-07-28. It publishes
nothing; the tool is `crates/smartme-bridge/tests/observe_primary_host_state.rs`.

> **STATUS: complete, and reviewed 2026-07-29.** An adversarial review of three independent layers
> corrected this document in eleven places — most consequentially, a birth that was validated against
> the **will** clauses, an ACL confound recorded as eliminated on unsound evidence, a
> `-host-reordering-rebirth` citation that does not describe the scenario it was attached to, and an
> *Open questions* discriminator that cannot work. Every correction names what it replaces rather
> than overwriting it. The relevance tally moved from `9 · 2` to `10 · 1`.
>
> **STATUS: complete.** The record (*Record of runs*, *What was found*) is measured; the
> interpretation (*Interpreting the result*) was written 2026-07-29 from it, with **no second
> observation** — see *Was a second restart worth it?*. This document **measures and rules on
> relevance; it decides nothing.** Story 4.5 decides whether to implement Primary Host support, and
> the conformance-matrix verdicts are untouched by this story.

## Why this exists

The Epic 1 retrospective found that Primary Host / STATE was absent from every planning artifact —
a whole Sparkplug mechanism nobody had considered. Story 4.3's conformance audit then filed **eleven
`gap (unimplemented)` clauses** against it. Story 4.5 must decide whether to implement it; this
document is the evidence it decides on, so that the decision rests on this deployment's behaviour
rather than on a reading of the specification.

## Before you start

- **The broker is production and Ignition is live on it.** This observation is read-only and the
  tool has no publish path at all — verified mechanically, not by inspection:
  `grep -cE "\.publish\(|try_publish|set_last_will"` over the test returns **0**.
- **The address is never committed.** `SMARTME_STATE_BROKER=host:port`, no default — deliberately a
  different variable from the bridge's own `SMARTME_BROKER_HOST`, so that pointing an observer at
  production is always an explicit act.
- **Two observers need two client ids.** A broker evicts the older session when a client id
  reconnects, so `SMARTME_STATE_CLIENT_ID` exists; without it the two would silently unplug each
  other, and watching both topic shapes *simultaneously* is what produced the central finding.

## Running it

```bash
SMARTME_STATE_BROKER=host:1883 \
SMARTME_STATE_FILTER='STATE/#' \
SMARTME_STATE_CLIENT_ID='obs-legacy' \
SMARTME_STATE_SECONDS=900 \
  cargo test -p smartme-bridge --test observe_primary_host_state -- --ignored --nocapture
```

Run a second instance with `SMARTME_STATE_FILTER='spBv1.0/#'` and a different client id. Arm both
**before** the restart; the transcript prints when the window closes.

## What each step proves — and what else could produce it

`CLAUDE.md` requires a human-run gate to state, per step, what *else* could produce the same result.
Two of these were live in this run, and one of them fooled the first pass completely.

### Step 1 — subscribe to `spBv1.0/STATE/#` and record what arrives

**Proves:** whether a Sparkplug 3.0.0-conformant Host Application is publishing STATE *right now*.

**What else could produce silence here — five candidates, four eliminated:**

| Candidate | Eliminated by |
| --- | --- |
| The subscriber decodes protobuf and **discards** what fails (`common::named_subscriber_on`) | A fresh observer that decodes nothing. **Measured**, not reasoned: `sparkplug_b::decode` rejects real-shaped STATE JSON with *"buffer underflow"* |
| A topic filter that does not match | Re-ran with `#`; found the traffic one level up on `STATE/<id>` |
| A broker ACL hiding the topic | **The broker is unauthenticated and enforces no per-topic ACL**, so there is nothing to withhold. *(An earlier edition of this row offered "the `#` sweep returned 78 messages on 61 topics" as the eliminator. That is unsound — an ACL is per-topic, and traffic on 61 other topics says nothing about `spBv1.0/#`. Step 2 below says as much in its own words. The observer now also checks the SubAck return code and fails the run outright on a refusal, so a future run eliminates this mechanically rather than by argument.)* |
| No Host Application configured at all | A host id *was* found, and later proved live by the restart |
| **The host publishes 3.0 STATE but has not reconnected since it was last configured** | **NOT eliminated — and this is what was actually true.** Only a state transition can eliminate it. See step 4 |

### Step 2 — sweep `#` and count

**Proves:** the broker is busy and reachable, so silence on a narrow filter is a property of the
topic and not of the connection.

**What else could produce a large count:** a busy broker proves reachability, not that STATE would
have been visible had it existed. This step **cannot** distinguish "no `spBv1.0` publisher" from "a
`spBv1.0` publisher that is currently quiet **and** has no retained message". That distinction is
the whole of finding 4, and no snapshot can make it.

*The 61 topics are deliberately not listed: unrelated home-automation traffic, some carrying device
identifiers, and this repository is public. Only the count is recorded, which is all the argument
needs.*

### Step 3 — subscribe to `STATE/#` and read the retained set

**Proves:** which host ids have *ever* published on the legacy topic, and their last known value.

**What else could produce a retained `ONLINE`/`OFFLINE`:** a retained message outlives its publisher
indefinitely. **This step cannot tell a live host from a client that died months ago** — which is
exactly what three of the four ids turned out to be. Reading this set as "four hosts" would have
been wrong; reading it as "four ids that once existed" is what it supports.

### Step 4 — arm both observers, then restart Ignition ← the step that carries the story

**Proves:** what the host *does*, as opposed to what the broker *remembers*. `retain=true` at
subscribe is a stored snapshot; `retain=false` is a delivery that happened while we were watching.

**The flag is not symmetric, and the write-up must not treat it as if it were.** MQTT 3.1.1 requires
a broker to clear the RETAIN flag when it delivers a retained message to a client that was *already*
subscribed. So `retain=true` proves a snapshot, but `retain=false` proves only *live delivery* — it
is silent about whether the message was **published** retained. That is why finding 1 scores "retain
true" from the settled-state re-read rather than from the live transition. (MQTT 3.1.1 is **not**
vendored here, so this is cited as the reason for a caution, never as evidence for a verdict — the
same bound as [#34](https://github.com/guycorbaz/smartme_mqtt/issues/34).)

**What else could produce each observation:**

| Observation | What else could produce it | Status |
| --- | --- | --- |
| `online: true` seen | The retained snapshot of a session established days ago | **Eliminated** — it arrived `retain=false` |
| An `OFFLINE` seen | An explicit death published by Ignition **or** the broker's will firing. Both produce an identical retained message on an identical topic | **NOT eliminated.** See *Open questions* — and note the discriminator recorded there, which this run had the means to apply and did not |
| No `spBv1.0` traffic *(step 1's conclusion)* | The host genuinely not implementing 3.0 — **the conclusion drawn, and it was wrong** | **Eliminated by this step.** A snapshot describes the broker's memory, not the host's behaviour |
| One restart looks conclusive | One sample | **NOT eliminated.** One restart was observed, and every claim below is a one-sample claim unless it says otherwise |
| `ONLINE` seen **twice** on `STATE/SCADA` | The broker **redelivering** one QoS-1 message, not the host publishing two. A redelivery carries the MQTT `dup` flag and reuses the packet id | **NOT eliminated for this run.** The observer captured neither `dup` nor `pkid` at the time, so "published twice" is recorded as what was *delivered*, not as what the host *did*. Both are now captured, so a repeat settles it |

**The third row is the lesson of this story.** Three careful, honest passes supported the conclusion
that this host does not speak Sparkplug 3.0. One state transition destroyed it. Had the acceptance
criterion been scoped to a snapshot instead of *"across an Ignition restart"*, Story 4.5 would have
inherited a confident, evidenced and false premise.

## Record of runs

**Environment:** Ignition 8.3.7, **MQTT Engine module v5.0.0-rc1** (recorded here because the module
decodes Sparkplug and governs conformance more directly than the platform version — the residual
Story 1.15 left open). Mosquitto broker on the LAN, unauthenticated.

> **Redaction decision, recorded because the story's own rule requires it either way.** The host ids
> below (`SCADA`, `scada`, `ignition`, `IamHost`) are **published unredacted, deliberately.** They
> are generic Sparkplug host identifiers, carry no site, customer or network information, and the
> whole `-phid-wait-id` ruling turns on their exact spelling and case — redacting them would destroy
> the finding. The broker address is never committed; the 61 unrelated topics are not listed because
> some carry device identifiers. The product versions are kept because the conformance argument is
> specific to MQTT Engine v5.0.0-rc1 and would be unverifiable without them.

### Run 1 — steady state, before the restart

| Filter | Window | Result |
| --- | ---: | --- |
| `spBv1.0/STATE/#` | 25 s | **0 messages** |
| `#` | 20 s | 78 messages, 61 distinct topics, **not one `spBv1.0/…` topic** |
| `STATE/#` | 8 s | **4 retained messages** |

```
retain=true  qos=1  STATE/scada     "OFFLINE" (7 bytes)
retain=true  qos=1  STATE/IamHost   "OFFLINE" (7 bytes)
retain=true  qos=1  STATE/ignition  "OFFLINE" (7 bytes)
retain=true  qos=1  STATE/SCADA     "ONLINE"  (6 bytes)
```

*The block above is a **transcription**, not the observer's own output — the tool renders QoS as
`AtLeastOnce` and prints one message per multi-line stanza. The `61 distinct topics` figure was
likewise counted by hand at the time; the instrument did not compute it. It does now, so a future
run can be pasted verbatim and reproduced. Retyped evidence is exactly the kind of claim this
project otherwise refuses, and it is flagged here rather than left for a reader to notice.*

### Run 2 — across an Ignition container restart

Both observers armed at **20:25:24 local**, 900 s window, distinct client ids. Guy restarted the
Ignition container. `retain=true` is a stored snapshot replayed at subscribe; `retain=false` is a
live publish.

| Observer | Message | retain | Payload |
| --- | --- | :---: | --- |
| `spBv1.0/#` | at subscribe | true | 42 bytes — `{"online":false,…}` |
| `spBv1.0/#` | **live** | **false** | 41 bytes — `{"online":true,"timestamp":1785263196684}` |
| `STATE/#` | at subscribe | true | `OFFLINE` on all four ids |
| `STATE/#` | **live** | **false** | `ONLINE` on `STATE/SCADA` — **twice** |

The death arrived **retained**, so the shutdown had begun before the observers finished subscribing:
the live `online:false` transition was not witnessed, only its stored result. That is why its origin
is undetermined, and it is a procedural finding for any repeat — *arm, confirm the subscription, and
only then stop the container*.

> **What this record does NOT contain, named rather than left to be discovered.** AC1 asks for
> *topic, raw payload, retain flag and QoS* per message. For the death — the one message whose
> content would have answered open question 1 — only the **byte count** and an elided payload were
> written down. **Its topic was not transcribed either**, so this record cannot by itself prove the
> death was on `spBv1.0/STATE/SCADA` rather than another `spBv1.0/` topic; that it was is inferred
> from the birth overwriting it on that topic. No per-message receive times were transcribed for
> either run, although the observer prints them as `+Nms` offsets. These are **transcription**
> failures, not tool limitations, and they are the reason AC1 is recorded as *met with named
> shortfalls* rather than simply met. A repeat must paste the transcript rather than retype it.

### Settled state after the restart

```
retain=true  qos=1  STATE/scada           "OFFLINE"                                  (7 bytes)
retain=true  qos=1  STATE/IamHost         "OFFLINE"                                  (7 bytes)
retain=true  qos=1  STATE/ignition        "OFFLINE"                                  (7 bytes)
retain=true  qos=1  STATE/SCADA           "ONLINE"                                   (6 bytes)
retain=true  qos=1  spBv1.0/STATE/SCADA   {"online":true,"timestamp":1785263196684}  (41 bytes)
```

## What was found

### 1. MQTT Engine v5.0.0-rc1 publishes **both** forms, and the 3.0 form is fully conformant

Checked clause by clause against the vendored `docs/spec/sparkplug-b-3.0.0/`:

The observed message is a **birth** (`"online":true`), so it is checked against the **birth** clauses.
An earlier edition of this table cited `-connect-will-topic` and `-connect-will-payload` for the
first two rows. That was wrong and not merely imprecise: the will clause requires *"one key MUST be
'online' and it's value is a boolean **'false'**"* (`:760-764`) — it is the **death** certificate
rule, and an observed `{"online":true,…}` fails it. The two clause families are near-identical in
shape, which is exactly why this project cites `tck-id`s instead of paraphrasing them.

| Requirement | Clause | Observed | |
| --- | --- | --- | :---: |
| Topic `spBv1.0/STATE/sparkplug_host_id` | `-connect-birth-topic` (`:776-778`) | `spBv1.0/STATE/SCADA` | ✅ |
| JSON UTF-8, keys `online` (bool `true`) + `timestamp` (number) | `-connect-birth-payload` (`:779-783`) | `{"online":true,"timestamp":1785263196684}` | ✅ |
| Retain true | `-connect-birth-retained` (`:786-787`) | `retain=true` | ✅ |
| QoS 1 | `-connect-birth-qos` (`:784-785`) | `qos=1` | ⚠️ see below |

**The QoS row is one notch weaker than a ✅ suggests, and is marked accordingly.** Delivered QoS is
bounded by the *subscription*, which was made at QoS 1. The observation therefore proves the host
published at **at least** QoS 1 — it cannot distinguish 1 from 2. The clause is a MUST on a specific
value, so what is established is "not lower than the MUST", not "equal to it". (The observer now
records the *granted* QoS from the SubAck too, because a broker downgrade would have made every
delivered message read `AtMostOnce` and turned this into a host non-conformance that never happened.)

**The death clauses are NOT verified for this host.** `-connect-will-payload` and the
`-host-application-death-*` family govern the `{"online":false,…}` message, whose payload was never
transcribed. Finding 1's claim of conformance covers the **birth** only.

It **also** publishes a legacy form on `STATE/SCADA` carrying the bare literals `ONLINE` / `OFFLINE`,
which matches no clause of the 3.0.0 specification. Whether that is a pre-3.0 convention cannot be
settled here: **only 3.0.0 is vendored**, and it carries no changelog. Same bound as
[#34](https://github.com/guycorbaz/smartme_mqtt/issues/34), where the MQTT character set could not be
cited for the same reason.

**The bridge can implement the specification as written against this deployment.**

### 2. The host id is `SCADA`, and only the restart proved it

Only `STATE/SCADA` and `spBv1.0/STATE/SCADA` moved, which is what proves Ignition owns **`SCADA`**.

**What the other three are is an inference, and it is not proven by this run.** `scada`, `ignition`
and `IamHost` stayed frozen at `OFFLINE` throughout, and a retained message does outlive its
publisher indefinitely — so *"residue from clients long gone"* is the plausible reading. But step 3
above states that this observation **cannot tell a live host from a client that died months ago**,
and one restart of *Ignition* does not test them: a second host that simply did not restart during
the window is observationally identical. Treat them as *"ids that once existed and did not move"*,
which is what the evidence supports.

`scada` and `SCADA` differ only in case. The specification's case clause,
`tck-id-case-sensitivity-sparkplug-ids` (`:63-67`), does **not** cover this: it binds *"Edge Nodes …
Sparkplug IDs (Group, Edge Node, or Device IDs)"*, and a `sparkplug_host_id` is none of those. The
host-id clause is `-operational-behavior-host-application-host-id` (`:753-754`), and it requires
uniqueness, not case-distinctness. So the case collision here is a **deployment hazard with no clause
behind it** — which is a weaker and more accurate thing to say than the earlier edition said.

### 3. The timestamp is real, so the anti-replay clauses are implementable here

`1785263196684` decodes to **2026-07-28 18:26:36 UTC**, i.e. 20:26:36 local — 72 seconds after the
observers were armed at 20:25:24, and therefore inside the window. A genuine epoch-millis value in a
plausible place, so `-phid-wait-timestamp` and `-termination-host-offline-timestamp`, which compare
successive STATE timestamps, have something to compare. The legacy form carries no timestamp at all,
which is why the steady-state pass had wrongly concluded they were not.

*An earlier edition said "about 90 seconds before it was read" and concluded from it that the host's
clock is sane. Neither is supported by this record: no per-message receive time was transcribed, so
the interval cannot be checked. The comparison is also the one this story's own Dev Notes warn
against — a STATE timestamp is a CONNECT-time value, not a publish time, so comparing it to a receive
clock is how Story 1.1 nearly misread `ValueDate`. What the value supports is that the field is
present, numeric and epoch-shaped. That is all the anti-replay clauses need, and all that is claimed.*

The death payload is **consistent with** carrying one too: 42 bytes against the birth's 41, and
`false` is exactly one character longer than `true`. That is consistency, not proof — it holds only
if the key set, ordering and whitespace matched the birth's, which an elided payload cannot show.
**Its value was not transcribed** and cannot be recovered, the retained death having been overwritten
by the birth on the same topic. See *Open questions* for what that does and does not cost.

### 4. A hazard that survives: the retained 3.0 STATE may simply not exist

`spBv1.0/STATE/SCADA` **did not exist before this restart** — two independent passes found nothing:
`spBv1.0/STATE/#` for 25 s, which a retained message would have been replayed into immediately on
subscribe, and the `#` sweep, which saw no `spBv1.0/…` topic of any kind. *(An earlier edition named
these as `#` and `spBv1.0/#`. No steady-state `spBv1.0/#` pass was run; the conclusion is unaffected,
since `spBv1.0/STATE/#` covers the topic in question, but the passes are now named as they were.)*
The plausible reading is that Engine was upgraded to v5.x and had not reconnected since, so its last
birth predated 3.0 support. **That is a hypothesis, not a measurement.**

What *is* measured: a bridge waiting for `spBv1.0/STATE/<host>` before birthing would have waited
**forever** in the broker state that existed an hour earlier, because the message it waits for had
never been published. **Story 4.5 must decide what the bridge does when the retained STATE is
absent.** "Wait for online" is not safe on its own.

## Interpreting the result

### What an edge node that ignores STATE loses — in this deployment

This is what the bridge did **at the time of this observation (2026-07-29)**: it held **no MQTT
subscription of any kind**. Verified mechanically at the time — `grep -rn "subscribe"
crates/smartme-bridge/src/` returned one `tracing_subscriber` initialiser and two comments, and no
`AsyncClient::subscribe` call anywhere.

> **Superseded the same day, by Story 4.6.** The bridge now subscribes to
> `spBv1.0/{group}/NCMD/{node}` at QoS 1, before every birth. It subscribes to **no STATE topic**,
> so nothing measured here is invalidated — but the blanket phrasing is, and the argument below
> rests on it, so see *"What Story 4.6 changed in this argument"* at the end of this section before
> using any of it.

**It loses nothing at all while Ignition is up.** One broker means no server-walking and no
stranding, and the command path is not what STATE would have given us.
*(Reworded twice. The Story 4.6 code review, 2026-07-29, corrected "accepts no NCMD/DCMD" — since 4.6
the bridge receives them. Story 4.7, 2026-07-30, removed "the bridge acts on no NCMD" outright: it
now acts on `Node Control/Rebirth`. The clause about DCMD stands — none is received.)* For the ordinary
running case, STATE is an offer this deployment makes and the bridge declines at no cost.

**What it loses is recovery after the host restarts — and the loss is total.** The chain is:

1. The bridge's NBIRTH went out **once**, at QoS 0 with `retain=false`
   (`the_delivery_table_matches_the_specification_clause_by_clause`). The broker stores nothing, so there
   is no copy for a returning host to collect.
2. Ignition restarts. **Measured:** the host re-births at 18:26:36 UTC and announces itself live on
   `spBv1.0/STATE/SCADA` with `retain=false`. **Inferred:** that a conformant edge node re-births in
   response. No clause commands an *already-born* Edge Node to re-birth merely on seeing
   `online: true`; the re-birth follows because `-phid-offline` / `-termination-host-offline` made it
   **disconnect on the preceding offline STATE**. An edge node that missed that offline STATE — it
   connected during the outage, or the host sent a clean DISCONNECT so the broker discarded the will
   (`-disconnect-intentional`, `:817-824`, explicitly permitted) — stays born and does not re-birth.
   The comparison baseline is therefore conditional on a step this chain used to leave unstated.
3. The bridge sees nothing — it is not subscribed **to STATE**. **Measured** (`grep -rn
   "subscribe"`; at the time of measurement it was subscribed to nothing at all, and since Story 4.6
   it is subscribed to its own NCMD topic and nothing else — either way it sees no STATE). Its own
   broker session is untouched by Ignition restarting, so **that event makes it neither reconnect nor
   re-birth**. **Inferred, not measured:** the session semantics are read from MQTT, and nothing in
   this record shows the bridge was even running during either run — indeed the `#` sweep found no
   `spBv1.0/…` topic at all, which suggests it was not.
4. The protocol's own repair for a host that arrives without a BIRTH is an NCMD
   `Node Control/Rebirth`. The bridge implemented no Rebirth handling. *(At the time of measurement
   it could not have received the request either; Story 4.6 made it receive and trace one; **Story
   4.7, 2026-07-30, made it answer one — so this leg of the chain is now FALSE and the repair is
   available.** The measurement is unaffected: it recorded the state on 2026-07-28 and that record
   stands. What changes is the argument built on it — see the dated section at the end.)* *(No `tck-id` is cited here on purpose. The clause this
   chain used to name, `-operational-behavior-host-reordering-rebirth` (`:565-568`), is the
   **out-of-order-sequence** remedy and is conditional on the host being *"configured with a
   'reordering timeout' parameter"* — nothing measured says Ignition is. The text that actually
   describes a host arriving late, `:943-951`, is **non-normative** and says the host **can** send a
   rebirth request, not that it must. Either way the bridge could not receive it, so the conclusion
   stands on the bridge's own missing subscription rather than on an obligation of the host's.)*

**So the two gaps compounded: no STATE-wait *and* no Rebirth meant that after an Ignition restart, no
*host-initiated* mechanism could re-establish the bridge's tags.** *(Past tense as of Story 4.7: the
Rebirth half is closed and a host-initiated mechanism now exists. The paragraph is kept in its
original shape because it is the reasoning that produced ADR 0016, and rewriting it would hide why
the ordering was chosen. What it concluded is superseded at the end of this section.)* The DATA keeps arriving at a host
that has no BIRTH to interpret it against, until something makes the bridge reconnect. That is the
deployment-specific evidence behind ranking **Story 4.7** (`Node Control/Rebirth`, the gap recorded
at `-rebirth-action-1/2/3`) above Story 4.5 — a re-ordering that is **decided in
[ADR 0016](adr/0016-rebirth-before-primary-host-wait.md), not here.** This story measures; the
ordering is an architectural position and it gets an ADR and an issue like any other.

**The finding that should change 4.5's shape.** The specification's own motivation for waiting on a
Primary Host is that the Edge Node *"store data while the Host Application is offline … then send
all of its stored data"* (`Sparkplug_5_Operational_Behavior.adoc:191-196`). **The bridge has no
store-and-forward.** So implementing PHID-wait *alone* here would not preserve one single
measurement — it would convert silent publication into deliberate non-publication and recover
nothing. Waiting is worth implementing for what it enables (a clean re-birth on the host's return),
not for the data it saves, and 4.5 should not justify it on the specification's stated grounds.

**One qualification, found while reading chapter 10 rather than by measurement.** "No host-initiated
mechanism restores the tag definitions" is true *of this broker*. A **Sparkplug Aware MQTT Server** —
a distinct conformance profile, enumerated at `Sparkplug_10_Conformance.adoc:71-83` — stores every
NBIRTH and DBIRTH that passes through it and republishes them as retained messages on `$sparkplug`
topics, precisely so a host that arrives late can collect them. **Mosquitto is not one**, so the
chain above holds here; but the remedy exists in the standard and is a third option alongside
STATE-wait and Rebirth. 4.5 should weigh it, and it is cheap to check whether MQTT Engine consumes
`$sparkplug` topics at all.

*Citation caveat: the line range above is the conformance chapter's **enumeration of the profile**,
not the clauses that define the storing-and-republishing behaviour, which live elsewhere with their
own `tck-id`s. This option is decision-bearing for 4.5, so whoever weighs it must pin those clauses
first rather than inherit this paragraph — `CLAUDE.md` asks for the `tck-id`, and this one does not
yet have it.*

**Where the evidence stops.** An earlier edition drew this boundary in the wrong place — it said
*"steps 1–3 are measured"* and located the inference in step 4, which is a specification citation
rather than a proposition about this deployment. Drawn correctly:

- **Measured:** the bridge's NBIRTH is QoS 0 and unretained (step 1); the host re-births live on
  `spBv1.0/STATE/SCADA` (step 2, first half); the bridge holds no subscription (step 3, first half —
  true as measured; Story 4.6 later added an NCMD subscription and no STATE one, which changes the
  sentence and not the boundary).
- **Inferred:** that a conformant edge node re-births in response to the host's return (step 2,
  second half — it depends on having disconnected on the offline STATE first); that the bridge's own
  session is untouched by Ignition restarting (step 3, second half — read from MQTT semantics, and
  the bridge's presence during either run was never recorded); and, the load-bearing one, **that
  Ignition's view of the edge node does not survive its own restart.**

That last was **not observed** — no bridge tag state was checked before or after the restart. It is
**cheaply falsifiable without causing a restart**: with the bridge running, look at its tags in the
Ignition tree after the *next* Ignition restart, whatever its cause. That step belongs in
`docs/ignition-contract-runbook.md` when Story 4.5 or 4.7 next touches it, and it is recorded here
rather than quietly relied on.

**One measured fact cuts the other way and belongs in this section.** The bridge **does** re-birth —
on every MQTT reconnect of its own. `mqtt_driver.rs` emits `Transport::Connected` on every `ConnAck`
and publishes a full BIRTH on it, so a broker restart, a network interruption or a keep-alive expiry
all repair the consumer's view without restarting the bridge. What no event on the *host* side can do
is prompt that reconnect. The loss described above is therefore real but not permanent-until-restart,
and the operator manual was corrected in the same pass for saying otherwise.

### What Story 4.6 changed in this argument

Added 2026-07-29, when Story 4.6 landed the NCMD subscription and made *"no MQTT subscription of any
kind"* false. This section exists because the sentence was **evidence**, not decoration, and
correcting a claim while leaving what it was holding up unexamined is the failure this project has
now paid for four times.

**The conclusion is unchanged. One of its three legs is gone.** The cost of ignoring STATE after an
Ignition restart rested on there being no host-initiated path back to a correct tag tree, and that
rested on three facts holding together:

| Fact | Before 4.6 | After 4.6 |
| --- | --- | --- |
| The NBIRTH is unretained, so a returning host finds no copy | true | **unchanged** — Sparkplug forbids retain |
| The bridge holds no NCMD subscription, so a Rebirth request cannot reach it | true | **false** — the request now arrives and is traced |
| The bridge implements no `Node Control/Rebirth`, so a request changes nothing | true | **unchanged** — Story 4.7 |

Two of three still hold, and **the third leg alone is sufficient** for the conclusion: a Rebirth request
that arrives and is ignored repairs exactly as little as one that never arrives. So the cost stands,
and so does the ranking in ADR 0016.

> **⏹ EXPIRED 2026-07-30, exactly as the note below said it would.** Story 4.7 landed. Leg three is
> now **false**: the bridge implements `Node Control/Rebirth` and answers a conformant request with a
> complete birth sequence. All three legs of the table are therefore resolved —
>
> | Fact | After 4.7 |
> | --- | --- |
> | The NBIRTH is unretained | **unchanged** — Sparkplug forbids retain |
> | The bridge holds no NCMD subscription | **false** since 4.6 |
> | The bridge implements no `Node Control/Rebirth` | **false** since 4.7 |
>
> — and since only leg three was load-bearing, **the conclusion it supported does not stand.** A
> host-initiated path back to a correct tag tree now exists. The cost of ignoring STATE is not zero,
> but it is no longer *"total loss of recovery"*: it is *"recovery depends on the host asking"*.
>
> **The ranking in ADR 0016 has been carried out and its argument is spent.** Story 4.5 must be
> re-weighed on its own evidence — chiefly the finding recorded elsewhere in this document that
> PHID-wait without store-and-forward preserves no measurement, which 4.7 does not touch. Nothing in
> this paragraph should be cited as a live reason for or against 4.5.

*Corrected by the Story 4.6 code review, 2026-07-29. This sentence read "**either** one alone is
sufficient", which is false and false in the direction that matters. The unretained NBIRTH does **not**
on its own imply the absence of a host-initiated repair path — a working Rebirth handler repairs
precisely **because** births are unretained; that is the mechanism Sparkplug provides for it. Only the
missing handler is load-bearing, which is what the justification in the same sentence actually argues.
The consequence of getting this wrong is dated: once Story 4.7 lands, leg three goes false, and anyone
re-checking this section under the old rule would see leg one still standing and conclude the cost of
ignoring STATE still stands — when it would not. **Read after 4.7 lands: this argument expires then,
and 4.5 must be re-weighed rather than inherited.** The same false clause was copied into the story
file's Completion Notes and `sprint-status.yaml`; both are corrected. ADR 0016's own wording was
already right — it says the conclusion "needs both halves" — so it was left alone.*

**But the argument for that ranking is now stronger, not weaker, and the difference is worth
stating.** *(Written at Story 4.6; superseded by the expiry box above. Kept because it records what
was believed when the ordering was acted on.)* ADR 0016 sequenced Story 4.7 (Rebirth) ahead of Story 4.5 (Primary Host wait) partly
because the bridge could not even *receive* a Rebirth. That premise has gone — and what replaces it
is sharper. The bridge now sits in a state where a live MQTT Engine on this broker can send it a
`Node Control/Rebirth`, the request is delivered, and the answer is a log line. Exactly one piece of
work stands between the deployment and a host-initiated repair, and it is Story 4.7. Before 4.6 that
gap was two pieces wide and could be described as a whole absent mechanism; now it is a single
handler, which is the cheapest it will ever be to close and the most conspicuous it will ever be to
leave open.

**Nothing here re-opens the measurement.** No STATE topic is subscribed, no STATE payload is parsed,
and no observation in *Record of runs* depended on the bridge's subscription state — the bridge was
almost certainly not even running during either run (step 3). This section revises what the
observation *means*, not what it saw.

### The eleven clauses, ruled

Story 4.3 filed these as `gap (unimplemented)` pointing at Stories 4.4–4.6. **This is a relevance
ruling, not a verdict** — the matrix verdicts are unchanged, and Story 4.5 decides what to build.

Note first that nine of the eleven are **conditional**, so nothing here forces the bridge into breach
today. "Relevant" below means *the deployment supplies what the clause needs, so the clause becomes
live the moment 4.5 configures a Primary Host* — not *the bridge is in breach today*.

The conditional is not one single phrase, and an earlier edition overstated it as if it were. Seven
clauses carry the verbatim *"if the Edge Node is configured to wait for a Primary Host Application"*
(`:201, :204, :208, :212, :235, :364, :372`). Two more are conditional on something else:
`-termination-host-offline-reconnect` (`:368-371`) conditions on an **event** — *"if the Edge Node
disconnects after being in a Sparkplug session due to a valid 'offline STATE message'"* — and
`-…-state-subs` (`:586-589`) on a **topology**, *"when using multiple MQTT Servers"*. The **two that
are not conditional at all** are `-birth-sequence-wait` (`:615-617`) and `-…-multiple-servers-walk`
(`:610-613`); both are ruled below on other grounds. The count is now reproducible from the spec.

| Clause | Ruling | Why, from what was observed |
| --- | --- | --- |
| `message-flow-edge-node-birth-publish-phid-wait` | **relevant** | A real, conformant Primary Host publishes `spBv1.0/STATE/SCADA` (finding 1). Verifying it before birthing is possible here — the steady-state pass had concluded the opposite |
| `-phid-wait-id` | **relevant** | There is an id to match: `SCADA`. The clause identifies the host by *"the last token in the STATE message topic"*, i.e. within `spBv1.0/STATE/…`, and **that namespace holds exactly one message** — see *Settled state after the restart*. The three other ids exist **only** on the legacy `STATE/<id>` topic, which no reading of this clause makes the bridge subscribe to, so the case collision between `scada` and `SCADA` **cannot** trap a bridge that implements the clause as written. *(An earlier edition called this clause "load-bearing, not formal" on the grounds that a case-insensitive match would bind the bridge to a dead client and it would never birth. That hazard requires reading the legacy topic, which nothing proposes. The ruling stands; the justification was wrong.)* Case-insensitive matching against the legacy form would still be a mistake — but a self-inflicted one, not a clause the deployment makes live |
| `-phid-wait-online` | **relevant** | The `online` key is present and is a JSON **boolean**, as the clause requires. Note it is satisfiable *only* on the `spBv1.0/` form — the legacy `STATE/SCADA` carries the bare literal `ONLINE`, with no boolean to validate |
| `-phid-wait-timestamp` | **relevant**; monotonicity **undetermined** | The timestamp is genuine epoch-millis (finding 3), so there is something to compare — this reverses the steady-state reading. But **one** online timestamp was captured, so movement *across* sessions is unmeasured. The clause's own *"if no previous … consider it the latest/valid"* branch is the one this deployment exercises on every first connect |
| `-phid-offline` | **relevant**; and the costly one | An offline STATE demonstrably exists here (`{"online":false,…}`, 42 bytes). Obeying the clause means publishing NDEATH and disconnecting on **every** Ignition restart. With no store-and-forward that discards the meter readings for the window rather than deferring them — 4.5 must weigh that, not assume it |
| `operational-behavior-edge-node-birth-sequence-wait` | **relevant**; **which reading applies is undetermined** | Unlike the `-phid-*` family this clause carries **no "if configured" conditional** — read literally it binds every Edge Node. But it sits inside § *Primary Host Application STATE in Multiple MQTT Server Topologies* (`:576-577`). One broker here, so the two readings **differ in this deployment** and 4.5 must choose one explicitly rather than inherit it |
| `-termination-host-offline` | **relevant** | Same evidence as `-phid-offline`: a real offline payload and real timestamps both exist. Same cost |
| `-termination-host-offline-reconnect` | **relevant** *(was `irrelevant`; changed by the Story 4.4 review)* | One broker; `MqttConfig` holds one `host` and one `port` and there is no connection list to walk — which is why this was first ruled irrelevant. That was wrong: the clause reads *"it MUST attempt to connect to the next MQTT Server in its connection list"*, and with one server **the clause still binds and produces a harmful outcome** — *"the next available MQTT Server"* is the same server, so a literal implementation degenerates into reconnect-to-self, a hot loop for as long as the host is offline. A clause that binds, that this deployment can trigger, and that forces 4.5 to specify an alternative is relevant by this table's own definition. The original cell already said *"4.5 must say what it does instead"*, which is not something an irrelevant clause requires |
| `-termination-host-offline-timestamp` | **relevant**; whether the hazard **occurs** here is undetermined | The guard is implementable on the `spBv1.0/` form (finding 3) and cheap. Whether this deployment actually delivers a stale death after a new session was not observed in one restart. *(An earlier edition offered the three ids permanently retained at `OFFLINE` as a real instance of the same failure class. That is a category error and is withdrawn: those are **legacy** payloads — the bare literal `OFFLINE`, 7 bytes — carrying **no timestamp at all**, and this clause compares timestamp values, so it cannot fire on them.)* |
| `-…-multiple-servers-state-subs` | **relevant** | The host half is satisfied — a STATE birth certificate is published, retained, QoS 1, even with one server. The edge-node half binds us and is unmet: the bridge subscribes to no STATE topic. *(As measured it subscribed to nothing at all; Story 4.6 added an NCMD subscription and no STATE handling, so the ruling stands and only its wording changes.)* Its preamble says *"when using multiple MQTT Servers"*, so like `-birth-sequence-wait` its applicability turns on a reading 4.5 must fix |
| `-…-multiple-servers-walk` | **irrelevant** | One broker, no next server — *"one broker, so server-walking cannot arise"*. Same degenerate-reconnect caveat as `-host-offline-reconnect` |

**Tally: 10 relevant · 1 irrelevant · 0 wholly undetermined.** None is left open as a whole, which is
what AC4 asked for: 4.5 rules without re-measuring.

*Was `9 · 2` until the Story 4.4 review moved `-termination-host-offline-reconnect` from irrelevant
to relevant, for the reason recorded in its row. The single remaining `irrelevant` is
`-…-multiple-servers-walk`.*

**Four carry a named undetermined residue** — `-phid-wait-timestamp` (monotonicity across sessions
unmeasured), `-birth-sequence-wait` (which of two readings applies), `-…-state-subs` (the same
reading question, from the topology side) and `-host-offline-timestamp` (whether the hazard occurs
here). *An earlier edition listed `-phid-offline` among the four and omitted `-…-state-subs`.
`-phid-offline` states a **cost** 4.5 must weigh, not an undetermined; the membership is corrected in
both directions here, and the count of four is unchanged by coincidence rather than by luck.*

**What the restart changed.** Read from the steady state alone, the four `-phid-wait*` clauses and
both `-timestamp` clauses would have been ruled *unimplementable here* — no 3.0 topic, no boolean,
no timestamp. Six of eleven rulings would have been wrong, and in the direction that argues against
building the thing.

### The cold-start state the rulings did not cover

Added by the Story 4.4 review. Finding 4 covers the case where the retained `spBv1.0/STATE/<host>`
is **absent**. There is a second, distinct starting state, and it is the one that actually existed on
this broker for hours: the retained STATE is **present and `false`**.

The two are not the same and they do not fail the same way:

| Broker state at bridge start-up | What a PHID-waiting bridge does | Outcome |
| --- | --- | --- |
| No retained STATE at all | `-phid-wait-timestamp`'s *"if no previous … consider it the latest/valid"* branch has nothing to accept; `-phid-wait-online` never sees `online: true` | Waits **forever**; never births. Finding 4 |
| Retained `{"online":false,…}` | The retained death **is** delivered, is well-formed, and is accepted as the latest valid STATE. The bridge correctly concludes the host is offline | Waits until the host returns. Correct — but with **no NDEATH to publish and no session to terminate**, since it never birthed. `-phid-offline` and `-termination-host-offline` both describe leaving a session that does not exist |
| Retained `{"online":true,…}` | Births immediately | The intended path |

**Ruling: relevant, and it belongs to Story 4.5 as a design obligation rather than a new clause.** No
`tck-id` among the eleven addresses start-up against a retained death — the clauses are written from
the point of view of an Edge Node already in a session. So this is not a twelfth clause; it is a hole
in the clause set that this deployment can drive straight into. 4.5 must state what the bridge does
in **all three** rows, not only the third.

### Was a second restart worth it?

**No, and none was taken.** A repeat performed **the same way** — arming the observers once the
shutdown is already under way, as happened here — would add a second timestamp sample and a live
`retain=false` death, and would still not settle open question 1: the two candidate messages are
byte-identical, so nothing in the payload separates them. The disruption would buy a sample, not an
answer.

*An earlier edition said the two are indistinguishable "regardless of when you subscribe". That is
true of their **content** and false of their **timing** — see open question 1. A run that is armed
and confirmed subscribed **before** the shutdown begins can separate them, because an explicit death
arrives while the host is still connected and a will arrives only when the broker declares the
session dead. So the question is answerable; it is the procedure, not the payload, that has to
change.*

**Take the sample opportunistically instead.** The next Ignition restart that happens for an
unrelated reason is free; arm the observers then, and confirm the subscription before anything stops.
The procedure a future run must follow is in *Open questions* below, and it costs nothing to hold.

## Open questions for Story 4.5

**1. Was the `OFFLINE` published by Ignition on shutdown, or delivered by the broker as its will?**
Undetermined and deliberately not guessed. It matters because it says whether this host announces
its own departure or relies on the broker noticing.

**It is NOT answerable from timestamps, and an earlier edition of this document was wrong to say it
was.** That edition proposed: death timestamp *equal to* the session's online timestamp → the will;
*later* → an explicit publish by Ignition. It cited `-connect-birth-payload` (`:779-783`), which does
establish that a will is stamped at **CONNECT** rather than at death.

The second branch does not exist for a conformant host. `-operational-behavior-host-application-death-payload`
(`:808-812`) describes *"The Death Certificate Payload **registered as the MQTT Will Message in the
MQTT CONNECT packet**"* — so a conformant Host Application that publishes its death explicitly
republishes **the will payload**, with the same CONNECT-stamped timestamp. An explicit death and a
broker-fired will are therefore **byte-identical**, and the proposed discriminator would classify
every explicit publish as "the will".

The cited precedent does not transfer either. `chaos_sigterm_no_lie` can discriminate on **our** NDEATH
only because `mqtt_driver.rs` re-stamps it at shutdown (`publisher.will(clock.wall())`, a second clock
read distinct from the CONNECT-time one). That is our implementation's choice — arguably itself a
deviation — and nothing binds Ignition to it.

**So the untranscribed timestamp cost less than this document previously claimed, and the open
question is harder than it claimed.** Transcribing it would not have answered this; capturing full
payloads next time will not answer it either. What *would*: an observer **connected before the
shutdown begins**, which sees whether the `retain=false` death arrives while the host is still
connected (an explicit publish) or at the moment the broker declares the session dead (the will).
That is a procedural requirement on the next observation, not a transcription discipline.

**2. What does the bridge do at start-up for each of the three possible broker states?** Not only the
absent one. See *The cold-start state the rulings did not cover*: absent STATE hangs forever
(finding 4), and a retained `{"online":false,…}` — the state that actually existed here for hours —
waits correctly but leaves the bridge holding clauses about terminating a session it never began.
A design question, not a measurement.

**3. Which of the two readings of `-birth-sequence-wait` applies?** See the ruling table. It is the
only one of the eleven whose *text* and *section context* disagree in a single-broker deployment,
and `-…-state-subs` raises the same question from the topology side.

**4. Was the second `ONLINE` a second publish or a redelivery?** The transcript records `ONLINE` on
`STATE/SCADA` twice, live. Without the MQTT `dup` flag and the packet id — neither of which the
observer captured at the time — a broker redelivering one QoS-1 message is indistinguishable from a
host publishing two. Both are captured now, so the next run answers it for free. It matters only
mildly, but it is the one anomaly in the transcript that carried no explanation.

## Clean-up

None required. The observation published nothing, used `clean_session=true` so no persistent session
was left queueing on the broker, and left no tags in the Ignition tree — unlike the Tier-3 contract
test, which needed five tag folders deleted afterwards.
