# ADR 0018 — Primary Host / STATE is ruled out; the repair path is host-initiated

- **Status:** Accepted
- **Date:** 2026-07-31
- **Related:** Story 4.5 (this is its deliverable), Story 4.4 (the measurement),
  [ADR 0016](0016-rebirth-before-primary-host-wait.md) (whose ordering argument is spent),
  [ADR 0011](0011-graceful-shutdown-requires-both-deaths.md), `docs/primary-host-state-observation.md`,
  [#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)
- **Supersedes nothing.** It closes an omission that had never been a position.

## Context

Sparkplug lets an Edge Node name a **Primary Host Application** and change its behaviour according to
that host's `STATE`: wait for it before birthing, disconnect when it goes offline, walk to another
broker. This bridge implements none of it — it births as soon as the broker answers, and reads no
`STATE` topic.

Until now that was an *omission*: nobody had decided it, and it appeared in no planning artifact while
the author's broker carried live `spBv1.0/STATE` topics. Story 4.4 measured the mechanism against this
deployment; Story 4.5 exists to turn the measurement into a position. This is that position.

**The ordering argument that used to stand in for one is spent.** ADR 0016 sequenced Rebirth (4.7)
ahead of this decision because *"a Rebirth that arrives and is ignored repairs exactly as much as one
that never arrives: nothing."* Story 4.7 landed on 2026-07-30 and that sentence is now false. This ADR
therefore re-weighs the question on its own evidence, which is what ADR 0016 instructed.

## Decision

**No Primary Host Application is configured, no `STATE` topic is read, and the bridge births as soon
as the broker accepts it.** The mechanism the specification offers is declined, deliberately, on the
four grounds below.

## Grounds

### 1. Declining is conformant. The specification says so in its own words.

`Sparkplug_5_Operational_Behavior.adoc:190-191`:

> *"**Specifying a Primary Host is not required for an Edge Node.** But it is often desired."*

And every one of the eleven clauses is conditional:

- **Nine carry the condition in their own text** — *"**If** the Edge Node is configured to wait for a
  Primary Host Application it MUST…"* (`-phid-wait`, `-wait-id`, `-wait-online`, `-wait-timestamp`),
  *"**When using multiple MQTT Servers and** Edge Nodes are configured with a Primary Host
  Application…"* (`-state-subs`), and the `-phid-offline` / `-termination-host-offline*` family, which
  describe leaving a session entered under a configured Primary Host.
- **`-birth-sequence-wait` is the one whose sentence is unconditional**, and it is scoped by its
  section rather than its wording. It sits under *"Primary Host Application STATE in **Multiple MQTT
  Server** Topologies"* (`:577`), immediately after `-walk` (*"move to the next available MQTT
  Server"*), and the sentence that follows it states its purpose: preventing an Edge Node from being
  *"stranded on an MQTT server"*, illustrated with a three-server diagram. **This ADR adopts the
  section-scoped reading.** With one broker and no configured Primary Host, it does not bind.

### 2. Implementing it would preserve no measurement, because the norm's own justification does not apply

The specification justifies the wait by store-and-forward (`:191-196`):

> *"it would be better for the Edge Node to **store data** while the Host Application is offline. Once
> the Host Application is properly connected, it could then **send all of its stored data** and
> continue publishing normally."*

**This bridge has no store-and-forward and none is planned** — a deliberate architectural choice
(Ignition rejects out-of-order data, and a persistent buffer is the unbounded-growth trap). So waiting
does not save a single reading. It converts silent publication into deliberate non-publication: more
honest, and worth something, but not the value the specification claims for it.

Measured by Story 4.4 and unaffected by anything since.

### 3. One broker. The problem the mechanism solves cannot occur here.

No server list, no walking, no stranding. The section these clauses live in is about multi-server
topologies; this deployment has one broker and no plan for a second.

### 4. Implementing it would introduce a failure mode that does not exist today

From the observation record's *Finding 4*:

| Broker state | Consequence of implementing the wait |
| --- | --- |
| **No retained STATE at all** | `-phid-wait-online` never sees `online: true`, and `-phid-wait-timestamp`'s *"if no previous … consider it the latest/valid"* branch has nothing to accept. **Waits forever. Never births.** |

**This is not hypothetical.** `spBv1.0/STATE/SCADA` **did not exist before the 2026-07-28 Ignition
restart** — two independent passes found nothing. A bridge implementing the wait, started on that
broker on that day, would have published nothing at all, indefinitely, and correctly.

So the trade is: **zero measurements preserved, in exchange for a bridge that can decline to publish
entirely**, on a deployment where the stranding it guards against cannot happen.

## What replaces it, and what is measured about that

The repair for a host that arrives without a BIRTH is an NCMD `Node Control/Rebirth`. The loop:

1. the bridge publishes DDATA every poll (report-by-exception is not implemented — [#32](https://github.com/guycorbaz/smartme_mqtt/issues/32));
2. a host that has restarted receives DATA from a node whose BIRTH it never saw;
3. that is the condition under which a host requests a rebirth;
4. **the bridge answers it** — Story 4.7.

**Steps 1 and 4 are measured.** Step 4 was verified against the live Ignition on 2026-07-31: MQTT
Engine renders a `Node Control/Rebirth` control for a node that declares the metric, sends the
tck-id spelling with boolean `true` and retain false, and the bridge republished its complete BIRTH
sequence under an unchanged `bdSeq`. Ignition's own counters agreed (`Rebirth Count = 2`,
`Birth Count = 3`, `bdSeq = 1`).

### ⚠️ This makes the periodic publish LOAD-BEARING, which it was not before

Step 1 is not a detail of the current implementation; it is now **a dependency of the recovery path
this ADR relies on**. Report-by-exception would remove it: with RBE the bridge publishes only on
change, so a host that restarts while a meter's value is steady receives **nothing**, never notices a
node whose BIRTH it never saw, and never asks. The trigger disappears and the loop above does not
start.

[#32](https://github.com/guycorbaz/smartme_mqtt/issues/32) already carries *"re-examine RBE once
Rebirth lands"*. It must now also carry: **the periodic publish has become the trigger for
host-initiated recovery, so removing it requires replacing it** — a periodic keep-alive publish, a
STATE subscription after all, or an explicit decision that recovery waits for the next reconnect.

Recorded here rather than left implicit, because writing a claim and leaving the sentences that depend
on it untouched is the defect this project has now logged six times. This one was found within the
hour, in this document.

**Step 3 is INFERRED, not measured**, and this ADR says so rather than blurring it. The evidence for
it is that Ignition exposes `Node Info → Rebirth (Last) Cause`, which read `Triggered by user` — a
label that only earns its existence if other causes exist, and the other causes in Sparkplug are
automatic. No automatic rebirth has been observed.

**The inference does not carry the decision.** Even if Engine never asked on its own, PHID-wait would
not be the remedy: it still preserves no reading. The remedies for *"a host that never asks"* are
store-and-forward, report-by-exception plus something, or an operator pressing the control — none of
them is *waiting before birthing*. Step 3 therefore bears on the **revisit condition**, not on the
decision.

## What this costs, stated plainly

- **A consumer that never asks never learns.** If a host restarts, does not request a rebirth, and
  nobody presses the control, it receives DDATA it cannot interpret until the bridge reconnects for
  some unrelated reason. Nothing in this ADR fixes that; it argues that PHID-wait would not have fixed
  it either.
- **`spBv1.0/STATE` is live on this broker and the bridge ignores it.** That is now a decision rather
  than an oversight, which is the whole point of writing this down.

## ADR 0011 is unchanged, and here is why

Story 4.5's second acceptance criterion asks explicitly whether
[ADR 0011](0011-graceful-shutdown-requires-both-deaths.md) — *both deaths fire on SIGTERM* — survives
this decision. **It does, unchanged.**

The concern was real: a Primary Host going offline changes *when* an Edge Node should stop publishing,
and `-phid-offline` / `-termination-host-offline` would have required the bridge to terminate its
session on an offline STATE — a second, host-driven path into the shutdown sequence, which ADR 0011
never considered. **Ruling STATE out removes that path before it exists.** The only way this bridge
stops is SIGTERM or a broker-side disconnect, and ADR 0011 covers both.

Had the decision gone the other way, ADR 0011 would have needed amending: an edge node that never
birthed (because it was waiting for STATE) has **no session to terminate and no NDEATH to publish**,
and both clauses describe leaving a session that does not exist.

## Consequences

- **The conformance matrix's verdicts for these clauses should be re-examined, and this ADR does not
  do it.** The five `-phid-*` rows are recorded `gap (unimplemented)`, which over-declares if the
  clauses do not bind — they resemble the DCMD rows, which are `n/a` on a stated condition. Story
  4.3's review chose `gap` deliberately and listed it as one of the three judgements a reviewer should
  attack first. Moving them would move two tallies, so it belongs to whoever next owns the matrix,
  with this ADR as the argument.
- **ADR 0016's ordering argument is formally spent**, and this ADR is the re-weighing it asked for.
- **`docs/primary-host-state-observation.md` must not be re-run.** It cost an Ignition restart on
  production. Its measurements stand; only their interpretation was open, and it is now closed.
- **The manual's *Known limitations* entry on Primary Host support becomes a decision** rather than a
  gap, and should say so with a pointer here.

## Revisit conditions

Any one of these reopens it:

0. **Report-by-exception is implemented** ([#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)).
   It removes the trigger this ADR's replacement path depends on. Not a reason to implement PHID-wait
   — see above — but a reason to re-rank the remedies before RBE lands, not after.
1. **A second broker appears.** The stranding problem becomes real and the multi-server section starts
   to bind.
2. **Store-and-forward is added.** The specification's own justification for the wait becomes
   applicable, and the trade in ground 2 inverts.
3. **The pre-production run shows MQTT Engine does NOT request a rebirth on its own** when it receives
   DATA from a node whose BIRTH it never saw. That would not make PHID-wait the remedy, but it would
   make *"a host that never asks"* the live case rather than the corner case, and the remedies above
   would need ranking. **This is question 1 on Story 4.8's batched list** — deliberately not measured
   here, because settling it requires an Ignition restart on production and the decision does not
   depend on it.

## Alternatives considered

**Implement PHID-wait now.** Rejected on grounds 2 and 4: it preserves nothing and introduces a
never-births state.

**Implement STATE observation without the wait** — read `spBv1.0/STATE`, log the host's transitions,
change no behaviour. Rejected as the worst of both: it adds a subscription, a parser and an
anti-replay rule (`-termination-host-offline-timestamp` is a MUST NOT) in exchange for a log line, and
a mechanism that is read but not acted on is the shape Story 4.6 had to make safe once already.

**Defer the decision to the pre-production run.** Rejected, and the rejection is the point.
`CLAUDE.md` forbids deferring a decision to an artifact that does not exist yet, because AR13 did
exactly that and sat unmade for the whole of Epic 1. The measurement that run will produce refines a
revisit condition; it does not decide anything here.
