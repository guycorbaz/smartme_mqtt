# Stories 1.9 & 1.10: `SparkplugPublisher` + `core/channel.rs`

Status: done

Tracked as GitHub issues [#11](https://github.com/guycorbaz/smartme_mqtt/issues/11) (1.9) and
[#12](https://github.com/guycorbaz/smartme_mqtt/issues/12) (1.10), label `epic-1`.
Autonomous sprint run 2026-07-25 (sprint-1-decisions.md D1). Delivered together: the channel
message is the publisher's input, so splitting them would have meant writing one against a
placeholder.

## Acceptance Criteria

**1.9** — (a) mapping confined to `sparkplug_publisher.rs`, proven by `arch_purity` — **PASS**
(weak proof, see deferred); (b) Good reading → kW/kWh units, device keyed by Serial, payload
timestamp == source ValueDate — **PASS**; (c) cold start → first BIRTH metrics STALE, never
GOOD-by-default — **PASS** (see deviation below); (d) Stale verdict → nothing fresh-looking
emitted, sink assertion holds — **PASS**.

**1.10** — pure message carrying `(MeterId, Measurement, Quality)`, no tokio/transport — **PASS**.
The second half ("both tasks reference it, neither redefines it") **cannot close until Stories
1.11/1.12 exist** — logged, and it needs its own purity clause then.

### Recorded deviation on AC 1.9(c)

Under decision D8 the meter tag set moved to DEVICE births. So the metrics the AC talks about
are the **DBIRTH** metrics (Null value + `Quality::Stale` — verified), not the NBIRTH's. The node
BIRTH now declares `Contract/Version`, which is `Good` on purpose: it is a compile-time fact
about the running software, not a measurement, and marking it Stale would be a lie in the other
direction. Recorded here rather than silently reinterpreted.

## Design

- `core/channel.rs`: `MeterUpdate { meter, measurement, published }`. The two qualities stay
  distinct on purpose — `measurement.quality` is what the SOURCE could tell us, `published` is
  the ORACLE's verdict. Collapsing them would discard what the state machine exists to compute.
- `adapters/sparkplug_publisher.rs`: concrete publisher (no `Publisher` trait, per architecture)
  over an injectable `Sink`; validated topics; last-known-reading memory per device so a rebirth
  re-declares what is known; typed `Published` outcome so a drop is traceable.

### Review Findings

Combined adversarial/edge/acceptance review 2026-07-25 — verdict "CONDITIONAL PASS", all
conditions now met. Applied:

- [x] [Review][Patch] **H1/H2 partial emission**: an illegal serial emitted the NBIRTH and then
  failed, leaving a live node with a truncated device set (and the test asserted only the error,
  hiding it). All topics are validated before the first emission; on error nothing is sent and
  the session is untouched — asserted, including a clean retry afterwards.
- [x] [Review][Patch] **H4 rebirth blanked known values**: every reconnect reset every tag to
  Null/Stale even when the bridge held a fresh reading (a data gap on every transport blip) →
  per-device last-reading memory, re-declared at rebirth with its own quality.
- [x] [Review][Patch] **M2 silent drop** contradicted "a per-device traced drop, never silence" →
  `Published { Emitted, DroppedBeforeBirth, DroppedUndeclaredDevice }`, `#[must_use]`.
- [x] [Review][Patch] **M1 dead `CONTRACT_VERSION`** — the constant justifying D8 never reached
  the wire → published as `Contract/Version` in the NBIRTH.
- [x] [Review][Patch] **M3** `mem::replace` with a valid-LOOKING throwaway session (bd_seq 1)
  could survive a panicking sink and collide with a published number → explicit `Session::Moving`
  transient, no plausible placeholder.
- [x] [Review][Patch] **M5** topic-level guard was a `debug_assert!` (absent in release, where it
  matters) and `device_topic` had none → `TopicError::WrongLevel`, enforced in every profile.
- [x] [Review][Patch] **M6** DDATA was publishable for a device no BIRTH declared → declared-set
  tracking + typed drop.
- [x] [Review][Patch] **M7** `is_publishable_as_live` encoded a liveness policy that diverged from
  the publisher's and had zero production callers → removed (one policy, in the publisher).
- [x] [Review][Patch] **M8/M10** documented WHY `Bad` nulls the value while `Stale` publishes it
  (a stale reading is true history, kept honest by its own ValueDate timestamp); tested the
  `will()` Live branch.
- Deferred (see deferred-work.md): the arch_purity text-proxy weakness (H3), 1.10's second AC,
  report-by-exception, publisher-side plausibility floor, `Sink` failure channel.

## File List

- crates/smartme-bridge/src/core/channel.rs (new), src/adapters/sparkplug_publisher.rs (new)
- crates/sparkplug-b/src/topic.rs (new), src/encode.rs (device_* + decode), src/lib.rs
- module wiring in core/mod.rs, adapters/mod.rs

## Change Log

- 2026-07-25: Implemented after party-mode D8 (device-level now); combined review, 9 patch groups
  applied incl. 4 structural; 131 workspace tests green. Status → done.
