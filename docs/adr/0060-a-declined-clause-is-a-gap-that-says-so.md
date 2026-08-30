# ADR 0060 — A declined clause is a gap that says so, and no count moves

- **Status:** accepted
- **Date:** 2026-08-30
- **Decides:** what verdict the conformance matrix records for a clause the bridge deliberately
  declines to implement.
- **Issue:** [#42](https://github.com/guycorbaz/smartme_mqtt/issues/42)
- **Related:** [ADR 0018](0018-no-primary-host-state-the-repair-is-host-initiated.md) (Primary Host /
  STATE declined), [ADR 0052](0052-uniqueness-is-asserted-by-the-operator-not-verified-by-the-node.md)
  (descriptor uniqueness declined), Story 4.2 (which split `gap` into two labels on the same
  reasoning), Story 4.3 (which chose `gap` over `n/a` for the eleven), Story 4.5.

## Context

**The matrix contradicts itself in a single cell, thirteen times.** Its own legend defines the
verdict:

> **gap (unimplemented)** | We do not do the thing the clause requires; the row carries an owning
> story, epic or issue

That is a debt: something owed, with an owner who will pay it. Eleven Primary-Host/STATE rows carry
that verdict *and* the annotation **"DECIDED, not pending: ADR 0018 declines the mechanism"**. Two
uniqueness rows carry it with **"decided, not oversight (ADR 0052)"**. The verdict says *not yet*;
the annotation says *never*. Both are in the same cell, and a reader scanning the Verdict column
sees only the first.

**Neither ADR could fix it, and each said so.** ADR 0018 handed the question to *"whoever next owns
the matrix"* — an owner who does not exist, which `CLAUDE.md` forbids as a deferral to an artifact
that does not exist wearing a friendlier face. Story 4.5 declined to move the rows on its own
authority for a better reason: the argument for moving them is ADR 0018, which 4.5 itself wrote, and
a story does not grade its own homework. ADR 0052 simply kept `gap (unimplemented)` and wrote the
decision into the cell beside it. **Two ADRs, two different annotation spellings, one unresolved
verdict** — which is the shape of a convention forming by accident.

**The question [#42] asks is not whether to implement.** That was settled on 2026-07-31 by ADR 0018,
on four grounds, with four revisit conditions. What is open is which word records it.

## Decision 1 — a third sub-label: `gap (declined)`

> **gap (declined)** — The clause binds and we do not do what it requires, **and that is a recorded
> position rather than a debt**: the row carries its ADR, and the ADR carries the conditions that
> would reopen it.

**No count moves, and that is a property rather than a convenience.** `gap` remains one verdict; this
is its third label, exactly as Story 4.2 made `unimplemented` and `unproven` its first two. That pass
recorded the reasoning this one reuses: *"the inherited definition already covers both cases, so this
is a labelling change, not a verdict change: no ADR, no re-audit, no number moves."* The
whole-specification total stays `118 · 8 · 22 · 155 = 303` and every chapter tally is untouched.

**The distinction is worth a label for the same reason the first two were**: the three need
different work. `unproven` wants a test. `unimplemented` wants a fix. **`declined` wants nothing** —
and a reader who cannot see that from the Verdict column will keep re-opening a question that has
been answered, which is precisely what happened to the eleven for thirty days.

**The word is not invented here.** `docs/manual/chapters/02-understanding-sparkplug.tex` has been
showing the operator a Sparkplug-feature table whose second column reads **declined** for Primary
Host / STATE and **absent** for DCMD and multiple brokers — the same distinction, in the same two
words, in the artifact with the least tolerance for nuance. The matrix was the document that lacked
the vocabulary, not the project.

## Decision 2 — `n/a` is refused, and the criterion that refuses it is now written down

The reflexive reading makes the eleven vacuous: nine carry their condition in their own text (*"**If**
the Edge Node is configured to wait for a Primary Host Application…"*), the bridge has no such
configuration, so nothing binds. The matrix rejected that reading and stated why, in a paragraph a
reader had to go looking for. **It is adopted here as the criterion**, because it generalises past
these eleven:

> **What is the absent capability a fact about?**
>
> - **A fact about the world we measure** → `n/a`. A smart-me meter has no writable output; nothing
>   we build changes that, and there is no datum a DCMD could address. The clause addresses a role we
>   do not play.
> - **A fact about our own software** → `gap`. The bridge *has* a session whose behaviour could
>   depend on a Primary Host; we never built the option. The clause addresses a role we **do** play,
>   in a way we chose not to.

The Primary-Host antecedent is the second kind, and the host is not hypothetical: the broker this
bridge publishes to carries live `spBv1.0/STATE` topics today. Calling the eleven `n/a` would file a
whole mechanism — one nobody had considered until Epic 4 audited for it — in the same column as the
MQTT-Server profiles, where nothing would ever look at it again.

**A reviewer wanting to overturn this must attack the criterion, not the verdict.** If "absent
capability" is one category rather than two, the eleven move to `n/a`, chapter 5 becomes
`32 · 1 · 6 · 60` and the whole-specification total becomes `118 · 8 · 11 · 166`.

## Decision 3 — the label is enforced, not merely defined

`scripts/check-conformance-arithmetic.py` gains two checks, because a convention that only lives in
a legend drifts the way the two annotation spellings above drifted:

1. **Every `gap (declined)` row cites an ADR.** The label's whole content is *"a decision exists"*;
   a row that claims it without naming the document is the annotation-without-evidence failure this
   ADR exists to end.
2. **A prose split over the gaps sums to the gap total**, in either the two-label or the three-label
   form. The existing check knew only `split N unimplemented / M unproven`; adding a third label
   without teaching the checker would have silently retired it.

**Both are falsified against fixtures that ship with them** (`scripts/fixtures/conformance/`), run by
`--self-test` on every gate pass rather than once by hand — including a fixture the checker must
**not** flag, so it proves discrimination and not noise. `CLAUDE.md`: *keep the mutations, do not
just record them.*

## What this applies to, today

**Thirteen rows move label, none moves verdict.**

| Rows | Chapter | Decided by |
| --- | --- | --- |
| `message-flow-edge-node-birth-publish-phid-wait`, `-wait-id`, `-wait-online`, `-wait-timestamp`, `-phid-offline` | 5 | ADR 0018 |
| `operational-behavior-edge-node-birth-sequence-wait`, `-termination-host-offline`, `-termination-host-offline-reconnect`, `-termination-host-offline-timestamp` | 5 | ADR 0018 |
| `operational-behavior-primary-application-state-with-multiple-servers-state-subs`, `-walk` | 5 | ADR 0018 |
| `intro-edge-node-id-uniqueness` | 1 | ADR 0052 |
| `topic-structure-namespace-unique-edge-node-descriptor` | 4 | ADR 0052 |

**ADR 0052 is amended on this one point.** Its Consequences read *"The matrix keeps
`gap (unimplemented)` for the descriptor clause, now pointing here rather than reading as an
oversight"*. The decision it records is unchanged; the verdict word becomes `gap (declined)`, which is
what that sentence was reaching for and had no label for.

## What this does NOT decide

**Two rows cite [#27], which is closed** — `case-sensitivity-sparkplug-ids` (chapter 5) and
`payloads-nbirth-edge-node-descriptor` (chapter 6). A `gap` whose owner is a closed issue satisfies
the legend's letter and fails its purpose, and the second of the two is the third statement of the
very requirement ADR 0052 decided. Whether they are `declined` under ADR 0052 or want an owner of
their own is a separate reading of ADR 0052's scope, and it is recorded on [#42] rather than settled
here — deciding it in passing, inside an ADR about labels, is how the eleven acquired three different
owner sets in the first place.

## Consequences

- **The Verdict column becomes readable on its own.** `declined` is visible without reading the cell,
  which is the failure mode that kept #42 open: every reader had to reach the annotation to learn
  that the question was closed.
- **`gap (unimplemented)`'s definition narrows** to what it always meant — a debt with an owner — and
  the legend now says so explicitly rather than by implication.
- **The two annotation spellings disappear.** *"DECIDED, not pending"* and *"decided, not oversight"*
  are replaced by the label plus its ADR link.
- **The eleven rows keep their 4.4 measurements** (`relevance relevant / irrelevant`, and the two
  undetermined readings). Declining a mechanism does not unmeasure it, and the measurements are what
  a revisit would start from.
- **A future `declined` row cannot be written without an ADR**, because the gate refuses it.

## Revisit conditions

1. **A `declined` row's ADR is superseded or reopened.** The label asserts a live decision; if ADR
   0018's conditions fire (a second broker, store-and-forward, RBE, or a host that never asks), its
   eleven rows return to `gap (unimplemented)` with an owner — and the label is what makes that
   transition visible.
2. **A third party asks for a conformance statement.** `declined` is this repository's word, not the
   specification's. Chapter 10's profiles are what an external claim is measured against, and a
   consumer of that claim may need `deviation` — a verdict this matrix already has, and which asserts
   something stronger: that we knowingly do otherwise *while claiming conformance*. Nothing here
   forecloses that re-reading; it would move counts, and this does not.

## Alternatives considered

**Move the eleven to `n/a`.** Rejected by Decision 2: it is the reading that makes an unbuilt
mechanism invisible, and the criterion distinguishing it from the DCMD rows is real and measured.

**Keep `gap (unimplemented)` and amend only the legend**, so that the verdict covers debts and
decisions alike. Rejected: it makes the Verdict column mean *less*, and the reader who scans the
column — the reader this whole failure is about — gains nothing. The legend would carry a
disjunction, and the annotation would still be the only way to tell which half applies.

**Write a fourth verdict, `declined`, outside `gap`.** Rejected: it moves counts, forces a re-audit
of what the totals mean, and breaks the property that makes this change safe. A declined clause *is*
unmet; hiding that in a fourth column is the `n/a` mistake with a longer name.
