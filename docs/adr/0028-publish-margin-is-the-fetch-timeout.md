# ADR 0028 — `publish_margin` is the fetch timeout, and NFR2 was never measurable without it

- **Status:** accepted
- **Date:** 2026-08-08
- **Supersedes:** nothing. **Amends:** **NFR2**, by giving a value to the one term of its formula
  that has never had one.
- **Story:** 3.3 · **Issue:** [#60](https://github.com/guycorbaz/smartme_mqtt/issues/60)

## Context

NFR2 reads, in the PRD and in `epics.md`, word for word:

> Per-meter staleness signalled no later than `last_success + 2×poll_interval + publish_margin`.

`publish_margin` appears four times in this repository — twice in the PRD, once in `epics.md`, once
in story 3.1's reasoning — **always inside that formula, and never with a value, a derivation, or a
definition anywhere.** There is no constant, no configuration key, no comment, no prose.

**A bound with a free variable cannot be met or missed. It can only be quoted**, and quoting is
exactly what has happened: every appearance of NFR2 so far is an *argument* — story 3.1 rejected a
single task walking the meters partly because it would make "NFR2's bound unmeetable" — and never a
*threshold* anything was measured against. A requirement used only as an argument does not need its
terms to have values, which is why this went a year without being noticed.

That is the same shape as the two failures this repository already records. AR13 deferred the
shutdown mechanism to a chaos test that did not exist, and the decision sat unmade for the whole of
Epic 1. Here the deferral is subtler: nothing says the value will be decided later, so nothing is
visibly outstanding.

Story 3.3 measures this bound. It cannot, until the term has a value.

## Decision

**`publish_margin` = the per-fetch timeout (`fetch_timeout`, 10 s today).**

Derived from the mechanism rather than chosen. For a meter that succeeds at `s` and then goes
silent, with `P` the publish period and `T` the fetch timeout:

| step | at the latest |
| --- | --- |
| last success, end of tick *k* | `s` |
| tick *k+1* starts (`MissedTickBehavior::Delay`) | `s + P` |
| its fetch fails by timeout | `s + P + T` |
| the verdict is published (ADR 0027's republish, `try_publish`, QoS 0) | `s + P + T + ε` |

NFR2 holds when `P + T + ε ≤ 2P + margin`, i.e. `margin ≥ T + ε − P`.

**The binding case is the minimum legal period, not the default.** At `P = PERIOD_MIN = 5 s`
(ADR 0020) that is `margin ≥ 5 s + ε`; at the shipped `P = 30 s` any margin at all — including zero
— satisfies it. A margin picked by looking at the default would be violated by a configuration the
bridge accepts, which is how a bound comes to be quoted rather than met.

`margin = T` satisfies every period in `[PERIOD_MIN, PERIOD_MAX]`, with the whole of `P` to spare,
and moves with `fetch_timeout` if that is ever configurable.

## What is deliberately NOT decided here

**NFR2 is not tightened to the latency the bridge actually achieves**, which is `s + P + T + ε` —
one period, not two. The `2×` predates ADR 0027: it was right when a failed fetch published nothing
and a host had to wait for a later cycle to learn anything, and since the republish landed, **one
missed tick is enough**.

Narrowing a published requirement to whatever the implementation currently does would leave nothing
able to catch a regression, and would have to be renegotiated the first time a retry is added
inside a tick. NFR2 stays a ceiling. Story 3.3's test asserts the ceiling and *records the observed
figure beside it*, so a change that doubles the real latency fails nothing and is still visible in
the output.

## Consequences

- NFR2 becomes measurable, and story 3.3 measures it at `PERIOD_MIN`, on the wire, with an injected
  clock — not at the state machine, because a verdict reached and not published is withheld
  (ADR 0027).
- The PRD's NFR2 line, `epics.md`'s, and the manual carry the definition.
- If `fetch_timeout` ever becomes configurable, the margin follows it and the bound stays true
  without renegotiation. That is the reason for deriving it rather than writing `10 s`.
- **`ε` is not given a number.** It is an encode plus a non-blocking `try_publish` on a bounded
  channel — microseconds against a 5-second budget — and the margin has a full period of slack
  above it. Naming a figure for it would be a precision this project has not measured, which is the
  habit this ADR exists to break.
