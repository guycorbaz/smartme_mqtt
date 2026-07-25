# Story 1.1: Audit smart-me `ValueDate` & HTTP `Date`-header semantics

Status: done

Tracked as GitHub issue [#1](https://github.com/guycorbaz/smartme_mqtt/issues/1) (label `epic-1`).

## Story

As the maintainer,
I want the real smart-me timestamp semantics audited on a captured payload,
So that the freshness formula rests on fact, not assumption.

## Acceptance Criteria

1. **Given** valid smart-me credentials, **when** a real `GET /Devices/` (and `GET /Devices/{id}`) request is captured, **then** the real payload replaces the synthetic `crates/smart-me-client/fixtures/smartme_sample.json` **and** the real HTTP response headers replace the synthetic `fixtures/http_headers/*` (at least a real `valid` case).
2. **Given** the captured payload and headers, **when** `ValueDate` is audited and the `Date` header checked, **then** the finding is recorded in `docs/adr/0004-freshness-date-header.md` stating either "formula confirmed" or the monotonic fallback.
3. **Given** the audit outcome, **when** issue #1 is closed, **then** the decision it references is the ADR, and Story 1.5 builds on it.

## Dev Agent Record

### Agent Model Used

claude-fable-5 (Claude Code) — autonomous sprint run authorized by Guy 2026-07-25
(commit+push per story; execution with local `.env` credentials approved).

### Completion Notes List

- **Token endpoint discovered** via `https://api.smart-me.com/.well-known/openid-configuration`:
  `POST https://api.smart-me.com/oauth/token`, `client_credentials` supported. Real exchange OK
  (Bearer, `expires_in: 3600`); ADR 0009 updated with the concrete URL.
- **Capture** (2026-07-25T13:06:33Z): `GET /Devices` → 200, 4 devices; `GET /Devices/{id}` → 200,
  same shape. Capture tooling: scratchpad script (curl + python), token never logged, token
  response deleted after use.
- **Audit verdict — formula `age = Date − ValueDate` CONFIRMED** (ADR 0004): `ValueDate` is the
  UTC measurement timestamp (`Z` suffix, 7-digit fractions); live meters aged 0.95 s/43 s/48 s vs
  the `Date` header; one dead meter carried a 96-day-old `ValueDate` (natural honest-STALE case);
  no DST ambiguity (both sides UTC); `Date` present, RFC 7231, lowercase over HTTP/2.
- **Contract corrections** captured in fixtures README: `DeviceEnergyType` is an int enum (1),
  `Serial` a number, `Id` a UUID string; payload much wider than the consumed fields.
- **Fixtures replaced**: `smartme_sample.json` = real 4-device capture anonymized
  (`Id`/`Serial`/`Name` scrubbed, everything else verbatim); `http_headers/valid.txt` = real
  trimmed capture; the 4 synthetic variants re-issued in the real HTTP/2 lowercase format,
  re-paired against the real `ValueDate` (README table updated).
- `fixtures_shape` test bin stays green (shape keys unchanged by anonymization).

### Decisions taken (autonomous run)

- Anonymization scope: only identity fields (`Id`, `Serial`, `Name`) scrubbed — electrical values
  and timestamps are not identifying and their realism is the point of the capture. (Pre-approved
  by Guy: "fixtures anonymisées avant commit".)
- Kept all 4 devices (not just one): the 96-day-stale device is the best available STALE test
  vector; multi-device realism exercises Story 1.7's per-meter mapping.
- Trimmed Cloudflare telemetry headers (`report-to`/`nel`/`cf-ray`/`alt-svc`) from the committed
  `valid.txt`: they carry account/session-correlatable tokens and are irrelevant to the oracle.

## File List

- crates/smart-me-client/fixtures/smartme_sample.json (replaced — real anonymized capture)
- crates/smart-me-client/fixtures/http_headers/{valid,absent,malformed,negative_skew,huge_skew}.txt (replaced)
- crates/smart-me-client/fixtures/README.md (rewritten — contract-of-record)
- docs/adr/0004-freshness-date-header.md (new — audit verdict)
- docs/adr/0009-smartme-auth-client-credentials.md (updated — token endpoint resolved)

## Change Log

- 2026-07-25: Story 1.1 executed against the real API; formula confirmed; fixtures now
  contract-of-record; ADR 0004 written; issue #1 closed. Status → done.
