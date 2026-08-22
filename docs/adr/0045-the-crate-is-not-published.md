# ADR 0045 — The crate is not published

- **Status:** accepted
- **Date:** 2026-08-22
- **Decides:** that `sparkplug-b` is not published to crates.io — a position, not a postponement.
- **Issue:** [#3](https://github.com/guycorbaz/smartme_mqtt/issues/3)

## Context

AR21 asks for the **publication bar**, and says so in its own words: *"Structured now; actual
publish deferred."* Story 8.2 delivered that bar — semver, README, CHANGELOG, MSRV, complete
metadata, a documented conformance scope, and a test that no public signature hands out a
third-party type. AR21 is **met**.

What was never decided is whether the deferred publish would happen. It has sat as [#3] since the
crate existed, and a deferral nobody revisits becomes an intention nobody owns.

## Decision

**`sparkplug-b` is not published to crates.io.** Guy decided this on 2026-08-22 and restated it.
The position is recorded here rather than left in an issue, because the difference between
*deferred* and *not planned* changes what other work is worth doing.

### What makes publishing a poor trade today

- **It cannot be undone.** A crates.io version is not deletable, only *yanked* — and a yanked
  version stays installable for anyone who already pinned it. This is the one irreversible act on
  the list, and this repository's own rule is to ask before those.
- **The crate is exercised almost entirely through the bridge.** Its four integration tests are the
  public-API purity guard, a `seq`/`bdSeq` property test, a context-leak check, and the Ignition
  gate. **Nothing feeds `decode` hostile bytes** — truncated, random, or a valid protobuf that is
  not this message — and `decode` is exactly where a stranger arrives with bytes they did not
  make. Story 8.2's review already found three defects in the eight lines a consumer meets first
  on crates.io, in a README that nothing compiled; it is now compiled, and it is one example.
- **The advertised conformance scope rests on a document with known stale citations.** The README
  states which clauses are in scope, backed by the conformance matrix — and [#101] records that at
  least four of its fifty-three code citations no longer point at code.

### The trademark boundary, which is not about royalties

The specification is **EPL-2.0** (`docs/spec/sparkplug-b-3.0.0/LICENSE`), which grants a
*"non-exclusive, worldwide, **royalty-free**"* copyright licence and the same for patents, covering
*"make, use, sell, offer to sell, import"*. **Implementing Sparkplug B and distributing or selling
the result costs nothing.**

What is not free is the **name**. `Sparkplug®`, `Sparkplug Compatible` and the logo are Eclipse
Foundation trademarks, with a formal compatibility programme and a TCK behind them. So:

- *"implements the Sparkplug B specification"*, with a stated conformance matrix and its declared
  deviations — a factual claim this project can support;
- *"Sparkplug Compatible"*, or the marks as branding — a different claim, needing the programme.

**If publication is ever reopened, this is a precondition and not a detail.**

## Consequences

- **[#3] closes as decided**, not as done. The bar remains; the act does not happen.
- **The crate's public API is free to break**, which it did twice on 2026-08-22 — ADR 0042 removed
  `BdSeq::before_first` and changed `NodeSession::start`'s signature. Both were free precisely
  because of this decision.
- **The publication bar is not wasted.** It closed a real leak (`prost::DecodeError` in a public
  signature), it made the README something `cargo test` fails on, and it forced the conformance
  scope to be written down. Those hold whether or not anything is published.
- **This ADR does not amend AR21**, which asked for the bar and deferred the publish. It decides
  the deferral.
