# Working rules for this repository

Short, hard-won rules. Each one exists because ignoring it already cost something here.

## Sparkplug: read the norm first

**Any doubt about Sparkplug B is settled by reading the specification, before anything else.**
A copy is committed here, pinned and greppable at `docs/spec/sparkplug-b-3.0.0/` (release tag
v3.0.0, EPL-2.0). Cite the `tck-id-…` identifier, not prose.

Not a supplier's documentation page, not a summary table, not memory. That rule is written here
because the alternative has already failed twice:

- **Contract v1 published quality codes a real host read as `Good`.** `Stale` was `500` and
  `Bad` was `0` — both taken from an "OPC-style triple" that Ignition does not use. A live host
  displayed `Good(500)`. Every non-good quality was silently reported as trustworthy, which is
  the exact failure this project exists to prevent.
- **ADR 0010 weakened FR20 on a false premise.** It claimed Sparkplug mandates QoS 0 for every
  edge-node message. The norm says the opposite for the will (`MUST be 1`) and says *nothing*
  about DATA — so the option it rejected as "a specification violation" was legal.

Both came from reading about the specification instead of reading it.

**And where Sparkplug DEFERS to MQTT, the same rule applies to MQTT.** Chapter 1 hands the identifier
character set to *"the MQTT Specification"* and stops. A repository that pins one norm and not the
other can follow that reference exactly one step before it has to guess — which is what happened:
`check_identifier`'s justification was written from memory for a month. The clauses actually relied
on are now quoted verbatim, with their `[MQTT-…]` identifiers and the document's date, at
**`docs/spec/mqtt/README.md`**. It is a pinned citation rather than a vendored copy, and it records
what could NOT be retrieved as well as what could.

It cost two blockings in one day (2026-08-29) before it was written: issue #34 could not be closed,
and issue #43's hypothesis could not be argued — only measured.

## smart-me: two sources, and neither answers the other's question

**What the API DECLARES is at `docs/spec/smart-me-api/openapi-v1.json`. What it actually SENT is
at `crates/smart-me-client/fixtures/`.** Check both, and know which one you are quoting.

The description is authoritative for **presence, type and nullability**. It is **wrong about
names** — it declares camelCase while the wire sends PascalCase, which is why `Device` carries
`#[serde(rename_all = "PascalCase")]`. And it is **near-silent about failure**: 82 of its response
declarations are `200`, and `GET /Devices/{id}` — the only call this bridge makes — declares
nothing else. Error behaviour is learned from the wire, never from this file.

This rule exists because not reading it cost two things on 2026-08-13. **Six of the eight fields
the client consumes are declared nullable and `Device` requires all eight** — an exposure that
stood unnoticed from story 1.6 in July, and one that matters because a null loses the field name
where a missing field keeps it (`invalid type: null, expected f64 at line 3 column 31` against
``missing field `ActivePower` ``), and because a single null would cost the whole reading, energy
index included. And story 3.4's drop-down was being planned against a data source deduced from a
code comment, while `GET /Devices` was there to be read.

**There is no version on the wire** — no path segment, no header; `info.version` says `v1` and
that is the only place it exists. So the copy is the only way to notice the API moved: re-fetch,
diff against the committed file, read the diff. Never refresh it silently.

## Tests: falsify before trusting

A test written against already-correct code proves nothing by passing. Any new test asserting an
invariant must be run against **deliberately broken code** and observed to fail; record the
falsification next to the test. If it cannot be made to fail, it is not yet a test.

Four tests in Epic 1 passed for the wrong reason — a fake clock that never advanced, a `bdSeq`
comparison of a constant against itself, a drain that ran where nothing could fail, and a
discriminator spanning two clocks. Falsification caught all four.

**And the mutation must be the fault's ordinary shape, not the shape it had the day you found
it.** A mutation that re-types the original defect measures your memory of that defect, not the
guard. Story 8.2 shipped a guard against third-party types in the public API, falsified once by
restoring the exact line it was written from — `pub fn decode(…) -> Result<Payload,
prost::DecodeError>`. Its review put two mutations past it with the suite green: the same type
imported under an alias and named bare, which is how a Rust author *normally* writes it, and a
signature wrapped across lines, which is what rustfmt does to any signature past the line width.
The guard was blind to the ordinary case and awake only to the accidental one.

**Keep the mutations, do not just record them.** A falsification performed once by hand proves
the guard for the day it was performed. Where the mutations can live in a fixture the guard is
run against — including one it must *not* flag, so it proves discrimination and not noise —
they are re-falsified on every CI pass instead.

## Manual test steps: state how they could pass wrongly

For a human-run gate, every step must say what *else* could make it pass. The Tier-3 contract
test nearly returned a false pass because two of its five steps showed a non-good quality for
reasons unrelated to the property under test.

## Specifications: never defer a decision to an artifact that does not exist

An acceptance criterion may not say "decided by the test/audit/spike that will exist later".
Either decide at drafting time, or write the measuring spike first and decide on its output.
AR13 deferred the shutdown mechanism to a chaos test that did not exist for the whole of Epic 1,
so the decision simply sat unmade.

## Before pushing

The full gate is a **pre-push hook** since 2026-08-15 (Epic 3 retrospective, D1): once per
clone, run `git config core.hooksPath scripts/hooks`, and every `git push` runs
`./scripts/ci-local.sh` whether you remembered it or not. The rule became a mechanism because
it was remembered for a story and forgotten for its repair — the one CI break of 2026-08-15.
`git push --no-verify` is the only escape, and using it is the on-record claim "this push
needs no gate".

`./scripts/ci-local.sh` (`--fast` skips the Docker-dependent chaos tests; the hook takes no
fast mode) reproduces the GitHub workflows verbatim and checks that `Cargo.lock` is committed.
After pushing, check `gh run list` — "tested locally" is not the claim "CI passes". The
isolated workflow builds with `--locked` and stayed red for six commits while local runs were
green.

Never `git add <directory>` after a dependency change: root-level files (`Cargo.lock`,
`Cargo.toml`, `deny.toml`) are missed that way.

## Decisions

Anything that changes a requirement, the wire contract, or an architectural position gets an ADR
in `docs/adr/` and a GitHub issue. Amend the PRD, epics and manual together — that is how FR20
(ADR 0010) and the shutdown mechanism (ADR 0011) were handled.

Record unmet acceptance criteria as **unmet**, with an issue. Two were, in Epic 1, and that is
what let them be closed properly instead of being discovered in production.

## Deployment facts that change the calculus

Ask rather than assume: the bridge is **not yet in production** and no tag historisation has
begun, so wire-breaking changes are currently cheap. That window closes without announcing
itself.
