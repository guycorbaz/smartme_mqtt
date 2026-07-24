# smartme_mqtt

A self-hosted **Rust bridge** that polls [smart-me](https://smart-me.com) cloud energy
meters and republishes instantaneous **power (kW)** and consumed **energy (kWh)** onto an
MQTT broker as **Sparkplug B**, where a SCADA/HMI ([Ignition](https://inductiveautomation.com/))
consumes them as tags.

It is a **personal reliability tool** — a compact binary for dependable 24/7 operation on
modest hardware (NAS / Raspberry Pi), deployed via `docker compose`, and configured,
previewed, and diagnosed through a built-in web UI. Open-sourced under **MIT**.

> ⚙️ **Status: early development.** The project is being built epic by epic. The Cargo
> workspace, CI gates, and shared primitives (Epic 0) are in place; the bridge is **not yet
> functional**. See [Project status](#project-status).

## The guiding principle — "never lies to the SCADA"

smart-me exposes meter data only through a **cloud REST API**, while supervision systems
speak **MQTT**. That gap is usually bridged with fragile glue (Node-RED, hand-rolled REST
sensors) that answers unit/freshness questions inconsistently. `smartme_mqtt` replaces that
with one dependable service whose single rule is: **never show the SCADA a false value
dressed as true.**

Concretely:

- **Correct, explicit units** — power exactly `kW`, energy exactly `kWh`; unknown or
  mismatched source units are **rejected, not guessed**.
- **Visible staleness (two mechanisms)** — when the bridge disconnects from the broker,
  Sparkplug **NDEATH (via LWT)** marks tags STALE natively; when the *cloud* fetch fails
  while the bridge is still alive on MQTT (the most likely failure), the bridge actively
  publishes **quality = STALE** rather than republishing a frozen value as fresh.
- **Serial-bound identity** — every published topic is bound to its meter's immutable
  serial number and verified, so a value is never attributed to the wrong meter.
- **Honest timestamps** — freshness is computed end-to-end from the meter's measurement
  time, never from the poll time.

## What it does (v1)

- Reads all meters of a single smart-me account over the cloud REST API, on a configurable
  poll interval, with bounded backoff and an explicit error taxonomy.
- Publishes power + energy as Sparkplug B (one EON node, one device per meter) with
  engineering units, source timestamps, and per-meter quality — consumed by Ignition's
  MQTT Engine.
- Serves a minimal web UI: configuration + live preview + diagnostics + a
  "state of the bridge" screen (dual source/sink health, culprit classification,
  human-readable timestamps).
- Runs behind [Traefik](https://traefik.io) via `docker compose`, connecting to an
  external MQTT broker; image-based updates with zero config loss.

Local Modbus, multi-account, and a historian are explicitly **out of scope** (Ignition owns
history; smart-me remains the metering-of-record — this data is informative-only).

## Architecture

A three-crate Cargo workspace with strict, mechanically-enforced boundaries:

| Crate | Role |
|-------|------|
| [`crates/sparkplug-b`](crates/sparkplug-b) | Pure, generic **Sparkplug B** library (protobuf via `prost`, `seq`/`bdSeq`/rebirth lifecycle). Zero application dependency — intended for crates.io. |
| [`crates/smart-me-client`](crates/smart-me-client) | Pure **smart-me REST client** (auth, endpoints, deserialization). Isolates `reqwest`/`serde`. |
| [`crates/smartme-bridge`](crates/smartme-bridge) | The **application**: canonical `Measurement`, the pure `Fresh\|Stale\|Failed` state machine, the 2-task async runtime, adapters, web UI, config, and wiring. |

The pure core (staleness/quality decisions) never imports async or transport crates —
**"no truth is ever decided inside an `async fn`"** — an invariant enforced at compile time
by `tests/arch_purity.rs`. Dependency direction is one-way (`sparkplug-b`, `smart-me-client`
→ `smartme-bridge`), enforced by the Cargo graph and `cargo-deny`.

## Project status

Built epic by epic, walking-skeleton-first:

- ✅ **Epic 0 — Socle:** 3-crate workspace, pinned toolchain, CI gates (`fmt`, `clippy`,
  `cargo-deny`, `arch_purity`, isolated-build), the Sparkplug `.proto`, fixtures scaffolding,
  and the atomic-persistence primitive.
- ⏳ **Epic 1 — Walking Skeleton:** one meter → Ignition with an honest STALE flag; the
  `ValueDate`/HTTP-`Date`-header freshness audit.
- ⏳ Epics 2–8: exhaustive oracles, full fleet, publishing state machine, configuration,
  observability UI, deployment, documentation.

## Building

Requires **Rust 1.97+** (edition 2024, pinned via `rust-toolchain.toml`), plus `protoc`,
`mold`, and `clang` for the Sparkplug protobuf build and the fast linker.

```sh
cargo build --workspace
cargo test  --workspace
```

Builds are capped to **2 parallel jobs** and use the **mold** linker (see
[`.cargo/config.toml`](.cargo/config.toml)) to stay light on modest hardware.

## License

[MIT](LICENSE) © Guy Corbaz
