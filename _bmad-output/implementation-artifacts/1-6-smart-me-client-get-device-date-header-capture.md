# Story 1.6: `smart-me-client` — GET device + `Date`-header capture

Status: done

Tracked as GitHub issue [#8](https://github.com/guycorbaz/smartme_mqtt/issues/8) (label `epic-1`).
Autonomous sprint run 2026-07-25 (sprint-1-decisions.md D1).

## Acceptance Criteria (with recorded deviations)

1. Auth (ADR 0009 supersedes the epic's "API key"): OAuth2 client-credentials primary, Basic
   fallback; `GET /Devices/{id}` over TLS deserializes the 8 audited fields AND captures the
   response `Date` header. — **PASS**
2. Non-TLS endpoint / TLS failure → hard-fail, no plaintext fallback (NFR13). — **PASS**
   (double enforcement: full URL validation at construction + reqwest `https_only`; rustls-only
   tree, no native-tls.)
3. Fixture contract-of-record via HTTP mock — **deviation D7**: no plaintext mock (the client
   cannot speak plaintext by construction); the contract tests parse the REAL fixture bytes
   through the exact same serde type parameters and header parser `get_device` uses, including
   the single-object `/Devices/{id}` envelope (`smartme_device_by_id.json`, generated from the
   real Story 1.1 capture after review flagged the list-vs-object gap).

## Design (decision D7)

- New workspace deps (architecture-locked stack): reqwest 0.13 (`rustls`, `webpki-roots`,
  `http2`, `json`, `form` — feature names changed in 0.13), thiserror 2, serde. deny gate:
  `CDLA-Permissive-2.0` allowed (Mozilla CCADB root-store DATA in webpki-root-certs).
- Client clock-free: `TokenState { access_token, expires_in_s }` is plain data; the caller
  anchors expiry with its injected `Clock`.
- `SmartMeError` owns its variants (no leaked reqwest types); **`is_fatal()` defined in-crate**
  so the bridge's transient/fatal mapping cannot drift. Fatal = NotHttps, Misconfigured,
  AuthRejected. Token-exchange 400 is fatal ONLY when the RFC 6749 error body blames the
  client/grant — a bare 400 (WAF artifact) stays transient and cannot latch the 1.5 absorbing
  `Failed`.
- Strict hand-rolled parsers (zero date-crate deps): `parse_value_date` (ISO-8601-Z,
  fixed-width digit tokens, calendar-valid, fraction truncated to ms) and `parse_imf_fixdate`
  (single-SP grammar, weekday-consistency check, calendar-valid, GMT literal).

### Review Findings

Adversarial review 2026-07-25 — the richest haul of the sprint; auditor verdict: AC1/AC2 PASS,
AC3 "fail as written" → **fixed by patch**. Applied:

- [x] [Review][Patch] `derive(Debug)` printed secrets (Credentials/TokenState/SmartMeClient) — NFR12 violation flagged by all 3 layers → manual redacting Debug impls + leak test [client.rs]
- [x] [Review][Patch] Default redirect policy could replay the client-secret form cross-host → `redirect::Policy::none()`, 3xx = transient status error [client.rs]
- [x] [Review][Patch] AC3 envelope gap: contract test parsed the LIST fixture while `get_device` parses a single object → real `/Devices/{id}` body added as `smartme_device_by_id.json` + test with the exact type parameter [tests/contract_of_record.rs]
- [x] [Review][Patch] `device_id` interpolated raw (path traversal / query smuggling / empty → collection endpoint) → strict `[0-9A-Za-z-]` validation [client.rs]
- [x] [Review][Patch] Base "validation" was a prefix check (userinfo smuggling, empty host, query/fragment) → full `Url` parse: https + host + no userinfo/query/fragment/path; sanitized error text [client.rs]
- [x] [Review][Patch] Token-exchange 400 blanket-fatal → RFC 6749 error-body discrimination [client.rs]
- [x] [Review][Patch] `AuthRejected{status:0}` sentinel for local misuse → distinct `Misconfigured` variant [client.rs]
- [x] [Review][Patch] No transient/fatal export (caller drift risk) → `is_fatal()` + test [client.rs]
- [x] [Review][Patch] Missing `Accept: application/json`; sub-second timeout unvalidated; `expires_in ≤ 0` accepted; `TokenState` Eq on secret material dropped [client.rs]
- [x] [Review][Patch] Parsers: signed/unpadded tokens accepted, Feb-30 silently normalized, weekday never cross-checked, double-space tolerated → fixed-width digit tokens, calendar validity, weekday consistency, single-SP grammar (both parsers + tests) [types.rs, http_date.rs]
- [x] [Review][Patch] Contract test `devices()[0]` ordering assumption → `find(name == "METER-A")`; fixture-coupling documented [tests/contract_of_record.rs]
- [x] [Review][Defer] One-shot token-refresh-then-retry on 401 after a previously-valid token (ADR 0009 "re-authenticate on 401") → the bridge adapter owns the loop (Story 1.7/1.11) [next story]
- [x] [Review][Defer] 429 `Retry-After` honoring → Epic 2 backoff (architecture: "rate-limit/429 backoff") [deferred-work.md]
- [x] [Review][Defer] Per-device tolerant list deserialization (one degraded device fails the whole array) → Epic 2 robustness [deferred-work.md]
- Dismissed: leap-second rejection (documented fail-closed: one spurious STALE per leap second beats a fabricated ms); `+00:00` offset support (the audited API emits `Z`; a format change must surface, not be silently absorbed); connect_timeout split, multiple-versions=deny (workspace policy Story 0.5).

## File List

- crates/smart-me-client/src/{lib,types,http_date,client}.rs (new/rewritten)
- crates/smart-me-client/tests/contract_of_record.rs (new)
- crates/smart-me-client/fixtures/smartme_device_by_id.json (new — real capture, anonymized)
- crates/smart-me-client/Cargo.toml, Cargo.toml (deps), deny.toml (CDLA), Cargo.lock

## Change Log

- 2026-07-25: Implemented; adversarial review (heaviest patch load of the sprint: 11 patch groups, 3 deferrals); all gates green incl. cargo-deny. Status → done.
