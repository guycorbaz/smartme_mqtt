# Story 3.6: Orphan retained messages — a requirement whose subject the bridge cannot produce

Status: done (2026-08-15) — **Guy arbitrated the same day: option 2, « Reformuler au
manuel »** (ADR 0035, [#84]). The bridge's half stays verified-by-construction (nothing
retained, clause-pinned); the operator's half is the manual's new troubleshooting section
(« A ghost value on a topic nothing publishes »: detect with `mosquitto_sub --retained-only`,
clear with `mosquitto_pub -r -n`, clean the host's persisted tag). PRD, epics and manual
amended together with the ADR, per the repository's rule. No purge machinery was built, and
that is the deliverable: a requirement re-aimed at the actor who can honour it.

## Story (as the epic states it)

As the operator,
I want retained messages on old topics purged when the mapping changes,
so that a SCADA host never reads a ghost value from a topic nothing publishes any more.

## The finding that reshapes it, verified rather than assumed

FR21 (*"purge orphan retained messages on old topics when a mapping changes — no ghost
values"*) has a premise: that retained messages exist on the bridge's old topics. **The bridge
cannot produce one:**

- Sparkplug MANDATES `retain = false` where it speaks at all (births:
  `tck-id-payloads-nbirth-retain`, `-dbirth-retain`; data: `-ndata-retain`, `-ddata-retain` —
  the story 4.17 review counted five of seven edge-node messages constrained), and the bridge
  CHOSE `retain = false` for the rest (DDEATH, the explicit NDEATH) — pinned by
  `qos_for`'s table tests since 4.17, each mandated row citing its clause.
- The will is registered `retain = false` likewise.
- The epic already recorded the consequence on 2026-08-08: *"it is also near-moot today — the
  bridge publishes everything with `retain = false`, so it cannot create the orphans FR21
  purges; the requirement guards against orphans left by something else."*

So FR21, read literally, asks this bridge to clean up after **tools that are not it** — a
prior integration, a manual `mosquitto_pub -r`, another publisher on the same namespace. The
bridge cannot know which topics such a tool retained onto; it can only guess at patterns under
its own group id, and publishing zero-byte retained clears onto guessed topics is machinery
built on speculation about other systems' behaviour — the exact shape this epic refused twice
today (3.4's decision 1, 3.5's decision 1).

## The three honest options, for Guy

1. **Withdraw FR21** (the FR14 pattern, ADR 0033's sibling): cleaning up after other tools is
   not this bridge's role; what IS its role — never leaving its own ghosts — is already
   guaranteed by construction and pinned by tests. An ADR records it; the PRD, epics and
   manual move together. *Cost: nothing. Residual: a foreign retained ghost, should one ever
   exist, is the operator's to clear.*
2. **Reshape into documentation**: keep the protective intent as a manual section — how to
   FIND a foreign retained message under the bridge's namespace (`mosquitto_sub -v --retained-only`)
   and how to clear it (`mosquitto_pub -r -n`), with the warning that a Sparkplug host
   persists what it discovers. The requirement becomes *"the manual tells the operator how"*,
   which is checkable. *Cost: a manual section. My recommendation, for what it is worth: the
   intent (no ghost values) is real, the mechanism (the bridge purging) was aimed at the
   wrong actor.*
3. **Build the purge anyway**: a UI action publishing zero-byte retained messages across the
   bridge's own topic patterns for the PREVIOUS mapping on every mapping change. *Cost: real
   machinery, a new wire behaviour, and its own failure modes (a purge racing a foreign
   publisher; patterns guessed) — for a fault the bridge cannot cause and nobody has
   observed.*

## What this story does NOT question

The retain-false invariant itself, and its tests — they stay regardless of the arbitration.

## References

- [Source: `_bmad-output/planning-artifacts/prd.md:294`] — FR21's letter
- [Source: `_bmad-output/planning-artifacts/epics.md:315`] — the near-moot record, 2026-08-08
- [Source: `docs/adr/0033-...md`] — the withdrawal precedent and its criterion ("can the
  bridge be wrong about this in a way it cannot detect?" — here, stronger: the bridge cannot
  even PRODUCE the fault's subject)
- [Source: story 4.17's table tests] — the retain rows, clause-cited
