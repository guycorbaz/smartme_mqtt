# Story 1.5: Pure `Fresh|Stale|Failed` state machine

Status: done

Tracked as GitHub issue [#7](https://github.com/guycorbaz/smartme_mqtt/issues/7) (label `epic-1`).
Autonomous sprint run 2026-07-25 (sprint-1-decisions.md D1).

## Story

As a developer,
I want the staleness decision as a pure, property-tested function,
So that "is this a lie?" is decided deterministically and off the network.

## Acceptance Criteria

1. `core/state_machine.rs` exposes a pure `step(prev, tick, now) -> (next, effect)` over `Fresh|Stale|Failed`, importing no tokio/transport crate.
2. Freshness = `http_date − value_date`; maps `system_time < 2020-01-01` → STALE, `age < 0` → STALE, `age > threshold` → STALE; only a fresh in-bounds reading → Fresh.
3. Cold start: STALE-until-proven (never a restored last-known value shown fresh).
4. The five header fixtures each map to their documented verdict (fresh only for `valid`), asserted by `tests/staleness_injected_clock.rs` via `FakeClock`.

## Design notes

- `Policy { max_age_ms }.step(prev, tick, now) -> (State, Quality)` — effect = the Quality to
  stamp on the publication. `Tick = Result<Reading, SourceError>` (the epic's tick fields live in
  `Reading`; `now` is the third argument, per the AC's own signature).
- **`Failed` is ABSORBING** (decision D6, from review): a fatal error latches until process
  restart — ADR 0009 "stop + surface", config is restart-only; a later Timeout must not launder
  `Bad` into `Stale`, a later Ok proves nothing about broken config. Fatal is judged BEFORE the
  boot-clock guard (clock-independent).
- Plausibility floor (2020-01-01) applies to the host `now` AND to `http_date` (an internally
  consistent pair dated 1970 is not a live reading).
- The integration test reads the REAL fixture files and parses them (IMF-fixdate hand parser,
  bounded fields) — contract-of-record exercised, not paraphrased. Verified ages: valid = +950 ms
  → Fresh; negative_skew = −3 599 050 ms; huge_skew ≈ +1 an.

### Review Findings

Adversarial review 2026-07-25. Auditor: **fully conformant** (AC1–AC4 PASS; tick-shape deviation
judged acceptable against the AC's own signature). Patches applied:

- [x] [Review][Patch] `prev` was a dead parameter — `Failed` silently recoverable, a Timeout after Fatal upgraded `Bad`→`Stale` → `Failed` now absorbing, first-match row in the transition table, 2 new tests [core/state_machine.rs]
- [x] [Review][Patch] Boot-clock guard masked `Fatal` as `Stale` → Fatal/absorbing judged before the floor guard + test [core/state_machine.rs]
- [x] [Review][Patch] Cloud pair never floor-checked (1970-consistent pair → Fresh) → `http_date < PLAUSIBILITY_FLOOR` → Stale + test [core/state_machine.rs]
- [x] [Review][Patch] `Quality::Stale` input row undocumented/untested → table row + test [core/state_machine.rs]
- [x] [Review][Patch] `SANE_NOW` constant was 2025 while the comment said 2026 → corrected [core/state_machine.rs]
- [x] [Review][Patch] Test parser accepted seconds ≥ 60 and day 0/32+ → range checks [tests/staleness_injected_clock.rs]
- [x] [Review][Patch] `Policy.max_age_ms` misconfig silent → documented (non-positive = all-Stale, fail-safe; Epic 3 config oracle rejects at load) [core/state_machine.rs]
- [x] [Review][Defer] Frozen/replayed feed (byte-identical response, `http_date` frozen WITH `value_date`) stays Fresh — needs cross-tick `http_date` monotonicity state; documented in code + ADR 0004; deferred as Epic 2 additive oracle [deferred-work.md]
- [x] [Review][Defer] 1-second `Date` truncation can yield spurious sub-zero ages (flapping risk) — spec-literal `age < 0 → STALE` kept (fail-safe direction); tolerance tuning deferred to Epic 2 [deferred-work.md]
- [x] [Review][Defer] Future-dated coherent pair (both stamps +1 an, small age) → Fresh — same cross-tick oracle as the frozen-feed case [deferred-work.md]
- Dismissed: hysteresis/debounce (instant demotion is the deliberate "when in doubt, STALE" choice — alarm noise accepted over lies); State/Quality "redundancy" (State drives the poll task, Quality is the wire effect; `(Stale, Bad)` vs `(Failed, Bad)` is the retryable/fatal distinction); meter-clock-vs-cloud-clock skew inside `age` (ADR 0004 documents the audited premise: live meters showed 0.9–48 s coherence).

## File List

- crates/smartme-bridge/src/core/state_machine.rs (new)
- crates/smartme-bridge/src/core/source.rs (modified — `pub type Tick`)
- crates/smartme-bridge/src/core/mod.rs (modified — decl + re-exports)
- crates/smartme-bridge/tests/staleness_injected_clock.rs (new — 6 fixture-driven tests)

## Change Log

- 2026-07-25: Implemented; adversarial review (7 patches, 3 deferrals D6); 13 inline + 6 integration tests; gates green. Status → done.
