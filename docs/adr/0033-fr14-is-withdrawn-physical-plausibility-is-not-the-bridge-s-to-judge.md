# ADR 0033 — FR14 is withdrawn: physical plausibility is not the bridge's to judge

- **Status:** accepted
- **Date:** 2026-08-12
- **Withdraws:** FR14. **Amends:** Epic 2's scope. **Supersedes:** nothing.
- **Decided by:** Guy, 2026-08-12, on reading story 2.4's draft — *"ce n'est pas le rôle de
  smartme_mqtt : ton rôle est de collecter des données de compteur et de les afficher, pas de les
  juger."*
- **Issue:** [#72](https://github.com/guycorbaz/smartme_mqtt/issues/72)

## Context

FR14 read: *"The bridge can flag instantaneous values outside plausible physical bounds rather than
propagating them silently."* It was assigned to Epic 2 and had no code. Story 2.4 was written to
give it some on 2026-08-12, and the draft is what exposed the problem — before any implementation,
which is the cheapest place for it to have surfaced.

**The draft could not fill in its own central criterion.** AC2 needed two numbers: the ceiling each
supply behind a meter can deliver, and whether any meter can see negative active power — smart-me
reports `ActivePower` signed, so photovoltaic injection would be indistinguishable from a fault if
the floor were set at zero. Neither number is knowable from anything the bridge receives. Both are
facts about an electrical installation.

**A second symptom pointed the same way.** The draft's AC7 had to argue at length about *where a
constant describing Guy's distribution board should live* — core constant, or configuration
reaching the validation table, the web UI form and the hot-versus-restart classification. That
argument is a sign, not an incident: a component debating how to store the rating of someone
else's breaker has taken on a responsibility that was never its own.

## Decision

**FR14 is withdrawn.** The bridge does not judge whether a value is physically plausible. Story 2.4
is withdrawn with it, unimplemented.

### The line this draws, and why it leaves the other oracles standing

The withdrawal is not a retreat from *"never lies to the SCADA"*. It separates two things the
requirement list had run together:

**Internal contradictions — kept.** The bridge detects these knowing nothing of the world beyond
what it is handed: a timestamp older than the allowance (FR11), an energy index that went backwards
(FR15), a serial that is not the one declared ([ADR 0029]), a unit it cannot convert, a payload
that is missing a field or carries a non-finite number (FR16). Each is a contradiction *inside the
data or against the configuration the operator wrote*. None requires a model of the installation.

**External plausibility — withdrawn.** This is the only class that requires knowledge the bridge
does not receive and cannot verify: what the supply can deliver. FR14 was its only member.

The test that separates them: **can the bridge be wrong about this in a way it cannot detect?** A
counter that went backwards is a fact about two numbers it holds. A power reading "too high" is an
opinion about a building, and the bridge would be wrong every time the building changed — a new
breaker, a heat pump, panels on a roof — without any signal that it had started refusing real
measurements.

### What replaces it

Nothing. A value the bridge cannot vouch for is already published with a quality that says so and a
cause that names why; a value it *can* read and convert is published with its own timestamp. What
the value *means* physically belongs to the consumer, which is where the knowledge of the
installation actually lives.

## Consequences

**No code changes, and no contract change.** FR14 had no implementation. `CONTRACT_VERSION` stays
at 6, the cause vocabulary is untouched, and nothing already on the wire moves. This is the whole
reason the withdrawal is cheap today and would not have been in three weeks.

**Epic 2 loses one FR and one story.** FRs covered become FR4, FR5, FR8, FR9, FR10, FR15, FR16.
Story 2.4 is withdrawn; 2.5, 2.6 and 2.7 are unaffected and are NOT renumbered — seventeen
references to story numbers live in Rust doc comments and the issue tracker, and [ADR 0030]'s
renumbering was already paid for once.

**[#69] loses its only identified subject, and that is now an open question rather than a wait.**
Story 2.3 AC3 changed the adoption rule for `last` and `energy_reference` from *"the source did not
mark it `Bad`"* to *"the composed verdict did not refuse it"*, and recorded itself UNMET because no
input could tell the two rules apart. A bounds oracle was the first candidate to produce a `Bad` on
a reading the source called `Good`. With FR14 gone, no such oracle is planned:

- FR16's payload validation (story 2.5) does not qualify as drafted — a non-finite value is already
  marked `Bad` by the source adapter, so the source is not content.
- FR10's clock-skew work (story 2.7) judges timestamps, which is the freshness path.

So the honest question is no longer *"when will this rule be provable?"* but **"does a rule no input
can distinguish from the one it replaced deserve to stay?"** — with reverting to the old guard as a
real answer. Deliberately not decided here: this ADR withdraws a requirement, and reversing a
shipped adoption rule is a separate decision that deserves its own examination rather than being
carried along by this one. Recorded on [#69].

**A drafting practice is vindicated and worth naming.** The repository's rule — *"never defer a
decision to an artifact that does not exist"* — is what surfaced this. Story 2.4 refused to invent
its bounds and said in its own status line that AC2 was not implementable. Had it picked a
comfortable 40 kW instead, the requirement would have shipped, the oracle would have worked in
tests, and the first refusal of a real reading would have arrived on some sunny afternoon with
nobody expecting it.

## What would reopen this

- **A meter the bridge itself specifies**, rather than one it reads. If the bridge ever configures
  or commissions a meter, it would hold the installation facts legitimately, and refusing an
  impossible reading would be within its knowledge.
- **A consumer that cannot do it.** If Ignition proves unable to express a plausibility band on a
  tag, the argument moves from *whose role* to *who is capable* — a different question with a
  different answer, and one to settle against a measured limitation rather than an assumption.

[ADR 0029]: 0029-the-declared-serial-is-checked-against-the-one-smart-me-reports.md
[ADR 0030]: 0030-epics-run-in-numeric-order.md
