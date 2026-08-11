# Story 4.17: Fix the Will QoS — the specification says 1, we send 0

Status: done

## Story

As the bridge,
I want my death certificate registered at the QoS the specification requires,
so that the broker is obliged to deliver it rather than permitted to lose it.

## Why it ran now, out of order

ADR 0030 sets the epic order at `2 → 3 → 4 → 6 → 7 → 8` and names this story as its one
exception, taken immediately. The reason is not that it is small: **the wire is only cheap to
break while nothing is historised**, and on 2026-08-10 a live Ignition began reading this bridge
(`spBv1.0/STATE/SCADA` online, and it sent a real rebirth request). Guy confirms nothing is
historised yet and will say when that changes — but that window closes without announcing
itself, and it already half-failed once: the dashboard said "no SCADA reads this" on the evening
of 2026-08-09 and one was reading by the next morning.

## Acceptance Criteria

1. **The will is registered at QoS 1, retain false**, per
   `tck-id-message-flow-edge-node-birth-publish-will-message-qos` (`Sparkplug_5:184`) and
   `-will-retained` (`:185`).
2. **`qos_for` stops returning one value for every message type**, because the specification does
   not.
3. **The unit test that pinned the violation is replaced**, not edited — it required QoS 0 for
   `MessageType::NDeath`, so any repair turned it red.
4. **The conformance matrix moves both affected rows** and every tally is recomputed and checked
   to sum.
5. **The manual is corrected everywhere it stated the deviation**, and rebuilds with no overfull
   box beyond the committed baseline.

## What was done — 2026-08-10

### The norm, read clause by clause rather than generalised

The old doc comment said *"The Sparkplug specification requires QoS 0 and retain false for every
edge-node message."* **That sentence was false in one direction and unsupported in the other**,
and it is the same over-generalisation ADR 0010 made about FR20 — the one `CLAUDE.md` records as
having cost this project a requirement. What the norm actually says for an Edge Node:

| message | QoS | clause |
|---|---|---|
| **will** | **1** | `tck-id-message-flow-edge-node-birth-publish-will-message-qos` (`Sparkplug_5:184`) |
| NBIRTH | 0 | `-nbirth-qos` (`:228`) |
| DBIRTH | 0 | `tck-id-message-flow-device-birth-publish-dbirth-qos` (`:425`) |
| NDATA, DDATA, DDEATH, explicit NDEATH | — | **silent** |

Three of seven mandated; the rest are choices, and they are now written as choices.

### The explicit NDEATH rides at QoS 1, and that is structural

The will is registered from `qos_for(MessageType::NDeath)` and the explicit death is published
from the same variant. Giving them different guarantees would require a distinction the norm
refuses to make: `tck-id-...-death-payload` (`Sparkplug_5:808-812`) makes the shutdown certificate
**byte-identical** to the registered will. So both ride at 1. A lost death is in any case the
precise lie this bridge exists to prevent — the host keeps a frozen value on screen and calls it
current.

### Left open, deliberately and in writing

**DDEATH stays at QoS 0 while NDEATH moves to 1.** A lost DDEATH strands one device on the host
exactly as a lost NDEATH strands the node, so the asymmetry is real and unresolved. Harmonising
them is a decision in its own right and was not the violation this story was written to fix. It
is recorded in `qos_for`'s doc comment and in the manual's *Known limitations*, so it is a named
question rather than an oversight.

### The test asserted the bug, and could not be repaired in place

`every_edge_node_message_is_qos_zero_and_never_retained` required `(AtMostOnce, false)` for every
type **including `NDeath`** — the variant that registers the will. Any fix turned it red, so it
would have had to be edited by whoever fixed the violation, which is where a test stops being
evidence.

Its replacement, `the_delivery_table_matches_the_specification_clause_by_clause`, is **split in
two on purpose**: the mandated rows each cite the clause that fixes them, the chosen rows say
they are ours. A single table would let a future edit move a MUST while looking like a preference.
`MessageType::NCmd` stays absent for the reason the old test already gave — it is inbound, and
asserting a publish rule about a message we never send would pass for an unrelated reason.

### Falsification — three mutations, all red, run before the fix was trusted

| mutation | result |
|---|---|
| `NDeath -> AtMostOnce` (the violation restored) | RED — *"NDeath is MANDATED at AtLeastOnce by the specification, not by us"* |
| `NBirth -> AtLeastOnce` | RED — *"NBirth is MANDATED at AtMostOnce…"* |
| `retain = true` on the unconstrained arm | RED — *"NData must never be retained"* |

Restored byte-for-byte, green. Each mutation failed with its own message naming its own reason,
which is what distinguishes a table test from one assertion wearing three hats.

### Consequences, followed rather than assumed

- **Conformance matrix:** `message-flow-edge-node-birth-publish-will-message-qos` (ch. 5) and
  `payloads-ndeath-will-message-qos` (ch. 6) move `gap (unimplemented)` → `conformant`. Chapter 5
  `30·1·19·49` → `31·1·18·49`; chapter 6 `36·4·10·59` → `37·4·9·59`; total `87·6·37·144` →
  `89·6·35·144`. Each chapter re-checked to sum to its own clause count and the total to 274.
  The "Findings carried forward" entry and the MQTT-Server paragraph — which argued the broker's
  will support was *"not idle here, because … it registers at QoS 0"* — are both amended.
