# Story 1.7: `SmartMeCloudSource` adapter + fail-closed unit conversion

Status: done

Tracked as GitHub issue [#9](https://github.com/guycorbaz/smartme_mqtt/issues/9) (label `epic-1`).
Autonomous sprint run 2026-07-25 (sprint-1-decisions.md D1).

## Acceptance Criteria

1. Power → `Kw` and energy → `Kwh` converted in exactly this adapter (nowhere else). — **PASS**
   (auditor verified workspace-wide: no other conversion site; the client crate explicitly
   documents "converted fail-closed in the bridge adapter — NEVER here").
2. Unknown/mismatched unit → fail closed: `Quality::Bad`, no guessed value (thin FR8). — **PASS**

## Design

- `adapters/smartme_source.rs` implements the async `Source` seam over `SmartMeClient`.
- Unit conversion is the adapter's exclusive job: exact-match `kW|W|MW` / `kWh|Wh|MWh`,
  everything else (including a `"KW"` casing drift) → `Quality::Bad`. Finiteness checked
  **before and after** the arithmetic (a finite input can overflow the ×1000 of a mega unit).
- The `Bad` carrier value is a documented non-value (`0.0`) — deliberately not plausible.
- Error mapping delegates to the client's own `is_fatal()` (single classification point, D7).
- Token lifecycle (ADR 0009): anchored on the **monotonic** clock (a lifetime is a duration —
  an NTP step must not stretch a dead token's apparent validity), margin 30 s, usable lifetime
  floored at 5 s so a short-lived token cannot turn every poll into an OAuth exchange. Exactly
  one refresh+retry on a 401 that hits a **previously-minted** token; a 401 on a token minted in
  the same call is the real thing and surfaces as `Fatal` immediately.

### Review Findings

Adversarial review 2026-07-25 (Blind Hunter + combined Edge/Auditor). AC1/AC2 PASS. Applied:

- [x] [Review][Patch] **`Bad` was silently downgraded to `Stale`** — an unparseable `ValueDate`
  pins `value_date` to the epoch, so the age guard fired before the quality match and published
  "old value" instead of "do not use this value". `judge_reading` now judges `Bad` FIRST; table
  row + 2 tests added [core/state_machine.rs]
- [x] [Review][Patch] MW/MWh conversion could overflow a finite input to infinity and publish it
  `Good` → finiteness re-checked after the arithmetic + test [adapters/smartme_source.rs]
- [x] [Review][Patch] `had_token` meant "credentials mode", not "previously-valid token" — a 401
  on a just-minted token earned a pointless second exchange + fetch (and delayed the `Fatal` by
  two round-trips every poll) → `ensure_token` now reports whether it minted, retry gated on
  `reusing_token` [adapters/smartme_source.rs]
- [x] [Review][Patch] Token expiry anchored on the WALL clock (NTP step backward extended a dead
  token's validity) → anchored on `MonotonicMs` [adapters/smartme_source.rs]
- [x] [Review][Patch] `expires_in ≤ 30 s` made `expires_at = now` → a full OAuth exchange on every
  poll, hammering the token endpoint → `TOKEN_MIN_LIFETIME_MS` floor [adapters/smartme_source.rs]
- [x] [Review][Defer] Token-lifecycle tests (expiry/refresh-retry) need an HTTP stub the workspace
  deliberately lacks; the pure mapping is fully covered. Land with the 1.11 task tests or an
  injectable client seam [deferred-work.md]
- [x] [Review][Defer] `map_device` collapses three failure modes into one `Bad` with no forensics
  (which field, which raw string) → tracing lands in Epic 2's diagnostics [deferred-work.md]
- [x] [Review][Defer] `Bad` carrier `0.0` on a cumulative counter is hostile to a
  quality-ignoring consumer (a delta-based historian sees a crash-to-zero) — revisit against the
  real Ignition behaviour in Story 1.15 / Epic 2 [deferred-work.md]
- Dismissed: retry-latency vs the poll wrapper timeout (the retry is now the exception, not the
  rule — and 1.11 owns the budget); meter-mismatch `Fatal` latching `Failed` (a wiring bug SHOULD
  demand a restart — ADR 0009 "stop + surface"); `AuthRejected` always fatal (ADR 0009 decision,
  not this story's to relitigate); serial/`http_date` validation (Epic 2 well-formedness).

## File List

- crates/smartme-bridge/src/adapters/{mod,smartme_source}.rs (new)
- crates/smartme-bridge/src/lib.rs (module added)
- crates/smartme-bridge/src/core/state_machine.rs (modified — Bad judged before the age guards)
- crates/smart-me-client/src/client.rs (modified — `uses_client_credentials()`)
- crates/smartme-bridge/Cargo.toml (dev-dep serde_json)

## Change Log

- 2026-07-25: Implemented; adversarial review (5 patches incl. a real Bad→Stale mislabelling bug,
  3 deferrals); 41 bridge unit tests green; all gates green. Status → done.
