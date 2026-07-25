# Story 1.3: `Clock` seam

Status: done

Tracked as GitHub issue [#5](https://github.com/guycorbaz/smartme_mqtt/issues/5) (label `epic-1`).
Autonomous sprint run 2026-07-25 — commit+push per story pre-approved by Guy (ref sprint-1-decisions.md D1).

## Story

As a developer,
I want time behind an injected `Clock` trait,
So that no truth is ever computed from a hardcoded `now()`.

## Acceptance Criteria

1. **Given** the `core` module, **when** it is compiled, **then** it defines `trait Clock` exposing a monotonic instant and a wall-clock time, with a `SystemClock` production impl and a `FakeClock` test double whose time advances only on explicit calls.
2. **Given** any logic module, **when** `tests/arch_purity.rs` and review inspect it, **then** no direct `SystemTime::now()`/`Instant::now()` call appears outside `SystemClock`.

## Design (decided in party mode — see sprint-1-decisions.md D4)

- Monotonic representation: **`MonotonicMs(pub i64)`** newtype in `core/clock.rs` (ms since an
  arbitrary process-local epoch), mirror of `UtcMillis` (Copy/Eq/Ord, saturating `Sub -> i64`).
  NOT `std::time::Instant` (unfabricable without the banned `Instant::now()`).
- Wall-clock: the existing `domain::UtcMillis` (no second wall-clock type — 1.2 dev note).
- `trait Clock { fn monotonic(&self) -> MonotonicMs; fn wall(&self) -> UtcMillis }` — object-safe,
  consumed as `&dyn Clock` / `Arc<dyn Clock + Send + Sync>`.
- `SystemClock { start: Instant }` — the ONLY holder of raw time sources; conversions via
  `try_from().unwrap_or(i64::MAX)` (never `as`), pre-epoch wall → `UtcMillis(0)` (honest
  sentinel, caught by 1.5's `< 2020-01-01 → STALE` guard).
- `FakeClock { mono: AtomicI64, wall: AtomicI64 }` — `&self` + Relaxed atomics (Sync for the
  Arc-shared tests of 1.5/1.11); `advance_ms` advances BOTH clocks; `set_wall` moves wall only
  (NTP-step scenario). Plain `pub` (integration test bins must import it); guarded by the
  arch_purity token ban instead of cfg/features.
- `tests/arch_purity.rs` gains a third scan: tokens `Instant::now(`, `SystemTime::now(`,
  `use std::time::Instant`, `use std::time::SystemTime`, `FakeClock` banned across `src/**`
  with the single exemption `core/clock.rs`. (Consequence: `FakeClock` is NOT re-exported at
  the `core` root — import it as `core::clock::FakeClock`.)

## Tasks / Subtasks

- [x] Task 1: `core/clock.rs` — `MonotonicMs`, `Clock`, `SystemClock`, `FakeClock` (AC: 1)
- [x] Task 2: `core/mod.rs` — declare module, re-export `Clock`, `MonotonicMs`, `SystemClock` (NOT `FakeClock`) (AC: 1)
- [x] Task 3: extend `tests/arch_purity.rs` with the raw-time-source scan (AC: 2)
- [x] Task 4: inline unit tests — fake determinism (no advance ⇒ equal reads), advance moves both clocks, `set_wall` isolated (NTP backward), saturating monotonic subtraction, thread-sharing, `SystemClock` smoke (AC: 1, 2)
- [x] Task 5: gates green — fmt, clippy `-D warnings`, `cargo test --workspace`

### Review Findings

Adversarial review 2026-07-25 (Blind Hunter, Edge Case Hunter, Acceptance Auditor). Auditor: fully
conformant — AC1/AC2 PASS, every D4 bullet PASS. Patches applied from the hunters' findings:

- [x] [Review][Patch] `FakeClock` RMW races: load+store lost updates, torn mono/wall pair, `set_wall` clobberable by concurrent `advance_ms` → replaced the two `AtomicI64` by a `Mutex<FakeNow>` with poison-recovery (`unwrap_or_else(into_inner)`, no panic path) [core/clock.rs]
- [x] [Review][Patch] Negative `advance_ms` doc-forbidden but unguarded (monotonic rewind fabricable) → signature is now `advance_ms(&self, ms: u64)`: unrepresentable, not documented-against [core/clock.rs]
- [x] [Review][Patch] `wall()` inner overflow path saturated to `i64::MAX` = fails "maximally fresh", opposite of fail-safe → both failure branches now yield the `UtcMillis(0)` STALE-caught sentinel [core/clock.rs]
- [x] [Review][Patch] `MonotonicMs` cross-instance comparability → `SystemClock` doc now mandates single instance shared via `Arc` (composition-root rule) [core/clock.rs]
- [x] [Review][Patch] Header doc overclaimed scan scope ("everywhere else") → scoped honestly to this crate's `src/`, integration tests + sibling crates held by review [core/clock.rs]
- [x] [Review][Patch] `system_clock_smoke` asserted host RTC past 2020 (fails on unset-RTC hosts, the exact environment the 0-sentinel tolerates) → asserts the honest floor instead [core/clock.rs]
- Dismissed (documented design, not defects): textual-scan evasions (threat model = good-faith developer, per D4/Murat; auditor rated it advisory); `FakeClock` ban covering inline `#[cfg(test)]` in `src/` (intended — pure-core unit tests consume plain `MonotonicMs` data, no clock needed); `Sub -> i64` returning bare millis (deliberate mirror of `UtcMillis`, D4); `FakeClock` compiled into release (guarded by scan, D4 V3); scan scope limited to bridge crate (siblings are pure/reviewed).

## Dev Agent Record

### Agent Model Used

claude-fable-5 (Claude Code) — autonomous sprint run.

### Completion Notes List

- `core/clock.rs`: `MonotonicMs(pub i64)` (saturating signed `Sub`), object-safe `Clock` trait,
  `SystemClock` (sole holder of `std::time`; `try_from`-only conversions; every wall failure path
  → `UtcMillis(0)` sentinel), `FakeClock` on `Mutex<FakeNow>` (consistent mono/wall pair,
  `advance_ms(u64)` moves both, `set_wall` models NTP steps both directions).
- `tests/arch_purity.rs`: third scan confines `Instant::now(`/`SystemTime::now(`/`use std::time::
  Instant`/`use std::time::SystemTime`/`FakeClock` to `core/clock.rs` across `src/**`.
- 6 inline tests (fake-never-advances-alone, advance-both, NTP-backward isolation, saturation,
  Arc-thread sharing, SystemClock smoke). Zero new dependencies.

## File List

- crates/smartme-bridge/src/core/clock.rs (new)
- crates/smartme-bridge/src/core/mod.rs (modified — module decl + re-exports, FakeClock excluded)
- crates/smartme-bridge/tests/arch_purity.rs (modified — raw-time-source scan added)

## Change Log

- 2026-07-25: Story created (JIT) after party-mode decision D4. Status → in-progress.
- 2026-07-25: Implemented + adversarial review (6 patches applied, auditor fully-conformant). All gates green. Status → done.
