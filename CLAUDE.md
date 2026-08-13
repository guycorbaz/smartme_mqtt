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

## Tests: falsify before trusting

A test written against already-correct code proves nothing by passing. Any new test asserting an
invariant must be run against **deliberately broken code** and observed to fail; record the
falsification next to the test. If it cannot be made to fail, it is not yet a test.

Four tests in Epic 1 passed for the wrong reason — a fake clock that never advanced, a `bdSeq`
comparison of a constant against itself, a drain that ran where nothing could fail, and a
discriminator spanning two clocks. Falsification caught all four.

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

Run `./scripts/ci-local.sh` (`--fast` skips the Docker-dependent chaos tests). It reproduces both
GitHub workflows verbatim and checks that `Cargo.lock` is committed. After pushing, check
`gh run list` — "tested locally" is not the claim "CI passes". The isolated workflow builds with
`--locked` and stayed red for six commits while local runs were green.

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
