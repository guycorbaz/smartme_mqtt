---
stepsCompleted: ['step-01-init', 'step-02-context', 'step-03-starter', 'step-04-decisions', 'step-05-patterns', 'step-06-structure', 'step-07-validation', 'step-08-complete']
inputDocuments:
  - '_bmad-output/planning-artifacts/prd.md'
  - '_bmad-output/planning-artifacts/product-brief-smartme_mqtt.md'
  - '_bmad-output/planning-artifacts/product-brief-smartme_mqtt-distillate.md'
workflowType: 'architecture'
project_name: 'smartme_mqtt'
user_name: 'Guy'
date: '2026-07-24'
lastStep: 8
status: 'complete'
completedAt: '2026-07-24'
---

# Architecture Decision Document

_This document builds collaboratively through step-by-step discovery. Sections are appended as we work through each architectural decision together._

## Project Context Analysis

_Consolidated from four party-mode stress-test rounds (Winston, Murat, Dr. Quinn, Amelia, John, Sally, Paige). Phased and endorsed by the team; contradicts no locked PRD decision._

### Requirements Overview

**Functional Requirements (~45, 9 categories):** meter acquisition (FR1-6), data integrity "never lies" (FR7-16, FR45), SCADA publishing via Sparkplug B (FR17-22), configuration (FR23-27), observability & diagnostics (FR28-37), logging (FR38), lifecycle/deployment (FR39-41), documentation (FR42), broker & versioning (FR43-44).

**Non-Functional Requirements (24):** reliability/availability (NFR1-4), data integrity (NFR5-8), performance/footprint (NFR3, NFR9-11: RSS ≤ 100 MB, FD ≤ 64, read→ACK p95 ≤ 3s), security (NFR12-16), interoperability/deployment (NFR17-21), maintainability (NFR22-23), UTC-clock assumption (NFR24).

### Scale & Complexity

- Domain: backend integration bridge + minimal embedded `axum` web UI (config + preview + diagnostics + state screen).
- Complexity: **Medium build**; correctness + 24/7 reliability are **High, non-negotiable**.
- Fixed scale: 4 meters, single smart-me account, single process. No real concurrency to model beyond the transport floor (see below).
- Components: 3-crate Cargo workspace (`sparkplug-b` [publishable-bar], `smart-me-client`, `smartme-bridge`) + web UI + config store + diagnostics subsystem.

### The integrity model — phased, not maximalist

Guiding invariant: *"when in doubt, publish STALE."* The cheap honest default covers most of the job; refinements are added only where a *silent, plausible* lie is possible.

**MVP-core (must ship):**
- **Staleness oracle** — the load-bearing check. Computed on a **monotonic `Instant`** age (immune to wall-clock drift).
- **Minimal physical-bounds** — NaN / absurd values (e.g. `10⁹ kW`) → STALE. A few lines, not an electrical model.
- **Cold-start = STALE-until-proven** — before the first successful fetch, publish quality=STALE; never restore a last-known value as fresh.
- **Identity binding (minimal)** — serial→device binding asserted (FR9/NFR7 locked). An unknown/mismatched serial must produce an **observable verdict** (STALE / traced-drop → no fraudulent publish on the wrong device topic), never a silent log-only. One test guards it. The *elaborate* identity oracle is deferred, the binding is not.
- **Two-mechanism staleness** (locked): transport NDEATH/LWT **+** app-level quality=STALE when cloud is down but node alive.

**Deferred-behind-a-real-incident** (cheap to add later; the per-serial state kept for staleness already carries the inputs — additive oracles, linear retrofit, no seam refactor):
- **kWh energy-continuity oracle** (monotonicity by serial, reset/rollover policy) — informative-only tool, smart-me does billing; no actor on the alert today.
- **Identity-oracle hardening** (periodic re-verification beyond startup binding).
- **Tier-3 re-arm automation.**

**Cut entirely** (gold-plating for a solo, informative-only tool):
- ❌ NTP-monitoring subsystem / continuous clock-sanity oracle / dedicated clock NFR.
- ❌ UI-can't-disarm-oracle guardrails — Guy is the sole user and sole viewer; he *is* the guardrail.

### The freshness formula — remove the host clock, don't monitor it

Real asymmetric failure mode: a **host clock running late** at cold boot (RTC not yet synced by chrony, resumed VM) makes `now() − ValueDate` read *fresh* on genuinely *stale* data — a silent lie no other oracle catches. Mitigation is **cheaper than the status quo**, not a subsystem:

- **`freshness = HTTP Date-header(response) − ValueDate`** — both timestamps live in the same (cloud) clock domain; the host clock leaves the equation.
- One-line guards: `system_time > 2020-01-01` at boot else STALE; `age < 0 → STALE`.
- Two distinct time uses: **staleness age** = monotonic `Instant`; the **timestamp written into the Sparkplug payload** = wall-clock and must be honest (this is where a bad clock lies to the SCADA about the *when*).
- The `Date` header is an **oracle input, not transport** → requires an **HTTP-headers fixture** (real `Date`, RFC 7231) with explicit edge cases each mapped to a known verdict: `Date` **absent**, **malformed**, **negative skew** (`Date < ValueDate`), **huge skew**. MVP-core, done now — not deferred.

### Runtime concurrency floor

Not "one tick, one publish" — **rumqttc's `EventLoop` must be pumped continuously** or nothing is sent. Honest minimum = **2 tasks + 1 channel**:
- **poll+publish task:** sequentially polls the 4 meters (single account → no parallelism gain, simpler backoff) and publishes.
- **mqtt-driver task:** owns the `EventLoop` (`loop { poll().await }`), handles reconnect → rebirth, and `bdSeq`.
- Plus the `axum` server.

Shared per-meter `Fresh|Stale|Failed` state via a **`watch` snapshot** (UI reads a coherent atomic snapshot). The state machine is a **pure module** `(prev, tick, now) → next`, testable without tokio; input is a fixed `tick` struct `{value, value_date, http_date, now}`. Config is **restart-only** (YAGNI hot-reload).

