# ADR 0004 — Freshness formula: `age = Date-header − ValueDate` CONFIRMED

- **Status:** Accepted
- **Date:** 2026-07-25
- **Related:** Story 1.1 (audit spike, issue #1), Story 1.5 (state machine), Story 1.6
  (client capture), ADR 0009 (auth), architecture open item ①, epics.md AR3/AR5.

## Context

The staleness oracle plans to compute `age = HTTP Date-header − ValueDate` and flag
`quality = STALE` when the age is negative, implausible, or over threshold. This rested
on two unaudited assumptions: what `ValueDate` actually means (measurement vs poll vs
server time; UTC vs local/DST), and whether the HTTP `Date` header is reliably present
and parseable. The documented fallback was monotonic-`Instant` staleness only.

## Audit (real capture, 2026-07-25)

Captured with the Story 1.1 tooling against the author's account (OAuth2 client
credentials, `POST https://api.smart-me.com/oauth/token`, then `GET /Devices` and
`GET /Devices/{id}`; both returned HTTP 200). Anonymized capture stored as
`crates/smart-me-client/fixtures/smartme_sample.json` + `http_headers/valid.txt`.

Findings:

1. **`ValueDate` is the measurement timestamp, in UTC.** ISO-8601 with `Z` suffix and
   7-digit fractional seconds (`2026-07-25T13:06:32.0500519Z`). Against the response
   `Date: Sat, 25 Jul 2026 13:06:33 GMT`, the three live meters showed ages of
   **0.95 s, 43 s and 48 s** — consistent with per-meter last-report times, ruling out
   "poll time" and "server time" (those would be ~0 for every device).
2. **A dead meter keeps its last `ValueDate`.** One device last reported
   `2026-04-20T12:04:35Z` (age ≈ 96 days at capture): exactly the honest-STALE case the
   bridge exists for. Confirmed with the maintainer (2026-07-26) — that meter is genuinely
   unplugged, so this is a real stale device and not a cloud-side data anomaly. `ActivePower` reads `0.0` on that device — a substituted-looking
   value that only the age exposes as a lie.
3. **No DST/local-time ambiguity.** Both sides of the subtraction are UTC (`Z` suffix;
   `Date` is RFC 7231 IMF-fixdate, GMT). Midnight/DST transitions cannot corrupt the age.
4. **`Date` header present and well-formed**, served via Cloudflare over HTTP/2 —
   header names arrive **lowercase** (`date:`); parsing must be case-insensitive.
   Second-level precision only, so ages have ±1 s quantization — irrelevant against
   multi-minute staleness thresholds.
5. **Contract corrections vs the synthetic fixtures:** `DeviceEnergyType` is an integer
   enum (1 = electricity), not a string; `Serial` is a JSON number; `Id` is a UUID
   string; many additional fields exist (per-phase voltage/current, tariff counters,
   `CounterReadingImport`/`Export`, `FamilyType`).

## Decision

**Formula `age = Date − ValueDate` is CONFIRMED** as the primary freshness input for the
Story 1.5 state machine (`age < 0 → STALE`, `age > threshold → STALE`, plausibility
window per AR5). The monotonic-`Instant` fallback remains documented but is NOT needed
for the smart-me cloud API. The Story 1.5 verdicts are pinned by the five
`http_headers/` fixtures (fresh only for `valid`).

## Consequences

- Story 1.5 uses `http_date − value_date` (both `UtcMillis`, signed saturating
  subtraction — already in `domain/measurement.rs`).
- Story 1.6's client must capture the `date` header case-insensitively and parse
  RFC 7231 IMF-fixdate; absence or parse failure is an oracle input (`None`), never a
  substituted timestamp.
- Story 1.7's deserializer must treat `DeviceEnergyType` as an integer and tolerate
  unknown extra fields (the real payload is much wider than the six fields we consume).
- Issue #1 closes with this ADR as the recorded decision; ADR 0009 gains the concrete
  token endpoint URL (`https://api.smart-me.com/oauth/token`).
