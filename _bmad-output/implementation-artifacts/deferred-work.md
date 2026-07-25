# Deferred Work

Items deferred from reviews; each carries its origin and where it should be picked up.

## Deferred from: code review of 1-2-domain-measurement-newtypes-quality (2026-07-25)

- String-key semantics before Epic 2 validation: `Serial::new("")` keys collide and `Eq`/`Hash` are case-sensitive (API vs config casing drift would create two logical devices) while Stories 1.9/1.10 already key maps on `Serial`/`MeterId`. Validation is scoped to Epic 2/5 per the story spec — revisit no later than Story 1.9.
- `TopicPath` accepts strings that are invalid as MQTT publish topics (empty string, `+`/`#` wildcards, interior NUL, leading/trailing `/`); failure surfaces only at the broker, far from the construction site. Well-formedness lands inside `new()` in Epic 2/5.
- Range policy unstated for physical/timestamp values: negative or `±inf` `Kwh` (a cumulative counter), pre-1970 `UtcMillis`, and the eventual `i64→u64` conversion at the Sparkplug encode boundary (wire timestamps are `uint64`). Epic 2 oracles / Story 1.8 own these guards.

## Deferred from: code review of 1-5-pure-fresh-stale-failed-state-machine (2026-07-25)

- Frozen/replayed-feed oracle: a byte-identical replayed response (`http_date` frozen together
  with `value_date`) keeps a plausible age and stays Fresh. Detection needs cross-tick state
  (`http_date` strictly advancing between accepted readings) — exactly the "per-serial state
  already carries the inputs" additive oracle the architecture defers to Epic 2. Same state also
  catches the future-dated coherent pair (both stamps shifted, small age).
- `Date`-header 1-second truncation tolerance: a genuinely fresh reading can compute age ≈ −900 ms
  and go spuriously STALE (flapping at sub-second phase alignment). Spec-literal `age < 0 → STALE`
  kept for Epic 1 (fail-safe: noise, never a lie); revisit the tolerance band (e.g. −2000 ms) with
  real polling data in Epic 2.
- `Policy::max_age_ms` validation (reject ≤ 0 at config load) — Epic 3 config oracle.

## Deferred from: code review of 1-6-smart-me-client (2026-07-25)

- One-shot token-refresh-then-retry on 401 after a previously-valid token (ADR 0009) — the
  bridge adapter owns that loop; land it in Story 1.7/1.11.
- 429 `Retry-After` honoring + rate-limit backoff — Epic 2 (architecture: "rate-limit/429
  backoff + token-refresh handling").
- Tolerant per-device list deserialization (today one degraded device fails the whole
  `GET /Devices` array parse; single-device path unaffected) — Epic 2 robustness.

## Deferred from: code review of 1-7-smartme-cloud-source (2026-07-25)

- Token-lifecycle tests (expiry boundary, 401 refresh-then-retry, double-401 → Fatal): need an
  injectable client seam or an HTTP stub the workspace deliberately lacks. Land alongside the
  Story 1.11 task tests.
- `map_device` diagnostics: the three failure modes (unknown unit / non-finite value /
  unparseable ValueDate) collapse into one undifferentiated `Bad` with no record of which field
  or which raw string failed. Epic 2 diagnostics/culprit classification owns this.
- `Bad` carrier value `0.0` on a cumulative kWh counter: a consumer that ignores the quality flag
  sees the counter crash to zero and snap back (a huge negative then positive delta). Revisit
  against real Ignition behaviour in Story 1.15 / Epic 2 — options: last-known-value carrier, or
  omit the metric entirely.
