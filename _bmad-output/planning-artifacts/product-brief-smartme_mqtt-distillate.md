---
title: "Product Brief Distillate: smartme_mqtt"
type: llm-distillate
source: "product-brief-smartme_mqtt.md"
created: "2026-07-24"
purpose: "Token-efficient context for downstream PRD creation"
---

# smartme_mqtt — Detail Pack for PRD

## Product in one line
Open-source Rust service that polls smart-me cloud energy meters via their REST API and republishes instantaneous power + consumed energy to MQTT for SCADA/HMI/home-automation consumption. Web-configurable, `docker compose`-deployed.

## Hard constraints (non-negotiable, stated by user)
- **Language: Rust.** Whole application written in Rust.
- **Distribution: source on GitHub, prebuilt image on Docker Hub.**
- **Deployment: starts via `docker compose`.**
- **License: MIT** (decided).
- **Web interface scope: configuration + live preview + diagnostics** (not just files/env).
- **Single smart-me account per instance.** Multiple accounts → run multiple instances (no multi-account UI in one process).
- **Logging: daily-rotated log files** for after-the-fact debugging (operational requirement).

## Requirements hints (from brief + research)
- Poll interval must be **configurable** (community default ~10 s; smart-me device updates at most 1×/sec).
- Auth: prefer **API Key** (`Authorization: ApiKey <key>`); support **HTTP Basic** as legacy fallback (Basic is deprecated by smart-me).
- Publish **retained** last-value messages so newly-connected SCADA sees current state immediately.
- Publish a **Last-Will `status`** (online/offline) topic per bridge/device.
- Emit **explicit units** (power W/kW via `ActivePowerUnit`; energy kWh via `CounterReadingUnit`) and **ISO-8601/UTC timestamps** from smart-me `ValueDate`.
- **Staleness detection** based on `ValueDate` age; never publish silently-stale data as fresh.
- Optional **Home Assistant MQTT discovery** (`homeassistant/sensor/<uid>/config`, device_class power/energy, state_class measurement/total_increasing).
- Secrets (API key) injected at **runtime, never baked into the image**; stored safely when entered via web UI.
- Configurable **MQTT topic layout**, default SCADA-friendly hierarchy e.g. `smartme/<site>/<serial>/{power,energy,voltage,...}`.
- Optional **bundled Mosquitto broker** in the compose stack for turnkey setups.
- **Daily log rotation** — pick a Rust logging stack that supports it (e.g. `tracing` + `tracing-appender` daily rolling file, or `log` + `flexi_logger`); logs to a mounted volume; configurable level.
- **Web UI provides diagnostics**: connection state (smart-me + broker), last successful poll, last error, and a live preview of current device values.

## Technical context / smart-me API facts
- Base URL: `https://api.smart-me.com/` (Swagger: `https://api.smart-me.com/swagger/index.html`). Legacy `www.smart-me.com/api/...` redirects.
- Key endpoints: `GET /Devices/` (list), `GET /Devices/{id}` (state), `GET /Devices/{id}/Values` (history), `GET /PartnerAllDevices` (multi), `POST /Actions` (control — out of scope v1).
- Relevant device fields: `ActivePower`, `ActivePowerUnit`, `CounterReading`, `CounterReadingUnit`, `CounterReadingImport/Export`, `CounterReadingT1`, `Voltage`, `Current`, `PowerFactor`, `ValueDate`, `Serial`, `Name`, `Id`. Values also addressable via OBIS codes.
- **Realtime/Webhook API** exists: smart-me POSTs **protobuf** payloads to a callback URL (DeviceId, DateTime, DeviceValues[]). Avoids polling but needs public HTTPS endpoint + protobuf decode.
- No hard published REST rate limit; higher limits behind Professional Licence. OAuth 2.0 also available (Professional Licence).
- Likely Rust crates: `tokio`, `reqwest`, `rumqttc`, `axum`, `serde`.

## Competitive intelligence
- **No existing dedicated smart-me→MQTT bridge on GitHub** — clear open gap.
- Reference architectures: `WouterGritter/smart-meter-mqtt-bridge` (local P1/Modbus, not cloud), `jibrilsharafi/EnergyMe-Home` (DIY ESP32 meter). Pattern analog: Zigbee2MQTT (poll/subscribe → normalize → structured MQTT + HA discovery).
- smart-me today integrated via generic REST in Home Assistant, Node-RED, Loxone — none MQTT-native, none SCADA-topic-aware, none with config UI. That triad is the differentiation.

