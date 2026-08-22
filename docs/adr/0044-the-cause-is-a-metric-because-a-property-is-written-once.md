# ADR 0044 — The cause is a metric, because a property is written once

- **Status:** accepted
- **Date:** 2026-08-22 (evening)
- **Decides:** how the cause reaches an operator, after [ADR 0043](0043-the-cause-is-declared-at-birth-or-it-does-not-exist.md)'s remedy was measured and failed. Bumps `CONTRACT_VERSION` to **12**, breaking.
- **Issue:** [#107](https://github.com/guycorbaz/smartme_mqtt/issues/107), and it answers [#68](https://github.com/guycorbaz/smartme_mqtt/issues/68)
- **Supersedes:** ADR 0043, seven hours after it was accepted.

## Context

ADR 0043 measured that Ignition materialises a metric property only when a BIRTH declares it, and
concluded that declaring it at BIRTH would make contract v4's `Cause` reach the operator at last.
**That conclusion was an inference, not a measurement**, and the Tier-3 session of the same evening
refuted it.

Contract v11, group `ContractV11`, a group the host had never seen:

| Gesture | What the host did |
| --- | --- |
| the cold-start BIRTH declares `Cause = no-reading-yet` | the property **appears**, with that value |
| a DDATA carries `Cause = reading-too-old` | **ignored** — the row does not move |
| a rebirth's BIRTH re-declares it | the host **takes** the new value |

**And the wire was read at the same instant as the screen**, by `observe_cause_property` subscribed
to `spBv1.0/ContractV11/#` while the operator watched the Designer:

```
spBv1.0/ContractV11/DDATA/BridgeContractNode/30000001
  Power    quality 2147484164 (Bad_Stale)   · Cause = reading-too-old
  Energy   quality 2147484164 (Bad_Stale)   · Cause = reading-too-old
```

| | On the wire | In the Designer |
| --- | --- | --- |
| quality | `2147484164` (Bad_Stale) | **`Bad_Stale`** — updated |
| `Cause` | **`reading-too-old`** | **`no-reading-yet`** — frozen |

**A metric property is written by a BIRTH and by nothing else.** The quality, which is also carried
as a property on the wire, *is* updated — so this is not "Ignition ignores properties". It is
narrower and stranger: the host tracks the properties it knows and freezes the ones it does not.

**v11 was therefore worse than v10, not merely no better.** Under v10 the operator saw nothing;
under v11 they saw `no-reading-yet` beside a healthy meter, for as long as the session lived.
Silence is uninformative. A stale cause is false, and false is the failure this project exists to
prevent.

## Decision

**The cause travels as two metrics — `Cause/Power` and `Cause/Energy` — and the `Cause` property is
removed.** A metric's value is precisely what a DDATA exists to change.

- **`Cause/…`, not `…/Cause`.** A `/` makes a **folder** in Ignition, established by
  `Contract/Version` and `Node Control/Rebirth`. `Power/Cause` would have made `Power` a folder,
  and `Power` is already a tag: the tree cannot hold both. `Cause/Power` gives a `Cause` folder
  holding two string tags.
- **The cause tag is `Good`, always**, even when the measurement it describes is not. This is a
  fact about the bridge's own judgement: no cloud call behind it, no clock that can make it old.
  Marking it `Stale` because its subject is stale would make the one tag that *explains* a fault
  unreadable exactly when it matters.
- **A good measurement publishes `no-cause`**, as under v11 and for a plainer reason now: a
  consumer reads a tag's current value, and an absent tag is a hole rather than a statement.
- **The property is removed rather than kept alongside.** Two representations of one fact drift,
  and one of them has just been measured to go stale in silence.

### Why the BIRTH still matters, though a property is not what is being declared

Ignition builds its **tag set** from what a BIRTH declares. A metric appearing only in a DDATA is a
tag the host never created. So the cold-start BIRTH declares both cause metrics — and declares them
as `no-reading-yet`, not `no-cause`, because those measurements are `Stale` and the neutral value
would be false. **What changes between a property and a metric is not whether a BIRTH must declare
it, but whether a DDATA may then update it.**

### What was rejected

**Forcing a rebirth on every cause change.** It works — the third row of the table above proves it —
and it is impracticable: a rebirth republishes the node's entire tree to move a twenty-character
string, and it would make every transient fault a storm.

**Keeping the property as well, for non-Ignition consumers.** One fact, one representation. The
second would be the one nobody tests.

## `CONTRACT_VERSION` moves to 12, and it is BREAKING

Two names appear in the tag set and **one disappears from it** — the first version in which a
contract name is removed. A consumer that learned to read the `Cause` property under v11 finds
nothing there under v12, which is exactly the silent breakage this constant exists to make visible.

**A fourth Tier-3 session is owed before production**, and it is short. Its decisive step is step 4:
the `Cause/Power` tag must change from `no-reading-yet` to `reading-too-old` **without a rebirth**.
That is the single observation v11 could not produce.

## Consequences

- **A defect was introduced and caught within the hour, by a test that already existed.**
  `metrics_for` degrades an absent value to `bad(value-unusable)` *inside* the metric builder, so
  the first version of this change left the cause metric reading the original verdict: an absent
  value went out `Bad` with a cause of `no-cause`. The published verdict is now computed once and
  used by both. `an_absent_value_is_never_published_as_good` is what said so.
- **`observe_cause_property` now reads the sibling metric** rather than the property, and skips
  `Cause/…` metrics when counting non-good ones. The instrument that settled this question keeps
  working on the answer it produced.
- **`NAME_SET_CHANGES` gains its second entry**, and the guard rewritten this afternoon handled a
  name *disappearing* without further change — which is what it was rewritten for.
- **The manual's chapter 5** carries the version table row, the prose, and the note about the cause
  tag's own quality.

## Falsification

| # | Mutation — the ordinary shape | Went red with |
|---|---|---|
| 1 | the two `cause_metric` calls dropped from `cold_start_metrics` | *"the cold-start BIRTH must DECLARE Cause/Power: Ignition builds its tag set from a BIRTH, so a metric that first appears in a DDATA is a tag the host never created"* |
| 2 | the cause metric fed `verdicts.for_metric(…)` instead of the published verdict — **the exact defect this change made for real**, restored deliberately | `an_absent_value_is_never_published_as_good`: *"names its cause, here the one that means exactly `not one usable number`. Got String("no-cause")"* |
| 3 | the cause tag takes its measurement's quality — the natural error of "keep them consistent" | two tests, in two places: *"the tag that EXPLAINS a fault must not be unreadable exactly when it matters"*, and the cold-start guard |
