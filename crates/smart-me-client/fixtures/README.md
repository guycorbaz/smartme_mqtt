# Fixtures — smart-me-client

**✅ REAL CAPTURE — contract-of-record (Story 1.1, captured 2026-07-25T13:06:33Z).**

`smartme_sample.json` is a real `GET /Devices` response from `api.smart-me.com`
(4 devices), **anonymized**: `Id`/`Serial`/`Name` are scrubbed placeholders; every other
field (values, units, `ValueDate`, voltages, currents, tariff counters) is verbatim from
the wire. Notable contract facts vs the old synthetic guess:

- `DeviceEnergyType` is an **integer** enum (1 = electricity), NOT the string `"Electricity"`.
- `Serial` is a JSON **number**, `Id` a UUID **string**.
- `ValueDate` is ISO-8601 **UTC with `Z` suffix** and 7-digit fractional seconds
  (.NET ticks), e.g. `2026-07-25T13:06:32.0500519Z`. It is the **measurement**
  timestamp: live meters showed ages of 0.9–48 s vs the response `Date` header, and one
  meter that stopped reporting kept its last `ValueDate` (96 days old) — a natural STALE, and a
  CONFIRMED one: that device is genuinely unplugged, so the fixture carries a real stale reading
  rather than a data anomaly.
- The API is served over HTTP/2 (Cloudflare): header names arrive **lowercase**
  (`date:`), parsers must be case-insensitive. `Date` is RFC 7231 IMF-fixdate, GMT.

`http_headers/valid.txt` is the real (trimmed) response-header capture; the other four
slots are synthetic variants in the same real HTTP/2 format. See ADR 0004 for the audit.

## `http_headers/` slots (each paired against `ValueDate = 2026-07-25T13:06:32.0500519Z`, METER-A)

| File | Purpose | Expected freshness verdict |
|------|---------|----------------------------|
| `valid.txt` | real captured `date`, +0.95 s after ValueDate | fresh (age ≈ 1 s) |
| `absent.txt` | no `date` header | STALE (no oracle input) |
| `malformed.txt` | unparseable `date` | STALE |
| `negative_skew.txt` | `date` 1 h before ValueDate | STALE (age < 0) |
| `huge_skew.txt` | `date` a year ahead | STALE (implausible age) |
