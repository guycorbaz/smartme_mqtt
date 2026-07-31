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

---

## Record of runs

| Date | Ignition | Contract | Result |
| --- | --- | --- | --- |
| 2026-07-26 | 8.3.7 | v2 | **Pass**, all five steps |
| 2026-07-26 | 8.3.7 | v1 | **Fail at step 4** — quality `STALE` displayed as `Good(500)`; see [#22](https://github.com/guycorbaz/smartme_mqtt/issues/22) |

A pass is only meaningful against a stated version, so add a row rather than editing one.

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
