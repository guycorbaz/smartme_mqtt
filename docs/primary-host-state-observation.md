# Primary Host / STATE — what this deployment actually publishes

**Story 4.4.** A read-only observation of the production broker, made 2026-07-28. It publishes
nothing; the tool is `crates/smartme-bridge/tests/observe_primary_host_state.rs`.

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
| A broker ACL hiding the topic | The `#` sweep returned 78 messages on 61 topics, so nothing was being withheld from this client |
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

**Proves:** what the host *does*, as opposed to what the broker *remembers*. `retain=false` is a
live publish; `retain=true` at subscribe is a stored snapshot.

**What else could produce each observation:**

| Observation | What else could produce it | Status |
| --- | --- | --- |
| `online: true` seen | The retained snapshot of a session established days ago | **Eliminated** — it arrived `retain=false` |
| An `OFFLINE` seen | An explicit death published by Ignition **or** the broker's will firing. Both produce an identical retained message on an identical topic | **NOT eliminated.** See *Open questions* — and note the discriminator recorded there, which this run had the means to apply and did not |
| No `spBv1.0` traffic *(step 1's conclusion)* | The host genuinely not implementing 3.0 — **the conclusion drawn, and it was wrong** | **Eliminated by this step.** A snapshot describes the broker's memory, not the host's behaviour |
| One restart looks conclusive | One sample | **NOT eliminated.** One restart was observed, and every claim below is a one-sample claim unless it says otherwise |

**The third row is the lesson of this story.** Three careful, honest passes supported the conclusion
that this host does not speak Sparkplug 3.0. One state transition destroyed it. Had the acceptance
criterion been scoped to a snapshot instead of *"across an Ignition restart"*, Story 4.5 would have
inherited a confident, evidenced and false premise.

## Record of runs

**Environment:** Ignition 8.3.7, **MQTT Engine module v5.0.0-rc1** (recorded here because the module
decodes Sparkplug and governs conformance more directly than the platform version — the residual
Story 1.15 left open). Mosquitto broker on the LAN, no auth.

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

| Requirement | Clause | Observed | |
| --- | --- | --- | :---: |
| Topic `spBv1.0/STATE/sparkplug_host_id` | `-connect-will-topic` (`:757-759`) | `spBv1.0/STATE/SCADA` | ✅ |
| JSON UTF-8, keys `online` (bool) + `timestamp` (number) | `-connect-will-payload` (`:760-764`) | `{"online":true,"timestamp":1785263196684}` | ✅ |
| Retain true | `-connect-birth-retained` (`:786-787`) | `retain=true` | ✅ |
| QoS 1 | `-connect-birth-qos` (`:784-785`) | `qos=1` | ✅ |

It **also** publishes a legacy form on `STATE/SCADA` carrying the bare literals `ONLINE` / `OFFLINE`,
which matches no clause of the 3.0.0 specification. Whether that is a pre-3.0 convention cannot be
settled here: **only 3.0.0 is vendored**, and it carries no changelog. Same bound as
[#34](https://github.com/guycorbaz/smartme_mqtt/issues/34), where the MQTT character set could not be
cited for the same reason.

**The bridge can implement the specification as written against this deployment.**

### 2. The host id is `SCADA`, and only the restart proved it

Only `STATE/SCADA` and `spBv1.0/STATE/SCADA` moved. `scada`, `ignition` and `IamHost` stayed frozen
at `OFFLINE` throughout — **retained residue from clients long gone**, since a retained message
outlives its publisher indefinitely. Note that `scada` and `SCADA` differ only in case, exactly the
hazard `tck-id-case-sensitivity-sparkplug-ids` warns about.

### 3. The timestamp is real, so the anti-replay clauses are implementable here

`1785263196684` decodes to **2026-07-28 18:26:36 UTC**, about 90 seconds before it was read. A
genuine epoch-millis value — so `-phid-wait-timestamp` and `-termination-host-offline-timestamp`,
which compare successive STATE timestamps, have something to compare. The legacy form carries no
timestamp at all, which is why the steady-state pass had wrongly concluded they were not.

The death payload carries one too: 42 bytes against the birth's 41, and `false` is exactly one
character longer than `true`, so its `timestamp` field is also 13 digits. **Its value was not
transcribed** and cannot be recovered — the retained death was overwritten by the birth on the same
topic. That omission costs a real answer; see *Open questions*.

### 4. A hazard that survives: the retained 3.0 STATE may simply not exist

`spBv1.0/STATE/SCADA` **did not exist before this restart** — two independent passes over `#` and
`spBv1.0/#` found nothing. The plausible reading is that Engine was upgraded to v5.x and had not
reconnected since, so its last birth predated 3.0 support. **That is a hypothesis, not a
measurement.**

What *is* measured: a bridge waiting for `spBv1.0/STATE/<host>` before birthing would have waited
**forever** in the broker state that existed an hour earlier, because the message it waits for had
never been published. **Story 4.5 must decide what the bridge does when the retained STATE is
absent.** "Wait for online" is not safe on its own.

## Interpreting the result

### What an edge node that ignores STATE loses — in this deployment

This is what the bridge does today: it holds **no MQTT subscription of any kind**. Verified
mechanically — `grep -rn "subscribe" crates/smartme-bridge/src/` returns one `tracing_subscriber`
initialiser and two comments, and no `AsyncClient::subscribe` call anywhere.

**It loses nothing at all while Ignition is up.** There is no command path to lose: the bridge
accepts no NCMD/DCMD, and one broker means no server-walking and no stranding. For the ordinary
running case, STATE is an offer this deployment makes and the bridge declines at no cost.

**What it loses is recovery after the host restarts — and the loss is total.** The chain is:

1. The bridge's NBIRTH went out **once**, at QoS 0 with `retain=false`
   (`every_edge_node_message_is_qos_zero_and_never_retained`). The broker stores nothing, so there
   is no copy for a returning host to collect.
2. Ignition restarts. **Measured:** the host re-births at 18:26:36 UTC and announces itself live on
   `spBv1.0/STATE/SCADA` with `retain=false`. A conformant edge node sees that transition and
   re-births, restoring its tag definitions.
3. The bridge sees nothing — it is not subscribed. Its own broker session is untouched by Ignition
   restarting, so **nothing makes it reconnect and nothing makes it re-birth**.
4. The protocol's remaining recovery is the host sending an NCMD `Node Control/Rebirth`
   (`tck-id-operational-behavior-host-reordering-rebirth`, `:565-568`). The bridge implements no
   Rebirth handling and could not receive the request without a subscription anyway.

**So the two gaps compound: no STATE-wait *and* no Rebirth means that after an Ignition restart, no
mechanism in the protocol can re-establish the bridge's tags.** The DATA keeps arriving at a host
that has no BIRTH to interpret it against. That is a deployment-specific statement of why
**Story 4.7** (`Node Control/Rebirth`, the gap recorded at `-rebirth-action-1/2/3`) is the higher priority
of the two.

**The finding that should change 4.5's shape.** The specification's own motivation for waiting on a
Primary Host is that the Edge Node *"store data while the Host Application is offline … then send
all of its stored data"* (`Sparkplug_5_Operational_Behavior.adoc:191-196`). **The bridge has no
store-and-forward.** So implementing PHID-wait *alone* here would not preserve one single
measurement — it would convert silent publication into deliberate non-publication and recover
nothing. Waiting is worth implementing for what it enables (a clean re-birth on the host's return),
not for the data it saves, and 4.5 should not justify it on the specification's stated grounds.

**One qualification, found while reading chapter 10 rather than by measurement.** "No mechanism in
the protocol restores the tag definitions" is true *of this broker*. A **Sparkplug Aware MQTT
Server** — a distinct conformance profile (`Sparkplug_10_Conformance.adoc:71-83`) — stores every
NBIRTH and DBIRTH that passes through it and republishes them as retained messages on `$sparkplug`
topics, precisely so a host that arrives late can collect them. **Mosquitto is not one**, so the
chain above holds here; but the remedy exists in the standard and is a third option alongside
STATE-wait and Rebirth. 4.5 should weigh it, and it is cheap to check whether MQTT Engine consumes
`$sparkplug` topics at all.

**Where the evidence stops.** Steps 1–3 above are measured; **step 4 — that Ignition's view of the
edge node does not survive its own restart — is an inference**, from the existence of the Rebirth
mechanism and from the bridge's BIRTH being unretained. It was not observed: no bridge tag state was
checked before or after the restart. It is **cheaply falsifiable without causing a restart** — with
the bridge running, look at its tags in the Ignition tree after the *next* Ignition restart, whatever
its cause. That step belongs in `docs/ignition-contract-runbook.md` when Story 4.5 or 4.7 next
touches it, and it is recorded here rather than quietly relied on.

### The eleven clauses, ruled

Story 4.3 filed these as `gap (unimplemented)` pointing at Stories 4.4–4.6. **This is a relevance
ruling, not a verdict** — the matrix verdicts are unchanged, and Story 4.5 decides what to build.

Note first that nine of the eleven are **conditional**: they bind only *"if the Edge Node is
configured to wait for a Primary Host Application"*. Nothing here forces the bridge to configure
one. "Relevant" below therefore means *the deployment supplies what the clause needs, so the clause
becomes live the moment 4.5 configures a Primary Host* — not *the bridge is in breach today*.

| Clause | Ruling | Why, from what was observed |
| --- | --- | --- |
| `message-flow-edge-node-birth-publish-phid-wait` | **relevant** | A real, conformant Primary Host publishes `spBv1.0/STATE/SCADA` (finding 1). Verifying it before birthing is possible here — the steady-state pass had concluded the opposite |
| `-phid-wait-id` | **relevant — and load-bearing, not formal** | There is an id to match (`SCADA`) **and three decoys** frozen at `OFFLINE` (finding 2). `scada` differs from `SCADA` only in case, so a case-insensitive match would bind the bridge to a permanently-offline dead client and it would **never birth**. This clause is what prevents that |
| `-phid-wait-online` | **relevant** | The `online` key is present and is a JSON **boolean**, as the clause requires. Note it is satisfiable *only* on the `spBv1.0/` form — the legacy `STATE/SCADA` carries the bare literal `ONLINE`, with no boolean to validate |
| `-phid-wait-timestamp` | **relevant**; monotonicity **undetermined** | The timestamp is genuine epoch-millis (finding 3), so there is something to compare — this reverses the steady-state reading. But **one** online timestamp was captured, so movement *across* sessions is unmeasured. The clause's own *"if no previous … consider it the latest/valid"* branch is the one this deployment exercises on every first connect |
| `-phid-offline` | **relevant**; and the costly one | An offline STATE demonstrably exists here (`{"online":false,…}`, 42 bytes). Obeying the clause means publishing NDEATH and disconnecting on **every** Ignition restart. With no store-and-forward that discards the meter readings for the window rather than deferring them — 4.5 must weigh that, not assume it |
| `operational-behavior-edge-node-birth-sequence-wait` | **relevant**; **which reading applies is undetermined** | Unlike the `-phid-*` family this clause carries **no "if configured" conditional** — read literally it binds every Edge Node. But it sits inside § *Primary Host Application STATE in Multiple MQTT Server Topologies* (`:576-577`). One broker here, so the two readings **differ in this deployment** and 4.5 must choose one explicitly rather than inherit it |
| `-termination-host-offline` | **relevant** | Same evidence as `-phid-offline`: a real offline payload and real timestamps both exist. Same cost |
| `-termination-host-offline-reconnect` | **irrelevant** | One broker; `MqttConfig` holds one `host` and one `port` and there is no connection list to walk. **But "irrelevant" is not "no behaviour needed"**: with one server, *"the next available MQTT Server"* is the same server, so a literal implementation degenerates into reconnect-to-self — a loop for as long as the host is offline. 4.5 must say what it does instead |
| `-termination-host-offline-timestamp` | **relevant**; whether the hazard **occurs** here is undetermined | The guard is implementable (finding 3) and cheap. Whether this deployment actually delivers a stale death after a new session was not observed in one restart. A **different** instance of the same failure class *is* real here: three ids permanently retained at `OFFLINE` |
| `-…-multiple-servers-state-subs` | **relevant** | The host half is satisfied — a STATE birth certificate is published, retained, QoS 1, even with one server. The edge-node half binds us and is unmet: the bridge issues no subscription at all. Its preamble says *"when using multiple MQTT Servers"*, so like `-birth-sequence-wait` its applicability turns on a reading 4.5 must fix |
| `-…-multiple-servers-walk` | **irrelevant** | One broker, no next server — *"one broker, so server-walking cannot arise"*. Same degenerate-reconnect caveat as `-host-offline-reconnect` |

**Tally: 9 relevant · 2 irrelevant · 0 wholly undetermined**, with **four carrying a named
undetermined residue** (`-phid-wait-timestamp`, `-phid-offline`, `-birth-sequence-wait`,
`-host-offline-timestamp`). None is left open as a whole, which is what AC4 asked for: 4.5 rules
without re-measuring.

**What the restart changed.** Read from the steady state alone, the four `-phid-wait*` clauses and
both `-timestamp` clauses would have been ruled *unimplementable here* — no 3.0 topic, no boolean,
no timestamp. Six of eleven rulings would have been wrong, and in the direction that argues against
building the thing.

### Was a second restart worth it?

**No, and none was taken.** A repeat performed the same way would add a second timestamp sample and
a live `retain=false` death — but it would **not** settle the question that actually matters,
because an explicit death and a broker will are indistinguishable on the wire regardless of when you
subscribe. The disruption would buy a sample, not an answer.

**Take the sample opportunistically instead.** The next Ignition restart that happens for an
unrelated reason is free; arm the observers then. The procedure a future run must follow is in
*Open questions* below, and it costs nothing to hold.

## Open questions for Story 4.5

**1. Was the `OFFLINE` published by Ignition on shutdown, or delivered by the broker as its will?**
Undetermined and deliberately not guessed. It matters because it says whether this host announces
its own departure or relies on the broker noticing.

**It is answerable from timestamps alone, and this run had the means.** The specification requires a
Host Application's birth timestamp to *"match the timestamp value that was used in the immediately
prior MQTT CONNECT packet Will Message payload"* (`-connect-birth-payload`, `:779-783`) — so a will
is stamped at **CONNECT**, not at death. Therefore:

- death timestamp **equal to** the preceding online timestamp of the same session → **the will**;
- death timestamp **later** → **an explicit publish by Ignition at shutdown**.

This is the same discriminator Story 1.13's `chaos_sigterm_no_lie` uses on our own NDEATH, and it
would have worked here — **the death payload's `timestamp` was simply not transcribed** before the
retained message was overwritten. Capture the full payload bytes of every STATE message next time;
the observer already prints them, so this is a transcription discipline, not a tool change.

**2. What does the bridge do when the retained STATE is absent?** Finding 4: "wait for online" hangs
forever in the broker state that existed an hour before the restart. This is a design question, not
a measurement.

**3. Which of the two readings of `-birth-sequence-wait` applies?** See the ruling table. It is the
only one of the eleven whose *text* and *section context* disagree in a single-broker deployment.

## Clean-up

None required. The observation published nothing, used `clean_session=true` so no persistent session
was left queueing on the broker, and left no tags in the Ignition tree — unlike the Tier-3 contract
test, which needed five tag folders deleted afterwards.
