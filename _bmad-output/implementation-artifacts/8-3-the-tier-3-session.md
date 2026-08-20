# Story 8.3: The Tier-3 session — the contract attested against a live Ignition

Status: ready-for-dev

> **This is the last thing the project owes, and the only one that cannot be done alone.** The
> bridge's contract has been attested against a real Ignition **once**, on 2026-08-03, at
> `CONTRACT_VERSION` **3**. It emits **10**. Seven versions have shipped since, and every one of
> them changed what an operator sees in the tag browser.
>
> **What the four epics since have proved is that the bytes are right by calculation and on the
> wire.** What no test in this repository can prove is that a real Sparkplug host *accepts* them
> — that is what a Tier-3 session is for, and why the runbook says plainly that there is no
> assertion in it worth the name: the instrument is a person looking at a tag tree.

## Story

As the author about to run this bridge against my own SCADA,
I want the contract it emits today observed in Ignition's tag browser, step by step,
so that "conformant" is something somebody watched happen rather than something a test suite
inferred.

## Before the session — what Guy needs at hand

- **Ignition running with the MQTT Engine module**, connected to the same broker as the bridge,
  and the **Designer open** with the MQTT Engine tag provider selected in the Tag Browser.
- **The Ignition version, written down.** A pass is only meaningful against a stated version.
- **A disposable group name** — `ContractV10`. The gate refuses `Site`, and the runbook explains
  why: a Sparkplug host *persists what it discovers*, so the group becomes a folder in the tag
  tree that outlives the test and has to be deleted by hand.
- **The broker's host and port.**

The command, from the runbook:

```bash
SPARKPLUG_CONTRACT_BROKER=<host>:1883 \
SPARKPLUG_CONTRACT_GROUP=ContractV10 \
  cargo test -p smartme-bridge --test ignition_contract -- --ignored --nocapture
```

**Step 5 cannot be automated and is the reason a person must be there**: the rebirth is
triggered *from the Designer*, and the gate reads the `bdSeq` off the wire before and after. A
rebirth we publish ourselves proves only that the bridge answers us.

## Acceptance Criteria

**AC1 — the bridge gate runs to completion against a stated Ignition version.**

**Given** the six steps of the bridge gate
**When** each is performed
**Then** its result is recorded — pass, fail, or *not observed* — beside the version of Ignition
it was performed against
**And** a step that could have passed for the wrong reason is recorded with what else would
have produced the same screen, which the runbook already states per step.

**AC2 — the v4→v10 additions are looked for, not assumed.**

**Given** that seven contract versions shipped since the last attestation
**When** step 4 shows a non-good value
**Then** the `Cause` property is looked for **in both the browser column and the tag's properties
pane** — its absence from one is a display setting, not a contract failure
**And** whether the operator can see *why* a value is not good is recorded as the observation it
is, because that is the whole purpose of v4.

**AC3 — what the session does NOT attest is written down, in the same document.**

**Given** the runbook's own two structural gaps — the per-metric verdict (v6) is not exercised
by a gate that publishes one verdict for both metrics, and [#68]'s question about what Ignition
does with a property it does not know
**When** the result is recorded
**Then** both are restated as still-unattested rather than quietly covered by a green run.

**AC4 — the tag tree is cleaned up, and that is part of the procedure.**

**Given** the folder `ContractV10` in Ignition's tag provider
**When** the session ends
**Then** it is deleted, and any retained message under the group is cleared
**And** the clean-up is recorded as done — a host that keeps discovering a dead test group makes
the next session's evidence ambiguous.

**AC5 — the record outlives the session.**

**Given** the result
**When** it is written up
**Then** it lands in `docs/ignition-contract-runbook.md`'s result log with the date, the
Ignition version, the contract version and the outcome per step
**And** the project's risk register and milestone 3 are updated to match — R3 is *avérée* today
precisely because this attestation is seven versions stale.

## Out of scope

- **The crate gate.** It attests a narrower thing (ADR 0012's quality-code question) and the
  runbook says to run it only for that purpose.
- **Fixing whatever the session finds.** A defect found here becomes an issue and a story; this
  one is the observation.

## Dev Notes

### What must not break

- **The group name is disposable and is not `Site`.** The gate enforces it; the tag tree is why.
- **Nothing here changes the contract.** If the session finds a defect, `CONTRACT_VERSION` moves
  in the story that repairs it, not in this one.

### References

- [Source: `docs/ignition-contract-runbook.md`] — the procedure, the two gates, and what each step proves
- [Source: `docs/ignition-contract-runbook.md`] — *"What changed since the last run — v3 → v10"*, written before the run so the scope is decided in advance
- [Source: `https://github.com/guycorbaz/smartme_mqtt/issues/68`] — the measurement this session carries but does not settle
- [Source: `CLAUDE.md`] — for a human-run gate, every step must say what else could make it pass
