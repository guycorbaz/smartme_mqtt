---
stepsCompleted: ['step-01-init', 'step-02-discovery', 'step-02b-vision', 'step-02c-executive-summary', 'step-03-success', 'step-04-journeys', 'step-05-domain', 'step-06-innovation', 'step-07-project-type', 'step-08-scoping', 'step-09-functional', 'step-10-nonfunctional', 'step-11-polish', 'step-12-complete']
inputDocuments:
  - '_bmad-output/planning-artifacts/product-brief-smartme_mqtt.md'
  - '_bmad-output/planning-artifacts/product-brief-smartme_mqtt-distillate.md'
documentCounts:
  briefs: 1
  research: 0
  brainstorming: 0
  projectDocs: 0
classification:
  projectType: 'backend-service / integration-bridge + web-ui'
  domain: 'energy-telemetry / SCADA-integration (informative-only)'
  complexity: 'medium'
  note: 'Build complexity MEDIUM; correctness (units/counters/staleness) & 24/7 reliability treated as HIGH, non-negotiable requirements. Data is informative-only — billing is done by smart-me, not via SCADA.'
  projectContext: 'greenfield'
workflowType: 'prd'
releaseMode: 'phased'
---

# Product Requirements Document - smartme_mqtt

**Author:** Guy
**Date:** 2026-07-24

## Executive Summary

**smartme_mqtt** is a self-hosted bridge that polls the author's smart-me energy meters and republishes instantaneous power (kW) and consumed energy (kWh) onto an MQTT broker, where a SCADA/HMI (Ignition) consumes them for display. It is a **personal reliability tool**: written in Rust for a small, dependable 24/7 footprint on modest hardware (NAS / Raspberry Pi), deployed via `docker compose`, and configured, previewed, and diagnosed through a built-in web UI. It is open-sourced under **MIT** for pragmatic reasons — GitHub CI auto-builds the **Docker Hub** image the author pulls to update his own deployment — with **no adoption or market ambition**.

The problem it solves is a real gap: smart-me exposes meter data only through a **cloud REST API**, while supervision systems speak **MQTT**. Today that gap is bridged with fragile glue (Node-RED, hand-rolled REST sensors) that no one maintains and that answers unit/freshness questions inconsistently. smartme_mqtt replaces that with one dependable service whose guiding principle is **"never lies to the SCADA"**: correct, explicit units and **visible staleness** — when smart-me is unreachable, a dedicated status/staleness tag informs the SCADA rather than silently republishing a frozen value.

The v1 data source is the **smart-me cloud REST API**, reading all four of the author's meters (2× Kamstrup+smart-me module — the priority data — and 2× smart-me Telstar 80A) from a single account. The v1 publisher is **Sparkplug B** (consumed by Ignition's MQTT Engine), chosen because it natively delivers the "never lies" principle — automatic STALE-on-DEATH, source-timestamped metrics, self-describing BIRTH (units + quality). The internal design keeps a **`Source` abstraction** ahead of a canonical measurement model, retained purely as a **test seam** (inject a fake source to exercise the staleness state machine and per-meter isolation deterministically, without network) — there is a single real source, the smart-me cloud client. The publisher is a **concrete `SparkplugPublisher`** (its NBIRTH/NDEATH lifecycle are methods on that type, not a `Publisher` trait; a trait would be introduced only if a second real publisher — e.g. a plain-JSON debug output — is actually written). Local Modbus is explicitly out of scope (untestable by the author, priority meters are cloud-only). Critically, the design separates **transport liveness** (bridge↔broker) from **data freshness** (bridge↔cloud): see the two-mechanism staleness requirement below.

### What Makes This Special

