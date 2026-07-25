# Story 1.8: `sparkplug-b` — encode + `seq`/`bdSeq` + NBIRTH/NDATA/NDEATH

Status: done

Tracked as GitHub issue [#10](https://github.com/guycorbaz/smartme_mqtt/issues/10) (label `epic-1`).
Autonomous sprint run 2026-07-25 (sprint-1-decisions.md D1).

## Acceptance Criteria

1. Payload from name/value/timestamp/Quality with `seq` (0–255 wrapping) and per-session
   `bdSeq`; builders for NBIRTH, NDATA, NDEATH (and the LWT payload). — **PASS**
2. Cumulative energy encoded as `Double`, never float32 (FR45). — **PASS** (stronger than the
   AC: there is no `Float(f32)` variant at all — the mistake is unrepresentable).
3. NBIRTH self-describing: metric names + engineering-unit properties (FR18). — **PASS**
4. `tests/prop_seq_bdseq.rs` asserts `seq` wrap 255→0 and `bdSeq` continuity across
   sessions. — **PASS** (exhaustive over all 256 values, not sampled; plus a session-level
   wrap test asserting the published payloads, which is the AC's own wording).

## Design

- `datatype.rs`: `DataType` with the spec's numbering as the actual discriminants (`code()` is
  a cast, not a drift-prone lookup table).
- `model.rs`: `Quality` gains its **wire mapping** (`code()` → 192/500/0, property key
  `Quality`) — the earlier Story 1.2 review flagged "a wire-format crate defining a wire
  concept with no wire representation is half a type". `MetricValue` (Int64/UInt64/Double/
  Boolean/String/**Null(DataType)**), `Metric` with `with_quality` / `with_engineering_unit`.
- `seq.rs`: `SeqCounter` (wrapping 255→0) and `BdSeq` (per-session, wrapping, `Copy` plain
  data the caller persists — the crate does no I/O).
- `encode.rs`: **type-state session** — `NodeSession` (will + birth) → `birth()` consumes it →
  `LiveSession` (data + rebirth + death). Encoding via prost.

### Review Findings

Adversarial review 2026-07-25 (Blind Hunter with Sparkplug spec expertise + combined
Edge/Auditor). AC1–AC4 PASS. The blind reviewer independently CONFIRMED the load-bearing
choices (NDEATH omits `seq`; NBIRTH `seq=0`; `bdSeq` wraps at 255 transported as Int64;
`Int64` two's-complement into the unsigned `long_value` field; `Quality` property type 3 and
key `Quality`; `engUnit` key; `Bytes = 17`). Applied:

- [x] [Review][Patch] **`resume()` replayed a `bdSeq`** — a stale will would be delivered
  against the live session (node marked dead while publishing). Rated CRITICAL by the blind
  reviewer; it also violated the very property `prop_bdseq_survives_a_restart...` exists to
  protect. Removed: `start()` is now the only constructor and always advances [encode.rs]
- [x] [Review][Patch] **`data()` before `birth()` emitted `seq = 0`** — indistinguishable from
  a BIRTH on the wire. Made unrepresentable via the type-state split (`NodeSession` →
  `LiveSession`), so the module's "a caller cannot get them wrong" claim is now true
  [encode.rs]
- [x] [Review][Patch] `is_null` never set: a `Bad`-quality metric had to fabricate a value →
  `MetricValue::Null(DataType)` publishes no value while still declaring the tag's type. This
  also answers the Story 1.7 deferral about the hostile `0.0` carrier [model.rs, encode.rs]
- [x] [Review][Patch] Ambiguous intra-doc link `[crate::encode]` (module vs re-exported fn) —
  a named constraint failure that would have shipped to docs.rs → `[mod@crate::encode]`
- [x] [Review][Patch] `BdSeq::first()` was an off-by-one trap (the first session publishes 1,
  not 0) → renamed `before_first()` with the semantics spelled out [seq.rs]
- [x] [Review][Patch] `next_seq()` read like an allocator next to `take()` → `peek_seq()`
- [x] [Review][Patch] `Quality::code()` returned `i32` then cast to `u32` unguarded → returns
  `u32` (all codes are non-negative by construction) [model.rs]
- [x] [Review][Patch] Conformance scope over-claimed by omission → `lib.rs` now names what is
  NOT implemented (`Node Control/Rebirth` + the command path, host STATE, device-level
  messages, aliases, templates, topic construction) and states that `prost` is a PUBLIC
  dependency
- [x] [Review][Patch] Missing `#[must_use]` on payload builders (silently dropping a payload
  you were supposed to publish is a live failure mode)
- [x] [Review][Patch] Untested edges → tests added for negative `Int64` round-trip
  (`-1`/`i64::MIN`/`i64::MAX`), non-finite doubles (bit-exact passthrough, with the NaN-vs-
  `PartialEq` trap documented), null metrics, and the session-level 255→0 wrap
- [x] [Review][Defer] `Node Control/Rebirth` + NCMD decode, host STATE handling, device-level
  D\* messages, aliases: a conformant node needs them; Epic 3 owns the command path. Declaring
  a capability the crate cannot act on would be its own lie — hence documented, not stubbed
  [deferred-work.md]
- [x] [Review][Defer] BIRTH-declares/DATA-validates metric registry; empty metric name and
  empty-BIRTH guards; `is_historical` for replayed buffered data [deferred-work.md]
- Dismissed: bdSeq-increments-at-CONNECT-not-construction (Story 1.12 owns the boot order and
  constructs the session per connection attempt — the crate cannot see CONNECT events);
  `Float`/`Int32` variants (deliberate — widening a counter is the lie FR45 forbids; more
  variants land when a caller needs them); `encode_to(&mut BufMut)` micro-optimisation (4
  meters, one publish per poll).

## File List

- crates/sparkplug-b/src/{datatype,model,seq,encode,lib}.rs (new/rewritten)
- crates/sparkplug-b/tests/prop_seq_bdseq.rs (new — 8 exhaustive property tests)

## Change Log

- 2026-07-25: Implemented; adversarial review (9 patch groups incl. two structural fixes —
  `resume()` removal and the type-state lifecycle — plus 2 deferral groups); 23 unit + 8
  property + 2 doctests + context-leak guard green; cargo-deny green. Status → done.
