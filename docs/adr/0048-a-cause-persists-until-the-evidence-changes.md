# ADR 0048 — A cause persists until the evidence that produced it changes

- **Status:** accepted
- **Date:** 2026-08-24
- **Decides:** what an operator is told when a tick carries no new evidence about a fault that is already known.
- **Issues:** [#75](https://github.com/guycorbaz/smartme_mqtt/issues/75) and [#79](https://github.com/guycorbaz/smartme_mqtt/issues/79), decided together because they are one question.

## Context

Two findings, filed a fortnight apart, from different reviews, on different files. Both were
recorded as *"the cause an operator sees depends on the last tick rather than on the fault"*, and
[#79] noted the family relation without either being acted on.

**[#75] — a latched meter.** `Policy::step` short-circuits on `prev == State::Failed` and publishes
`Cause::SourceRefused` unless *this* tick is itself fatal. With an expired credential:

```
credential expires   ->  credential-rejected
one network hiccup   ->  source-refused        <- the label changed, the fault did not
the network returns  ->  credential-rejected
```

Four paths reach that arm — a `Timeout`, a `Transient`, a `RateLimited`, or a good reading — where
the code's own justification enumerates one and calls it unlikely. A network hiccup on a latched
meter needs nothing unusual at all. **An operator looking just after the hiccup sees exactly the
undifferentiated cause story 2.6 was written to remove**, with nothing on screen saying the precise
information existed thirty seconds earlier.

**[#79] — a meter slower than the poll.** `MeterMemory::last_value_date` is overwritten on every
successful fetch, and the AC2 discrimination asks whether `value_date` advanced *since the previous
poll*. A meter that measures less often than the bridge polls re-serves the same measurement on the
intermediate ticks, so a wrong-clock meter publishes `timestamps-disagree` on the one poll after
each new measurement and `reading-too-old` on all the others. **This is the realistic regime**:
ADR 0004's captures showed real meters reporting on the order of a minute, and the default poll
period is 30 s — roughly half the ticks mislabel a producing meter as stopped.

The code is honest about why, in both places. `State::Failed` carries no payload, so the refusal is
genuinely unavailable; and a re-served measurement genuinely does not advance its `value_date`. In
each case the function answers correctly *given what it is told*.

## Decision

**A cause persists until the evidence that produced it changes.** Where a tick carries no new
evidence about a fault already known, the bridge republishes the cause it already reached rather
than a weaker one that happens to fit this tick.

Two applications, and they are the two issues.

### `State::Failed` carries the refusal that latched it

`State::Failed(Refusal)`. `Refusal` is `Copy`, so `State` stays `Copy` and nothing about how it is
passed around changes. The latch arm then republishes `refusal.cause()` on every subsequent tick,
and the generic arm disappears.

**`Cause::SourceRefused` is NOT withdrawn.** It loses its last live producer, and story 2.6 kept the
variant precisely for this case — but the cause vocabulary is part of the versioned contract, and
removing a variant is a `CONTRACT_VERSION` bump for a string no longer emitted. Keeping an unemitted
variant costs nothing on the wire; removing it costs a contract version. It stays, with its doc
saying it has no producer.

### A re-served measurement keeps the over-age cause it already had

Where `value_date` has not advanced, the tick re-serves a measurement already judged. It carries no
evidence about whether the meter's clock is wrong or the meter has stopped, so the discrimination
keeps its previous answer instead of falling back to the one that fits a single reading.

**Option 1 of the three [#79] listed, and the choice is about thresholds.** Option 2 — learning the
meter's own cadence — needs a multiplier, and a multiplier is a number nobody measured, which is the
shape this repository has refused since ADR 0020. Option 3 — accept the flap as the honest per-tick
answer — is defensible on the letter and wrong on the purpose: the per-tick answer is *not* honest
when the previous tick knew more.

**The residual, named rather than discovered:** a skewed meter that *then* stops keeps saying
`timestamps-disagree` until something else changes. That is a worse answer than the flap in exactly
one case, and a better one in the regime ADR 0004 measured.

## Consequences

**The quality never moves either way**, in both halves: `Failed` publishes `Bad`, and both over-age
causes publish `Stale`. So the wire's fail-safe direction is untouched and this is an operator-
routing change, not a safety one. `CONTRACT_VERSION` does not move — the vocabulary is unchanged.

**Which cause fires does change, and that is observable** to a consumer keying on it. Contract v12
publishes the cause as its own metric (ADR 0044), so this is visible on a tag browser: a latched
meter's cause stops flickering.

**ADR 0032's tier 2 is unaffected but adjacent.** It noted that the latching tier has no live tie
case because `Policy::step` returns at its first guard. Carrying the refusal does not change that;
it is the neighbourhood, and [#71] still owns the question.

## Falsification

Recorded with the tests: restoring the generic arm turns the latch guard red on the hiccup tick;
dropping the sticky rule turns the cadence guard red on the intermediate poll; and a control in each
proves the rule discriminates — a NEW refusal must replace the old one, and an advancing
`value_date` must be judged afresh.