- **Silent correctness as the product** — the value is not "REST→MQTT" (that's glue); it is trustworthy signal: explicit kW/kWh units, source `ValueDate` timestamps, and **native Sparkplug B quality/STALE semantics** (BIRTH certificates, DEATH-driven staleness) so a dead source or dead bridge is never shown as fresh. The bridge transports *trust*, telling the SCADA when not to believe a number.
- **Confidence in a single screen** — the web UI unifies config + live preview + diagnostics so the author sees the full chain *source → live value → MQTT destination* at a glance, with auto-discovery of meters and actionable errors, not stack traces.
- **Operationally boring, on purpose** — a compact Rust binary in a small **single-arch** container (multi-arch deferred to Growth), low idle footprint, image-based updates via Docker Hub.
- **Clear documentation as a first-class deliverable** — README, `.env` configuration reference, MQTT contract, troubleshooting guide, and update procedure — so future maintenance "avoids trouble."

## Project Classification

- **Project Type:** Backend service / integration bridge with a companion web configuration UI; containerized, self-hosted.
- **Domain:** Energy telemetry / SCADA integration — **informative-only** (billing/metering-of-record is done by smart-me, not via the SCADA).
- **Complexity:** **Medium** (build). Correctness (units, energy counters, staleness) and 24/7 reliability are treated as **HIGH, non-negotiable** requirements despite the medium build complexity.
- **Project Context:** Greenfield (new product, no existing codebase).

## Success Criteria

These criteria operationalize the Executive Summary's guiding principle, **"never lies to the SCADA"**. The single variable that defines success: **the SCADA never sees a false value dressed as true** — treated as a **runtime invariant in the code**, guarded by a small set of oracles, not merely a test suite.

### User Success (author-as-user)
- Live, correct power+energy from all 4 meters onto MQTT quickly from a clean machine — and the **meter→topic identity is proven during first setup** (serial shown next to each value), so speed never enables mis-wiring.
- **Single-screen confidence** in the web UI: per meter, live value (correct kW/kWh) + **freshness age** + exact MQTT topic + serial + "published ✓".
- Any fault (bad credentials, broker down, stale source, dead meter) is diagnosable from the UI in seconds.

### Project Success (personal objectives)
- Ignition shows correct, fresh data unattended for months; **automatic recovery** after smart-me API / broker outages without manual intervention.
- Updates painless: 1× `docker pull` + restart, **zero config loss**.
- The README lets the operator troubleshoot months later without re-reading the code.

### Technical Success — "never lies" as a runtime invariant
- **Correctness (HIGH):** power exactly **kW**, energy exactly **kWh**, values match the meter to the digit; unknown/mismatched source units are **rejected, not guessed** (publish nothing rather than a wrong value).
- **Identity binding (HIGH):** every published topic is bound to the meter's **serial number**, asserted at ingestion (invariant in code) at startup *and* periodically — **0 mis-mapping**.
- **Freshness & timestamp honesty (HIGH):** freshness is computed **end-to-end from the meter's measurement timestamp** (smart-me `ValueDate`), never from the fetch/poll time; per-meter staleness flips within a **hard bound** when a source goes silent. `ValueDate` semantics (measurement vs poll vs server time) **must be audited on a real payload**; if no reliable measurement time exists, that limitation is documented in "never lies".
- **Partial-failure isolation (HIGH):** if 1 of 4 meters goes silent, *it* flips to stale **individually** while the other 3 stay fresh.
- **Two-mechanism staleness — the load-bearing requirement (HIGH):** transport liveness ≠ data freshness. (1) **Transport death:** Sparkplug **NDEATH (via LWT) / DDEATH** propagate STALE to Ignition when the bridge *disconnects from the broker* (process crash/network) — native protocol guarantee. (2) **Application-level staleness:** when the **cloud fetch fails or a meter's data ages past a threshold while the bridge is still alive on MQTT** (the *most likely* failure — bridge UP, smart-me DOWN), the bridge must actively publish those metrics with **quality = STALE** *without* killing the node. Relying on DEATH alone would leave a live session republishing a frozen value as fresh — exactly the silent lie the project forbids.
- **Sequence integrity (HIGH):** the Sparkplug `seq` (0–255 wrap) and `bdSeq` state machine must stay synchronized (rebirth on gap; **respond to Ignition NCMD/Rebirth requests with a fresh NBIRTH**; NDEATH `bdSeq` == session NBIRTH `bdSeq`) — a desync must fail *safe* (Ignition marks STALE), never accept mislabeled metrics.
- **Reliability:** bounded backoff recovery for API + broker; growth-bounded data structures (retention caps) + `restart: unless-stopped` neutralise the leak class; **no unbounded memory/FD growth**.

### The three guarding oracles (runtime + tested)
1. **Staleness** — value with a measurement timestamp; on doubt, publish `stale/unknown`, never a number.
2. **Identity** — serial-number binding per topic; reject unknown/permuted serials.
3. **Physical bounds** — kWh monotonic non-decreasing per meter (except explicitly-detected reset); power within a crude physical envelope; ΔkWh consistent with kW×Δt. Simple bounds, not a model of the electrical network.

### Measurable Outcomes (oracle + threshold)
| Outcome | Threshold / Oracle |
|---|---|
| Unit/scale correctness | Vector tests {smart-me payload → expected MQTT value}: zero, negative (export), huge, null, missing field |
| Unknown unit | Rejected, never `0.0`, never published |
| Energy plausibility | **kWh monotonic non-decreasing** per meter (except detected reset); power within physical bounds; ΔkWh ≈ kW×Δt within tolerance |
| Identity binding | Serial asserted on every topic at startup + periodically → **0 mismatch** |
| Staleness latency | `stale=true` no later than **last_success + 2×poll_interval + publish_margin**, `publish_margin` = fetch timeout (ADR 0028) — measured on the wire at `PERIOD_MIN`, injected clock |
| Partial failure | 1/4 silent → that meter stale, other 3 fresh (asserted) |
| Death behaviour (transport) | Kill process / cut bridge↔broker → Ignition marks STALE via NDEATH/DDEATH (chaos test a) |
| Cloud-down freshness (app) | Bridge UP + smart-me unreachable → metrics published with **quality=STALE**, node stays alive, no frozen value shown fresh (chaos test b — the most frequent failure) |
| Sparkplug conformance | **Manual pre-release contract test against a real Ignition MQTT Engine** (not CI): flux accepted, correct values + units, STALE on death, **NCMD/Rebirth answered with fresh NBIRTH**. CI runs the property tests: `seq` wrap 255→0, rebirth-on-gap, `bdSeq` NDEATH==NBIRTH |
| Recovery | After API/broker outage, auto-return to fresh + rebirth, no human action (contract-level, injected clock) |
| Memory/FD | **AC-LEAK-01**: 100k-iteration loop, RSS + FD count stable via `/proc/self` (~30 s). No formal 48–72h soak gate — **production 24/7 is the soak**; FD + last-success-per-meter exposed on the health endpoint. |
| Update | 1× `docker pull` + restart, **zero config loss** (automated integration test) |

### Test strategy (proportionate to a personal 4-meter tool)
- **Tier 1 (write first, deterministic, no I/O):** unit-conversion tests, injected-clock staleness state machine, kWh monotonicity (property test), identity binding.
- **Tier 2 (mockable via traits):** contract tests against a mock cloud replaying `401/429/500/timeout/empty-body/bad-unit` → each maps to a defined internal state; per-meter partial-failure isolation.
- **Tier 2b (Sparkplug state machine — property tests):** `seq` monotonic wrap 0→255→0, rebirth-on-gap (NBIRTH re-emitted), `bdSeq` NDEATH (LWT) == session NBIRTH, DDATA never before DBIRTH, LWT/NDEATH registered *in the CONNECT packet* before connect. This is where ~90% of Sparkplug silent-lie risk lives.
- **Tier 2c (Sparkplug golden / round-trip — the CI substitute for the manual Ignition test):** field-level golden vectors (NBIRTH/NDATA/NDEATH decoded by an *independent* protobuf decoder — assert aliases, datatypes incl. energy = Double, `seq`, `bdSeq`, metric presence; **not** raw-byte comparison) + a `Measurement`→publish→decode→`Measurement` round-trip, + an invariant that **every alias in NDATA was declared in the last NBIRTH**. Catches crate / `cargo update` / rename regressions between releases without Ignition.
- **Tier 3 (contract — the deciding oracle, MANUAL pre-release gate):** a **contract test against a real Ignition MQTT Engine** (the author's own / a local dev instance) — asserts Ignition accepts the Sparkplug flow, shows correct values/units, marks tags STALE on DEATH, **and honours an Ignition-issued NCMD/Rebirth request (bridge responds with a fresh NBIRTH)**. Run **manually before each release, NOT in automated CI** (no Ignition in CI). This external human-run oracle is what makes the hand-rolled protobuf trustworthy (**non-negotiable for the Sparkplug-now decision + the public conformance guarantee for the published `sparkplug-b` crate**). Trade-off: conformance regressions are caught pre-release, not automatically per-commit.
- **Tier 4 (chaos, two distinct scenarios — both must go STALE):** (a) **STALE-on-DEATH** — kill the process / cut the bridge↔broker link → Ignition marks STALE via NDEATH (native). (b) **STALE-on-cloud-timeout** — bridge stays UP on MQTT but the smart-me cloud is unreachable/frozen → the bridge must publish **quality=STALE** on the affected metrics while the node stays alive (the *most frequent* real failure; this is the test that actually protects the SCADA from a frozen value shown as fresh).
- **Design seams required:** `trait Clock` (first-class — never `SystemTime::now()` hardcoded), `trait Source` (test seam), an inspectable per-meter state (`Fresh|Stale|Failed`), and a **minimal injectable output sink** (`Fn(MeterId, Measurement/Quality)` or channel) so bridge tests can assert "on Stale, no fresh NDATA is emitted" without encoding Sparkplug. **No `Publisher` trait** — the concrete `SparkplugPublisher` owns its NBIRTH/NDATA/NDEATH lifecycle as methods; a trait is introduced only if a second real publisher is built.
- **Fixtures-first:** first repo commit includes a real captured `fixtures/smartme_sample.json`; a checked-in Sparkplug B `.proto` (via `prost`/`build.rs`) with a small payload decoder for eyeball debugging of binary frames.
- **AC-LEAK-01** retained (100k-iteration RSS/FD check); no formal soak gate — production 24/7 is the soak.

## Product Scope

### MVP — Minimum Viable Product
- `SmartMeCloudSource` (over the pure `smart-me-client` crate), 4 meters, **dynamically configurable poll interval**.
- **Concrete `SparkplugPublisher`** (v1, the only publisher): one EON node, a device per meter; metrics **power (kW)** and **energy (kWh)** with engineering units, measurement timestamp, and serial-bound identity; NBIRTH/DBIRTH on connect, NDEATH (LWT)/DDEATH on transport death, **application-level quality=STALE when the cloud fetch fails while the node stays alive**, `seq`/`bdSeq` + rebirth-on-gap, **responds to Ignition NCMD/Rebirth**. Ignition consumes via MQTT Engine (Sparkplug mode).
- **Two-mechanism staleness** wired in the publish path (transport DEATH **+** app-level quality=STALE) — the load-bearing "never lies" guarantee.
- *(Deferred, not v1)* a plain-JSON debug publisher; if built, it introduces the `Publisher` trait at that point.
- **Web UI**: config + live preview + diagnostics (unified screen). The **preview decodes the Sparkplug metrics + their quality (GOOD/STALE)** — not raw protobuf, and no leftover JSON model. Meter auto-discovery, actionable errors; a **health endpoint** exposing per-meter last-success + FD/memory.
- **`.env` credentials** (never logged, never re-shown in clear).
- **Daily-rotated logs** (configurable level/retention).
- **`docker compose`** + **single-arch image on Docker Hub** (target architecture picked for the author's host; multi-arch deferred) via GitHub CI.
- **README + config reference + MQTT contract + troubleshooting + update procedure.**
- **MIT license.**
- **Test harness:** Tier 1 (unit + injected-clock staleness) + Tier 2 (mock cloud) + Tier 2b (Sparkplug seq/bdSeq/rebirth property tests) + **Tier 3 contract test vs real Ignition (manual pre-release, not CI)** + Tier 4 chaos (STALE-on-DEATH + STALE-on-cloud-timeout) + AC-LEAK-01; fixtures + Sparkplug `.proto` committed first.
- **Runtime guarding invariant:** the three oracles (staleness, identity, physical bounds) live in the publish path.

### Growth Features (Post-MVP)
- User-templatable topic scheme.
- Optional formal soak / chaos exercises; multi-arch image.

*(Dropped: local Modbus TCP source — untestable by the author, needs a Pro subscription, and the priority Kamstrup meters are cloud-only. The `Source` trait is retained solely as a test seam, not for multi-source.)*

### Vision (Future)
- Additional cloud meter back-ends behind the `Source` trait.
- smart-me **Realtime/Webhook** (protobuf push) ingestion.
- Home Assistant MQTT discovery / plain-JSON publisher promoted from debug to a supported second output (if ever wanted alongside Sparkplug).

## User Journeys

The sole human user is the author (Guy) across roles; the SCADA (Ignition) is a non-human consumer. Every journey carries a failure beat — *detect → understand → repair* — because the product exists to dissolve the anxiety of not knowing whether the numbers can be trusted. Guiding rule: **the web UI must reflect the truth the SCADA receives, not the truth the bridge hopes for** — every green indicator proves the final link (the message accepted for transmission, correct topic, correct identity).

### Journey 1 — First Run (Guy, the installer)
**Happy path.** Guy copies the example `docker-compose.yml`, fills `.env` (smart-me credentials, broker), runs `docker compose up -d`. The container logs `Web UI ready → http://<host>:PORT`. He clicks **"Test connection"**; the UI auto-discovers all 4 meters by name + serial, shows per-meter live value (`3.42 kW`, `12876.5 kWh`), freshness age, and the exact MQTT topic — serial beside each so he can't cross-wire.
**Failure beats:** empty/partial `.env` → an explicit **"no credentials" empty-config state**, visually distinct from *error* and *loading* (never an infinite spinner pretending to search). Wrong/expired key → **named error taxonomy** (401 auth / 403 perms / timeout / 200-but-empty-list) with the repair gesture — **never collapse an auth failure into "0 meters found"**. Before publishing, a **mapping confirmation** ("are these 4 meter→topic mappings correct?") — the only guard against a mis-map the machine cannot detect.
*Reveals:* example compose, `.env` config, connection test, meter auto-discovery, empty-config state, error taxonomy, live preview, default+editable topic mapping, first-run mapping confirmation.

### Journey 2 — A meter goes silent (Guy, the troubleshooter)
**Detect.** An Ignition trend for one Kamstrup flatlines; the diagnostics screen shows 3 meters green ("last success 8 s ago") and the fourth **stale** ("last success 43 min ago") with the last error. **Understand.** The MQTT status tag already flipped to `stale`, so Ignition was never fooled; the smart-me portal shows that meter lost WiFi — not the bridge's fault, and the bridge told the truth. **Repair.** He fixes the meter; the tag returns to fresh automatically, no restart.
*Reveals:* per-meter diagnostics (last success, last error), per-meter staleness isolation on MQTT, actionable errors, automatic recovery.

### Journey 3 — Updating the bridge (Guy, the maintainer)
**Happy path.** CI publishes a new Docker Hub image; Guy runs `docker compose pull && docker compose up -d`; same `.env` + config volume re-read; meters reappear within one poll cycle; LWT flips offline→online.
**Failure beats:** a bad new image or broken config → the documented **update procedure includes a rollback point and a post-update verification signal**. Config change at runtime (topic edit) → the UI **previews the consequence before commit** ("this publishes on X and abandons Y; the last retained value on Y will linger — purge?") to avoid an orphan retained ghost.
*Reveals:* image-based update, config persistence across restarts, documented update+rollback procedure, LWT continuity, config-change consequence preview.

### Journey 4 — The SCADA consumer (Ignition) · machine journey
On connect, the bridge emits **NBIRTH/DBIRTH** (self-describing metrics: units, serial, source timestamp) and Ignition auto-discovers the meters as tags with correct engineering units. **NDATA/DDATA** carry updates on every poll, changed or not — the bridge does **not** implement report-by-exception. It *could not safely* do so until it answered NCMD/Rebirth, since a late-joining consumer would otherwise never learn the value of a meter whose reading never changes; **that blocker lifted when Stories 4.6–4.7 landed (2026-07-30)** — a consumer can now ask for a fresh birth and be answered. RBE is still not implemented, and is now an open decision rather than a foreclosed one ([#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)); the residual question is that the repair is host-initiated, so a consumer that never asks still never learns. On bridge or device death, **NDEATH (via LWT) / DDEATH** make Ignition mark the affected tags **STALE automatically** — the native protocol guarantee behind "never lies".
**Failure beat (source vs sink):** bridge up + broker unreachable at boot = fresh data from the API but nothing published. The UI must expose **two independent healths — "source (smart-me)" and "sink (MQTT)"** — and **"published ✓" appears only once the NDATA has been accepted for transmission**, never on API read; reconnection triggers a **rebirth (NBIRTH)** and backoff is visible and timestamped. *(Corrected 2026-07-28 — read "broker ACK" until then, which FR20's own amendment had already ruled out: Sparkplug mandates QoS 0, at which no acknowledgement exists. As written it was an unimplementable UI requirement — see ADR 0010 and #33.)*
*Reveals:* Sparkplug BIRTH/DATA/DEATH lifecycle, auto-discovery + engineering units, native STALE-on-DEATH, dual source/sink health, publish confirmation bounded by what QoS 0 can actually prove, versioned Sparkplug/MQTT contract.

### Journey 5 — The Cold Reopening (future-Guy, the returning stranger)
**Opening.** Guy returns after eight months, tired, at 3am. Ignition shows nothing. A meter may have moved houses; the broker IP may have changed; or nothing changed and he simply forgot how his own tool breathes. His need is not information — it's **re-orientation** ("tell me what you believe is true so I can trust or contradict you").
**Detect→understand.** A single **"state of the bridge" screen** answers without him knowing what to ask: per meter, last value + **human timestamp** ("3 min ago" / "6 days ago ⚠️"), the **exact broken link** in the chain (`smart-me ✓ → parse ✓ → MQTT publish ✗ (broker refused since 14/03)`), a **culprit label** distinguishing *world* ("smart-me not responding") vs *you* ("token expired") vs *bridge* ("never managed to publish"), and an **auto-written context line** ("configured on X, last change Y, 4 meters, 2 priority Kamstrup").
**Repair.** He acts on the named culprit, and a **health timeline** (not a snapshot) answers the true 3am question — *since when?*
*Reveals:* health/state screen, per-meter health timeline, broken-link localization, culprit classification (world/you/bridge), auto-written config context line.

### Journey Requirements Summary
- **Onboarding/config:** example compose, `.env` config + persistent volume, connection test, meter auto-discovery, explicit **empty-config state**, **error taxonomy** (401/403/timeout/empty-list), default+editable topic mapping, **first-run mapping confirmation**, identity (serial) display.
- **Publishing/contract:** Sparkplug B lifecycle (NBIRTH/DBIRTH/NDATA/DDATA/NDEATH/DDEATH), serial-bound identity, explicit kW/kWh units, source timestamp, per-meter quality (GOOD/STALE) via transport DEATH + app-level staleness, **publish confirmed only once accepted for transmission** *(corrected 2026-07-28, per FR20 and ADR 0010 — no ACK exists at QoS 0)*.
- **Observability/diagnostics:** unified web UI (config+preview+diagnostics), **dual source/sink health**, per-meter last-success/last-error, **state screen with health timeline + broken-link localization + culprit classification + auto-written context line**, health endpoint, daily-rotated logs.
- **Reliability:** automatic recovery (bounded backoff, visible + timestamped), per-meter staleness isolation, config persistence across restarts, **config-change consequence preview**.
- **Lifecycle:** image-based update via Docker Hub, documented **update + rollback** procedure with post-update verification.
- **Documentation (first-class):** README (incl. success-verification signal), **Config Reference** (var · required · format · example · default · effect-if-missing), **standalone versioned MQTT Contract** (topic grammar + closed meter/metric lists, topic→Ignition-tag table, payload spec, QoS/retain/LWT, freshness/quality semantics, poll cadence), **Troubleshooting guide** (≥8 failure modes: meter silent, auth 401, rate-limit 429, broker unreachable, single-meter vs global, priority-Kamstrup partial, stale-but-alive, clock/DST, Docker restart/retain — each as Symptom → Cause → Action → **Confirmation**), Update Procedure, and **one annotated data-flow diagram** (meter → bridge → broker → Ignition with failure points pinned).

## Domain-Specific Requirements

*No regulatory regime applies (informative-only energy telemetry; not grid control, not safety-critical — NERC CIP / IEC 62443 / functional safety out of scope). The constraints below are domain-technical conventions, not compliance.*

### Technical Constraints (energy telemetry / OT-adjacent)
- **Engineering-unit discipline:** publish SI-consistent, explicit units (power **kW**, energy **kWh**); the unit travels with the value; unknown units are rejected, not guessed.
- **Cumulative-counter semantics:** energy is a **monotonic accumulator**; consumers compute deltas. Counter reset/rollover must be detected and never propagated as a silent regression.
- **Data quality/freshness signalling (SCADA norm):** every value carries a source timestamp and a per-meter quality/staleness status, so the SCADA can distinguish *fresh*, *stale*, and *bridge-dead* (LWT) — the OT-native equivalent of a tag "quality" flag.
- **Current-state on (re)connect:** Sparkplug BIRTH certificates convey each metric's current value, units, and quality to a (re)connecting Ignition host; STALE-on-DEATH plus app-level quality prevent a cached value from reading as fresh.

### Integration Requirements
- **Upstream:** smart-me **cloud REST API** only, single account, 4 meters. Single source; the `Source` trait is a test seam, not a multi-source abstraction (local Modbus dropped).
- **Downstream:** **Sparkplug B** over MQTT, consumed by **Ignition** (MQTT Engine, Sparkplug mode) — chosen for native quality/STALE/auto-discovery; a **versioned MQTT/Sparkplug Contract** document is the integration interface of record. A plain-JSON debug publisher, if ever built, would (re)introduce a `Publisher` trait at that point (v1 has none).

### Security / Exposure Posture (self-hosted, trusted-network assumption)
- Credentials in `.env`, **never logged**, never re-shown in clear.
- The config web UI is an exposure surface: **default bind and auth posture must be decided** (loopback-by-default vs documented "trusted network only") — deferred to the architecture step, flagged here as a domain constraint.
- Threat model stated explicitly (assume trusted host/LAN) rather than left tacit.

### Risk Mitigations (domain-specific)
- **Silent wrong value** (unit factor, mis-mapped identity) → runtime oracles (units, serial identity, physical bounds) + human mapping confirmation at first run.
- **Silent stale value** (cloud frozen while the bridge is alive) → **application-level quality=STALE** (LWT/NDEATH covers only transport/process death).
- **No regulatory/billing exposure** — reinforced by an **explicit non-goal**: this data is informative-only and must not be used as metering-of-record.

## Integration Bridge — Technical Requirements

A cloud-API-client + Sparkplug-B-publisher service with a small embedded web UI. It exposes **no public API** (no SDK, no versioning surface) beyond the config web UI and a health endpoint. The contracts that matter are **inbound** (smart-me REST) and **outbound** (Sparkplug B to Ignition).

### Workspace (3 crates)
- `crates/sparkplug-b` — **pure, generic Sparkplug B library** (protobuf via `prost`, EON-node/device model, `seq`/`bdSeq`/rebirth + NCMD-rebirth state machine, NBIRTH/DBIRTH/NDATA/DDATA/NDEATH/DDEATH, LWT). **Zero smartme dependency.** The author **intends to publish it to crates.io** → higher bar for this crate only: stable documented API, semver, README+CHANGELOG, no third-party types leaked, clean error types, `#![forbid(unsafe_code)]`.
- `crates/smart-me-client` — **pure smart-me REST client** (auth, endpoints, deserialization to smart-me domain types). Zero bridge dependency; heavy deps (`reqwest`/`serde`) isolated so its tests don't rebuild the MQTT stack.
- `crates/smartme-bridge` — the app: canonical `Measurement` + `Source`/`Clock` (modules), web UI, config, wiring, and the adapters `SmartMeCloudSource: Source` and the concrete `SparkplugPublisher`.
- Dependency direction enforced by the Cargo graph + `cargo-deny`. The `Measurement → Sparkplug metric` mapping **and** the error→culprit (world/you/bridge) classification live in the **bridge**; `sparkplug-b` and `smart-me-client` expose only pure typed errors and never know the bridge.

### Upstream: smart-me REST client
- **Auth:** API Key (`Authorization: ApiKey <key>`) primary; HTTP Basic fallback; TLS mandatory (hard-fail otherwise).
- **Endpoints:** `GET /Devices/` (discovery), `GET /Devices/{id}` (state). Base `https://api.smart-me.com/`.
- **Fields:** `ActivePower`/`ActivePowerUnit` (→ kW), `CounterReading`/`CounterReadingUnit` (→ kWh), `ValueDate` (**measurement timestamp — semantics to audit on a real payload**), `Serial`, `Id`, `Name`.
- **Poll interval:** runtime-configurable.
- **Rate limits (unknown):** configurable interval + bounded exponential backoff with jitter; honor `Retry-After` on 429.
- **Error classification:** `401/403` → stop + `auth_error` (no retry loop); `429` → back off; `5xx`/timeout → bounded backoff; `200` empty/partial/unknown-unit → reject, publish nothing fresh, mark **quality=STALE**.
- **Fixture-first:** a real captured `fixtures/smartme_sample.json` is the parsing/correctness contract-of-record.

### Downstream: Sparkplug B publisher (concrete `SparkplugPublisher`)
- One EON node, a device per meter; metrics **power (kW)** + **energy (kWh)** with engineering units, measurement timestamp, serial-bound identity.
- Lifecycle: NBIRTH/DBIRTH on connect; NDATA/DDATA on every poll, changed or not — **not report-by-exception**; NDEATH (LWT)/DDEATH on transport death; **responds to Ignition NCMD/Rebirth** with a fresh NBIRTH; monotonic `seq`/`bdSeq` with rebirth-on-gap. *(Corrected 2026-07-28 — this line claimed report-by-exception, which the bridge has never done; no FR required it. See `tck-id-principles-rbe-recommended` and #32.)*
- **Two-mechanism staleness (load-bearing):** transport DEATH **+** application-level `quality=STALE` when the cloud is unreachable while the node stays alive.
- Consumed by Ignition MQTT Engine (Sparkplug mode). A **versioned Sparkplug/MQTT Contract** doc is the integration interface of record.

### Internal seams (enable tests)
- `trait Clock` (first-class, injected — never `SystemTime::now()` hardcoded).
- `trait Source` (test seam only — single real `SmartMeCloudSource` + fakes in tests for the staleness state machine & per-meter isolation).
- Inspectable per-meter state `Fresh | Stale | Failed`; a minimal injectable output sink for bridge-level "on Stale → no fresh NDATA" assertions.
- **No `Publisher` trait** in v1 (Sparkplug is the only publisher).

### Web UI / health (minimal HTTP surface, `axum`)
- Config + live preview + diagnostics + state screen; the **preview decodes Sparkplug metrics + quality (GOOD/STALE)**.
- Health endpoint exposing per-meter last-success + FD/memory.
- **Config boundary:** `.env` holds secrets (immutable at runtime, never touched by the UI, never logged); the UI persists only non-secret config (meter→topic mapping, poll interval, broker settings) to a separate config file it can re-read at runtime.
- **Bind/auth posture deferred to architecture** (loopback-default vs documented trusted-network).

*Skipped (per project type): public API endpoint specs, external versioning, SDK, visual/branding design.*

### Open items for the Architecture step
- **[BLOCKING — resolve early]** Audit smart-me `ValueDate` timestamp semantics on a real payload (measurement vs poll vs server time) **before the staleness mechanism is fixed** — the whole quality=STALE oracle depends on it.
- Web UI network bind + auth posture (see Config boundary above).
- crates.io / Docker Hub release pipeline (later, once the project works).
- Log retention window (N days) — a 24/7 tool must not fill the disk.

## Project Scoping & Phased Development

### MVP Strategy & Philosophy
**Approach:** *Problem-solving MVP* — the smallest thing that puts correct, freshness-qualified Kamstrup+Telstar data into the author's own Ignition, honestly. Success = the SCADA trusts the numbers, unattended, for months. No market/learning MVP (sole user, no adoption goal).
**Resource reality:** solo developer, evenings; Rust for reliability; effort deliberately bounded by the "personal 4-meter tool" yardstick (rigor spent only where a *silent lie* is possible).

### MVP Feature Set (Phase 1) — must-haves
Core journeys supported: First Run, Meter Goes Silent, Updating the Bridge, SCADA Consumer, Cold Reopening (all five — each is a real operating mode for the author).
Must-have capabilities (without any one, the product fails its "never lies" purpose):
- `smart-me-client` cloud source, 4 meters, dynamic poll interval, error classification.
- `SparkplugPublisher` (kW/kWh, engineering units, serial identity, measurement timestamp) + **two-mechanism staleness** (NDEATH + app-level quality=STALE) + NCMD/Rebirth.
- Web UI: config + preview (Sparkplug-decoded + quality) + diagnostics + state screen; health endpoint.
- `.env` credentials (never logged); daily-rotated logs.
- `docker compose` + single-arch Docker Hub image via CI; **MIT**.
- Documentation set (README, config ref, versioned Sparkplug/MQTT contract, troubleshooting, update).
- Test harness incl. the **Ignition contract test** (deciding oracle, manual pre-release) + fixtures.
- Published `sparkplug-b` crate (bar: semver/docs/README).

### Post-MVP
- **Phase 2 (Growth):** user-templatable topic scheme; **runtime poll-interval hot-reload** (v1 = edit `.env` + restart); optional formal soak/chaos exercises; multi-arch image.
- **Phase 3 (Vision):** additional cloud back-ends behind `Source`; smart-me webhook ingestion; JSON debug publisher (introduces a `Publisher` trait if ever built).
- **Dropped (not phased):** local Modbus source.

### Risk Mitigation Strategy
- **Technical Risks:** the hand-rolled Sparkplug state machine (seq/bdSeq/rebirth) is the riskiest assumption → property tests (CI) + **manual pre-release contract test vs the author's own Ignition** (feasible; no Ignition in CI); fail-safe by design (desync → STALE, never a mislabeled value). Correctness risks (unit factor, mis-mapped identity, stale-but-alive) → three runtime oracles + first-run mapping confirmation + dual staleness + timestamp audit. Unknown `ValueDate` semantics → audit on a real payload before locking.
- **Market Risks:** none — sole user, no adoption ambition.
- **Resource Risks:** scope bounded to stay solo-maintainable; production 24/7 is the soak; formal soak/chaos deferred to Growth.

## Functional Requirements

*Capability contract. Anything not listed here will not be built.*

### Meter Data Acquisition
- **FR1:** The bridge can connect to the smart-me cloud account using operator-provided credentials (API key, Basic-auth fallback).
- **FR2:** The bridge can discover the account's meters, identified by name and serial number.
- **FR3:** The bridge can read each meter's instantaneous power and cumulative energy on a configurable interval.
- **FR4:** The bridge can recover automatically from transient source failures (timeout, 5xx, rate-limit) with bounded backoff.
- **FR5:** The bridge can distinguish permanent auth failures (stop + surface) from transient ones (retry).
- **FR6:** The bridge can handle a known meter that disappears from discovery — mark it stale/absent, never a silent disappearance.

### Data Integrity & Trust ("never lies")
- **FR7:** The bridge can publish power in kW and energy in kWh with the unit explicitly attached to each value.
- **FR8:** The bridge can reject readings with unknown/mismatched source units rather than publishing a guessed value.
- **FR9:** The bridge can bind each published value to its meter's **immutable serial number** and verify that binding — if the API-returned serial differs from the bound serial, the value is marked quality BAD and not published; a meter name change never re-attributes a topic.
- **FR10:** The bridge can attach the meter's measurement timestamp to each value, treat all timestamps as **UTC end-to-end**, and flag abnormal source clock skew.
- **FR11:** The bridge can detect a meter's data going stale (aged past a configurable threshold, default 2× poll interval) and mark that value's quality as stale — *even while still connected to the broker*.
- **FR12:** The bridge can signal staleness per meter independently (one silent meter doesn't affect the others).
- **FR13:** The bridge can signal to the SCADA when the bridge itself is no longer alive.
- **FR14:** The bridge can flag instantaneous values outside plausible physical bounds rather than propagating them silently.
- **FR15:** The bridge can detect energy-counter non-monotonicity (reset / rollover / meter replacement), mark the quality, and never publish a negative delta as a valid measurement.
- **FR16:** The bridge can validate the completeness and numeric domain of each smart-me payload before publishing; a missing/null/NaN field, or a value outside per-metric min/max bounds, yields degraded quality, never a substituted value.
- **FR45:** The bridge can encode cumulative energy as a 64-bit double (never float32), preserving full kWh resolution up to at least 10⁷ kWh (a float32 counter silently loses the last kWh near a million — a precision lie).

### SCADA Publishing
- **FR17:** The bridge can publish meter data to an MQTT broker in a form Ignition consumes as tags (Sparkplug B).
- **FR18:** The SCADA can auto-discover the meters and their engineering units from the bridge's published metadata.
- **FR19:** The bridge can respond to a SCADA-initiated rebirth request by re-announcing its metrics.
- **FR20:** The bridge never over-claims delivery: a value is reported as published only once it has been accepted for transmission, and a value it could not hand over yields a per-device traced drop rather than silence. *(Amended 2026-07-26 — see ADR 0010: the original wording required a broker ACK, which the Sparkplug-mandated QoS 0 makes impossible by protocol.)*
- **FR21:** The bridge can purge orphan retained messages on old topics when a mapping changes (no ghost values).
- **FR22:** The bridge can apply a defined policy to readings acquired during a broker outage — bounded buffer preserving the source timestamp, or a traced drop; never a re-timestamped replay.

### Configuration
- **FR23:** The operator can provide **the smart-me credential** via environment/`.env` — `SMARTME_CLIENT_ID` and `SMARTME_CLIENT_SECRET`, one pair, and the only settings the environment carries besides `SMARTME_STATE_DIR`. *(**Rescoped a second time, 2026-08-04, [ADR 0023](../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md).** It previously covered broker details too, and claimed the environment path "must remain sufficient on its own: a bridge whose configuration can only be completed through a browser cannot be brought up headless". That claim does not survive ADR 0023 — but the need behind it does, and is met better: **a headless bring-up writes `config.toml` by hand**, a documented file with a versioned schema, rather than eleven environment variables.)*
- **FR46:** The operator can **change and persist configuration from the web UI** — the meter mapping, the publish period, the broker details, the log directory and retention — without editing a file or restarting the container. *(Added 2026-08-03, [ADR 0021](../../docs/adr/0021-configuration-is-editable-from-the-ui.md). The PRD's own product description has said since day one that the bridge is "configured, previewed, and diagnosed through a built-in web UI" and Journey 1 has the operator clicking "Test connection", but **no FR said it** — so the requirement list, which is what coverage is measured against, described a `.env`-only product. The publish period is bounded by [ADR 0020](../../docs/adr/0020-the-publish-period-is-bounded-and-cannot-be-turned-off.md).)*
  *(**Amended 2026-08-04, [ADR 0023](../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md): "and the smart-me credentials" is WITHDRAWN.** No secret is submitted through the UI, so **NFR12** — which said all along that credentials live only in `.env`/env vars — is restored rather than excepted. [ADR 0019](../../docs/adr/0019-no-auth-on-the-config-ui-secrets-are-write-only.md)'s write-only rule loses its subject: its *never rendered* clause survives as a guard, not as a feature.)*
- **FR24:** The operator can configure the meter→topic/tag mapping, with sensible defaults.
- **FR25:** The operator can confirm the meter→topic mapping before data is published (first-run confirmation).
- **FR26:** The bridge can validate the full configuration at startup (topic uniqueness, well-formed serials, completeness) and **refuse to publish** on invalid config rather than start partially — opening no session, emitting no birth, and saying so on the configuration screen. *(**Amended 2026-08-06, [ADR 0026](../../docs/adr/0026-a-configuration-it-cannot-use-stops-the-bridge-publishing-not-serving.md), [#57](https://github.com/guycorbaz/smartme_mqtt/issues/57).** It read "refuse to **start**", and the process exited before the web server was spawned — so the screen that repairs a configuration was behind the refusal, and the commonest first-run mistake, a state directory nobody `chown`ed, became a restart loop with no browser path out. The harm this requirement names is "rather than start partially", and that harm requires having settings and using them; here there are none and nothing is published. The validation, the fault set and the wording of every fault are unchanged.)*
- **FR27:** The bridge can persist configuration across restarts and image updates.

### Observability & Diagnostics
- **FR28:** The operator can view, in one place, each meter's live value, unit, freshness age, target topic, serial, and published status.
- **FR29:** The operator can see the independent health of the source (smart-me) and the sink (MQTT broker), from a single internal source of truth on source/sink/bridge state.
- **FR30:** The operator can see, per meter, the last successful read, the last error, and where in the chain a failure occurred.
- **FR31:** The operator can see actionable error messages (auth vs permissions vs timeout vs empty result), not stack traces.
- **FR32:** The operator can distinguish an empty/unconfigured state from an error state from a loading state.
- **FR33:** The bridge can expose a health/status endpoint (per-meter last-success, resource usage) consumable by the Docker healthcheck.
- **FR34:** The operator can see a culprit label (world / you / bridge) on each fault, derived from the error nature and source-vs-sink health.
- **FR35:** The operator can see an auto-written, human-readable, timestamped configuration context line (created, last change, meter count, priority meters) on the state screen.
- **FR36:** The operator can open a "state of the bridge" orientation screen — a multi-meter overview with human-readable timestamps (absolute + relative).
- **FR37:** The operator can trigger an on-demand end-to-end validation for a chosen meter (source → value → sink) and see the three links light up on one screen.

### Logging
- **FR38:** The bridge can write daily-rotated log files at a configurable level **and configurable retention (N days)**, never recording secrets.

### Lifecycle & Deployment
- **FR39:** The operator can deploy and start the whole system via `docker compose`.
- **FR40:** The operator can update the bridge by pulling a new image without losing configuration.
- **FR41:** The operator can follow a documented update procedure with a rollback point and post-update verification.

### Documentation
- **FR42:** The operator can rely on a documentation set (README, config reference, versioned Sparkplug/MQTT contract, troubleshooting, update) sufficient to install and troubleshoot without reading the code.

### Broker & Versioning
- **FR43:** The operator can run the bridge against either a **bundled MQTT broker** (e.g. Mosquitto, in the compose stack) or an **external broker**, with the broker connection **optionally secured** (TLS and/or authentication) or plain, per configuration.
- **FR44:** The operator can see the **running application/image version** in the web UI and on the health endpoint.

## Non-Functional Requirements

*Selective — only categories that matter for this product. Quality attributes; the measurable oracles are in Success Criteria.*

### Reliability & Availability
- **NFR1:** Runs unattended for weeks; automatic recovery from smart-me API and MQTT broker outages without manual restart (bounded exponential backoff + jitter, e.g. 1 s → 60 s cap).
- **NFR2:** Per-meter staleness signalled no later than `last_success + 2×poll_interval + publish_margin`, where **`publish_margin` = the per-fetch timeout** ([ADR 0028](../../docs/adr/0028-publish-margin-is-the-fetch-timeout.md), 2026-08-08). The term had never had a value: it appeared only inside this formula, so the bound could be quoted and not met or missed. It is derived rather than chosen — the binding case is `PERIOD_MIN`, not the default period, where any margin at all would do. Measured on the wire at `PERIOD_MIN` by story 3.3; the ceiling is deliberately looser than the latency the bridge achieves (`last_success + poll_interval + fetch_timeout` since ADR 0027 made one missed tick enough), so that a regression has something to fail against.
- **NFR3:** No unbounded memory/FD growth — **RSS_max ≤ 100 MB** on target; **RSS slope ≤ 1 %/24 h** by linear regression on RSS sampled every 60 s; **FD ≤ 64** via `/proc/self/fd`.
- **NFR4:** Availability is best-effort; during a smart-me outage the system stays honest (quality=STALE) rather than available — integrity is never traded for availability.

### Data Integrity & Correctness ("never lies")
- **NFR5:** Units exactly kW/kWh, values match the meter to the digit — 0 unit/scale errors.
- **NFR6:** Energy counters monotonic non-decreasing except on detected reset — 0 negative deltas published as valid.
  - *Residual recorded 2026-08-11 ([#67](https://github.com/guycorbaz/smartme_mqtt/issues/67)):* after a reset the published sequence is `Good(4843.822) → Bad(null) → Good(12.5)`, so a consumer differencing the two **valid** measurements either side of the refusal still obtains a negative delta. Story 2.2 AC3 mandates that behaviour — latching instead would take a working meter off the wire for an event already past — so it is not an AC violation, but the letter of this NFR does not admit it. To be settled at Epic 2's close: amend the wording to what the bridge guarantees (no negative delta between two consecutive valid measurements with no refusal between them), or close the gap on the wire.
- **NFR7:** 0 mislabeled-identity values (serial-bound, verified at startup + periodically).
- **NFR8:** No value ever presented as fresh when its measurement timestamp exceeds the staleness threshold (dual-mechanism staleness).

### Performance & Footprint
- **NFR9:** Idle CPU/RAM low enough to co-exist on a Raspberry Pi / NAS (RSS_max target < ~100 MB).
- **NFR10:** A new reading reaches MQTT within one poll cycle; read→broker-ACK latency **p95 ≤ 3 s, p99 ≤ 5 s** over a 24 h window under nominal load (no throughput requirement — 4 meters).
- **NFR11:** Time-to-first-value < 15 min from a clean machine, with identity binding proven within it.

### Security
- **NFR12:** Credentials only in `.env`/env vars, perms 0600, never in the image, never logged (incl. rotated logs) — log-grep test.
- **NFR13:** All smart-me traffic over TLS; hard-fail if unavailable (esp. Basic-auth fallback).
- **NFR14:** Web UI exposure safe-by-default (bind/auth posture decided in architecture); credentials never re-shown in clear.
- **NFR15:** Explicit non-goal / threat model stated: informative-only (not metering-of-record); trusted-host/LAN assumption documented.
- **NFR16:** The broker connection may be secured (TLS and/or auth) or plain per config; when secured, broker credentials follow the same discipline as smart-me creds (never logged).

### Interoperability & Deployment
- **NFR17:** Sparkplug B output conforms to what Ignition MQTT Engine accepts — verified by a **manual pre-release contract test against a real Ignition** (values, units, STALE-on-death, NCMD/Rebirth). Not in automated CI (no Ignition in CI).
- **NFR18:** The Sparkplug/MQTT contract is a standalone versioned document; a breaking change bumps the contract version.
- **NFR19:** The published `sparkplug-b` crate follows semver with a stable, documented public API (no third-party types leaked), complete crate metadata (`license`/`description`/`repository`), and a **documented conformance scope** — it implements the Sparkplug B subset this project uses, stated as such in the README (not a claim of full-spec conformance). Publish acceptance: `cargo publish` succeeds and `cargo add sparkplug-b` in a clean project compiles an encode→decode round-trip.
- **NFR20:** The bridge works with either a bundled MQTT broker (e.g. Mosquitto) or an external broker.

### Maintainability & Operability
- **NFR21:** Single-arch Docker Hub image with a Docker healthcheck; image-based updates preserve config (zero-config-loss integration test).
- **NFR22:** Test seams (`Clock`, `Source`, injectable sink) enable deterministic tests without network; property/mock-contract/chaos tests run in CI; the Ignition contract test is a manual pre-release gate; `cargo-deny` in CI enforces dependency direction **and licenses**.
- **NFR23:** Documentation sufficient for the author to install, operate, and troubleshoot without reading the code.

### Operating Assumptions
- **NFR24:** Relies on an NTP-synchronized host clock (UTC); freshness/staleness guarantees assume a correct host clock.

### Deliberately excluded (not applicable)
- **Scalability:** fixed at 4 meters / single account; no growth dimension (multi-account = multiple instances).
- **Accessibility:** single private operator; no public-audience or regulatory accessibility requirement.

## Assumptions & Dependencies

*Environmental conditions the "never lies" guarantee depends on. If one breaks, it must surface to the SCADA (quality=STALE), never be absorbed silently.*
- **Active smart-me account/subscription** with valid credentials; on lapse the API stalls or returns stale → surfaced as quality=STALE.
- **Ignition MQTT Engine (Cirrus Link) in Sparkplug mode**, licensed edition, available on the receiving side — without it there is no sink.
- **All 4 meters remain on one smart-me account** (single-source, single-account model). Multi-account is a declared out-of-scope boundary (run multiple instances).
- **smart-me REST API contract stability** — a single unversioned upstream; the manual pre-release contract test + the CI golden/round-trip tests (Tier 2c) are the sentinels for schema drift.
- **Author availability for the manual pre-release Ignition contract test.** Noted tension: the *Cold Reopening* journey assumes long absence while the manual oracle assumes periodic presence — the **CI golden/round-trip test (Tier 2c) resolves this** by catching Sparkplug regressions without the author, leaving the manual test as a per-release confirmation only.
- **NTP-synchronized host clock (UTC)** — see NFR24.
