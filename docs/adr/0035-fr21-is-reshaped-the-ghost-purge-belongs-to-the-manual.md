# ADR 0035 — FR21 is reshaped: the ghost purge belongs to the manual, not the machine

- **Status**: accepted
- **Date**: 2026-08-15
- **Deciders**: Guy Corbaz, choosing among the three options story 3.6's draft laid out
  (« Reformuler au manuel »)
- **Amends**: FR21 (PRD, epics and manual moved together, per the repository's decision rule).
  **Sibling of**: ADR 0033 (FR14's withdrawal) — the same criterion, a different verdict.
- **Issue**: [#84](https://github.com/guycorbaz/smartme_mqtt/issues/84)

## Context

FR21 read: *"The bridge can purge orphan retained messages on old topics when a mapping
changes (no ghost values)."* Its premise is a retained message on the bridge's old topics —
and the bridge cannot produce one:

- Sparkplug mandates `retain = false` where it speaks at all (`tck-id-payloads-nbirth-retain`,
  `-dbirth-retain`, `-ndata-retain`, `-ddata-retain` — five of the seven edge-node message
  types constrained, counted clause by clause at story 4.17), and the bridge CHOSE
  `retain = false` for the remaining two (DDEATH, the explicit NDEATH) — every row pinned by
  `qos_for`'s table tests, each mandated row citing its clause. The will is registered
  `retain = false` likewise.
- The epic recorded the consequence on 2026-08-08: the requirement is near-moot — it guards
  against orphans left by *something else* (a prior integration, a manual `mosquitto_pub -r`,
  another publisher sharing the namespace).

Read literally, FR21 asked this bridge to clean up after tools that are not it, on topics it
can only guess at. Publishing zero-byte retained clears onto guessed topics is machinery built
on speculation about other systems' behaviour — the shape Epic 3 refused twice on 2026-08-15
(stories 3.4 and 3.5, each declining a mechanism resting on unobserved behaviour).

ADR 0033's criterion — *can the bridge be wrong about this in a way it cannot detect?* —
applies here in a stronger form: the bridge cannot even produce the fault's subject. But
unlike FR14, the protective INTENT is real and actionable: a foreign retained ghost, should
one exist, would sit under the bridge's namespace and mislead the same SCADA host, and an
operator can find and clear it in two commands.

## Decision

**FR21 is reshaped, not withdrawn: the intent (no ghost values) is kept; the actor changes.**

1. **The bridge's own guarantee stays what it is**: it publishes nothing retained, by mandate
   or by choice, and the clause-cited tests that pin every row stay untouched. The bridge
   cannot leave a ghost; no purge machinery is built.
2. **The operator's half becomes documentation**: the manual's troubleshooting chapter gains
   a section on foreign retained messages — how to detect one under the bridge's namespace
   (`mosquitto_sub -v --retained-only`), how to clear it (`mosquitto_pub -r -n` on the exact
   topic), and why it matters (a Sparkplug host persists what it discovers, so a ghost
   outlives its publisher).
3. **FR21's text becomes checkable**: *"The bridge publishes nothing retained (verified by
   clause-cited tests), and the manual documents how an operator detects and clears a foreign
   retained message under the bridge's namespace."* The first half is machine-checked today;
   the second is a section whose existence a reviewer can verify.

## Consequences

- No new wire behaviour, no new failure modes, no purge racing a foreign publisher.
- The PRD's FR21, the epics' references, and the manual move together with this ADR.
- Story 3.6 closes on this decision; Epic 3's story set is settled.
- The residual is named: clearing a foreign ghost remains a human act, informed by the manual.
