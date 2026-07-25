# Story 1.4: `Source` seam + fake

Status: done

Tracked as GitHub issue [#6](https://github.com/guycorbaz/smartme_mqtt/issues/6) (label `epic-1`).
Autonomous sprint run 2026-07-25 (commit+push per story pre-approved — sprint-1-decisions.md D1).

## Story

As a developer,
I want the meter source behind a `Source` trait with a fake,
So that the staleness machine can be exercised deterministically without network.

## Acceptance Criteria

1. **Given** the `core` module, **when** it is compiled, **then** it defines `trait Source` yielding a per-meter reading `{ value, value_date, http_date }` (or a typed error), plus a `FakeSource` that scripts `Ok` / transient error / timeout sequences.
2. **Given** `FakeSource`, **when** a test drives it, **then** it can reproduce fetch success, a transient failure, and a cloud timeout without any network or tokio.

## Design (party-mode decision D5 — see sprint-1-decisions.md)

- Async trait in core via desugared RPITIT `-> impl Future<Output = Result<Reading, SourceError>> + Send`
  (language feature only — arch_purity stays green; the explicit `Send` is what lets the 1.11 poll
  task stay generic across `tokio::spawn`). Sync alternative rejected: would break 1.7's
  "impl Source" AC and make the real timeout path unexercisable (1.14 twin).
- `Reading { value: Measurement, http_date: Option<UtcMillis> }` + `value_date()` accessor —
  no duplicated timestamp; `None` http_date = missing oracle input, never invented.
- `SourceError { Timeout, Transient{reason}, Fatal{reason} }` — skeleton taxonomy on the
  transient/fatal split; `Timeout` minted by the poll task's timeout wrapper.
- `FakeSource`: `VecDeque` script, `then()`/`then_hang()` builders; exhaustion → `Err(Fatal)`
  (never panic — compiled into prod builds; never repeat-last — a fake that lies); `Hang` =
  `std::future::pending` for the paused-time chaos twin; side effects INSIDE the future
  (an unpolled fetch consumes nothing — review patch); `remaining()`/`is_exhausted()`
  introspection. Confined by arch_purity (`FakeSource`, `poll_now(` tokens → core/source.rs).
- `poll_now` noop-waker helper (std-only, `Waker::noop`) drives Respond futures without a runtime.

## Tasks / Subtasks

- [x] Task 1: `core/source.rs` — `Reading`, `SourceError`, `Source`, `FakeSource`, `poll_now` (AC: 1, 2)
- [x] Task 2: `core/mod.rs` — module decl, re-export `Reading`/`Source`/`SourceError` (NOT the fake/helper) (AC: 1)
- [x] Task 3: arch_purity — per-token home-file table, `FakeSource` + `poll_now(` confined (AC: 2)
- [x] Task 4: inline tests — script order (Ok→Transient→Timeout), exhaustion fail-closed, Hang pends without runtime, calls log, unpolled-fetch-consumes-nothing, http_date None (AC: 1, 2)
- [x] Task 5: gates green — fmt, clippy `-D warnings`, tests

### Review Findings

Adversarial review 2026-07-25 (Blind Hunter, Edge Case Hunter, Acceptance Auditor). Auditor:
**fully conformant** (AC1/AC2 + all D5 bullets PASS). Patches from the hunters:

- [x] [Review][Patch] Fake side effects fired at `fetch()` call, not first poll — a fetch built then dropped (lost `select!` race) consumed a script entry the real source would never spend → side effects moved inside the `async move` block + `unpolled_fetch_consumes_nothing` test [core/source.rs]
- [x] [Review][Patch] Script exhaustion could mask harness bugs (green-but-wrong tests) → `remaining()`/`is_exhausted()` introspection so tests assert full consumption; Fatal reason string stays the discriminator [core/source.rs]
- [x] [Review][Patch] `poll_now` reachable from production code unconfined → token `poll_now(` added to the arch_purity table; doc states drop-on-Pending semantics [core/source.rs, tests/arch_purity.rs]
- [x] [Review][Patch] `FakeSource` lacked `Debug`; `poll_now` result silently discardable → `#[derive(Debug)]`, `#[must_use]` [core/source.rs]
- Deferred: `calls` observability across `tokio::spawn` (1.11 tests drive the loop directly per its AC; revisit there if a handle is needed); per-meter script attribution (single-meter skeleton, documented on the type).
- Dismissed: panic-on-exhaustion (contradicts no-panic-in-prod, D5); `Reading::age()` helper (the freshness computation belongs to the 1.5 oracle, one owner); `Eq` on reason strings (intra-project, matches! used); scan-evasion variants and `ends_with` over-match (accepted threat model, D4); NaN via `Reading` PartialEq (policy documented in 1.2/1.7).

## Dev Agent Record

### Agent Model Used

claude-fable-5 (Claude Code) — autonomous sprint run.

### Completion Notes List

- `core/source.rs`: async `Source` trait (RPITIT + `Send`), always-valid `Reading` (bad units
  arrive as `Quality::Bad` inside the `Measurement`, fail-closed — 1.7's contract), 3-variant
  skeleton error taxonomy with hand-rolled `Display`/`Error` (no thiserror, zero new deps),
  lazily-effectful `FakeSource` with scripted silence (`Hang`) and fail-closed exhaustion,
  std-only `poll_now`. 7 inline tests. arch_purity restructured to a per-token home-file table.

## File List

- crates/smartme-bridge/src/core/source.rs (new)
- crates/smartme-bridge/src/core/mod.rs (modified — decl + re-exports)
- crates/smartme-bridge/tests/arch_purity.rs (modified — per-token table, 2 new tokens)

## Change Log

- 2026-07-25: Implemented after party-mode D5; adversarial review (4 patches); gates green. Status → done.
