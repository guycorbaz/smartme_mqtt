# Story 1.14: Chaos — STALE-on-DEATH + STALE-on-cloud-timeout

Status: done

Issue [#16](../../issues/16). Autonomous sprint run 2026-07-25.

## Acceptance Criteria

1. Bridge running against a testcontainers MQTT broker; process killed or link cut →
   **an independent subscriber sees the affected tags marked STALE via NDEATH**. — **PASS**
2. Broker up, smart-me cloud unreachable → metrics published with `quality = STALE`, **no frozen
   value shown fresh**. — **PASS**

## What the tests actually prove

Both assert from an **independent subscriber** — a separate MQTT client that shares nothing with
the bridge but the broker. That is the point: a bridge that lied to a SCADA host would lie to its
own logs just as convincingly, so the only trustworthy oracle is what a third party receives.

- `chaos_stale_on_death`: the bridge is **aborted**, not asked to stop — a power cut, not a
  shutdown. The broker then publishes the will it was handed at connect time. The test asserts
  the death's `bdSeq` **equals the birth's**, because a certificate a consumer cannot attribute
  to the session it belongs to is one it will ignore, leaving the frozen value on screen. It also
  asserts the death carries no sequence number.
- `chaos_stale_on_cloud_timeout`: the API base points at TEST-NET-1 (RFC 5737, guaranteed
  unroutable), so the fetch times out the way a silent cloud does. The node stays alive on the
  broker throughout. The test asserts the device BIRTH declares both tags `STALE` with **no
  value**, then sweeps every message published over several poll intervals and fails if any one
  of them claims `Good` or carries a usable value on a DDATA. Deterministic twin:
  `poll_publish::tests::a_silent_cloud_times_out_into_stale_instead_of_wedging`.

## Change Log

- 2026-07-25: Both chaos tests written and passing against a real Mosquitto container.
  146 workspace tests green; fmt, clippy, cargo-deny green.
