# Story 8.3: The Tier-3 session — the contract attested against a live Ignition

Status: in-progress

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

## Session Record — 2026-08-21

**Ignition 8.3.7 Maker Edition, MQTT Engine 5.0.0-rc1, contract v10, broker
192.168.1.30:1883, group `ContractV10`, node `BridgeContractNode`, meter `30000001`.** Two passes
of the bridge gate an hour apart, on the same group. Conducted with the operator at the Designer;
the full account is in `docs/ignition-contract-runbook.md` under *What the 2026-08-21 session
established, and what it left owed*.

**The story is NOT done.** Step 6 was not performed in either pass, so NFR17 is not re-attested
at v10, R3 stays *avérée* and milestone 3 stays unreached.

### AC1 — the six steps, with their result

| Step | Result |
|---|---|
| 1 — cold start | **Pass.** Null metrics accepted with their datatype, **no invented `0`**, `EngUnit` `kW`/`kWh`, device folder named by the serial, `Node Control/Rebirth` present and `false`, `Contract/Version = 10` read by a real host for the first time |
| 2 — first reading | **Pass.** `1.234` / `5678.9`, both `Good` |
| 3 — the update | **Pass.** `2.345` / `5679.1`, timestamp moved, counter up |
| 4 — honest STALE | **Pass**, and it is the point: `Good → Bad_Stale` with the node **online** and `Death Count = 0`. Values **froze** rather than blanking |
| 5 — rebirth from Ignition | **Pass.** NCMD received from the Designer, one NBIRTH and one DBIRTH gained, `bdSeq unchanged at 1 ✓`, both log events printed |
| 6 — the two deaths | **NOT PERFORMED.** The session ended before it |

**Where the Rebirth control lives** (asked by this story): `Edge Nodes/<group>/BridgeContractNode/
Node Control/Rebirth` — a metric name containing `/` becomes a folder under MQTT Engine.

**How steps could have passed wrongly, and what excluded it.** Step 4's `Bad_Stale` is worthless
on its own — a tag Ignition has just created reads that way; the `Good → Bad_Stale` transition
with `online = true` and `Death Count = 0` is what excludes a transport-level staleness, which is
the false pass that nearly slipped through on the v2 run. Step 5's birth could have come from a
reconnect; since story 4.10 one CONNECT is one `bdSeq`, so `bdSeq unchanged at 1` excludes it
without needing the log.

### AC2 — the v4→v10 additions, looked for rather than assumed

**Looked for in both places** — the browser column and the tag's properties pane — as this
criterion requires. **`Cause` was absent after step 4's DDATA and present after step 5's rebirth
DBIRTH.** Contract v4's property does not reach the operator unless a BIRTH declares it; a
property arriving first in a DDATA is ignored by MQTT Engine.

