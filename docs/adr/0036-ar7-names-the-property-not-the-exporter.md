# ADR 0036 — AR7 names the property, not the exporter

- **Status:** accepted
- **Date:** 2026-08-18
- **Amends:** AR7 (`epics.md:141`, `architecture.md:79` and `:286`). **Supersedes:** nothing.
- **Issue:** [#89](https://github.com/guycorbaz/smartme_mqtt/issues/89)

## Context

AR7 has said since Epic 0 that a broker-down reading is a **traced drop**, and it spelled the
tracing as `readings_dropped_total{meter,reason}` — Prometheus notation, down to the label
names. Story 4.11 implemented the requirement on 2026-08-18 and did **not** implement that
metric. It ships the counts as a `dropped_readings` array on `/healthz` instead.

**The substitution was made in a Dev Note.** `CLAUDE.md` says anything that changes a
requirement or an architectural position gets an ADR and a GitHub issue; the 2026-08-18
adversarial review found the note where the ADR should have been. The precedent is direct and
embarrassing: [ADR 0027](0027-a-failed-source-is-a-fault-the-screen-must-name.md) was written
for a *smaller* version of this same question — one new `/healthz` field and whether the status
code moves — and story 4.11 cites it three times while making the larger change silently.

**Why no exporter was built.** There is no metrics registry in this repository, no exporter,
and no dependency that would provide one. `cargo deny` governs what may be added and the
dependency direction is an architectural invariant with its own CI gate (`arch_purity`).
Introducing a metrics stack is a decision about the operator surface, which is Epic 6, and
about what the deployment scrapes, which is Epic 7. Story 4.11 could not take it, and taking
it quietly would have been worse than not taking it.

**What the consumers actually are.** This bridge has two: an Ignition SCADA reading Sparkplug,
and a container healthcheck reading `/healthz`. Neither speaks Prometheus. Nothing in the
deployment scrapes metrics, and `docker-compose.yml` has no scraper. The notation in AR7
described an idiom, not an integration that exists.

## Decision

**1. AR7 keeps its intent and loses its mechanism.** The requirement is restated as the
property it always was:

> On full or broker-down, a reading that cannot be handed over is a **per-device traced drop**:
> counted per meter and per reason, and traced at WARN with the reading's source timestamp. The
> counts must be readable by an operator without reading the log. No persistent buffer.

The Prometheus spelling is withdrawn from AR7. **This is the same move as
[ADR 0035](0035-fr21-is-reshaped-the-ghost-purge-belongs-to-the-manual.md), three days
earlier**: the requirement changes actor, not intent.

**2. `/healthz` is where the counts are readable today**, under `dropped_readings`, and that is
a *satisfaction* of AR7 rather than an exception to it.

**3. An exporter is not forbidden — it is unowned.** If Epic 6 or Epic 7 brings a metrics
surface, the same counters feed it and `readings_dropped_total{meter,reason}` is the name to
use. Nothing in this ADR argues against it; what it refuses is a requirement mandating a
mechanism the project has no consumer for.

**4. What AR7 must NOT be read as promising, stated because story 4.11's review found the
claim being made.** The counts are a **floor on what was lost, never a ceiling**. Three paths
lose a reading without any counter seeing it: the transport client answers `Ok` on queueing
rather than on sending ([#85]), the inbox is dropped unread at shutdown ([#87]), and a refused
DBIRTH leaves the device declared so every later reading looks published ([#88]). AR7 governs
the **hand-over**, not the journey. The manual says so; so does this ADR.

## Consequences

### What changes today

Nothing in the code. `epics.md:141`, `architecture.md:79` and `architecture.md:286` are amended
to state the property; the manual already describes the behaviour. This ADR is a correction to
the record, which is where the defect was — the same sentence ADR 0025 had to write.

### What it costs

**A count on `/healthz` is polled, not scraped.** A Prometheus counter carries its own history
and can be alerted on with a rate; a cumulative integer in a JSON body cannot. An operator
reading `/healthz` once cannot tell a running outage from one that healed hours ago — that is
[#91], filed the same day, and this ADR does not close it. Accepting AR7's amendment means
accepting that recency is a separate decision rather than something the metric would have given
for free.

## What would reopen this

A consumer that scrapes. If Epic 7's deployment gains a metrics endpoint — or if the SCADA-side
alarm proves insufficient and the bridge is asked to alert on its own losses — the exporter
stops being unowned and AR7's original spelling becomes the right one again.

[#85]: https://github.com/guycorbaz/smartme_mqtt/issues/85
[#87]: https://github.com/guycorbaz/smartme_mqtt/issues/87
[#88]: https://github.com/guycorbaz/smartme_mqtt/issues/88
[#91]: https://github.com/guycorbaz/smartme_mqtt/issues/91
