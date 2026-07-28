# Primary Host / STATE — what this deployment actually publishes

**Story 4.4.** A read-only observation of the production broker, made 2026-07-28. It publishes
nothing; the tool is `crates/smartme-bridge/tests/observe_primary_host_state.rs`.

> **STATUS: the record is complete, the interpretation is not.** Everything under *Record of runs*
> and *What was found* is measured and finished. **AC2** (what an edge node ignoring STATE loses
> here) and **AC4** (the eleven clauses, ruled relevant / irrelevant / undetermined) are **still to
> be written** — see *Still to do*. The raw data is committed first because it cost an Ignition
> restart to obtain and lived only in a session temporary directory.

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

*The 61 topics from the `#` sweep are deliberately not listed: they are unrelated home-automation
traffic and some carry device identifiers. This repository is public. Only the count is recorded,
which is all the argument needs — the sweep existed to prove the broker was busy and that no
`spBv1.0` topic was present.*

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

### 4. A hazard that survives: the retained 3.0 STATE may simply not exist

`spBv1.0/STATE/SCADA` **did not exist before this restart** — two independent passes over `#` and
`spBv1.0/#` found nothing. The plausible reading is that Engine was upgraded to v5.x and had not
reconnected since, so its last birth predated 3.0 support. **That is a hypothesis, not a
measurement.**

What *is* measured: a bridge waiting for `spBv1.0/STATE/<host>` before birthing would have waited
**forever** in the broker state that existed an hour earlier, because the message it waits for had
never been published. **Story 4.5 must decide what the bridge does when the retained STATE is
absent.** "Wait for online" is not safe on its own.

## How this observation could have passed wrongly

`CLAUDE.md` requires a human-run gate to state what *else* could produce each result. Two of these
were live.

| Observation | What else could produce it | Eliminated by |
| --- | --- | --- |
| Silence on `spBv1.0/STATE/#` | The old `common::named_subscriber_on` decodes protobuf and **discards** what fails; STATE is JSON | A fresh observer that decodes nothing. **Measured**: `sparkplug_b::decode` rejects STATE JSON with *"buffer underflow"* |
| Silence on `spBv1.0/STATE/#` | Wrong topic filter, or an ACL, or a non-standard shape | Re-ran with `#`, saw 61 topics and found the STATE traffic one level up |
| `online: true` | The retained snapshot from a session established days ago, not a live publish | Recording `retain`. **This was live:** the birth arrived `retain=false` |
| No `spBv1.0` traffic **← the one that fooled this pass** | The host genuinely not implementing 3.0 — **the conclusion drawn, and it was wrong** | The restart. A snapshot describes the broker's memory, not the host's behaviour |
| A single restart looks conclusive | One sample | **Not eliminated.** One restart was observed. A second would be worth having |

**The fourth row is the lesson.** Three careful, honest passes supported the conclusion that this
host does not speak Sparkplug 3.0. One state transition destroyed it. Had this story been scoped to
a snapshot instead of *"across an Ignition restart"*, Story 4.5 would have inherited a confident,
evidenced and false premise.

## Still to do — Story 4.4, Tasks 5 and 6

- [ ] **AC2** — state plainly what an edge node that ignores STATE loses **in this deployment**,
      anchored in what was observed above rather than in general argument.
- [ ] **AC4** — rule each of the eleven clauses **relevant / irrelevant / undetermined**. The list is
      in the story file. Note that findings 1 and 3 move several of them from "unimplementable here"
      to "implementable", which is the opposite of what the steady-state pass suggested.
- [ ] Decide whether a **second restart** is worth the disruption, given the one-sample caveat.
- [ ] Task 6 — point the eleven conformance-matrix rows at this document **without changing their
      verdicts**; 4.5 decides.

## Open question for Story 4.5

**Was the `OFFLINE` published by Ignition on shutdown, or delivered by the broker as its will?**
Undetermined and deliberately not guessed: both produce an identical retained message, and the
observer subscribed after the fact. Distinguishing them needs an observer connected *before* the
shutdown begins. It matters, because it tells us whether this host announces its own departure or
relies on the broker noticing.

## Clean-up

None required. The observation published nothing, used `clean_session=true` so no persistent session
was left queueing on the broker, and left no tags in the Ignition tree — unlike the Tier-3 contract
test, which needed five tag folders deleted afterwards.