- **Manual:** the chapter 2 mechanism table, the chapter 5 *Delivery semantics* notes and the
  *Known limitations* entry. The limitations entry is **replaced rather than deleted**, by the
  DDEATH asymmetry that now stands in its place.
- **`subscribe_to_commands`'s doc comment** said *"`qos_for` returns QoS 0 for every message"* to
  argue that the subscribe QoS does not contradict it. The argument survives; its premise did not.
- **Manual rebuilt:** exit 0, 68 pages, and the overfull boxes are **exactly the five in the
  committed baseline**, verified by building HEAD and comparing rather than by assuming.

**A defect of my own, found by that comparison.** The first build carried a *new* overfull vbox
of 22.9 pt. Three attempts blamed the chapter 5 notes and none of them removed it; the cause was
a **cell in chapter 2's mechanism table**, which a `tabular` cannot break across a page. Story
4.10's review had recorded that exact shape — *"two verbose table cells made a non-breakable
tabular taller than the page"* — and it was made again one story later. Shortening the cell
removed it.

## Dev Agent Record

### Completion Notes List

- AC1–AC5 met. `[#26]` closable.
- `./scripts/ci-local.sh` — see the commit.

### File List

- `crates/smartme-bridge/src/app/mqtt_driver.rs`
- `docs/sparkplug-conformance.md`
- `docs/manual/chapters/02-understanding-sparkplug.tex`
- `docs/manual/chapters/05-mqtt-sparkplug-contract.tex`

### Review Findings — 2026-08-11

Three review layers (blind adversarial, edge-case, acceptance audit). **The behaviour this story
changed is correct** — `qos_for` returns QoS 1 for the will and QoS 0 for NDATA/DDATA, which is
what the norm requires. Every finding below is about what the story WROTE DOWN, and the first is
a third instance of the failure `CLAUDE.md` was written to stop.

- [ ] [Review][Patch] **The norm is NOT silent on NDATA and DDATA — it mandates QoS 0, and this story replaced a true sentence with a false one** [`crates/smartme-bridge/src/app/mqtt_driver.rs:196`] — `tck-id-payloads-ndata-qos` (`Sparkplug_6_Payloads.adoc:1314-1315`) and `tck-id-payloads-ddata-qos` (`:1371-1372`) both read *"MUST be published with the MQTT QoS set to 0"*. Verified by grep against the vendored spec during review. The deleted text was accurate. Consequences: the doc table row, the test's `// CHOSEN. The norm is silent on these` grouping (`:1747`), the manual (`05-…tex:255-257`), and the count "three of seven mandated" (it is five). `docs/sparkplug-conformance.md:979,1006,1016` still states the MUST, so the repository now contradicts itself. The test whose stated purpose is *"a single table would let a future edit move a MUST while looking like a preference"* files two MUSTs under preference.
- [ ] [Review][Patch] **The citation justifying "the explicit NDEATH rides at QoS 1, structurally" is about the Host Application, not an Edge Node** [`crates/smartme-bridge/src/app/mqtt_driver.rs:205-207`] — `Sparkplug_5:808-812` is `tck-id-operational-behavior-host-application-death-payload`: the Host Application's STATE will, JSON UTF-8, `online`/`timestamp`. Verified during review. Nothing in chapters 5 or 6 mandates byte-identity between the explicit NDEATH and the registered will. The QoS 1 choice stands on its own merits; it is a CHOICE, and is currently presented as structural on an off-topic clause.
- [ ] [Review][Patch] **`-will-retained` does not recompose into a real identifier** [`crates/smartme-bridge/src/app/mqtt_driver.rs:194`] — under the stated prefix it yields `…-publish-will-retained`, which does not exist. The clause is `tck-id-message-flow-edge-node-birth-publish-will-message-will-retained` (`Sparkplug_5:185`). The line number and the verdict are right; the abbreviation is not.
- [ ] [Review][Patch] **Nine `conformant` rows cite a test that no longer exists** [`docs/sparkplug-conformance.md:288,306,308,310,383,648,979,1006,1016`] — `every_edge_node_message_is_qos_zero_and_never_retained` was replaced, not renamed-through. It survives as evidence in the conformance matrix, in prose at `:405,:1152,:1331`, in `docs/adr/0017-a-retained-ncmd-is-a-replay-not-a-request.md:13` and in `docs/primary-host-state-observation.md:314`. Confirmed: 9 files still name it. `:1331` also describes as open a finding this story closed.
- [ ] [Review][Patch] **AC1's evidence is weaker than the sibling clause the same document grades `gap (unproven)`** [`crates/smartme-bridge/src/app/mqtt_driver.rs:1726`] — `the_delivery_table_matches_the_specification_clause_by_clause` asserts on `qos_for(NDeath)`, a pure function. No assertion observes the QoS handed to `set_last_will`. Three lines away, `-will-message-retain` stays `gap (unproven)` with the reason *"no test observes the registered will's retain flag"* — the same derivation, graded differently, and the tally moved on the difference.

**Came back clean:** the shutdown flush carries no QoS-1-specific hazard (a fresh `AsyncClient`
per session, so no cross-session redelivery of an unacked death); AC3 (the test was replaced, not
edited) and AC4's arithmetic (ch. 5 `31+1+18+49=99`, ch. 6 `37+4+9+59=109`, total `274`) both
verify.
