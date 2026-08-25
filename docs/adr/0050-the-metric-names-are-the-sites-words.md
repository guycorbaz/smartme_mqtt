# ADR 0050 — The metric names are the site's words, because a consumer renders them as folders

- **Status:** accepted
- **Date:** 2026-08-25
- **Decides:** what language the metric names this bridge publishes are written in.
- **Issue:** [#110](https://github.com/guycorbaz/smartme_mqtt/issues/110).
- **Source:** SCADA technical report v0.10, §16.9.5 *Nomenclature Sparkplug*, open point `A38`. Decided together with [ADR 0049](0049-the-device-is-named-by-its-measuring-point-and-vouched-for-by-its-serial.md), which shares its contract bump and its window.

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

**The four metric names this bridge chooses take the site's words:**

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

## Consequences

**`CONTRACT_VERSION` moves to 13, and this is breaking.** Four names in the tag set change at once.
Every consumer binds its tags by these strings, so a renamed metric simply stops arriving under the
name a host is watching — the silent breakage the version number exists to make visible. Combined
with ADR 0049 in one bump: one window, one attestation.

**The window is a calendar fact, not a preference.** While the supervisor does not historise
sub-metering, a rename costs a restart and nothing else. Once it does, renaming a metric breaks its
series exactly as a group rename would, and nothing announces that the window has closed — the site
tracks it as risk `R9`.

**A Tier-3 attestation is owed** (action H7). The runbook's steps now read the new names.

**Nothing about a cause changes.** The vocabulary, the latch/degrade rule and every published quality
are as they were; `cause/puissance` carries the same strings `Cause/Power` did.

## Falsification

`contract_golden`'s name list is a set of **literals**, never the constants, precisely so a rename
cannot pass through it: with `CONTRACT_VERSION` left at 12, each renamed constant fails against the
v12 golden by name. That is the guard doing what it was written for in 2026-08-11, and it is what
this change was measured against before it was written down.
