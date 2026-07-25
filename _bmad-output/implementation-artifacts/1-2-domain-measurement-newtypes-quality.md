# Story 1.2: Domain model — `Measurement`, physical newtypes, `Quality`

Status: done

Tracked as GitHub issue [#4](https://github.com/guycorbaz/smartme_mqtt/issues/4) (label `epic-1`). Do not commit or push without Guy's explicit approval; when a commit is approved, reference `#4`.

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want the canonical domain types with units carried in the type,
So that no serial, topic, or physical quantity is ever a bare `String`/`f64`.

## Acceptance Criteria

1. **Given** the `domain` module, **when** it is compiled, **then** it defines newtypes `Kw(f64)`, `Kwh(f64)`, `Serial`, `MeterId`, `TopicPath` and a canonical `Measurement { meter: MeterId, serial: Serial, power: Kw, energy: Kwh, value_date, quality: Quality }`, **and** `Quality { Good, Stale, Bad }` is a single definition aligned with the `sparkplug-b` quality enum.
2. **Given** the `domain` module, **when** `tests/arch_purity.rs` scans it, **then** it imports no `tokio`/`axum`/`reqwest`/`rumqttc` (stays pure).
3. **Given** a raw `f64` or `String`, **when** a developer tries to use it as a serial, topic, or physical quantity, **then** the type system rejects it (construction goes through the newtype).

## Tasks / Subtasks

- [x] Task 1: Define `Quality` in `sparkplug-b` (AC: 1)
  - [x] Create `crates/sparkplug-b/src/model.rs` containing ONLY `pub enum Quality { Good, Stale, Bad }` with derives `Debug, Clone, Copy, PartialEq, Eq` (no `Default` — see Dev Notes)
  - [x] In `crates/sparkplug-b/src/lib.rs`, add `pub mod model;` and `pub use model::Quality;` — keep `#![forbid(unsafe_code)]` and the existing `protobuf` module untouched
  - [x] Rustdoc for `Quality` must be generic Sparkplug-audience prose (this crate is crates.io-facing); it must NOT contain the tokens `smartme`, `ignition`, or `SMARTME_` (enforced by `tests/no_context_leak.rs`)
- [x] Task 2: Create `crates/smartme-bridge/src/domain/quality.rs` (AC: 1)
  - [x] Single line of substance: `pub use sparkplug_b::Quality;` — this IS the "single definition aligned with the sparkplug-b quality enum" (see Dev Notes: Quality decision)
- [x] Task 3: Create `crates/smartme-bridge/src/domain/measurement.rs` (AC: 1, 3)
  - [x] Newtypes `Kw(pub f64)`, `Kwh(pub f64)` — derives `Debug, Clone, Copy, PartialEq`. The inner field is deliberately public (adapters/tests read `.0`; there is nothing to validate on a bare quantity), whereas the string newtypes below keep their field private — see next bullet. Do NOT add `From<f64>` impls (they hide units at call sites).
  - [x] Newtypes `MeterId`, `Serial`, `TopicPath` over a **private** `String` with `new(impl Into<String>) -> Self` and `as_str(&self) -> &str` (+ `Display`) — derives `Debug, Clone, PartialEq, Eq, Hash`. Private field is deliberate: Epic 2/5 add well-formedness validation inside `new()` without breaking any caller — do not "simplify" to a public field.
  - [x] Timestamp newtype `UtcMillis(pub i64)` (UTC epoch-milliseconds) — derives `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord`
  - [x] Subtraction for `UtcMillis`, exactly this shape: `impl Sub for UtcMillis { type Output = i64; }` returning **signed** milliseconds — Story 1.5 computes `age = http_date − value_date`, which can be negative (`age < 0 → STALE`)
  - [x] `pub struct Measurement { pub meter: MeterId, pub serial: Serial, pub power: Kw, pub energy: Kwh, pub value_date: UtcMillis, pub quality: Quality }` — exact field names from the epic spec; derives `Debug, Clone, PartialEq` (`Eq` is impossible: `Kw(f64)` — don't waste a compile cycle trying)
- [x] Task 4: Update `crates/smartme-bridge/src/domain/mod.rs` (AC: 1, 2)
  - [x] Declare `pub mod measurement; pub mod quality;` and re-export the full public surface at the module root — exactly: `Measurement`, `Kw`, `Kwh`, `MeterId`, `Serial`, `TopicPath`, `UtcMillis`, `Quality` (downstream stories name `domain::Quality` etc. directly)
  - [x] Update the module doc (currently an Epic-0 scaffold saying "implemented from Epic 1 onward") while keeping the PURE-invariant note and the `tests/arch_purity.rs` reference
- [x] Task 5: Inline unit tests (`#[cfg(test)] mod tests` beside the code, per project pattern) (AC: 1, 3)
  - [x] Construction + accessor round-trip for the string newtypes (`new`/`as_str`/`Display`)
  - [x] `UtcMillis` subtraction: positive, zero, and **negative** result
  - [x] `Quality` variant equality; assert `domain::Quality` and `sparkplug_b::Quality` are the same type (e.g. a function taking `sparkplug_b::Quality` fed a `domain::Quality` value — compiles only if identical)
  - [x] A ```compile_fail``` doctest on `Kw` (or `Measurement`) showing a bare `f64` rejected where `Kw` is expected — makes AC 3 mechanically checkable
- [x] Task 6: Verify all gates green (AC: 1, 2, 3)
  - [x] `cargo fmt --all --check` (matches CI verbatim)
  - [x] `cargo clippy --workspace --all-targets -- -D warnings`
  - [x] `cargo test --workspace` — every pre-existing test bin must stay green: `arch_purity` (now actively scanning `domain/`), `prop_persist_atomic` (bridge), `no_context_leak` (sparkplug-b), `fixtures_shape` (smart-me-client)

### Review Findings

Adversarial review 2026-07-25 (layers: Blind Hunter, Edge Case Hunter, Acceptance Auditor). All 3 ACs pass; findings below are refinements, not AC violations.

- [x] [Review][Patch] Doc misquotes Story 1.10: says the channel message is "keyed" `(MeterId, Measurement, Quality)` but `Measurement` can never be `Eq + Hash` (contains `f64`); epic spec says "carries" [crates/smartme-bridge/src/domain/measurement.rs:78-79]
- [x] [Review][Patch] No NaN/non-finite policy stated on `Kw`/`Kwh`: `Kw(f64::NAN)` breaks `PartialEq` reflexivity (`m.clone() != m`), silently defeating any future equality-based dedup — document that finiteness is enforced at the source-adapter boundary (Story 1.7, fail-closed) [crates/smartme-bridge/src/domain/measurement.rs:14-31]
- [x] [Review][Patch] `compile_fail` doctest passes on ANY compile error (e.g. a broken path), not just the intended type mismatch — pin the error code (`compile_fail,E0308`) [crates/smartme-bridge/src/domain/measurement.rs:21]
- [x] [Review][Patch] `UtcMillis::sub` uses unchecked `self.0 - rhs.0`: an adversarial/garbage timestamp near `i64::MIN`/`MAX` from the external API panics in debug and wraps in release — a wrap flips the sign, so `age < 0 → STALE` could misclassify garbage as fresh; use `saturating_sub` (keeps `Output = i64` per spec) [crates/smartme-bridge/src/domain/measurement.rs:46]
- [x] [Review][Patch] Macro doc overpromises "Epic 2/5 add validation without breaking any caller" — `new() -> Result` IS a signature break; reword to what the private field actually guarantees (all construction funnels through `new()`, so validation lands in one place) [crates/smartme-bridge/src/domain/measurement.rs:57-59]
- [x] [Review][Defer] String-key semantics before Epic 2 validation: `Serial::new("")` keys collide and `Eq`/`Hash` are case-sensitive (API vs config casing drift = two devices) while 1.9/1.10 already key maps on these types — deferred, validation scoped to Epic 2/5 per spec; revisit at Story 1.9 [crates/smartme-bridge/src/domain/measurement.rs:50-75]
- [x] [Review][Defer] `TopicPath` accepts strings invalid as MQTT publish topics (empty, `+`/`#` wildcards, interior NUL, leading/trailing `/`) — fails only at the broker, far from construction — deferred, well-formedness is Epic 2/5 per spec [crates/smartme-bridge/src/domain/measurement.rs:90-94]
- [x] [Review][Defer] Range policy unstated for physical/timestamp values: negative/`±inf` `Kwh` (a cumulative counter), pre-1970 `UtcMillis`, and the eventual `i64→u64` Sparkplug wire conversion inherit an unguarded boundary — deferred, Epic 2 oracles / Story 1.8 encode boundary [crates/smartme-bridge/src/domain/measurement.rs:26-37]

## Dev Notes

### Decision: where `Quality` lives (read before coding)

The architecture is explicit: *"Quality is one enum, one definition: `Quality { Good, Stale, Bad }` in `sparkplug-b`. The bridge's error taxonomy maps into it; no ad-hoc quality strings anywhere."* [Source: _bmad-output/planning-artifacts/architecture.md#Naming Patterns]. `sparkplug-b` has no `model.rs` yet (Story 1.8 owns seq/lifecycle/encode). Therefore this story creates the **minimal** `model.rs` holding only `Quality`, and `domain/quality.rs` **re-exports** it. Do NOT define a second enum in the bridge with a mapping — two enums "kept aligned" is exactly the drift the architecture forbids. Story 1.8 will extend `model.rs` (Metric etc.) and must not move or rename `Quality`.

- `arch_purity` bans only `tokio`/`rumqttc`/`axum`/`reqwest` in `domain`/`core`; importing `sparkplug_b::Quality` is legal and intended (`sparkplug-b` is itself a pure lib, prost-only).
- The `Measurement`→Sparkplug **metric mapping** is a different thing and stays out of scope: it lives only in `adapters/sparkplug_publisher.rs` (Story 1.9), enforced by the second `arch_purity` test.

### Decision: `value_date` type = `UtcMillis(i64)`, no chrono

Do NOT add `chrono`/`time`/`jiff`. AR15 fixes the payload form: *"All timestamps UTC ISO-8601 on the wire, `i64` epoch-millis in Sparkplug payloads; never local time"* [Source: _bmad-output/planning-artifacts/epics.md#Additional Requirements AR15]. An `i64` epoch-millis newtype matches the Sparkplug encoding directly, keeps the dependency tree minimal (`cargo-deny` gate), and gives Story 1.5 the signed subtraction it needs for the `age < 0 → STALE` guard. ISO-8601 rendering is a wire/UI concern for later epics, not domain.

### Downstream consumers (design for these, don't build them)

- Story 1.3's `Clock` trait exposes "a monotonic instant and a wall-clock time" — `UtcMillis` is the intended wall-clock representation for that trait; 1.3 must NOT mint a second timestamp type.
- Story 1.4 `Source` yields `{ value, value_date, http_date }` — `http_date` is NOT a `Measurement` field; it travels in the Story 1.5 `tick` struct. Don't add it to `Measurement`.
- Story 1.5 computes `freshness = http_date − value_date` → hence `UtcMillis` signed subtraction.
- Story 1.7 (`SmartMeCloudSource`) is the ONLY place units are converted (fail-closed on unknown units). Domain types carry canonical units only — no conversion logic, no `From<f64>` sugar that hides units.
- Story 1.9 keys the Sparkplug device by `Serial` and Story 1.10's channel message carries `(MeterId, Measurement, Quality)` — hence `Eq + Hash` on the identifier newtypes.

### Constraints & anti-patterns (from architecture, enforced in CI)

- NO `Default` impls — a defaulted `Measurement` or `Quality` is a substituted value, the exact lie the project exists to prevent ("never a substituted value"; cold-start honesty is Story 1.5/1.9 logic).
- NO `serde` derives on domain types — nothing persists `Measurement`; `persist.rs` is generic and serves `bdSeq`/config only. Add later only when a real consumer appears.
- NO validation/bounds/monotonicity logic — those are the Epic 2 oracles. `Serial::new("")` succeeding is fine for now; the private-field + `new()` shape keeps adding validation in Epic 2/5 non-breaking.
- NO `unwrap()`/`expect()`/`panic!` outside `#[cfg(test)]`.
- Naming: types `CamelCase`, fields/modules `snake_case` (rustfmt/clippy `-D warnings` enforce).

### Existing files being modified — current state

- `crates/smartme-bridge/src/domain/mod.rs` — doc-comment-only scaffold (Story 0.6); says the domain types "are implemented from Epic 1 onward". Replace that sentence with real module decls; PRESERVE the note that the module is PURE and referenced by `tests/arch_purity.rs`.
- `crates/sparkplug-b/src/lib.rs` — `#![forbid(unsafe_code)]` + `pub mod protobuf` (prost include). Only ADD the model module + re-export; keep the crate doc's provenance style, and remember every word of rustdoc here is crates.io-audience.
- `crates/smartme-bridge/src/core/mod.rs` — NOT touched by this story (Clock/Source/state machine are 1.3–1.5).

### Testing standards

Unit tests inline (`#[cfg(test)] mod tests`) beside the code; integration tests live in `tests/` only when they need the crate boundary [Source: _bmad-output/planning-artifacts/architecture.md#Structure Patterns]. AC 3 is a compile-time guarantee — no runtime test can prove it; the private-field string newtypes + typed `Measurement` fields ARE the proof. The two guard test bins (`arch_purity.rs`, `no_context_leak.rs`) were written in Epic 0 to auto-activate on these files: run them, don't modify them.

### Build & workflow conventions

- `.cargo/config.toml` caps builds at `jobs = 2` and links via mold/clang — do not override, do not pass `-j`.
- Verification sequence: `cargo fmt --all --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace`.
- Epic 0 commit style: imperative subject + story provenance in module docs (see `git log` `7d27d3d`). Module doc comments explain the invariant they serve, not the code below them.

### Project Structure Notes

- Target tree [Source: _bmad-output/planning-artifacts/architecture.md#Complete Project Directory Structure]: `domain/ {mod, measurement (Kw/Kwh/Serial/MeterId/TopicPath), quality}.rs` — exactly the three files this story creates/updates in the bridge, plus `sparkplug-b/src/model.rs`.
- Dependency direction stays one-way: `smartme-bridge` depends on `sparkplug-b` (existing path dep); nothing depends on the bridge. Nothing new in any `Cargo.toml` (zero new external dependencies in this story).
- Story 1.1 (the `ValueDate`/`Date`-header audit spike, issue #1) is blocked on a real API capture and is NOT a prerequisite here — 1.2 needs no network, no credentials, and no fixture data.

### References

- Story spec (verbatim ACs): _bmad-output/planning-artifacts/epics.md#Story 1.2
- Strong domain typing rule (AR14) & time discipline (AR15): _bmad-output/planning-artifacts/epics.md#Additional Requirements
- Single `Quality` definition + naming/format patterns: _bmad-output/planning-artifacts/architecture.md#Implementation Patterns & Consistency Rules
- Module organization & boundaries: _bmad-output/planning-artifacts/architecture.md#Project Structure & Boundaries
- Purity enforcement mechanics: crates/smartme-bridge/tests/arch_purity.rs
- crates.io purity guard: crates/sparkplug-b/tests/no_context_leak.rs
- FR coverage: FR7 (unit carried with the value) — thin slice; FR45/FR8 land in 1.7/1.8

## Dev Agent Record

### Agent Model Used

claude-fable-5 (Claude Code)

### Debug Log References

- Red phase: `cargo check -p smartme-bridge` failed as expected with E0432 `no Quality in the root` (domain/quality.rs re-export written before sparkplug-b::Quality existed).
- Green phase: all gates pass — `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` (zero warnings), `cargo test --workspace` (15 tests across 6 bins + 1 compile_fail doctest, 0 failures).

### Completion Notes List

- `Quality { Good, Stale, Bad }` defined once in `sparkplug-b/src/model.rs` (derives Debug/Clone/Copy/PartialEq/Eq, no Default); rustdoc is generic Sparkplug prose — `no_context_leak` green. `domain/quality.rs` is a pure re-export (`pub use sparkplug_b::Quality;`), so AC 1's "single definition aligned" holds by construction, and a unit test proves type identity (a `domain::Quality` value fed to a function taking `sparkplug_b::Quality`).
- `measurement.rs`: `Kw(pub f64)` / `Kwh(pub f64)` (no `From<f64>`), private-`String` newtypes `MeterId`/`Serial`/`TopicPath` via a local `string_newtype!` macro (`new`/`as_str`/`Display`, derives incl. `Eq + Hash`), `UtcMillis(pub i64)` with `impl Sub → i64` (signed, for Story 1.5's `age < 0 → STALE`), and `Measurement` with the exact epic field names (derives Debug/Clone/PartialEq — no `Eq`, `Kw` wraps `f64`).
- AC 3 is enforced at compile time: private fields on string newtypes force `new()`, typed `Measurement` fields reject bare `f64`/`String`, and a `compile_fail` doctest on `Kw` makes it mechanically checkable (runs green in `cargo test`).
- `domain/mod.rs` re-exports the full public surface (`Measurement`, `Kw`, `Kwh`, `MeterId`, `Serial`, `TopicPath`, `UtcMillis`, `Quality`); module doc updated, PURE-invariant note and `arch_purity` reference preserved. `arch_purity` now actively scans the new files — green.
- Zero new dependencies; no Cargo.toml touched. No serde, no Default, no validation logic, no unwrap/expect/panic outside `#[cfg(test)]` — per architecture constraints.

### File List

- crates/sparkplug-b/src/model.rs (new)
- crates/sparkplug-b/src/lib.rs (modified — `pub mod model;` + `pub use model::Quality;`)
- crates/smartme-bridge/src/domain/quality.rs (new)
- crates/smartme-bridge/src/domain/measurement.rs (new)
- crates/smartme-bridge/src/domain/mod.rs (modified — module decls + re-exports, doc updated)

## Change Log

- 2026-07-25: Story 1.2 implemented — domain newtypes (`Kw`, `Kwh`, `MeterId`, `Serial`, `TopicPath`, `UtcMillis`), canonical `Measurement`, single `Quality` definition in `sparkplug-b` re-exported by the bridge. 5 unit tests + 1 compile_fail doctest added; all CI gates green. Status → review.
- 2026-07-25: Adversarial code review (Blind Hunter / Edge Case Hunter / Acceptance Auditor): all 3 ACs pass. 5 patches applied (doc fixes on 1.10 wording + NaN policy + macro promise, `compile_fail,E0308` pin, `UtcMillis::sub` → `saturating_sub` + saturation tests); 3 items deferred to Epic 2/5 (see `deferred-work.md`); 8 findings dismissed as noise/spec-mandated. All gates re-run green. Status → done.
