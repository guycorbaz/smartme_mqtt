# ADR 0054 — The backlog's rank lives in labels, and the criterion story 9.2 mandates has expired

- **Status:** accepted
- **Date:** 2026-08-30
- **Decides:** the arbitration suspended since 2026-08-15 — labels or GitHub milestones — and what
  the scheme ranks by.
- **Issue:** [#113](https://github.com/guycorbaz/smartme_mqtt/issues/113) (Epic 9, story 9.2)
- **Related:** ADR 0051 (Epic 9 and the milestone register), risk `R2`, and
  `notes/tri-arriere-issues-2026-08-22.md`, which held the ranking off the forge.

## Context

`R2` has asked for a ranked backlog since the project opened, and none has held, because no
criterion imposed itself. The production decision of 2026-08-22 supplied one — *does this issue
change the wire?* — and a triage was written against it. **That triage lives in a note, and it was
made by reading titles.** Story 9.2 exists to move it onto the forge and to re-read the bodies.

## Decision 1 — labels, not milestones

**A rank is not a milestone, and this repository has already given that word a meaning.**

- GitHub milestones are **exclusive**: an issue has at most one. The relation that is genuinely
  exclusive here is *which epic produced this issue*, and ADR 0051 already calls
  `gestion/jalons.md` **the milestone register**. Spending GitHub's milestones on a rank would
  collide with the one meaning the project has settled on.
- A rank has to be visible in a list and combinable with other axes — an issue can be both
  *blocked upstream* and *costly to defer*. Labels are non-exclusive; milestones are not.
- The vocabulary already exists in labels (`epic-0`…`epic-8`), so the forge stays internally
  consistent rather than gaining a second scheme.

**Epic attachment stays where it is**, in `gestion/jalons.md`, and stays a body reading. Moving it to
GitHub milestones is a separate question this ADR does not take: it would make the register derived
rather than authoritative, which changes how the project is run and is not what `R2` asked for.

## Decision 2 — the axis, and why it is not the one story 9.2 wrote down

Story 9.2's criterion reads: *"the scheme distinguishes at least **changes the wire** from **does
not**, which is the only criterion the project has ever found decisive."*

**Applied today, that pile is EMPTY.** All ten issues the 2026-08-22 triage placed in pile A are
closed — the last five on 2026-08-29 and 2026-08-30. Story 9.1 emptied it, which was its purpose.

**And the criterion had already come apart from what it stood for.** It was never *changing the
wire* that mattered; it was **what deferring costs**, and the wire was its proxy while the free-change
window was open. Two events ended that:

- **2026-08-28** — commissioning, and historisation started the same hour. The window closed, so
  "wire change" stopped meaning "free now, expensive later".
- **2026-08-29** — ADR 0053 changed the wire and cost **no** history, because the enumeration that
  governs a history rewrite (identity, topic grammar, metric name, unit) did not move. The proxy and
  the thing it stood for were measured apart.

**So the axis is the one the note itself named — the cost of deferring — stated directly:**

| Label | What deferring costs | Members today |
|---|---|---:|
| `cost:data` | a measurement can be **lost or reported wrongly** | 7 |
| `cost:capability` | something stays **impossible** until it is done | 4 |
| `cost:knowledge` | a debt of **proof, trace or record** — it costs *knowing* | 12 |

Two orthogonal markers, non-exclusive by design and therefore impossible under milestones:

- **`blocked-upstream`** — cannot be worked here at all; the cause is a dependency ([#20], [#50],
  both on `rumqttc`'s pinned `rustls-webpki`). Without it a reader mistakes a blocked requirement for
  an unstarted one, which is the confusion [#50] was split out to prevent.
- **`awaiting-measurement`** — the next step is a measurement in front of a real host, not work
  ([#115]). One member, and it earns its name: it is the successor of the emptied wire pile.

`cost:data` is deliberately the smallest and the loudest. It is the rank that touches the project's
motto: an issue in it can leave the SCADA holding something untrue.

## What the body reading found that the title reading could not

Story 9.2 required this check, and it produced one correction and one refinement.

**The correction — [#63] was misfiled by the note's own criterion.** The note placed it in pile C,
*debts of trace and proof*. Its body says the heartbeat set has, since `2a4d5ca`, been what decides
**whether a DDEATH is sent at all**. By the note's own test — *does this change the wire?* — it
belonged in pile A, and it was not there because the title says "nothing asserts that supervisor
spawns one poll task per meter" and a title cannot say that. It stays `cost:knowledge` under this
ADR, because the property is right by construction and what is missing is its proof — but **the
2026-08-22 note misranked it, exactly as it warned it might.**

**The refinement — pile B split in two.** *Bites in operation* held both "a measurement can be
wrong" ([#80], [#85], [#87], [#91], [#78], [#82], [#83]) and "a capability is absent" ([#20], [#49],
[#50], [#52]). Those cost different things and are worked by different people at different times;
one label could not say so.

**And one re-read falls due.** The note wrote: *"[#99] est classé ici comme documentaire, mais il
porte sur la clause QoS de la volonté — donc sur le même mécanisme que [#43] … Si [#43] se traite,
[#99] se relit à ce moment-là."* [#43] closed on 2026-08-29. The re-read is now owed, and it is
noted on [#99] rather than done here.

## Consequences

- **The ranking survives the session that produced it**, which is the whole of `R2`'s complaint. It
  is queryable — `gh issue list --label "cost:data"` — instead of living in a note that ages like
  the epic attachment does.
- **`notes/tri-arriere-issues-2026-08-22.md` becomes a historical record**, not a live instrument. It
  keeps its reasoning, which is what made this ADR possible, and says at its head that the forge is
  now authoritative.
- **`R2` is restated rather than closed.** What it asked for exists; what it still names is that a
  rank by nature does not rank by risk — the note's own limit, *"la gravité d'une issue et la gravité
  du risque qu'elle porte ne coïncident pas"* — and no label scheme will fix that.
- **A rank is a claim and will drift like any other.** Nothing checks a label against the body it was
  drawn from. The mitigation is that there are only three values and each is defined by what it
  costs, not by what it is about, so a wrong one is visible to anyone who reads the issue.
