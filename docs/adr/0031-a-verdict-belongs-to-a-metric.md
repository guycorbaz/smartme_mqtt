# ADR 0031 — A verdict belongs to a metric, not to the reading

- **Status:** accepted
- **Date:** 2026-08-11
- **Amends:** [ADR 0012](0012-quality-codes-spec-versus-host.md) in scope only — the codes are
  unchanged; what moves is *how many verdicts a reading produces*.
  **Supersedes:** nothing.
- **Decided by:** Guy, 2026-08-11, on the review of stories 2.1 and 2.2.
- **Issue:** carried by story 2.3.

## Context

Story 2.1 built the oracle layer and gave it one composition rule: every judgement about a reading
composes into **one** verdict, worst-wins. `metrics_for` then stamped that verdict on both
published metrics, and nulled both when it was `Bad`.

That was right while exactly one thing produced a verdict — freshness, which genuinely judges the
whole response. It stopped being right the moment story 2.2 added an oracle that looks at **one
number**.

### What it does today, and it is not a corner case

`appart-est` publishes `Power` and `Energy`. Its energy index goes backwards — a reset, a rollover,
a replaced meter. The monotonicity oracle refuses the reading, and the bridge publishes:

| metric | value | quality | cause |
| --- | --- | --- | --- |
| `Energy` | `null` | `Bad` | `counter-went-backwards` |
| `Power` | `null` | `Bad` | `counter-went-backwards` |

**The second row is a lie of the kind this project exists to prevent.** The instantaneous power
reading is current, correct, and judged by nothing that objected to it. It is withheld from the
host, and then labelled with a fault belonging to the number beside it. An operator reading that
tag browser is told the power measurement failed because a counter went backwards, which is not a
thing that happened.

The composition layer had no vocabulary for "which metric did you look at", so a per-metric oracle
had no way to say so.

### Why it could not wait

Story 2.4 is physical bounds, which is per-metric by nature: a power reading outside plausible
bounds says nothing whatever about the energy index beside it. Written against a layer that cannot
express what it judged, that story would be written twice — and the wire contract would move twice,
the second time to correct the first.

## Decision

**A judgement declares what it is about, and composition happens per metric.**

```rust
enum Measured { Power, Energy }
enum Scope { Reading, Metric(Measured) }
struct Judgement { scope: Scope, verdict: Verdict }

fn compose_for(metric: Measured, judgements: &[Judgement]) -> Verdict
fn compose_for_meter(judgements: &[Judgement]) -> Verdict
```

Four things follow, and each was a choice:

### 1. `Scope::Reading` stays, and is not a convenience

Freshness, the host-clock guard and ADR 0029's identity check genuinely judge the whole response: a
reading that is too old is too old in both its numbers, and a reading from the wrong meter is the
wrong meter's power *and* energy. Forcing those oracles to enumerate metrics would make every new
metric a place to forget one — relocating the failure mode rather than removing it.

### 2. `Measured` is not the Sparkplug metric name

`core/` is the pure functional core and may not know what a metric is called on the wire. The name
is contract, owned by `adapters/sparkplug_publisher.rs` and pinned by `contract_golden`. What the
core needs is which of the two physical quantities an oracle looked at.

### 3. The meter still has ONE verdict, and it is the worst of them

`compose_for_meter` is what a latch decision and every operator surface read, because *"is this
meter trustworthy"* is not a question about one number. Publishing `Power` as `Good` while the
energy index is refused is right on the wire and would be a lie on a status page. Both are computed
from the same judgements in one place, so they cannot disagree — which is how `/healthz` came to
call a meter `Fresh` while the broker was being told `Bad`
([#62](https://github.com/guycorbaz/smartme_mqtt/issues/62)).

### 4. `Bad` still means null, per metric

The rule that a `Bad` metric carries no value is untouched. What changes is that it applies to the
metric that was refused, and not to its neighbour.

## Consequences

### The contract moves 5 → 6, and it is BREAKING

Nothing is renamed and no tag is added. But a consumer that recorded `Power = null` whenever the
energy index was refused records a real value for the identical physical situation from this
version on, so historised points from either side cannot be compared without knowing which side
they are on — the manual's own criterion for breaking rather than additive.

The change is a *correction*: the nulls were a fault reported on a metric the bridge had no
complaint about. A correction that alters what a stored point MEANS is still breaking. Calling it
additive because the new behaviour is better is how a consumer gets surprised.

### It costs a decision on every future oracle

Whoever adds one must now answer *"what does this judge?"*. That is the intended cost. The wrong
answer is recoverable and visible; not being asked the question is what produced the row above.

**When in doubt, judge the reading.** Degrading too much is the honest failure, and it is the one
the bridge has made from the start.

### It does not, by itself, publish more values

A reading refused for freshness or identity still degrades every metric, because those oracles are
reading-scoped. The only metrics that stop being withheld are those no oracle objected to.

## What would reopen this

- **A metric an oracle can only judge jointly with another.** None exists: power and the energy
  index are independent measurements. A derived metric (a computed rate, say) would be judged by
  the worst of its inputs, and that is a composition question this decision already answers.
- **A host that cannot render per-metric qualities.** Ignition can — quality is a per-metric
  property, not a per-message one — but it is measured for the codes and not for this. If a Tier-3
  run ever shows a host collapsing them, the finding belongs here.
