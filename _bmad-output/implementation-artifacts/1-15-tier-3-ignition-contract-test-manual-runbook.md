# Story 1.15: Tier-3 Ignition contract test (manual) + runbook

Status: done (2026-07-26) · Issue [#17](../../issues/17)

Closes Epic 1. Both ACs met — and the test earned its cost on its first run.

## What was built

- `crates/sparkplug-b/tests/ignition_contract.rs` — `#[ignore]`d, interactive. Publishes a
  scripted session, stops at each of five steps, prints what to look for and waits. There is
  no automated assertion worth the name: the assertion is a human on the tag browser.
- `crates/sparkplug-b/tests/ignition_contract.rs::quality_code_probe` — added mid-story to
  settle a diagnosis by measurement rather than derivation (below).
- `docs/ignition-contract-runbook.md` — arming, running, what each step proves, how to read a
  failure per step, and the required Ignition-side clean-up.

`rumqttc` and `tokio` became dev-dependencies of `sparkplug-b`. They never enter the
dependency graph of anyone consuming the published crate, and the isolated-build guard
(`--no-default-features`) does not compile them; `no_context_leak` still passes.

## What it found: quality codes that read as GOOD

**The point of the story, and it landed on the first run.**

Step 4 — republish the same values with quality `STALE`, node still online — showed both tags
as **Good** in Ignition. Guy caught it and pushed back rather than accepting the run as a
pass. That was the whole difference: steps 1 and 5 *looked* like corroboration and were
worthless as evidence.

- **Step 1** shows a non-good quality, but the metrics are `Null` there — Ignition would show
  that regardless of our property. Confounded.
- **Step 5** shows Bad, but that is MQTT Engine's own STALE-on-NDEATH, not our property.

**Step 4 was the only step that isolated the quality property.** Before the fix there was no
evidence Ignition had *ever* honoured it.

### Diagnosis, measured not assumed

A second observation settled it: `192` displayed as `Good`, `500` as `Good(500)`. The
parenthesised subcode is the tell — the quality **level lives in the top bits** of the 32-bit
code and the low 16 bits are a subcode. `500` has clear top bits, so it is a *good-level*
code with an unrecognised subcode. The published tables list 256–511 as an "uncertain" band,
but those are subcode allocations; the raw integer decides nothing on its own.

Cross-checked against the one documented example: Cirrus Link shows `Bad_Disabled` as
`-2147483133`, and `0x80000000 | 515` is exactly that.

The derivation was sound, but a derivation is not a measurement and these numbers were about
to become a wire contract. `quality_code_probe` published six tags carrying identical values
and differing only in their `Quality` property, so the host itself named each code. Confirmed.

### The defect

| Quality | v1 published | Ignition read it as |
| --- | --- | --- |
| `Good` | 192 | Good ✔ right by coincidence |
| `Stale` | 500 | **`Good(500)`** |
| `Bad` | **0** | **`Good_Unspecified`** |

Both non-good codes failed **towards good** — the one direction a quality field must never
fail. `Quality::Bad` is reached on live paths: a fatal source error, an unrecognised unit, a
non-finite value. A v1 bridge told its SCADA that an unusable value was trustworthy.

Fixed in `CONTRACT_VERSION` 2: `Stale` → `Bad_Stale` (`-2147483132`), `Bad` → `Bad`
(`-2147483136`). `Stale` deliberately reuses the code a host raises on a node DEATH, so
transport-level and app-level staleness present identically — one visible outcome, whichever
mechanism noticed, which is what the two-mechanism design always promised.

Guarded by a property that outlives the exact constants: **no non-good quality may have clear
top bits**. That is what was violated, not any particular number.

## Why nothing else could have caught it

The defect lived in the code from Story 1.8. It survived 148 green tests, a three-layer
adversarial review, and two chaos tests that specifically assert quality propagation —
because every one of them compares our encoder to our decoder, or to our own enum. None could
see that `500` does not mean "stale" to a real host.

This is precisely the argument the PRD made for a Tier-3 oracle, and it paid for itself on
first contact.

## Verified after the fix

**Ignition 8.3.7, contract v2, 2026-07-26: all five steps pass** — including step 4 (both tags
go non-good while the node stays online) and step 5 (the double NDEATH of ADR 0011 is a no-op
for Ignition, not a fault). Recorded in the runbook's run table alongside the v1 failure.

Worth noting for anyone re-reading the diagnosis: Cirrus Link's published documentation for the
MQTT modules is written against Ignition 8.1, while the host here is 8.3.7. That mismatch did
not matter, because the quality codes were established by *measuring* what this host honoured
rather than by reading those tables — which is fortunate, since the tables are what misled the
original implementation.

## Residual

- **The MQTT Engine module version was not recorded**, only the Ignition platform version. The
  module is what decodes Sparkplug, so it governs conformance more directly. Capture it at the
  Epic 8 release-gate re-run.
- Test groups left in the Ignition tag tree need deleting: `ChaosTest`, `ContractTest`,
  `ContractTest2`, `QualityProbe`, `ContractV2` — the cost of measuring against the only
  broker available, which is production.
- `quality_code_probe` is kept: the same class of question will recur whenever the quality set
  is extended.

## File List

- `crates/sparkplug-b/tests/ignition_contract.rs` (new)
- `crates/sparkplug-b/Cargo.toml` (dev-dependencies)
- `docs/ignition-contract-runbook.md` (new)
- `crates/sparkplug-b/src/model.rs` (quality codes + regression guard)
- `crates/sparkplug-b/src/encode.rs` (test expresses the code through the enum; pins Int32)
- `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs` (`CONTRACT_VERSION` 1 → 2)
- `docs/manual/chapters/04-mqtt-sparkplug-contract.tex` (quality table, version history)

## Change Log

- 2026-07-26: Contract test + runbook delivered; found and fixed the quality-code defect
  (#22, contract v2); re-verified against Ignition. 148 workspace tests green; fmt, clippy
  `-D warnings` and the manual build all green.
