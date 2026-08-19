# ADR 0038 — The leak gate measures per-iteration growth, not a 24-hour slope

- **Status:** accepted
- **Date:** 2026-08-19
- **Amends:** NFR3's *measurement method* (PRD, `epics.md:92`) and Story 4.15's acceptance
  criteria. **Does not amend:** NFR3's thresholds, which stand unchanged.
- **Issue:** [#97](https://github.com/guycorbaz/smartme_mqtt/issues/97)

## Context

NFR3 says: *no unbounded memory/FD growth — **RSS_max ≤ 100 MB**; **RSS slope ≤ 1 %/24 h** by
linear regression on RSS sampled **every 60 s**; **FD ≤ 64** via `/proc/self/fd`.* AC-LEAK-01
is the run that checks it: *a **100 000-iteration** loop*, which the PRD sizes at *"~30 s"*.

**Those three numbers cannot all be honoured, and the measurement is what showed it.** A spike
on 2026-08-19 assembled the poll loop, the oracle, the publisher, the outbox, the MQTT driver
and a real mosquitto container, and ran the loop at its fastest legal pacing:

- **5 012 iterations in 5.0 s — exactly 1 000 per second**, which is the 1 ms ticker and not the
  cost of the work; the iteration itself is faster and the loop waits.
- **100 000 iterations therefore take ≈ 100 seconds**, not the ~30 s the PRD assumed.
- RSS: **20 224 → 20 960 kB**, then constant to the kilobyte for the last four seconds.
- File descriptors: **10 → 11**, flat.

A 100-second run sampled *every 60 seconds* yields **two points**. A linear regression through
two points is an exact line: it has no residual, no confidence, and no meaning. And projecting
a slope from 100 seconds to 24 hours is an extrapolation of **864×** — a number arrived at by
multiplication rather than by observation.

**The PRD already says the soak is not here.** *"No formal 48–72h soak gate — production 24/7
is the soak."* The 24-hour slope was written as if a soak existed to produce it.

**One arithmetic fact reframes the whole question.** At the default publish period of 30 s, a
production bridge performs **2 880 iterations per day**. 100 000 iterations is therefore
**34.7 days of production** for one meter. The run is not a poor substitute for a 24-hour
observation — it is 35 times longer than one, compressed. What it cannot do is observe *wall
clock* effects (log rotation, certificate renewal, a broker's daily churn); what it does better
than any 24-hour soak is exercise the per-iteration path.

## Decision

**1. The thresholds are unchanged.** RSS_max ≤ 100 MB and FD ≤ 64, both read from `/proc/self`.
They are absolute, they are what NFR3 exists for, and nothing here touches them.

**2. The sampling cadence is proportional to the run, not fixed at 60 s.** The gate samples at
least **100 times** across the run, so a regression has something to regress. At 100 000
iterations that is roughly one sample per second.

**3. The slope is expressed per thousand iterations, and the threshold is derived rather than
chosen:** **≤ 80 kB per 1 000 iterations**, by linear regression over the samples.

At 2 880 iterations/day that is 230 kB/day, or **0.23 % of RSS_max per day** — an order of
magnitude stricter than the 1 %/24 h it replaces, and it is measured on the window it is stated
for instead of extrapolated to one. Over the full 100 000-iteration run it permits 8 MB of
growth, which is generous against the 736 kB the spike observed and tight against anything that
grows without bound.

**4. The 24-hour projection is withdrawn, not restated in new units.** The gate reports what it
measured — iterations, elapsed time, RSS_max, RSS slope, FD_max — and makes no claim about a
day, a week or a month. *Production 24/7 is the soak*, as the PRD already says.

**5. What the run does not cover is named in the gate itself.** The real HTTP client is
exercised on its **failure** path only. It refuses any endpoint that is not `https` and
validates certificates, so 100 000 *successful* fetches against a local server are not
reachable without installing a trust root — and the client is the likeliest place for a
descriptor leak, which is precisely why the omission is stated rather than left to be inferred
from a green run.

## Consequences

### What this buys

A gate that can actually fail. The version in NFR3 could not be executed at all: whoever ran it
would have had two samples and a slope of zero by construction, and a green result would have
meant nothing. The replacement is stricter in the dimension that matters — per-iteration growth
— and honest in the one it drops.

### What it costs, and it is real

**A slow leak below 80 kB/1 000 iterations passes.** That is 8 MB over the run and,
extrapolated at the default period, about 84 MB/year — which RSS_max would eventually catch and
this gate would not. The bound is a floor on detection, not a proof of absence, and it is set
where the allocator's own noise stops being distinguishable from growth.

**And the run is 100 s rather than ~30 s**, so the PRD's sizing is wrong by a factor of three.
The number came from the pacing floor, which is the loop's own minimum and not something this
project would want to change to make a test faster.

### What it does not fix

The HTTP client's nominal path stays unmeasured for leaks. If that becomes a concern, the
answer is a trust root in the test image — a real cost, taken then, against evidence rather
than in advance.

## What would reopen this

A soak environment. If this project ever gains somewhere a bridge can run for days under
observation, the 24-hour slope becomes measurable and this ADR should be re-read rather than
worked around — the thresholds it kept are the ones such a soak would test.
