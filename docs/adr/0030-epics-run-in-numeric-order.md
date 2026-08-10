# ADR 0030 — Epics run in numeric order, starting with Epic 2

- **Status:** accepted
- **Date:** 2026-08-10
- **Amends:** [ADR 0025](0025-the-execution-order-actually-followed.md), decisions 2 and 3.
  **Supersedes:** nothing.
- **Decided by:** Guy, 2026-08-10 — *"j'aimerais que les epic soient maintenant traitées dans
  l'ordre afin d'éviter des confusions"*, then, after reading what Epic 2 contains, *"je tranche :
  on implémente maintenant l'epic 2."*

## Context

ADR 0025 recorded the order actually followed and set `6 → 3 → 2 → 7 → 8` for the future. That
sequence has two defects, and neither is a wrong choice — it is an **incomplete** one.

**It reasons about whole epics while three of them are partial.** Epic 4 stopped at 4.10 of 19;
Epic 6 has two stories against twelve functional requirements.

**Its leading `6` did not mean "do Epic 6".** On 2026-08-05 it meant *finish 6.1 and 6.2*, which
its own insertion says outright — *"the five stories in `review` are reviewed before anything new
is built"*. Those five closed on 2026-08-10. **That `6` is spent**, and Epic 6 remains untouched
on ten of its twelve requirements.

**Two blocks of real work sat outside the sequence entirely**: the rest of Epic 6 — where
[#62](https://github.com/guycorbaz/smartme_mqtt/issues/62) lives — and eight of Epic 4's nine
remaining stories, deferred by ADR 0025's own decision 3.

The practical result was three epics `in-progress` at once (3, 4, 6) against a plan of record
that described none of their remainders.

**What settled the head of the queue.** Asked what Epic 2 actually contains, the answer was
verified against the code rather than the descriptions, and it is thinner than the prose implies:
of the four runtime oracles, **unit rejection exists only as a self-described "thin slice"**
(story 1.7 — an unknown unit yields `Quality::Bad`, with the three failure modes collapsing into
one undifferentiated verdict), **serial-identity binding was delivered by accident** (ADR 0029,
because a real fault demanded it), and **physical bounds and energy-counter monotonicity do not
exist at all**. Eleven items in `deferred-work.md` are parked on this epic.

FR15 in particular — detect a counter reset, rollover or meter replacement, and never publish a
negative delta as a valid measurement — is the question that will be asked the moment the frozen
`appart-est` returns. Nothing today would detect it.

## Decision

**1. The order is `2 → 3 → 4 → 6 → 7 → 8`** — plain numeric order, and Epic 2 starts now.

**2. An epic that is open closes before another opens.** This rule takes effect from Epic 2
onward. It cannot be applied retroactively: epics 3, 4 and 6 are already open and stay so, in
queue behind Epic 2, which is precisely the situation the rule exists to stop recurring.

**3. One named exception: story 4.17 is taken immediately**, out of Epic 4's backlog and ahead of
everything else. It fixes a confirmed specification violation
(`tck-id-message-flow-edge-node-birth-publish-will-message-qos`, `Sparkplug_5:184`, [#26]) and the
wire is only cheap to break while nothing is historised. On 2026-08-10 a live Ignition began
reading this bridge; Guy confirms nothing is historised **yet** and will say when that changes.
The exception exists because that window closes without announcing itself, not because the rule is
soft.

**4. Two scope decisions are named here so they are not discovered later.** An epic cannot close
while a story in it is undecided:

- **Story 3.6 is near-moot and must be re-scoped or withdrawn before Epic 3 can close.** FR21
  purges *orphan retained messages*, and this bridge publishes everything with `retain = false`
  — the specification requires it — so it cannot create the orphans FR21 purges (`epics.md:287`
  already says so). What is genuinely owed at a namespace change is an **orphan tag folder in
  the host**, which is Ignition's own persisted tree and which no MQTT purge can reach.
  `deferred-work.md` conflates the two and is corrected alongside this ADR.
- **Story 4.16 is blocked on amending NFR10, not on work.** NFR10 asks for read-to-broker-ACK
  latency, and ADR 0010 established there is no ACK at the QoS 0 Sparkplug mandates. The
  measurable analogue must be agreed. It pairs with 4.18, which corrects ADR 0010's wording; both
  are a specification sitting, not an implementation.

## Consequences

### What this costs, and it is a real cost

**The healthcheck now waits for four epics.** The bridge runs in production, a SCADA reads it, and
nothing watches the bridge itself — no `HEALTHCHECK` line, and nothing inside the image can consume
`/healthz` ([#56]). On 2026-08-10 a meter froze for over three hours, the wire reported it honestly
as `Bad_Stale`, and every operator surface called the fleet healthy ([#62]). Epic 6 answers the
second half of that and Epic 7 the first; both now sit behind Epics 2, 3 and 4.

**The parade is outside this repository and available now.** The information an alarm needs is
already on the wire, correctly qualified and correctly timestamped: quality `Bad_Stale`, and a
payload timestamp that is the reading's `ValueDate` rather than the publication time. A SCADA-side
alarm on tag quality needs no change here at all. That requirement is recorded in the SCADA project
(`notes/2026-08-10-alarmes-de-panne-de-compteur.md`), and it is what carries the operational risk
while Epics 2, 3 and 4 run.

Accepting this order means accepting that **the bridge's own surfaces stay silent about a degraded
meter until Epic 6**, and that the alarm lives with the host in the meantime. That is a deliberate
trade, made with the cost stated, not an oversight.

### What it buys

The founding claim — *the bridge never lies* — stops resting on the freshness state machine alone.
That is the narrowing ADR 0025 recorded as *"a cost in absence, and therefore easy to lose"*, and
it is the one thing on the list that no other epic addresses. Eleven parked items get an owner.

And the numbers can be read as an order again, which ADR 0025's decision 4 (do not renumber) had
made impossible to do by inspection.

### What is unchanged

ADR 0025's decisions 1 and 4 stand: the order actually followed remains recorded as fact, and the
epics are still not renumbered.

## What would reopen this

A production event the SCADA-side alarm fails to cover. That alarm is now load-bearing for the
whole period this order defers Epics 6 and 7, which is a heavier role than it had when it was
written this morning.

[#26]: https://github.com/guycorbaz/smartme_mqtt/issues/26
[#56]: https://github.com/guycorbaz/smartme_mqtt/issues/56
[#62]: https://github.com/guycorbaz/smartme_mqtt/issues/62
