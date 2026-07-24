# Fixtures — smart-me-client

**⚠️ SYNTHETIC PLACEHOLDERS — NOT the contract-of-record.**

`smartme_sample.json` and `http_headers/*.txt` here are hand-written synthetic samples
created in Story 0.3 so parsing/oracle tests have a home before real data exists. Values
are obviously fake (round numbers, zero UUIDs, `SYNTHETIC-*` names).

The **real** captured payload + HTTP headers land in **Epic 1** (the `ValueDate`/`Date`-header
audit spike), which replaces `smartme_sample.json` with a real `GET /Devices/` capture and
fills the `http_headers/` slots with real RFC 7231 `Date` headers. Only that captured
fixture is the parsing contract-of-record.

## `http_headers/` slots (each paired against `ValueDate = 2026-07-24T12:00:00Z`)

| File | Purpose | Expected freshness verdict |
|------|---------|----------------------------|
| `valid.txt` | well-formed `Date`, +5 s after ValueDate | fresh (age ≈ 5 s) |
| `absent.txt` | no `Date` header | STALE (no oracle input) |
| `malformed.txt` | unparseable `Date` | STALE |
| `negative_skew.txt` | `Date` before `ValueDate` | STALE (age < 0) |
| `huge_skew.txt` | `Date` a year ahead | STALE (implausible age) |
