# ADR 0051 — Epic 9 absorbs the backlog, and issue attachment does not move

- **Status:** accepted
- **Date:** 2026-08-27
- **Decides:** whether the roadmap reopens, and on what terms the open issues are worked.
- **Issue:** [#113](https://github.com/guycorbaz/smartme_mqtt/issues/113).
- **Amends:** [ADR 0030](0030-epics-run-in-numeric-order-starting-with-epic-2.md) — the numeric-order rule stands and is satisfied, since no epic is open. **Supersedes:** nothing.
- **Decided by:** Guy, 2026-08-27 — *« ouvre l'épique 9 pour absorber l'arriéré »*, in answer to the project review's finding.

## Context

`epics.md` was declared exhausted on 2026-08-22, and nine epics closed. The project review of
2026-08-27 measured what that left: `sprint-status.yaml` carried **87 stories `done`, one
`optional`, one `withdrawn`, and nothing in progress**. For a dossier whose action engine is a
forge, the portfolio framework replaces the "one next action" control with *an epic must be in
progress* — so the project had, in its own terms, **no next action at all**, and had had none for
five days.

Meanwhile thirty-one issues were open with no ranking anyone could act on. A cost-of-delay triage
exists — `notes/tri-arriere-issues-2026-08-22.md`, three piles under the criterion *does this issue
change the wire?* — but it lives off the forge, which is the substance of risk `R2`, open since the
project's first day.

**Twenty closures in five days had soldered no milestone.** The backlog thinned where it was
softest, not where a milestone stood: E1 and E6 have been one issue from soldered for days, and that
issue is not the one that fell.

## Decision

**Epic 9 is opened, and it adds no requirement.** Every story closes issues that already exist. No
FR, NFR or AR is added, and each acceptance criterion is written against those issues rather than
against new behaviour.

**Its spine is the 2026-08-22 triage**, and Story 9.1 — the six wire-changing issues — runs *before*
the story that ranks the rest. That inversion is deliberate: the six are constrained by an event
that arrives without announcing itself, the first historised tag, and the others are not.

**Issue attachment does not move.** The open issues stay attached in `gestion/jalons.md` to the
epics that produced them.

> Re-attaching them to Epic 9 would **solder E1–E6 by an act of writing** — six milestones gained
> for nothing, on the same day the register was told to count issues. That is exactly what the rule
> of 2026-08-15 exists to prevent: *one open issue retains its milestone.*

Epic 9 says who does the work now; it does not say where the defect came from. **Closing its stories
is what solders E1 to E6**, and that is the mechanism rather than a side effect.

**Four issues stay outside it.** [#24], [#25], [#101] and [#112] carry conduct and not product, and
retain no epic milestone. Story 9.4 names [#101]'s exclusion explicitly, so that an absence reads as
a decision.

## Consequences

**The milestone count grows to thirteen, and the new one starts unmet.** One epic, one milestone: E9
joins the register. `jalons_atteints` stays at 6, `jalons_total` moves from 12 to 13 — **opening an
epic lowers the completion ratio**, which is the honest arithmetic and worth stating before someone
reads it as a regression.

**ADR 0030 is satisfied rather than broken.** Its rule — a new epic does not open while another is
open — holds trivially: none is.

**E9's own milestone can be soldered without closing the backlog.** No issue is attached to E9
itself, so under the rule it is soldered when its stories are done. That is deliberate: what the
backlog costs is measured by E1–E6, and duplicating the measurement on E9 would double-count it.

**The retrospective is `required`** and, per the 2026-08-10 amendment, is a condition of closing
rather than a debt the epic carries. It must state **how many of the twenty-seven issues fell to
work and how many to a body reading**. [#2] stood open from the project's first day with no matter
in it for weeks; if the ratio leans the same way again, the lesson is about the backlog's
composition and not about this epic.

## Falsification

**This ADR's claim is checkable in one command**, and that is the reason to write the terms down
rather than to trust them:

```sh
gh issue list --state open --json number,labels --jq '[.[] | select(.labels | any(.name | startswith("epic-9")))] | length'
```

**It must answer `0`** for as long as attachment has not moved. A non-zero answer means the open
issues have been re-attached to Epic 9 — the one failure mode this decision exists to forbid, and
the one that would silently hand the project six milestones it has not earned.

[#2]: https://github.com/guycorbaz/smartme_mqtt/issues/2
[#24]: https://github.com/guycorbaz/smartme_mqtt/issues/24
[#25]: https://github.com/guycorbaz/smartme_mqtt/issues/25
[#101]: https://github.com/guycorbaz/smartme_mqtt/issues/101
[#112]: https://github.com/guycorbaz/smartme_mqtt/issues/112
