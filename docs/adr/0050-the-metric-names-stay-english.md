# ADR 0050 — The metric names stay English, and the site's report keeps asking otherwise

- **Status:** rejected — proposed and implemented on 2026-08-25, reversed by Guy the same day, before anything shipped
- **Date:** 2026-08-25
- **Decides:** what language the metric names this bridge publishes are written in.
- **Issue:** [#110](https://github.com/guycorbaz/smartme_mqtt/issues/110).
- **Source:** SCADA technical report v0.10, §16.9.5 *Nomenclature Sparkplug* — then open point `A38`, retired on 2026-08-27 when §16.9.5 was revised to retain the English names. Decided together with [ADR 0049](0049-the-device-is-named-by-its-measuring-point-and-vouched-for-by-its-serial.md), which shares its contract bump and its window.

## Context

The site's naming rule — lower case, digits, hyphen and underscore, French without accents — is
written for **topics**. A metric name is not a topic: it lives in the payload, on the code's side,
where English is the language this repository keeps. So the rule could be read as not reaching it,
and that reading was weighed rather than waved away.

It loses to one fact, which the projection itself states: the **quantity** has no room among the
three Sparkplug identifiers, so it is lodged in the metric name — and consumers restore metric names
as **folders**. Under an operator's eyes a metric name therefore behaves as one more path level.
Leaving it in English puts a `Power` at the end of a French tag tree, and the inconsistency shows up
exactly where it costs most: the tree somebody walks when they are looking for a measurement.

## Decision

**REVERSED.** The names stay `Power`, `Energy`, `Cause/Power`, `Cause/Energy`. Guy arbitrated on
2026-08-25, after the rename had been implemented and before it left the clone; the argument below
was made, weighed and overridden, and it is kept in full because it is the argument a later reader
will meet first.

**What overrides it is the accent** (stated on 2026-08-27, when the site's report was revised to
match). The site's naming rule forbids diacritics, and French quantity names carry them: adopting
the site's vocabulary would publish `energie` for *énergie* — permanently misspelt words, shown as
folders in the very tag tree the folder argument set out to make coherent. English has no accent to
lose, so its names are exact. The argument above therefore defeats itself on its own ground.

**This ADR is therefore a record of a decision NOT taken.** A later reader who renames the four
constants is not tidying an oversight: they are re-taking this decision, and they owe a
`CONTRACT_VERSION` bump and a fresh Tier-3 attestation for it, exactly as the rejected proposal did.
The one thing that must not happen is the rename arriving quietly as a cleanup.

### What was proposed, and implemented, before it was reversed

**The four metric names this bridge chooses would have taken the site's words:**

| constant | v12 | v13 |
|---|---|---|
| `METRIC_POWER` | `Power` | `puissance` |
| `METRIC_ENERGY` | `Energy` | `energie` |
| `METRIC_CAUSE_POWER` | `Cause/Power` | `cause/puissance` |
| `METRIC_CAUSE_ENERGY` | `Cause/Energy` | `cause/energie` |

**Only the language changes.** The structure is untouched — including the `/` a consumer renders as a
folder, and the pairing of a cause metric with the metric it qualifies (ADR 0044).

**Two names deliberately do not move.**

- `Node Control/Rebirth` is fixed word for word by the specification. Five MUST clauses in three
  chapters require that exact string (`tck-id-topics-nbirth-rebirth-metric`,
  `tck-id-payloads-nbirth-rebirth-req`, `tck-id-operational-behavior-data-commands-rebirth-name`,
  `-datatype`, `-value`), because a host addresses it by name. It is not ours to translate.
- `Contract/Version` is ours, and stays English on the site's own reasoning rather than in spite of
  it. It is a fact about the **service** that publishes, not a quantity of the site — the same
  category that keeps the edge node named after the service and exempt from the equipment
  nomenclature. The units (`kW`, `kWh`) stay for the same kind of reason: they are symbols, not
  words.

## Consequences of the reversal

**The wire and §16.9.5 of the site's report disagreed for two days, and no longer do.** The report
retained `puissance` and `energie` when this ADR was written; on 2026-08-27 it was revised to retain
the English names on the accent argument, and its anomaly `A38` was **retired** rather than left
open with no date. The number is not reassigned. Each document remains authoritative in its own
domain — what changed is that they now agree, and the agreement is recorded on both sides.

**`CONTRACT_VERSION` still moves to 13** — for ADR 0049 alone, which is breaking on its own account:
the device id moves from the serial to the short name, and every series a consumer holds under the
old device id stops being written to.

**The window argument survives the reversal and now cuts the other way.** While the supervisor does
not historise sub-metering, a rename costs a restart and nothing else; once it does, renaming a
metric breaks its series. The window that would have made this rename cheap is the same one that is
about to close, so the decision not to rename is also a decision to accept the later price if the
question is reopened. Said plainly rather than discovered: the site tracks the window as risk `R9`.

**Nothing else in the contract is affected.** The cause vocabulary, the latch/degrade rule, every
published quality and the two service metrics are as they were.

## Falsification

The guard is unchanged and it is what makes this reversible in either direction:
`contract_golden`'s name list is a set of **literals**, never the constants, so a rename cannot pass
through it silently. Renaming any of the four constants without moving `CONTRACT_VERSION` fails
against the golden by name — which is precisely how the original proposal was measured before it was
written down, and how the next one will be.
