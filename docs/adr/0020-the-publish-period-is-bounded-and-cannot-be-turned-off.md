# ADR 0020 — The publish period becomes a setting, bounded above, with no "off"

- **Status:** Accepted, **with one number owed** — see *The maximum* below
- **Date:** 2026-08-03 *(recording a decision Guy took on 2026-08-01)*
- **Related:** [ADR 0018](0018-no-primary-host-state-the-repair-is-host-initiated.md) (which made
  this value load-bearing), [#32](https://github.com/guycorbaz/smartme_mqtt/issues/32) (RBE),
  Epics 5 and 6, `poll_publish.rs:143`, `main.rs:237`

## Context

Guy decided on 2026-08-01 that the publish period becomes a web-UI setting. That closes #32 from the
other end: report-by-exception stays out, and cadence becomes a knob instead.

**Two facts about the starting position, because the planning notes state neither.**

First, **the period is not a setting today at all.** It is `Duration::from_secs(30)`, hard-coded at
`main.rs:237`, reachable by no environment variable. (`poll_publish.rs:175` has a 5 s default, but
that is a test fixture and nothing in production reads it.) So this is not moving an existing knob
into the UI — it is creating one.

Second, and this is the constraint that shapes the whole decision: **ADR 0018 made the periodic
publish load-bearing.** It ruled out Primary Host / STATE on the grounds that the repair path for a
host that arrives without a BIRTH is host-initiated, and step 1 of that loop is *"the bridge
publishes DDATA every poll"*. ADR 0018 says so in its own text, under a heading warning that this
*"makes the periodic publish LOAD-BEARING, which it was not before"*.

A form field that lets an operator set the period to `0`, or to `never`, or to a value long enough
that a restarted host waits an afternoon, therefore **silently undoes an ADR**. Nothing would fail;
the recovery path would simply stop starting.

## Decision

**The publish period is a stored setting, exposed in the configuration UI, with three properties:**

1. **A minimum**, so an operator cannot turn the bridge into a load generator against the smart-me
   cloud or the broker.
2. **A maximum**, above which the value is rejected — because the maximum *is* the worst-case
   latency of the recovery path ADR 0018 depends on.
3. **No "off", no `0`, no empty-means-disabled.** The field cannot express "never".

The UI must state, next to the field, *why* the bounds exist. An operator who sees only a rejected
value will work around it; one who is told the period is what lets a restarted host find the node
will not want to.

## The maximum — the one number this ADR owes

The bound is not arbitrary and must not be invented at implementation time. It is the answer to:
**how long may a host that restarted go without hearing from us before somebody notices?**

`CLAUDE.md` forbids deferring a decision to an artifact that does not exist, so this ADR states the
derivation and the proposed value rather than leaving it to the story:

- the period bounds the delay before a restarted host receives DDATA and can request a Rebirth;
- it also bounds the age of the data a consumer holds, which is what the whole *never lies*
  guarantee is about;
- the current hard-coded value is **30 s**, and it has never been felt to be too fast.

**Proposed: minimum 5 s, maximum 300 s, default 30 s** — the default preserving today's behaviour
exactly, the maximum being five minutes, which is short enough that a restarted host repairs within
one coffee and long enough to cut cloud traffic by an order of magnitude for someone who wants that.

**Guy has not ratified the maximum.** It is the single open point in this ADR; everything else is
settled. Until he does, the story must not pick a different number quietly.

## Consequences

- **Story-level:** the bounds belong in the acceptance criteria as a rejection test, not in a
  comment. The falsification is cheap — submit `0`, submit the maximum plus one, assert both are
  refused and that the stored value did not change.
- **ADR 0018 gains a dependency it can see.** Its warning that *"removing the periodic publish
  requires replacing it"* now has a concrete guard: the UI cannot express its removal.
- **#32 must record this.** It already carries *"re-examine RBE once Rebirth lands"*; it should also
  carry that a bounded period is now the agreed alternative to RBE, so a future reader does not
  reopen RBE as though nothing had been decided.
- The period joins the values that move out of `.env` and into stored configuration — which puts it
  under whatever ADR 0019's open item 5 settles, though it is not itself a secret.

## What is NOT decided here

Whether the *fetch timeout* (`main.rs:238`, 10 s) follows the period into the UI. It is coupled — a
timeout longer than the period overlaps polls — and that coupling deserves its own thought rather
than a hurried "yes". The story that implements this must state the relationship it enforces, even
if the answer is that the timeout stays fixed.