That answers [#68] and unblocks story 2.1's task 3. It is [#107], with its residual stated there:
the group was reused, so the clean version of the experiment is still owed — one pass on a group
Ignition has never seen, collapsing and re-expanding the tag at step 4 before reading. **It should
be run before any contract change is decided on this basis.**

*Recorded honestly, because it nearly went the other way*: the operator's first report of the
step-4 absence was withdrawn ("je n'ai pas fait attention"), and the conclusion drawn from it was
retracted. The second pass re-established it on independent evidence. An observation nobody can
situate is not a measurement — the rule this runbook already carried, applied against my own
first write-up.

### AC3 — what this session does NOT attest

- **Step 6, at all.** Not performed. The double NDEATH of ADR 0011 remains unobserved by a
  consumer.
- **The per-metric verdict, v6's breaking change.** Every step publishes one verdict for the whole
  reading, so `Verdicts::uniform` is what reached the wire and the v6 path never diverged from the
  v5 one. **A pass here says nothing about v6's headline change.**
- **The clean form of [#107]'s measurement**, for the reason given above.
- ~~**The MQTT Engine version.**~~ **Not a gap — struck 2026-08-22.** The version is
  `5.0.0-rc1`, given on 2026-07-28 (story 4.4, task 4) and carried by the run register's
  2026-08-03 row. This bullet claimed it had never been recorded; it had been recorded twice, and
  the register was two lines from the claim. Nothing is owed.

### AC4 — clean-up

**Done.** `Edge Nodes/ContractV10/BridgeContractNode` deleted by the operator, confirmed. The
broker needs none: every message is published with `retain = false`.

### AC5 — the record

`docs/ignition-contract-runbook.md`: a row in *Record of runs* and a full section beneath it. The
runbook's *Both gates* paragraph was **corrected in the same commit** — it still told operators
that neither gate installs a `tracing` subscriber and to treat every log-shaped item as absent.
That was true when written and false on the day; the session observed both rebirth events
printing.

Issues: [#44] closed on observation, [#100] confirmed on the wire, [#107] and [#108] opened.

### What the session found that nobody was looking for

- **[#108] — the gate's own step-1 checklist says `Contract/Version` reads `3`.** The bridge emits
  10. An operator following it to the letter records a failure where there is a success, and one
  nearly did. The repair is to print the constant, not a literal.
- **[#108] — step 4 asks whether the value froze, where the Designer cannot answer it.** The Value
  column renders the quality string for any non-good tag, so frozen and blanked look identical
  there. The `value` sub-row holds the number, and the checklist does not say so.
- **The tag browser rounds to two decimals** (`FormatString` defaults to `#,##0.##`), while steps
  2 and 3 ask for the values to be checked *exactly*. Mid-session this produced a false alarm that
  cost ten minutes: a `Power` display one refresh behind read as a metric that had not updated.
- **`EngHigh = 100` is an Ignition default**, and `Energy = 5678.9` is fifty-six times it. A tag
  with scaling enabled would clamp to `100` and look exactly like a unit bug in the bridge.

### What was still owed before this story closes — ALL PAID 2026-08-22

1. ~~**Step 6**, in a full pass.~~ **Done.** See the 2026-08-22 record below.
2. ~~**[#107]'s clean measurement** on a virgin group.~~ **Done**, on `ContractV10b`.
3. ~~The MQTT Engine version.~~ **Struck 2026-08-22 — it was never missing.** `5.0.0-rc1`.

## Session Record — 2026-08-22 — THE COMPLETE PASS

**Ignition 8.3.7 Maker Edition, MQTT Engine 5.0.0-rc1, contract v10, broker 192.168.1.30:1883,
group `ContractV10b`, node `BridgeContractNode`, meter `30000001`. Six steps, all passed.**
The full account, including the log queries and the two steps found unmeasurable, is in
`docs/ignition-contract-runbook.md` under *What the 2026-08-22 session established*.

**NFR17 is re-attested at contract v10. R3 falls. Milestone 3 is reached.**

### AC1 — the six steps, with their result

| Step | Result | What else could have produced the same screen, and how it was excluded |
| --- | --- | --- |
| 1 — cold start | **Pass** | A typeless or string tag would also read `Bad_Stale` with no value. Excluded by reading the datatype in the Script Console: `Float8`. The Tag Editor cannot answer this — see the runbook's new note |
| 2 — first reading | **Pass on values and qualities; the timestamp clause NOT attested** | A clamped `Energy` would look like a unit bug: excluded, the tags carry `Scale Mode = Off` / `No_Clamp`. The acquisition-vs-reception clause cannot be measured by this gate at all — it stamps readings with `now`, so the check passes either way |
| 3 — values update | **Pass** | A display one refresh behind reads as a metric that never moved (it cost ten minutes on 2026-08-21). Excluded by refreshing before concluding |
| 4 — STALE, node online | **Pass** | A tag reads not-good for reasons unrelated to quality. Excluded by node `online` and `Death Count = 0` before believing it. Values **frozen not blanked**, read in the `value` row: `2,35` / `5 679,1` |
| 5 — rebirth from the Designer | **Pass** | A reconnect would mint a **new** `bdSeq` since story 4.10; the gate printed `bdSeq unchanged at 1 ✓`. A retained NCMD would log `reason=Retained`; none. The operator wrote `true` twice, so the counts are 2/2 — one birth of each kind per request, the ratio the step asks for |
| 6 — the two death certificates | **Pass** | A keep-alive timeout takes ~30 s: excluded, the deaths are instant and paired to certificates in Ignition's log. Another node going offline: excluded, the log names `ContractV10b/BridgeContractNode` |

### AC2 — the v4→v10 additions, looked for rather than assumed

**`Cause` is absent**, on a virgin group, after folding and unfolding the tag, and the Tag Editor's
`Custom` category is empty. So the observation is no longer explicable by the reused group that
confounded 2026-08-21. **The operator cannot see why a value is not good** — the whole purpose of
contract v4 does not reach them.

`Contract/Version = 10` was read by the host, as on 2026-08-21.

### AC3 — what this session does NOT attest

- **The per-metric verdict (v6).** Unchanged from 2026-08-21: every step publishes one verdict for
  the whole reading, so `Verdicts::uniform` is what reached the wire and the v6 path never diverged
  from the v5 one. A pass here says nothing about v6's headline change.
- **[#68]'s question — what Ignition does with a property it does not know — is answered only in
  its negative half.** `Cause` does not surface. **The measurement has no positive control**: it is
  the only custom metric property this bridge publishes, so nothing proves Engine *could* have
  surfaced one. Two mechanisms remain compatible, and they call for different repairs — declaring
  `Cause` at BIRTH, or carrying the cause as a metric. Detailed in the runbook.
- **Step 2's acquisition-vs-reception timestamp clause.** Not measurable by this gate; recorded as
  unattested rather than passed, with the repair named.

### AC4 — clean-up

`Edge Nodes/ContractV10b/BridgeContractNode` deleted, and only that folder.

### AC5 — the record

Written to `docs/ignition-contract-runbook.md`: a new row in the run register and a full section.
Milestone 3 and R3 to be updated in the project sheet outside the repository.

### What this session found that nobody was looking for

- **`Offline DateTime` tracks the WILL**, not the explicit certificate — 16:59:31, the second of two
  deaths 2.001 s apart. This confirms the 2026-07-31 probe and settles what 2026-08-03 left
  unobserved: ADR 0011's *"the explicit certificate is immediate"* is real on the wire and
  **invisible from the host side on this field**.
- **Ignition does not complain about the second death.** Its Engine module logged exactly two lines
  in a 3 h 10 export, both `INFO`. The step's question had never had a direct answer before — only
  an inference from a counter.
- **`Death Count` read between the two deaths gives `1`**, and manufactures a divergence with every
  other run. It happened here and was caught. The runbook now says to read it twice.
- **The log noise is measured, not asserted**: 109 lines in the 25 s around the deaths, 9 `ERROR`
  and 32 `WARN`, none of them ours. **The two lines that matter are 2 of 109** — scrolling would
  have produced a confident false failure.