**Locked seam rule:** the state machine lives **entirely inside the poll+publish task**; Stale/Failed degrades the *published metric quality* and never crosses into the mqtt-driver task, which knows only connection birth/death (`bdSeq`/rebirth). The `watch` snapshot (inter-task state propagation) and the injectable sink (egress seam under `SparkplugPublisher`) are **orthogonal — no overlap**. (Prevents a later refactor rewiring the watch into the mqtt-driver.)

### Broker-down data policy — RESOLVED v1 = traced-drop, no buffer

Unanimous. `try_publish` (non-blocking) so the cloud poller never suffers broker back-pressure; on full/broker-down → **per-device traced drop** (`readings_dropped_total{meter,reason}` + WARN with source timestamp). Sparkplug NBIRTH-on-reconnect + next fresh NDATA self-heals the cumulative counter. **No persistent buffer** (avoids Ignition's out-of-order rejection *and* the unbounded-growth trap). Anti-replay invariant: **every published Sparkplug timestamp == its source `ValueDate`, never the flush/`now` time.** Retention/drop logic lives **above the injectable sink** → unit-testable without a broker.

Chaos matrix completed with the **broker DOWN→RECOVERY / cloud-fresh** transition: on broker return, republish current state (rebirth) with `published-ts == source-ValueDate`, no replay of old values (anti-replay verified at the reconnection instant — the down→up transition, not just static-down).

### Sparkplug state & boot ordering

`bdSeq` (and alias map) is **stateful across restarts** — persisted. Boot order: `bdSeq → NDEATH serialized → LWT set in CONNECT → connect → SUBSCRIBE to NCMD → NBIRTH`. Reconnect triggers a rebirth, and re-runs the subscribe with it. *(The NCMD step was added by Story 4.6 — `tck-id-message-flow-edge-node-ncmd-subscribe` requires it **prior to** the NBIRTH. Corrected by the Story 4.6 code review, 2026-07-29, which found this line and `epics.md`'s AR10 still at five steps while the code and the manual had moved to six.)* `sparkplug-b` exposes **primitives, not a domain** (`publish_metric(name, value, timestamp_ms, quality)`) with quality as a **minimal enum (GOOD/STALE/BAD)**; the bridge translates its transient/fatal taxonomy into it. Keeps the crate `Measurement`-free and publishable.

### Observability — honesty must be *legible*, not just correct (UX-driven)

The UI **consumes the published state, never recomputes it** (else it can show LIVE while the sink publishes STALE). To make truth legible to a tired human at 3am, the **published state must carry**:
- `published_at` **and the staleness threshold used** (so the UI colors the *value itself*: fresh / amber / frozen).
- `last_changed_at` distinct from `last_published_at` (a value repeated 3× = stable network or frozen sensor?).
- **`culprit` (world / you / bridge) as a first-class field**, plus the next repair gesture — not an ad-hoc UI deduction.
- **Root-cause grouping** — a dead token yields 4 muted meters as *consequences* under one cause; never 4 equal-rank reds.
- **Persisted *expected* state** (configured meter→topic→serial mapping) so the Cold-Reopening screen can *reconcile* memory vs reality and put config/identity events on the health timeline ("token expired 12 Mar", "meter #3 absent from API since 2 Apr").
- Distinct visual vocabulary for empty-config vs 401 vs 403 vs timeout vs 200-empty (never an infinite spinner).

### Documentation — first-class, dual-audience

- **The versioned Sparkplug/MQTT Contract must encode the newly-decided semantics** (traced-drop policy, per-device staleness, anti-replay `published-ts == source-ValueDate`, the oracle set → quality mapping, cold-start STALE). **Contract↔code drift is the worst-case lie** (the *document* lies for you): each contract invariant gets a **test named after its clause**, and **`CONTRACT_VERSION` is embedded in the payload/BIRTH** + CHANGELOG per bump.
- **Two missing artifacts to add:** an **ADR trail** (the *why* behind the counter-intuitive locked decisions — traced-drop over buffer, no `Publisher` trait, STALE-until-proven — so future-Guy doesn't "fix" a deliberate choice) and a **Tier-3 Ignition-test runbook** with the re-arm checklist.
- **Dual-audience watertightness:** the published `sparkplug-b` rustdoc must never leak bridge context (no smart-me / 4-meters / Ignition), and must state a **"Conformance scope"** (which Sparkplug B subset is covered). Bridge docs target future-Guy; crate docs target strangers.

### Technical Constraints & Dependencies

- **Locked (do not relitigate):** Rust; 3-crate workspace (`sparkplug-b` publishable-bar — structured now, published only when Guy wants); Sparkplug B v1 sole publisher (concrete `SparkplugPublisher`, no `Publisher` trait); `axum`; docker compose + single-arch Docker Hub via CI; MIT; `.env` secrets; daily-rotated logs; two-mechanism staleness; test seams `Clock`/`Source`/injectable sink.
- **Dependency direction** enforced by Cargo graph + `cargo-deny`; the `Measurement → Sparkplug metric` mapping and error→culprit classification live **only in the bridge**.
- **Error taxonomy across crates:** each crate exposes its own `thiserror` error, no leaked third-party types, no `anyhow` in libs; the bridge classifies **transient (→ Stale/retry) vs fatal (→ Failed)** — a taxonomy that *drives the state machine*.
- **External deps:** smart-me cloud REST (single, unversioned; add **rate-limit/429 backoff + token-refresh** handling); Ignition MQTT Engine (Sparkplug mode, licensed); host clock (largely removed from the freshness path via the `Date`-header formula).
- **Missing seam = MQTT transport** (EventLoop/reconnect/LWT) → integration-only (docker broker); everything else stays unit-testable via the seams.

### Cross-Cutting Concerns

Data-integrity invariant ("never lies") · two-mechanism staleness · legible-honesty in the UI (enriched published state) · secret handling (`.env`, never logged) · single-source-of-truth diagnostics + culprit classification · bounded resilience (backoff + jitter; no unbounded growth) · time/UTC discipline (Date-header freshness) · Sparkplug sequence & `bdSeq` integrity (fail-safe → STALE) · contract↔code non-drift.

### Open items for the Architecture steps

1. 🔴 **[BLOCKING]** Audit smart-me `ValueDate` semantics on a real payload (measurement vs poll vs server time; UTC/DST; sample across midnight + a DST change) **and confirm the HTTP `Date` header is present/usable** for the freshness formula.
2. Web UI network bind + auth posture (loopback-default vs documented trusted-network).
3. crates.io / Docker Hub release pipeline (later).
4. Log retention window (N days) — 24/7 tool must not fill the disk.
5. **Broker/token secrets-at-rest boundary** when stored via the config file (same-file vs separate / env / `0600` / Docker secret) — must never leak into traced logs or the enriched published state. *Coupled with item 2.*
6. **Atomic write for config + `bdSeq`** (write-temp + fsync + rename) — a corrupt `bdSeq` at boot breaks the whole Sparkplug lifecycle (homelab power-loss).
7. **Docker healthcheck semantics** — reflect real poll state vs mere process-liveness; must not restart the container where an honest STALE is the better answer (a restart destroys the continuity being protected).
8. ~~**Graceful shutdown vs LWT** — mirror of the boot ordering; decide whether to emit a clean DEATH on SIGTERM or rely on the LWT.~~ **RESOLVED 2026-07-26 (ADR 0011): both.** Explicit NDEATH on SIGTERM, then drop the connection so the will fires too. See the Graceful-shutdown decision below and AR13.

*Explicit non-item: single-arch target = a Dockerfile line (`linux/amd64` or `arm64` per host), not an architectural decision.*

## Starter Template Evaluation

### Primary Technology Domain

**Rust backend / integration-bridge**, multi-crate Cargo workspace with an embedded `axum` web UI. Not a frontend-web domain → no `create-*`-style monolithic starter applies.

### Starter Options Considered

- **`cargo-generate` community template (axum/tokio boilerplate):** rejected — the 3-crate structure is already dictated by the PRD; a template imposes an organization that would have to be dismantled, and risks pulling opinionated deps against the "minimal footprint" NFR.
- **Fork of an existing Sparkplug-B Rust repo:** rejected — the PRD requires a *bespoke, publishable* `sparkplug-b` crate (`#![forbid(unsafe_code)]`, no leaked third-party types, documented conformance scope); a fork contaminates that guarantee and the crates.io bar.
- **Native `cargo` workspace scaffolding:** **selected** — idiomatic, zero hidden dependencies, matches the locked crate boundaries exactly.

### Selected Starter: None (native Cargo workspace scaffolding)

**Rationale for Selection:** the architecture (3 crates, strict dependency direction, pure lib crates) is already specified and non-negotiable; a starter would fight it. Native scaffolding keeps the dependency tree auditable (`cargo-deny`) and the footprint small.

**Initialization Command (first implementation story):**

```bash
# Workspace root
cargo new --name smartme_mqtt smartme_mqtt && cd smartme_mqtt
# Convert root to a virtual workspace (edit Cargo.toml: [workspace], members = [...])
cargo new --lib crates/sparkplug-b
cargo new --lib crates/smart-me-client
cargo new --bin crates/smartme-bridge
```

**Edition & toolchain:** Rust **edition 2024**, pinned via `rust-toolchain.toml` (reproducible CI + Docker builds).

**Verified current crate versions (crates.io, 2026-07-24) — starting baseline, exact pins decided in the deps story:**

| Crate | Latest stable | Role |
|---|---|---|
| `tokio` | 1.53.1 | async runtime |
| `axum` | 0.8.9 | web UI / health server |
| `rumqttc` | 0.25.1 | MQTT client + EventLoop |
| `prost` | 0.14.4 | protobuf (Sparkplug B) |
| `reqwest` | 0.13.4 | smart-me REST client |
| `serde` | 1.0.229 | (de)serialization |
| `tracing-subscriber` | 0.3.23 | structured logging |
| `tracing-appender` | 0.2.5 | daily-rotated log files |
| `thiserror` | 2.0.19 | per-crate typed errors |
| `cargo-deny` | 0.20.2 | dependency-direction + license gate (CI) |

**Architectural Decisions Provided (by convention, not a starter):**
- **Language & Runtime:** Rust edition 2024, `tokio` async, pinned toolchain.
- **Build Tooling:** Cargo virtual workspace; `cargo-deny` in CI enforces dependency direction + licenses; `build.rs` + `prost` for the checked-in Sparkplug `.proto`.
- **Testing Framework:** built-in `cargo test` (unit + integration `tests/`), property tests for the Sparkplug state machine, an independent protobuf decoder for golden/round-trip (Tier 2c).
- **Code Organization:** `crates/sparkplug-b` (pure lib), `crates/smart-me-client` (pure lib), `crates/smartme-bridge` (bin: `Measurement`, `Source`/`Clock` modules, adapters, web UI, config, wiring).
- **Development Experience:** `tracing` + `tracing-appender` for daily-rotated logs; fixtures-first (`fixtures/smartme_sample.json` + captured HTTP headers).

**Note:** Project initialization using the command above should be the **first implementation story**.

## Core Architectural Decisions

_Environment forks confirmed by Guy; ops/data decisions reviewed and endorsed in party-mode (Winston, Murat, Amelia)._

### Decision Priority Analysis

**Critical (block implementation):** data/state model, config & `bdSeq` persistence, secrets boundary, web UI exposure, broker provisioning, healthcheck semantics, shutdown behavior — **all resolved below.**

**Important (shape architecture):** already resolved in Project Context Analysis (2-task concurrency, freshness formula, traced-drop, error taxonomy, per-device state machine) — not re-decided here.

**Deferred (post-MVP):** kWh-continuity oracle, identity-oracle hardening, Tier-3 re-arm automation, crates.io publish of `sparkplug-b`, multi-arch image, runtime poll hot-reload of structural fields.

### Data Architecture

- **No database.** State is **in-memory** (`watch<[MeterState; N]>` snapshot) + two small **persisted TOML files** on a mounted volume. Rationale: 4 meters, informative-only, cumulative counters self-heal — no historian in the bridge (Ignition's job).
- **Persisted files (2):** (1) non-secret **config** (meter→topic mapping, poll interval, broker host/port, log level/retention); (2) **`bdSeq`** + minimal Sparkplug session state. Format **TOML** (`serde`), human-editable, matches the Config-Reference doc.
- **Config propagation = `ArcSwap<Config>`, not a global restart.** Non-structural fields (poll interval, log level, mapping labels) **hot-swap** — the loop reads `config.load()` per cycle after the UI's atomic write + `store()`. Structural fields (broker host/port, meter count → `[MeterState; N]`, secrets) are **restart-required** and show a **"pending restart"** flag in the UI. Single-writer discipline on the config file (UI serializes its writes).
- **Atomic write (item ⑥):** a pure `persist_atomic(path, &T)` helper — write-temp + `fsync(file)` + **`fsync(parent_dir)`** + rename (parent-dir fsync prevents a lost rename on crash). `bdSeq` is written **only by the mqtt-driver task** (which owns it) via this stateless helper → no task-boundary crossing; the `Fresh|Stale|Failed` state machine stays entirely in the poll+publish task.
- **Startup validation:** config validated at startup; **refuse-to-start on invalid** (FR26) — table-driven, each invalid class → its *named* error. Fixture-first parsing contract (`fixtures/smartme_sample.json` + captured HTTP headers).

### Authentication & Security

- **Web UI exposure (items ②/⑤): behind Traefik.** The container does **not publish a host port** (`expose:` only, no `ports:`); the app **binds `0.0.0.0:PORT` inside the container**, reachable solely over Traefik's shared Docker network. **Traefik** handles routing + TLS via labels and optional auth via a proxy middleware (`basic-auth`/`forward-auth`). **No in-app auth** — the trust boundary is Traefik, not the app; nothing listens directly on the LAN.
- **Secrets (item ⑤): `.env` / env vars only** (perms `0600`, never in the image, never logged incl. rotated logs). **The UI never reads, writes, or re-displays secrets** — only non-secret config. smart-me API key + optional broker password live in `.env`.
- **TLS:** mandatory for smart-me (hard-fail otherwise, NFR13). Broker connection optionally TLS/auth or plain per config (FR43/NFR16); when secured, broker creds follow the same `.env` discipline.
- **Threat model:** informative-only, trusted host/LAN, stated explicitly (NFR15).

### API & Communication Patterns

- **Inbound:** smart-me **cloud REST** via `smart-me-client` — `Authorization: ApiKey` primary, HTTP Basic fallback; `GET /Devices/` (discovery) + `GET /Devices/{id}` (state); bounded exponential backoff + jitter on 429/5xx/timeout, honor `Retry-After`; `401/403` → stop + `auth_error`.
- **Outbound:** **Sparkplug B** over MQTT via the concrete `SparkplugPublisher` + `sparkplug-b` (pure lib, primitives `publish_metric(name, value, timestamp_ms, quality)`, quality enum GOOD/STALE/BAD). NBIRTH/DBIRTH → NDATA/DDATA (**every poll, not RBE** — corrected 2026-07-28; RBE is blocked on NCMD/Rebirth, see `tck-id-principles-rbe-recommended` in the conformance matrix) → NDEATH(LWT)/DDEATH; NCMD/Rebirth honored; `try_publish` non-blocking → per-device **traced-drop** on broker-down; anti-replay `published-ts == source ValueDate`.
- **Freshness formula (item ①):** `freshness = HTTP Date-header − ValueDate` (removes host clock) + guards (`system_time > 2020` at boot → STALE; `age < 0` → STALE). **⚠️ Depends on the [BLOCKING] audit** — first implementation spike captures a real payload+headers; **fallback if `Date`/`ValueDate` unreliable:** monotonic-`Instant` staleness only + documented limitation. No architecture rework either way (age is an isolated `tick` field).
- **Error handling:** `thiserror` per crate, no leaked third-party types, no `anyhow` in libs; bridge classifies **transient (→ Stale/retry) vs fatal (→ Failed)** driving the state machine.

### Frontend Architecture (embedded web UI)

- **`axum` server** serving a **minimal server-rendered UI** (config + live preview + diagnostics + "state of the bridge" screen). No SPA framework. **UI assets embedded via `rust-embed`** (axum feature; `debug-embed=false` → filesystem in dev, embedded in release) — lives in the `smartme-bridge` app crate only, **zero impact on `sparkplug-b` purity** (unidirectional app→lib dependency).
- **UI consumes the published state, never recomputes it** — reads the `watch` snapshot (atomic, coherent across meters). Published state carries `published_at` + threshold, `last_changed_at`, `culprit` (world/you/bridge) first-class + repair gesture, root-cause grouping, and the persisted *expected* mapping for Cold-Reopening reconciliation.
- **Health endpoint `GET /healthz`** (JSON) exposing per-meter last-success, FD/RSS, running version.

### Infrastructure & Deployment

- **Broker (FR43): external only** — `docker compose` runs the bridge alone, connecting to the operator's existing broker (the one Ignition already consumes). No bundled Mosquitto.
- **Traefik integration:** the bridge joins Traefik's **`external: true` named Docker network**; exposed via router labels (`traefik.enable=true`, rule + TLS resolver). A commented **`docker-compose.override.yml.example`** provides a `ports:`-mapped fallback for non-Traefik users. Single-arch image (`linux/amd64` or `arm64` per host), `restart: unless-stopped`, no host port mapping.
- **Liveness heartbeat (item ⑦, gravé):** the poll+publish loop updates a **`last_loop_tick` (monotonic `Instant`) at the top of every iteration, before the network call**. `GET /healthz` returns unhealthy **only if `now − last_loop_tick > N × poll_interval`** (N≈3) — a wedged/deadlocked poller restarts; an **honest STALE never does** (a failing fetch still advances the tick). Per-meter freshness stays informational, not a restart trigger.
- **Graceful shutdown (item ⑧): guarantee the DEATH fires.** A *clean* TCP close on SIGTERM can be read by some brokers as a graceful disconnect → **LWT not published** → Ignition shows the last value as live (a silent lie — the highest-risk trap here). **Resolved 2026-07-26 (ADR 0011): both mechanisms, not either.** On shutdown the bridge publishes an explicit NDEATH, keeps the transport pumping until it reaches the wire, then **drops** the connection — never a clean MQTT DISCONNECT, which instructs the broker to discard the will. Measured against Mosquitto 2 by `chaos_sigterm_no_lie`: the explicit certificate arrives immediately, and the will follows when the socket closes at process exit, so a consumer sees two NDEATHs carrying the same `bdSeq`. Duplicate deaths are idempotent for a consumer that has already marked the node down; Story 1.15's Ignition contract test should confirm that in the field. The either/or was deferred to this test on purpose (AR13); requiring the explicit branch is what makes a planned stop immediate instead of waiting on the broker to notice a socket — up to 1.5× keep-alive if the connection is left half-open.
- **Logging:** `tracing` + `tracing-appender` daily-rotated files, level `info` default, **retention 14 days default** (item ④, configurable), secrets never recorded.
- **CI/CD (item ③, later):** GitHub Actions runs the test tiers + `cargo-deny` and builds/pushes the Docker Hub image; `sparkplug-b` crates.io publish is a **separate, manual, deferred** pipeline.

### Test-tier additions (from decision review)

- **`POLLER-WEDGE` (Tier 4 chaos):** inject a `future::pending()` in the loop → `last_loop_tick` freezes → `/healthz` flips healthy→unhealthy within 2×poll_interval (proves the *positive* half of the healthcheck claim).
- **`SIGTERM-NO-LIE` (Tier 4 chaos):** a real SIGTERM to the real binary → an independent subscriber asserts the **explicit** DEATH is delivered, told apart from the broker's will by its timestamp (ADR 0011). The "no fresh DDATA survives" half is **unverified**: the scenario has no reachable cloud, so no reading exists to survive — proving it needs a TLS-terminating fake of the smart-me API (deferred).
- **`bdSeq` crash-during-persist (Tier 2b extension):** kill mid-persist → value is old-or-new, never corrupt; NBIRTH uses `bdSeq+1` monotone.
- **Startup validation (table-driven):** each invalid-config class → refuses with its named error.

### Decision Impact Analysis

**Implementation sequence (first stories):**
1. Cargo workspace scaffolding (init command from Starter section).
2. `smart-me-client` + fixtures + **`ValueDate`/`Date`-header audit spike** (unblocks the freshness formula).
3. `sparkplug-b` core (encode + seq/bdSeq/rebirth state machine + property tests).
4. Bridge: `Measurement`, pure state machine, 2-task runtime, `SparkplugPublisher`, traced-drop.
5. `axum` UI + health endpoint + published-state model.
6. Config/`bdSeq` persistence (atomic write) + `.env` + logging.
7. Compose + Dockerfile + Traefik labels + healthcheck + CI.

**Cross-component dependencies:** the freshness formula (① audit) gates the staleness oracle; the error taxonomy drives the state machine → published quality → UI; `bdSeq` persistence gates Sparkplug lifecycle correctness; the `last_loop_tick` heartbeat gates the healthcheck semantics.

## Implementation Patterns & Consistency Rules

### Pattern Categories Defined

**Critical conflict points identified:** ~10 areas where independent agents could diverge — most dangerously the **Sparkplug metric/topic grammar** (an external contract) and **time/quality handling** (where "never lies" is won or lost).

### Naming Patterns

**Rust code naming (enforced by `rustfmt` + `clippy`, `-D warnings` in CI):**
- Types/traits/enums `CamelCase`; functions/vars/modules `snake_case`; consts `SCREAMING_SNAKE_CASE`. Standard — no deviation.
- **Strong domain typing — no stringly-typed identifiers.** Newtypes: `MeterId`, `Serial`, `TopicPath`, `Kw(f64)`, `Kwh(f64)`. A raw `String`/`f64` for a serial, topic, or physical quantity is an **anti-pattern** (the identity/unit oracles depend on the type wall).
- **Quality is one enum, one definition:** `Quality { Good, Stale, Bad }` in `sparkplug-b`. The bridge's error taxonomy maps *into* it; no ad-hoc quality strings anywhere.

**Sparkplug metric & MQTT topic grammar (CONTRACT — fixed once, versioned):**
- Group/Node/Device IDs and metric names follow a **single documented scheme** defined in the versioned Sparkplug/MQTT Contract, not chosen per-agent. Metric names: `Power` (kW), `Energy` (kWh) with Sparkplug engineering-unit properties; per-meter device keyed by `Serial`.
- Any topic/metric-name change **bumps `CONTRACT_VERSION`** and is reflected in the contract doc + its clause-named test.

**Config & environment naming:**
- TOML config keys: `snake_case`. Env vars (`.env`): `SCREAMING_SNAKE` with the **`SMARTME_` prefix** (e.g. `SMARTME_API_KEY`, `SMARTME_BROKER_PASSWORD`). Secrets are env-only; non-secret settings are TOML-only — never both.

### Structure Patterns

- **Unit tests** inline (`#[cfg(test)] mod tests`) beside the code. **Integration tests** in each crate's `tests/`. **Property tests** named `prop_*`; **chaos tests** named `chaos_*` (e.g. `chaos_poller_wedge`, `chaos_sigterm_no_lie`); the manual Ignition test is `#[ignore]`/feature-gated `ignition_contract`.
- **Module organization (bridge):** `domain` (`Measurement`, `Quality`, newtypes), `core` (pure `Clock`/`Source` traits + the `Fresh|Stale|Failed` state machine), `adapters` (`smart_me_cloud_source`, `sparkplug_publisher`), `ui`, `config`, `persist`, `app` (wiring/tasks). Pure modules never import `tokio`/`axum`.
- Fixtures under `crates/<crate>/fixtures/` (checked in first). The Sparkplug `.proto` under `crates/sparkplug-b/proto/`.

### Format Patterns

- **All timestamps are UTC, ISO-8601** on the wire (health JSON, logs) and `i64` epoch-millis inside Sparkplug payloads. **Never local time.**
- **Health endpoint JSON: `snake_case` fields**, explicit units in field names (`power_kw`, `energy_kwh`, `age_seconds`), quality as the enum's lowercase string (`good`/`stale`/`bad`).
- **Physical values always carry their unit in the type** (`Kw`, `Kwh`) up to the publish boundary; unit conversion happens in exactly one place (the `SmartMeCloudSource` adapter), fail-closed on unknown units.

### Communication / State Patterns

- **Time:** never call `SystemTime::now()`/`Instant::now()` directly in logic — always the injected `Clock`. Freshness age = monotonic `Instant`; payload timestamp = `Date-header − ValueDate` wall-clock. Hard rule (the clock is a test seam and a correctness dependency).
- **State propagation:** the `watch<[MeterState; N]>` snapshot is the **single source of truth** read by the UI and publisher; nothing recomputes meter state independently. The state machine is a **pure function** `(prev, tick, now) → next` — no I/O inside it.
- **Publish:** `try_publish` only (never blocking `.await` on the publish path); a drop is a **traced** drop (`readings_dropped_total{meter, reason}` + WARN), never silent.

### Process Patterns

- **Error handling:** `Result<T, E>` with per-crate `thiserror` enums; **no `unwrap()`/`expect()`/`panic!` outside tests and `main` startup**; no `anyhow` in library crates. Every fallible boundary classifies **transient (→ Stale/retry) vs fatal (→ Failed)** — never a bare `?` that loses the classification the state machine needs.
- **Logging (`tracing`):** structured fields, canonical names reused everywhere — `meter_serial`, `quality`, `culprit` (`world`/`you`/`bridge`), `topic`, `age_seconds`. **Secrets never logged** (enforced by a log-grep test). Levels: `error` (fatal/needs action), `warn` (traced drop, transient fault), `info` (lifecycle), `debug` (payload eyeballing).
- **Never a substituted value:** on any doubt (missing field, unknown unit, unreadable timestamp) publish `quality != Good`, never a default/guessed number.

### Enforcement Guidelines

**All AI agents MUST:**
- Use the domain newtypes and the single `Quality` enum — never raw strings/floats for serials, topics, units, or quality.
- Route all time through `Clock`; never hardcode `*::now()` in logic.
- Keep pure modules (`domain`, `core`) free of `tokio`/`axum`/`reqwest`/`rumqttc` imports; keep `sparkplug-b` and `smart-me-client` free of `Measurement`.
- Emit only `try_publish` + traced drops; publish `Quality` honestly, never a substituted value.
- Bump `CONTRACT_VERSION` + update the contract doc + its test on any topic/metric-name/semantics change.

**Enforcement:** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo-deny` (dep direction + licenses), and the clause-named contract tests all run in CI; violations fail the build.

**Anti-patterns (rejected in review):** stringly-typed serials/topics; `SystemTime::now()` in logic; `anyhow` in a lib crate; a silent drop or a default value substituted for a bad reading; `Measurement` leaking into `sparkplug-b`; logging a secret; recomputing meter state in the UI.

## Project Structure & Boundaries

_Reviewed in party-mode (Amelia, Winston, Paige); three tree-level build blockers fixed, enforceability made mechanical._

### Complete Project Directory Structure

```
smartme_mqtt/
├── README.md  CHANGELOG.md  LICENSE          # MIT
├── Cargo.toml                                # [workspace] resolver="2", [workspace.dependencies], members
├── Cargo.lock                                # committed
├── rust-toolchain.toml                       # pinned edition-2024 toolchain
├── Justfile                                  # dev tasks: manual Ignition test, release steps, isolated builds
├── deny.toml                                 # cargo-deny: external deps + licenses (NOT internal topology)
├── rustfmt.toml  clippy.toml
├── .env.example                              # every SMARTME_* var, commented (config-ref source)
├── .gitignore  .dockerignore                 # ignores .env, /target, /data
├── Dockerfile                                # multi-stage, single-arch, embeds UI assets
├── docker-compose.yml                        # bridge only, Traefik labels, external network
├── docker-compose.override.yml.example       # non-Traefik fallback (ports:)
│
├── .github/workflows/
│   ├── ci.yml                                # fmt+clippy+cargo-deny+arch-test+test tiers+AC-LEAK-01
│   ├── sparkplug-isolated.yml                # build `sparkplug-b` ISOLATED (--no-default-features) — feature-leak guard
│   └── release.yml                           # Docker Hub image (deferred pipeline)
│
├── crates/
│   ├── sparkplug-b/                          # PURE generic lib (crates.io bar)
│   │   ├── Cargo.toml                        # #![forbid(unsafe_code)], rust-version=MSRV, minimal deps (prost only)
│   │   ├── README.md  CHANGELOG.md           # crate-audience; Conformance scope (NFR19)
│   │   ├── build.rs   proto/sparkplug_b.proto
│   │   ├── src/ {lib, model (Metric+Quality{Good,Stale,Bad}), seq (seq+bdSeq), lifecycle (BIRTH/DATA/DEATH+LWT+rebirth), encode, decode, error}.rs
│   │   └── tests/
│   │       ├── prop_seq_bdseq.rs             # Tier 2b
│   │       ├── golden_roundtrip.rs           # Tier 2c independent decoder
│   │       ├── no_context_leak.rs            # NEG test: fails if `smartme`/`ignition`/`SMARTME_` appears in src
│   │       └── ignition_contract.rs          # Tier 3 MANUAL (#[ignore])
│   │
│   ├── smart-me-client/                      # PURE REST client
│   │   ├── Cargo.toml                        # rust-version=MSRV; isolates reqwest/serde
│   │   ├── src/ {lib, auth, client (+Date-header capture), model, backoff, error}.rs
│   │   ├── fixtures/ {smartme_sample.json, http_headers/*.txt (valid/absent/malformed/skew)}
│   │   └── tests/contract_mock_cloud.rs      # Tier 2
│   │
│   └── smartme-bridge/                       # THE APP
│       ├── Cargo.toml                        # rust-version=MSRV; deps both libs + tokio/axum/rumqttc/tracing; dev: testcontainers
│       ├── src/
│       │   ├── lib.rs                        # crate lib: exposes domain/core/app for integration tests; run()
│       │   ├── main.rs                       # thin: calls lib::run()
│       │   ├── domain/ {mod, measurement (Kw/Kwh/Serial/MeterId/TopicPath), quality}.rs   # PURE
│       │   ├── core/                          # PURE — test seams
│       │   │   ├── clock.rs  source.rs  state_machine.rs
│       │   │   └── channel.rs                # inter-task message type (Measurement+quality) — PURE, lives here not app/
│       │   ├── adapters/ {smart_me_cloud_source, sparkplug_publisher}.rs
│       │   ├── app/ {poll_publish (state machine + last_loop_tick), mqtt_driver (EventLoop + bdSeq), supervisor}.rs
│       │   ├── config/ {mod (ArcSwap), validate}.rs
│       │   ├── persist.rs                    # persist_atomic: write-temp+fsync(file)+fsync(dir)+rename
│       │   ├── ui/ {server (/healthz, rust-embed), routes, state (reads snapshot)}.rs + assets/
│       │   └── error.rs                       # BridgeError; transient vs fatal
│       └── tests/
│           ├── common/                        # shared test-utils
│           │   ├── mod.rs                     # fake Clock, fake Source, injectable sink
│           │   └── broker.rs                  # testcontainers mosquitto: spawn/kill/restart harness
│           ├── arch_purity.rs                 # arch test: greps forbidden imports (core/domain pure; mapping only in publisher)
│           ├── staleness_injected_clock.rs   partial_failure_isolation.rs
│           ├── chaos_stale_on_death.rs        chaos_stale_on_cloud_timeout.rs
│           ├── chaos_broker_recovery.rs       chaos_poller_wedge.rs   chaos_sigterm_no_lie.rs
│           ├── bdseq_crash_persist.rs         config_validation_table.rs
│           ├── contract_golden.rs             # fails if Measurement→metric mapping changes w/o CONTRACT_VERSION bump
│           ├── leak_ac01.rs                   zero_config_loss_update.rs
│
├── docs/
│   ├── index.md                              # symptom-indexed ("broker down→§X", "frozen values→staleness") + breadcrumbs
│   ├── glossary.md                           # shared lexicon: STALE, traced-drop, ValueDate, LWT, re-arm, CONTRACT_VERSION
│   ├── operations-quickstart.md              # 3am triage map ("it's broken, start here")
│   ├── configuration-reference.md            # every SMARTME_* + TOML key (var·required·format·example·default·effect)
│   ├── mqtt-sparkplug-contract.md            # STANDALONE VERSIONED; ends with CONFORMANCE TABLE (INV-n → clause-named test)
│   ├── troubleshooting.md                     # ≥8 failure modes: Symptom→Cause→Action→Confirmation
│   ├── update-procedure.md                    # pull+restart, rollback, post-update verification
│   ├── ignition-contract-runbook.md           # Tier-3 steps + re-arm checklist
│   ├── data-flow.md                           # annotated diagram, failure points pinned
│   └── adr/ 0001-sparkplug-b-v1 · 0002-traced-drop-over-buffer · 0003-no-publisher-trait ·
│           0004-freshness-date-header · 0005-stale-until-proven ·
│           0006-healthcheck-no-restart-on-stale · 0007-external-broker-only · 0008-three-crate-split
│
└── data/                                      # mounted volume (gitignored): config.toml + bdseq.toml
```

### Architectural Boundaries & Enforcement

- **Inbound boundary:** `smart-me-client` — the only code speaking HTTP to smart-me; exposes smart-me domain types + typed errors; no `Measurement`, no MQTT.
- **Outbound boundary:** `sparkplug-b` — the only code speaking Sparkplug/protobuf; exposes primitives + `Quality`; no smart-me, no `Measurement`.
- **Translation boundary (bridge only):** the two adapters. `SmartMeCloudSource` maps smart-me types → `Measurement` (unit conversion, fail-closed); `SparkplugPublisher` maps `Measurement` → Sparkplug metrics (where "never lies" is tested first) + error→culprit classification.
- **Task boundary:** `poll_publish` owns the state machine + `last_loop_tick`; `mqtt_driver` owns the rumqttc EventLoop + `bdSeq`; they communicate over the pure `core/channel.rs` message type; the state machine never crosses into `mqtt_driver`.
- **Secret boundary:** `.env` → `config` (read-once secrets) → adapters; the `ui` module never touches secrets.
- **Enforcement layers:** (1) Cargo graph → no inter-crate cycle (mechanical); (2) `resolver="2"` + isolated CI build + `no_context_leak.rs` → `sparkplug-b` crates.io purity; (3) `arch_purity.rs` → intra-crate purity (`core`/`domain` free of tokio/axum; mapping only in `sparkplug_publisher.rs`); (4) `cargo-deny` → external deps + licenses; (5) `contract_golden.rs` + the contract's conformance table → contract↔code non-drift.

### Requirements → Structure Mapping

| FR category | Location |
|---|---|
| Meter Data Acquisition (FR1-6) | `smart-me-client/`, `adapters/smart_me_cloud_source.rs` |
| Data Integrity "never lies" (FR7-16, FR45) | `core/state_machine.rs`, `adapters/sparkplug_publisher.rs`, `domain/` |
| SCADA Publishing (FR17-22) | `sparkplug-b/`, `app/mqtt_driver.rs`, `adapters/sparkplug_publisher.rs` |
| Configuration (FR23-27) | `config/`, `persist.rs`, `.env.example` |
| Observability & Diagnostics (FR28-37) | `ui/`, `app/` (state snapshot), `docs/troubleshooting.md` |
| Logging (FR38) | `main.rs`/`lib.rs` (tracing init), cross-cutting |
| Lifecycle & Deployment (FR39-41) | `Dockerfile`, `docker-compose*.yml`, `docs/update-procedure.md` |
| Documentation (FR42) | `docs/` |
| Broker & Versioning (FR43-44) | `config/`, `app/mqtt_driver.rs`, `ui/` (version), `lib.rs` |

### Data Flow

`smart-me cloud REST` → `smart-me-client` (fetch + Date-header) → `SmartMeCloudSource` (→ `Measurement`, unit fail-closed) → `poll_publish` task (state machine: Fresh|Stale|Failed via `Clock`) → `watch` snapshot → { `SparkplugPublisher` (→ `mqtt_driver` → broker → Ignition) **and** `ui` (read-only) }. The health endpoint reads the same snapshot + `last_loop_tick`.

## Architecture Validation Results

### Coherence Validation ✅

**Decision Compatibility:** All choices cohere. Rust edition-2024 + `tokio`/`axum`/`rumqttc`/`prost`/`reqwest` are mutually compatible at the verified versions. The 2-task model fits rumqttc's EventLoop reality; `ArcSwap<Config>` fits restart-only-for-structural-fields; traced-drop + `try_publish` fit the non-blocking publish path; the freshness formula (`Date-header − ValueDate`) is consistent with the injected-`Clock` seam. No contradictory decisions found.

**Pattern Consistency:** The naming/typing rules (domain newtypes, single `Quality` enum, no `SystemTime::now()` in logic) directly support the oracle invariants. The error taxonomy (transient/fatal) drives the state machine → published quality → UI — one consistent chain. Sparkplug metric/topic grammar is contract-governed, not per-agent.

**Structure Alignment:** The 3-crate tree enforces the boundaries mechanically (Cargo graph + `resolver="2"` + isolated CI + `arch_purity.rs` + `cargo-deny`). The `lib.rs`+thin-`main.rs` fix makes the integration/chaos tests compilable; `tests/common/` + `testcontainers` broker harness make the chaos tier runnable in CI. No structural element blocks a decision.

### Requirements Coverage Validation ✅

**Functional Requirements (FR1-45):** All 9 categories mapped to concrete components. Spot-checks: FR9 (serial identity) → `domain` newtypes + `sparkplug_publisher` + `arch_purity`; FR11-13 (per-device staleness, two-mechanism) → `core/state_machine` + `mqtt_driver` LWT; FR20 (no over-claimed delivery; traced drop — amended, ADR 0010) → `mqtt_driver`; FR21 (orphan-retained purge) → `sparkplug_publisher`; FR22 (broker-outage policy) → traced-drop; FR26 (refuse-to-start) → `config/validate` + `config_validation_table`; FR40-41 (update/rollback) → `zero_config_loss_update` + `docs/update-procedure`; FR42 (docs) → full `docs/` tree; FR43-44 → `config` + `ui`. No FR without an architectural home.

**Non-Functional Requirements (NFR1-24):** NFR1-2 → backoff + state machine; NFR3/9 → no-DB in-memory + retention caps + `leak_ac01`; NFR5-8 → the five oracles + anti-replay; NFR10 → single-poll path, measurable; NFR12-16 → `.env`-only secrets + Traefik posture + TLS hard-fail; NFR17-19 → Tier-3 runbook + conformance table + crates.io purity guards; NFR22 → seams + cargo-deny; NFR24 → largely removed from the freshness path via Date-header. All NFRs addressed.

### Implementation Readiness Validation ✅

**Decision Completeness:** critical decisions documented with verified versions; open items are *sequenced*, not undefined (the `ValueDate`/`Date`-header audit is story #2 with a documented fallback → no rework path). **Pattern Completeness:** naming, structure, format, communication, and process patterns specified with enforcement + anti-patterns. **Structure Completeness:** complete tree, boundaries, FR mapping, data flow — all present and specific.

### Gap Analysis Results

- **Critical gaps:** none open. (The `ValueDate` semantics audit is a bounded first-story spike with a documented fallback — not an architectural gap.)
- **Important (sequenced, not blocking):** exact `Date`-header reliability confirmed only at implementation; `sparkplug-b` crates.io publish deferred (structured, not published).
- **Nice-to-have:** ADR bodies, the annotated data-flow diagram, and conformance-table population are authored during implementation.

### Validation Issues Addressed

The three tree-level build blockers (bin-only tests, missing `tests/common/`, homeless channel type) and the enforceability gaps (feature-unification leak, intra-crate purity, contract drift) were caught in the structure review and folded into the tree before this validation.

### Architecture Completeness Checklist

**Requirements Analysis**
- [x] Project context thoroughly analyzed
- [x] Scale and complexity assessed
- [x] Technical constraints identified
- [x] Cross-cutting concerns mapped

**Architectural Decisions**
- [x] Critical decisions documented with versions
- [x] Technology stack fully specified
- [x] Integration patterns defined
- [x] Performance considerations addressed

**Implementation Patterns**
- [x] Naming conventions established
- [x] Structure patterns defined
- [x] Communication patterns specified
- [x] Process patterns documented

**Project Structure**
- [x] Complete directory structure defined
- [x] Component boundaries established
- [x] Integration points mapped
- [x] Requirements to structure mapping complete

### Architecture Readiness Assessment

**Overall Status:** **READY FOR IMPLEMENTATION** (all 16 checklist items `[x]`; no Critical Gaps open).

**Confidence Level:** **High** — grounded in a validated PRD, five party-mode stress-test rounds, verified crate versions, and mechanical boundary enforcement.

**Key Strengths:** the "never lies" invariant is enforced at runtime *and* by tests *and* by structure; boundaries are mechanically enforceable (not moral); the MVP is proportionately scoped with cheap high-value refinements retained; documentation is first-class with contract↔code non-drift wired in.

**Areas for Future Enhancement:** kWh-continuity + identity-oracle hardening (deferred-behind-incident), `sparkplug-b` crates.io publish, multi-arch image, runtime hot-reload of structural fields, Tier-3 re-arm automation.

### Implementation Handoff

**AI Agent Guidelines:** follow the documented decisions exactly; use the patterns consistently; respect the crate/task/secret boundaries; treat this document as the architectural source of truth.

**First Implementation Priority:** the Cargo workspace scaffolding (Starter section command), immediately followed by the `smart-me-client` + `ValueDate`/`Date`-header audit spike that unblocks the freshness formula.
