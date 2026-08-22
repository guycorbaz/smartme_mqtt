# ADR 0043 — The cause is declared at BIRTH, or it does not exist

- **Status:** accepted
- **Date:** 2026-08-22
- **Decides:** the arbitration story 2.1's task 3 deferred on 2026-08-10, now that it has its measurement. Bumps `CONTRACT_VERSION` to **11**, breaking.
- **Issue:** [#107](https://github.com/guycorbaz/smartme_mqtt/issues/107), and it answers [#68](https://github.com/guycorbaz/smartme_mqtt/issues/68)

## Context

Contract v4 exists so an operator can see **why** a value is not trustworthy: every non-good
metric carries a `Cause` property naming the check that refused it. Seven contract versions have
refined that vocabulary — v5, v7, v8, v9 and v10 each added or split causes.

**None of it ever reached an operator.**

Measured during the Tier-3 session of 2026-08-21, and again on 2026-08-22 on a group the host had
never seen:

| When | On the wire | In the tag browser |
|---|---|---|
| cold-start DBIRTH | no property at all | — |
| DDATA, quality `Stale` | `Cause = reading-too-old` | **no `Cause` row** |
| DBIRTH after a rebirth | metrics re-announced **with** their properties | **`Cause = reading-too-old`** |

**Ignition materialises a metric property only when a BIRTH declares it. A property arriving for
the first time in a DDATA is ignored.** The third row is the positive control: the same host, the
same property, present the moment a BIRTH declared it.

So in production, a meter healthy at its BIRTH that degrades later publishes its cause in DDATA
only — and **nobody will ever see it**, unless a rebirth happens to intervene. Our bytes are
conformant. The design assumption did not survive contact with the host.

## Decision

**The `Cause` property is declared in every BIRTH and published in every message — a `Good`
metric included, where it carries the explicit value `no-cause` (`CAUSE_NONE`).**

And **the cold-start BIRTH declares `no-reading-yet`**, a cause added to the vocabulary by this
ADR, rather than the neutral value.

### Why publishing on every message follows from declaring at BIRTH

This is not a second decision; it is the same one. **A host holds the last value of a property it
was sent.** If the property is declared at BIRTH and then omitted the moment a metric recovers,
Ignition keeps displaying `reading-too-old` beside a healthy number, indefinitely. That is
strictly worse than the silence it replaces: silence is uninformative, a stale cause is **false**,
and this project exists to prevent exactly that failure.

So the choice is not *declare or not*. It is *declare and always publish*, or *neither*.

### Why the cold start names a cause instead of `no-cause`

Those two metrics are `Stale`. Answering *no cause* for a non-good metric would be a lie of the
same shape. Until v11 they were the only non-good pair this bridge published that named no reason
at all — an operator seeing `Bad_Stale` seconds after a start could not tell a fresh bridge from a
broken feed. `no-reading-yet` degrades and never latches: the first successful poll ends it.

### What this costs, stated rather than minimised

*"A good metric carries no cause"* was a real principle, and it is right: a reason published beside
every good value is noise a consumer learns to ignore, and then misses the day it means something.

**The principle survives where it is true and pays where it is not.** In the domain, nothing
changes: `Verdict::cause()` is still an `Option`, and the `Cause` vocabulary still names only
reasons — `no-cause` is deliberately **not** a `Cause` variant. What pays is the wire, at one
boundary, which is where a host-shaped concession belongs.

## `CONTRACT_VERSION` moves to 11, and it is BREAKING

By the constant's own rule, on two counts. The **tag set changes**: a consumer browsing a `Good`
metric sees a property that did not exist there under v10 — the criterion that bumped v4. And the
**cause vocabulary grows by one** — the criterion that bumped v5. The Tier-3 runbook's promise,
*two runs sharing a version number attest to the same tag set*, would be false across this
boundary.

**The 2026-08-22 attestation therefore no longer covers what the bridge emits.** A Tier-3 session
is owed before production, and it is short — and it doubles as the experiment that confirms this
repair works, because the property either appears at step 4 or it does not.

## Consequences

- **A guard had to be repaired to accept a legitimate change.** `contract_golden`'s completeness
  check asserted that every version's name list had the same length as the shipped one. It could
  only pass while the contract's names never changed, and it failed here saying *"a name was added
  or dropped without the golden saying so"* — about a golden that said so perfectly well. **It
  forbade change where it meant to require a declaration.** Replaced by `NAME_SET_CHANGES`, which
  requires the declaration and refuses a padded entry: a version listed there must actually differ
  from its predecessor. This is Epic 8 retrospective action **H1** arriving on the day it was
  written.
- **The manual's guard learned the neutral value from the constant**, not from a list written into
  the test — it accused `no-cause` of being an invention, correctly, until it was given the live
  source.
- **The manual said `Contract/Version` was `8` while the bridge emitted `10`.** Corrected to 11,
  and worth naming: the chapter's mechanical guard reads routes and cause slugs, not numbers, so a
  stale constant in prose is still unguarded here.
- **`Cause::NoReadingYet` is now in the vocabulary**, so `/healthz` and the screens can show it.

## Falsification

| # | Mutation — the ordinary shape | Went red with |
|---|---|---|
| 1 | the `with_property` dropped from `cold_start_metrics` — what "simplifying an unused-looking line" produces | *"the cold-start BIRTH must DECLARE Cause on Power: a property a BIRTH does not declare is one Ignition never materialises"* |
| 2 | the cold start declares `CAUSE_NONE` — the natural error of declaring without asserting anything | `left: "no-cause"`, `right: "no-reading-yet"`, *"the neutral value would be a lie about a non-good metric"* |
| 3 | `metrics_for` falls silent again on a good metric (`None => built`) — the exact code that stood here | three tests, in two modules: the publisher's cause test, the per-metric refusal test, and `poll_publish`'s end-to-end one |
| — | *(recorded)* mutation 1 leaves `metrics_for`'s tests **green**, and mutation 3 leaves the BIRTH test green. The BIRTH declaration and the every-message publication are two halves that fail independently, which is why each has its own guard. |
