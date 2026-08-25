# ADR 0049 — The device is named by its measuring point, and vouched for by its serial

- **Status:** accepted
- **Date:** 2026-08-25
- **Decides:** what value the Sparkplug `device_id` carries, and what replaces the guarantee the previous value gave for free.
- **Issue:** [#111](https://github.com/guycorbaz/smartme_mqtt/issues/111).
- **Source:** SCADA technical report v0.10, §16.9.3 *Nomenclature Sparkplug* — open points `A37` (the rename) and `A39` (the guard). Decided together with [ADR 0050](0050-the-metric-names-are-the-sites-words.md), which shares its contract bump and its window.

## Context

The site's projection of its vocabulary onto Sparkplug has been normative since 2026-08-05: domain →
`group_id`, gateway → `edge_node_id`, equipment → `device_id`, the zone is not projected, and the
quantity descends into the metric name. What it did not say was **which values** those identifiers
carry. That was written on 2026-08-25, and it moves one of them.

Since story 1.9 the Sparkplug device id has been the meter's **serial**. That was chosen for a real
reason: it is the only identifier the gateway reads off the device itself, so it is the only one it
can guarantee is not attached to the wrong meter. A short name is a hand-written correspondence in a
configuration file, and nothing says it is right.

**The decisive argument against it is about what a supervisor historises.** It does not follow a box,
it follows a **measuring point** — the metering of a flat, a room, a feeder. A Sparkplug identifier
is the key under which a consumer files that history, so building the key on the serial makes a
*replaced meter* into a *new device*: the series breaks at the exact moment nothing changed for the
operator. That is the same failure the site's equipment nomenclature avoids by forbidding a brand or
a model inside an identifier.

The immutability the serial was chosen for is already held by the short name, through the site's own
rule: a number is assigned once, never reassigned, and left vacant when a device is removed. Two
meters cannot share a history.

## Decision

**The Sparkplug device id is the meter's short name — `meters[].meter_id`, `cptNN` at this site —
and not its serial.**

The serial does not leave. It becomes the bridge's **internal key**: what the smart-me API answers
with, what routes a reading to its declaration, and what [#88]'s undo names. Exactly one place
translates one into the other — the publisher's declaration table, which now holds the published
name beside the last reading.

**And the rename comes with two obligations, which are not refinements of it but the replacement for
what it gives up.** Detaching the published identifier from the serial removes the one thing the
serial guaranteed: that a name cannot be attached to the wrong meter. A swapped configuration line
would publish one flat's measurements under another's name, and **no value would stop being
plausible** — the failure mode this bridge exists to refuse.

1. **The DBIRTH declares the serial**, as the `serial` property on every metric it carries
   (`PROPERTY_SERIAL`). The wire still says which physical meter is speaking, and a person in front
   of a tag browser can make the same check the bridge makes.
2. **A meter whose fetched serial is not the declared one is refused**, not warned about. That guard
   already exists — `UnverifiedReading::verify`, ADR 0029 — and latches `identity-mismatch` on the
   first answer. What changes is its standing: it was a second lock, and it is now the only one.

### A property, and why that does not contradict ADR 0044

ADR 0044 took the *cause* out of a property, on a measurement: a metric property is written by a
BIRTH and by nothing else, so a cause carried as one stood frozen at its birth value while the world
moved. A serial is the opposite case. It **cannot** move within a session — a serial that changed
under us latches the meter off the wire — so *frozen at its birth value* is precisely what is wanted.
The two decisions read the same measurement and reach opposite conclusions because they describe
opposite kinds of fact.

It is declared on **every** metric of the DBIRTH rather than on a chosen one: a host materialises a
property only where a BIRTH declares it, and an operator inspecting a tag should not have to know
which of the four carries the identity.

**The key is `serial`, in English, where the metric names beside it are not.** ADR 0050 translates
the metric names because a consumer renders them as folders — they become path levels under an
operator's eyes. A property key is not rendered that way: it sits beside `engUnit` and `quality`, in
the company of Sparkplug's own vocabulary, which is where this repository keeps English. The site's
report requires *the serial in a property of the birth certificate* and does not name the key, so
this is ours to choose; arbitrated by Guy on 2026-08-25, against a first draft that spelled it
`serie`.

## Consequences

**`CONTRACT_VERSION` moves to 13, together with ADR 0050.** A device id is not a tag name, but the
tag set a consumer browses is reached *through* it: every series filed under `9202685` stops being
written to. One bump rather than two, because the Tier-3 runbook's promise is that two runs sharing a
version attest to the same tag set — and a v13 that existed only between two commits, never attested,
would put a number in that table that nothing stands behind.

**It owes a Tier-3 attestation** (action H7 of the epic-8 retrospective). `docs/ignition-contract-runbook.md`
records v13 as awaiting one, with the session's own steps updated: the device folder is now the short
name, and one step reads the `serial` property.

**The window is open and closes without announcing itself.** The supervisor does not historise
sub-metering yet, so today this costs a restart; once it does, it costs a broken series. The site
tracks that as risk `R9`.

**Renaming a meter now buries its device.** `reconfigure::classify_meters` used to claim no
certificate for a rename, on the correct reasoning that a label change altered nothing on the wire.
It does now: the old name leaves the wire, so a host that is not told keeps showing its last value as
current. The burial is owed; the birth still waits for the restart the rename already costs, because
nothing polls the new name until then.

**The offline serial checks change shape, not existence.** A serial no longer has to be a legal topic
level — that check moved to the meter id, which is now the device level of every topic. The
leading-zero refusal stays exactly as it was; what it costs an operator who ignores it is now a
latched `identity-mismatch` instead of a silent `DroppedUndeclaredDevice`, and its message says so.

**One accidental safety net survives on purpose.** Because the declaration table stays keyed by
serial, a reading whose serial matches no declaration is still `DroppedUndeclaredDevice` rather than a
measurement published under somebody else's name. That is a second line behind the concordance guard,
not a substitute for it: with the guard removed, the first mismatched reading would be dropped and
the meter would publish nothing at all, which is a silence rather than a lie — and a silence is what
this repository has twice had to learn to make loud.

## Falsification

Recorded with the tests. Deleting the concordance guard's refusal must publish a foreign meter's
reading — that is the mutation the guard exists for. Keying the topic on the serial again must fail
the device-level assertions. And `contract_golden` refuses a `CONTRACT_VERSION` of 13 with no golden
written for it, which is what caught this change before anything else did.