## Scope signals
- **MVP in:** REST polling (API Key + Basic fallback) for one account, publish power+energy to MQTT (retained + LWT), web UI (config + live preview + diagnostics), daily-rotated logs, Docker Hub image + compose (optional Mosquitto), HA MQTT discovery, MIT license.
- **Deferred / out:** webhook/protobuf ingestion (v2), device control/actions (read-only v1), **Sparkplug B (out of scope now, possible future addition)**, multi-account in one instance (run N instances instead), storage/historian/charting (SCADA's job), hosted SaaS/multi-tenant.

## Resolved decisions (2026-07-24)
- License = **MIT**.
- **One smart-me account per instance**; multi-account handled by running multiple instances.
- Web UI = **configuration + preview + diagnostics**.
- **Sparkplug B out of scope for now** — not dropped forever; a possible future addition. Keep the publish layer open to it (see extensibility note). Rationale for deferring confirmed by user's own setup (below).
- Added requirement: **daily-rotated log files** for debugging.

## Primary user / target SCADA (concrete)
- User's actual SCADA is **Ignition**, and they read **plain MQTT tags without Sparkplug B**. This validates the "structured retained topics" approach as sufficient for the primary use case — Sparkplug's auto-discovery value does not apply here.
- **Extensibility note for architecture:** design the publish layer behind an abstract `Publisher` trait (e.g. `PlainMqttPublisher`) so an optional Sparkplug B publisher could be added later without refactoring the core — without building it now.

## Project nature — REFRAMED (2026-07-24, vision party-mode)
- **This is a PERSONAL tool built for the author (Guy), not a product chasing adoption.** Written in Rust specifically for reliability.
- **GitHub is used for two pragmatic reasons only:** (a) in case someone finds it useful — but the author is indifferent to adoption; (b) to trigger automated Docker image builds (CI).
- **Docker Hub is used to simplify the author's OWN update workflow** (pull new image).
- **Implication:** market-size / moat / community / "THE reference" framing is OUT. Do NOT write the PRD around adoption metrics. The ONE product principle worth keeping from the vision debate — because it serves the author's own Ignition SCADA — is **"never lies to the SCADA"**: correct units + explicit staleness/freshness signalling. That is reliability, not marketing.
- **Vision, honest version:** a rock-solid personal bridge from smart-me meter data to MQTT that never publishes a wrong or silently-stale value; open-sourced pragmatically (CI → Docker Hub), zero adoption ambition.
- **Success criteria shift:** drop GitHub stars / Docker pulls / community metrics. Real success = author's own SCADA shows correct, fresh data, unattended for months, with painless image-based updates.

## Documentation — explicit REQUIREMENT (2026-07-24)
- Author explicitly wants **clear documentation "to avoid trouble"** — treat docs as a first-class deliverable, not an afterthought. Primary reader = future-Guy reconfiguring/troubleshooting/updating months later (adoption is not the point, but self-serviceability is).
- Minimum doc set the PRD should require:
  - **README** — what/why, prerequisites, `docker compose up`, where the web UI is.
  - **Configuration reference** — every `.env` variable (credentials, broker, poll interval, bind address, log level/retention…), defaults, examples.
  - **MQTT contract** — topic list, explicit units (kW/kWh), retained/LWT semantics, staleness/status tag — so Ignition is wired without guessing.
  - **Troubleshooting guide** — common errors (smart-me auth, broker unreachable) → cause → action; how to read the daily-rotated logs.
  - **Update procedure** — how to pull the new Docker Hub image without breaking config.
  - **Commented example `docker-compose.yml`** that works first try.
- Testable acceptance angle: a fresh reader can go from zero to data-in-MQTT using only the README + example compose.

## Direct/local meter access — DROPPED (2026-07-24, later same day)
- **Decision: local Modbus TCP is OUT of scope entirely** (not just deferred). Rationale: the author cannot test it, it needs a smart-me Professional subscription to enable, the register map is fragile, and the priority Kamstrup meters are cloud-only anyway. Simplify to a single upstream: the smart-me cloud REST API.
- **Consequence for the `Source` trait:** its multi-source justification is gone; it is retained ONLY as a **test seam** (single real `SmartMeCloudSource` impl + a fake in tests to exercise staleness/isolation deterministically without network). Not a multi-source abstraction.
- Research below is kept for reference only (in case a future author revisits), but it is NOT in the plan.

### (Reference only — not planned) earlier Modbus research
- Author is interested in reading the meter **directly / locally** (on the LAN) instead of via the smart-me cloud API, **if feasible**. Motivation: avoid cloud dependency and possible rate limits.
- **Architecture implication:** introduce a **`Source` abstraction (trait)** — v1 default = smart-me **cloud REST**; a potential **local/direct** source (e.g. Modbus TCP/RTU, M-Bus, or local HTTP — depends on exact meter model) can be added behind the same trait. This is the SAME extensibility Winston/Victor argued for, but justified by cloud-vs-local (not multi-vendor).
- **Research findings (2026-07-24):** Local read IS possible via **Modbus TCP (port 502, FC03)** — but ONLY on **Telstar 80A, Telstar CT, Pico EV-Charger**. No Modbus RTU, no local HTTP/REST data endpoint, no native device MQTT, no LAN push. M-Bus Gateway only pushes to cloud.
  - Caveats: requires **smart-me Professional subscription** to enable Modbus; **no static IP** (use DHCP reservation); **poll ≥2 s**; device may need periodic internet to keep the Modbus port open (cloud-independence is PARTIAL — reads local, enablement cloud-tied); register map quirk: **Modbus address = internal register − 1**, values int32 big-endian across 2 registers with scaling factors — verify against smart-me's official register sheet per model/firmware.
  - Modbus exposes essentially the same quantities as cloud: active power (W, total + per phase), energy import/export (kWh), voltage, current, power factor, timestamp.
  - Rust: **`tokio-modbus`** (async TCP). LOWER complexity than the cloud path (no OAuth, no JSON pagination, no rate-limit backoff). ~a few hours.
- **Two concrete Source implementations behind the trait:** `SmartMeCloud` (REST, all models) and `SmartMeModbusTcp` (local, Telstar/Pico only). Cloud vs local = a config choice. Same normalization → `Publisher` pipeline downstream.
- **RESOLVED — author's fleet (2026-07-24):** 4 meters on a SINGLE smart-me account:
  - **2× Kamstrup + smart-me module** — cloud REST only (Kamstrup module NOT in the Modbus-TCP supported list). **These are the author's PRIORITY data.**
  - **2× smart-me Telstar 80A (3-phase)** — Modbus-TCP capable (confirmed from device label photo), but Modbus enablement needs Pro subscription (author unsure if he has it).
- **v1 SOURCE STRATEGY — LOCKED = Option A: Cloud REST baseline.** `SmartMeCloud` (REST) reads ALL 4 meters from the single account. This is chosen because (a) the priority Kamstrup meters are cloud-only, (b) one source covers the whole fleet homogeneously, (c) no dependency on the uncertain Pro subscription.
  - **`SmartMeModbusTcp` (local, Telstar-only) = future enhancement**, behind the same `Source` trait, added later IF the author confirms Pro + wants local reliability for the Telstars. NOT in v1 scope, not needed for the priority use case.
  - Keep the `Source` trait in the architecture regardless (near-zero cost now, enables the local path later without refactor).
- Note: composes with the abstract `Publisher` trait → clean internal canonical measurement model sits between `Source` and `Publisher`.
- Sources: https://doc.smart-me.com/interfaces/modbus-tcp · https://doc.smart-me.com/interfaces/api · community register listing https://github.com/PerJarlemark/smart-me

## MQTT publisher = Sparkplug B in v1 (decided 2026-07-24)
- **Sparkplug B is the v1 primary publisher** (not deferred). Rationale: it natively delivers the "never lies to the SCADA" core — automatic STALE-on-DEATH (NDEATH/DDEATH via LWT), source-timestamped metrics, self-describing BIRTH (units + quality), report-by-exception — which the non-Sparkplug path can only hand-build (fragile, split across a Rust/Ignition seam).
- **Deciding factor met:** the author CAN run an automated contract test against a real Ignition MQTT Engine (Murat's condition) → the hand-rolled protobuf is externally verifiable. Without that oracle the decision would flip to plain-first.
- **Test obligations (non-negotiable):** (1) contract test vs a **real Ignition MQTT Engine — run MANUALLY as a pre-release gate, NOT in CI** (author confirmed: no Ignition in CI; the test is still feasible against his own/local Ignition, which satisfies Murat's external-oracle condition); (2) property tests on `seq` (0–255 wrap), rebirth-on-gap, `bdSeq` NDEATH==NBIRTH, DDATA-never-before-DBIRTH, LWT-in-CONNECT-packet — these run in CI; (3) chaos test asserting Ignition→STALE on process/network/meter death.
- **`Publisher` trait must be built now with `&mut self` + `on_connect`/`on_shutdown` lifecycle hooks** (Sparkplug emits BIRTH/DEATH there; a plain publisher no-ops). This is the one Sparkplug-readiness cost paid in v1 regardless.
- **Optional plain-JSON debug publisher** behind the same trait for human-readable inspection/fallback (honors the earlier "JSON payload OK" note) — not the primary path.
- Rust reality: `rumqttc` + `prost` + checked-in `sparkplug_b.proto` + `build.rs` + a small binary-payload decoder for eyeball debugging. ~×4–5 code, ~×3–4 tests vs plain, but fail-safe and bounded.

## Coherence fixes from PRD party-mode #7 (2026-07-24)
- **`Publisher` trait DROPPED; `Source` trait KEPT.** Sparkplug is the only publisher → a concrete `SparkplugPublisher` (NBIRTH/NDATA/NDEATH as methods), NOT a trait. Bridge tests use a **minimal injectable sink** (`Fn(MeterId, Measurement/Quality)` / channel) to assert "on Stale → no fresh NDATA". `Source` stays as a **legitimate test seam** (fake source + injected `Clock` to test the staleness state machine + per-meter isolation deterministically — untestable via the real cloud). A `Publisher` trait is introduced only if a real plain-JSON debug publisher is later built. (This supersedes the earlier "Publisher trait with `&mut self` + lifecycle hooks" note.)
- **🔴 CRITICAL — two-mechanism staleness (Dr. Quinn's Achilles heel):** transport liveness (bridge↔broker) ≠ data freshness (bridge↔cloud). Sparkplug DEATH only fires on broker disconnect. The MOST LIKELY failure is **bridge UP + cloud DOWN** → no NDEATH → a live session would republish a frozen value as fresh = the exact silent lie the project forbids. FIX: (1) DEATH for transport/process death; (2) **application-level `quality=STALE`** published on affected metrics when the cloud fetch fails / data ages past threshold, WITH the node staying alive. Both required.
- **Chaos test split into two:** (a) STALE-on-DEATH (kill process/cut broker link); (b) **STALE-on-cloud-timeout** (bridge UP, cloud unreachable) — the frequent one, previously uncovered.
- **Timestamp oracle:** staleness computed from the meter **measurement timestamp**, never fetch time. **Audit smart-me timestamp semantics on a real payload** (`ValueDate` = measurement vs poll vs server?); if unreliable, document the limitation in "never lies".
- **Sparkplug contract must handle NCMD/Rebirth:** Ignition can request a rebirth → bridge responds with a fresh NBIRTH; + monotonic seq/bdSeq across sessions. Add the rebirth scenario to the Ignition contract test.
- **Web UI preview redefined:** decode Sparkplug metrics + quality (GOOD/STALE); remove all leftover JSON-payload model (JSON path superseded by Sparkplug protobuf).
- **Dropping Modbus is net-positive for "never lies"** (Dr. Quinn): it costs *availability*, not *integrity* — and avoids a two-source reconciliation lie (which meter do you believe when cloud and Modbus disagree?). Don't regret it.

## Architecture: 3-crate Cargo workspace (decided 2026-07-24, after party-mode)
- **Justification is ISOLATION + testability for the workspace overall.** BUT — update 2026-07-24: the author now INTENDS to **publish `sparkplug-b` to crates.io**. So publishing is a real goal **for that one crate only** (not `smart-me-client`, not the bridge).
- **`sparkplug-b` publish bar (scoped to this crate):** stable + documented public API (rustdoc + examples), **semver**, README + CHANGELOG, clear license (MIT); **no third-party types leaked** in the public API (`rumqttc`/`prost` not exposed, or deliberately re-exported+versioned); clean library error types (no `anyhow` in the public API); `#![forbid(unsafe_code)]`; docs.rs builds; **zero smart-me/`Measurement` coupling** (now non-negotiable, not just nice-to-have). This definitively validates `sparkplug-b` as a separate crate. The Ignition contract test doubles as the public conformance guarantee.
- Non-publishing crates (`smart-me-client`, `smartme-bridge`) keep the lighter personal-tool bar. Victor's "publishing = vanity" caution still applies to THOSE, not to `sparkplug-b`.
- **3 crates (NOT 4 — `smartme-core` dropped as a crate; unanimous):**
  - `crates/sparkplug-b` — PURE, generic Sparkplug B library: protobuf encode, EON-node/device model, `seq`/`bdSeq`/rebirth state machine, BIRTH/DATA/DEATH, LWT. **ZERO smartme dependency.** Earns its crate: most dangerous logic (where "never lies" is won) + only real reuse candidate + a hard wall against smartme-type contamination ("can't import", not "shouldn't").
  - `crates/smart-me-client` — PURE smart-me REST client: auth (API Key + Basic fallback), endpoints, deserialization into smart-me **domain types**. **ZERO bridge dependency.** Kept separate to isolate its heavy deps (`reqwest`/`serde`) so `cargo test -p smart-me-client` doesn't rebuild the MQTT stack.
  - `crates/smartme-bridge` — the app: `Measurement` canonical model + `Source`/`Publisher`/`Clock` traits **as modules** (`mod domain`/`mod core` — they have a single consumer, so a module not a crate), web UI, config, wiring, and the thin adapters `SmartMeCloudSource: Source` and `SparkplugPublisher: Publisher`.
- **Key seams:** neither `sparkplug-b` nor `smart-me-client` depends on `Measurement`; the bridge's two adapters do all translation. Dependency direction (`sparkplug-b`, `smart-me-client` → `smartme-bridge`; nothing depends on the bridge) enforced by the Cargo graph + **`cargo-deny` in CI**.
- **The one boundary most likely to be wrong (watch it):** ownership of the `Measurement → Sparkplug Metric` mapping — it MUST live in `SparkplugPublisher` (bridge, e.g. `bridge/src/adapters/sparkplug_publisher.rs`), never in `sparkplug-b`, else the lib becomes smartme-aware and extraction dies. This file is also where "never lies to the SCADA" is tested first.
- **Test placement:** `smart-me-client/tests/` (real fixture JSON) · `sparkplug-b/tests/` (property tests + `ignition_contract.rs`, `#[ignore]`/feature-gated) · adapters in `bridge/tests/` with trait fakes.
- **Future:** a `SmartMeModbusTcp` local source (Telstar) slots in as another module/crate behind the same `Source` trait.

## Decisions from PRD party-mode review (2026-07-24)
- **Criticality = informative only.** Billing/metering-of-record is done by smart-me itself, NOT via the SCADA. The SCADA (Ignition) consumes the data for display/information only → confirms **complexity MEDIUM** (no billing/decision-critical impact from a wrong value).
- **Canonical units fixed:** power = **kW**, energy = **kWh**. Publish these units explicitly per tag; reject/flag unknown source units (fail-closed, no guessed conversion).
- **On smart-me unreachable / stale data:** publish a dedicated **status/staleness tag** to inform the SCADA (fail-loud, "inform don't guess"). Do not silently republish a stale retained value as if fresh. Also publish measurement `ValueDate` timestamp alongside values.
- **Polling rate:** must be **dynamically configurable** (at runtime via web UI). smart-me rate limits are unknown → configurable interval is the mitigation; use bounded backoff on errors.
- **Credential storage:** in a **`.env` file / environment variables**. Guardrails: never logged (incl. daily-rotated logs), `.env` perms `0600`, never baked into the image.
- **Counter publishing (working decision, confirm in PRD):** publish **raw smart-me `CounterReading` (kWh) + `ValueDate`**; cumulative/delta computation is Ignition's responsibility (less state to maintain/test in the bridge). Consistent with "informative only".

## Open questions for PRD (remaining)
- **Web UI network exposure & auth:** bind on loopback (127.0.0.1) by default or 0.0.0.0? Auth on the UI, or documented "trusted network only"? (credentials decision is `.env`, but the UI that reads/shows them is still a surface — must be trancheé).
- **Counter regression handling:** confirm raw-publish; define behaviour when `CounterReading` drops (reset/rollover/stale) — publish as-is, or flag? (lower priority now that data is informative-only).
- Which MQTT topic scheme is the default, and how much is user-templatable?
- Is HA discovery in the v1 MVP or a fast-follow?
- Target hardware baseline (Raspberry Pi / NAS) for footprint acceptance criteria (multi-arch arm64+amd64 image required either way).
- Historical `Values` endpoint — used for backfill on startup, or purely live polling in v1?
- Log retention policy (how many rotated days kept) and default log level.
