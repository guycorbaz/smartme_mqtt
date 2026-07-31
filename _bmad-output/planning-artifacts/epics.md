---
stepsCompleted: ['step-01-validate-prerequisites', 'step-02-design-epics']
storyCreationMode: 'just-in-time'
storiesDetailed: ['epic-0', 'epic-1', 'epic-4']
implemented: ['epic-0']  # all 8 stories green: fmt + clippy -D warnings + 11 test-bins + cargo-deny
inputDocuments:
  - '_bmad-output/planning-artifacts/prd.md'
  - '_bmad-output/planning-artifacts/architecture.md'
  - '_bmad-output/planning-artifacts/product-brief-smartme_mqtt.md'
  - '_bmad-output/planning-artifacts/product-brief-smartme_mqtt-distillate.md'
  - '_bmad-output/planning-artifacts/prd-validation-report.md'
---

# smartme_mqtt - Epic Breakdown

## Overview

This document provides the complete epic and story breakdown for smartme_mqtt, decomposing the requirements from the PRD and Architecture into implementable stories. No separate UX Design document exists; UI requirements are carried by the PRD (FR28–FR37) and the Architecture "Observability — legible honesty" section.

## Requirements Inventory

### Functional Requirements

**Meter Data Acquisition**
- FR1: The bridge can connect to the smart-me cloud account using operator-provided credentials (API key, Basic-auth fallback).
- FR2: The bridge can discover the account's meters, identified by name and serial number.
- FR3: The bridge can read each meter's instantaneous power and cumulative energy on a configurable interval.
- FR4: The bridge can recover automatically from transient source failures (timeout, 5xx, rate-limit) with bounded backoff.
- FR5: The bridge can distinguish permanent auth failures (stop + surface) from transient ones (retry).
- FR6: The bridge can handle a known meter that disappears from discovery — mark it stale/absent, never a silent disappearance.

**Data Integrity & Trust ("never lies")**
- FR7: The bridge can publish power in kW and energy in kWh with the unit explicitly attached to each value.
- FR8: The bridge can reject readings with unknown/mismatched source units rather than publishing a guessed value.
- FR9: The bridge can bind each published value to its meter's immutable serial number and verify that binding — if the API-returned serial differs from the bound serial, the value is marked quality BAD and not published; a meter name change never re-attributes a topic.
- FR10: The bridge can attach the meter's measurement timestamp to each value, treat all timestamps as UTC end-to-end, and flag abnormal source clock skew.
- FR11: The bridge can detect a meter's data going stale (aged past a configurable threshold, default 2× poll interval) and mark that value's quality as stale — even while still connected to the broker.
- FR12: The bridge can signal staleness per meter independently (one silent meter doesn't affect the others).
- FR13: The bridge can signal to the SCADA when the bridge itself is no longer alive.
- FR14: The bridge can flag instantaneous values outside plausible physical bounds rather than propagating them silently.
- FR15: The bridge can detect energy-counter non-monotonicity (reset / rollover / meter replacement), mark the quality, and never publish a negative delta as a valid measurement.
- FR16: The bridge can validate the completeness and numeric domain of each smart-me payload before publishing; a missing/null/NaN field, or a value outside per-metric min/max bounds, yields degraded quality, never a substituted value.
- FR45: The bridge can encode cumulative energy as a 64-bit double (never float32), preserving full kWh resolution up to at least 10⁷ kWh.

**SCADA Publishing**
- FR17: The bridge can publish meter data to an MQTT broker in a form Ignition consumes as tags (Sparkplug B).
- FR18: The SCADA can auto-discover the meters and their engineering units from the bridge's published metadata.
- FR19: The bridge can respond to a SCADA-initiated rebirth request by re-announcing its metrics.
- FR20: The bridge never over-claims delivery: a value is reported as published only once it has been accepted for transmission, and a value it could not hand over yields a per-device traced drop rather than silence. *(Amended 2026-07-26 — ADR 0010.)*
- FR21: The bridge can purge orphan retained messages on old topics when a mapping changes (no ghost values).
- FR22: The bridge can apply a defined policy to readings acquired during a broker outage — bounded buffer preserving the source timestamp, or a traced drop; never a re-timestamped replay.

**Configuration**
- FR23: The operator can provide credentials and broker details via environment/`.env`.
- FR24: The operator can configure the meter→topic/tag mapping, with sensible defaults.
- FR25: The operator can confirm the meter→topic mapping before data is published (first-run confirmation).
- FR26: The bridge can validate the full configuration at startup (topic uniqueness, well-formed serials, completeness) and refuse to start on invalid config rather than start partially.
- FR27: The bridge can persist configuration across restarts and image updates.

**Observability & Diagnostics**
- FR28: The operator can view, in one place, each meter's live value, unit, freshness age, target topic, serial, and published status.
- FR29: The operator can see the independent health of the source (smart-me) and the sink (MQTT broker), from a single internal source of truth on source/sink/bridge state.
- FR30: The operator can see, per meter, the last successful read, the last error, and where in the chain a failure occurred.
- FR31: The operator can see actionable error messages (auth vs permissions vs timeout vs empty result), not stack traces.
- FR32: The operator can distinguish an empty/unconfigured state from an error state from a loading state.
- FR33: The bridge can expose a health/status endpoint (per-meter last-success, resource usage) consumable by the Docker healthcheck.
- FR34: The operator can see a culprit label (world / you / bridge) on each fault, derived from the error nature and source-vs-sink health.
- FR35: The operator can see an auto-written, human-readable, timestamped configuration context line (created, last change, meter count, priority meters) on the state screen.
- FR36: The operator can open a "state of the bridge" orientation screen — a multi-meter overview with human-readable timestamps (absolute + relative).
- FR37: The operator can trigger an on-demand end-to-end validation for a chosen meter (source → value → sink) and see the three links light up on one screen.

**Logging**
- FR38: The bridge can write daily-rotated log files at a configurable level and configurable retention (N days), never recording secrets.

**Lifecycle & Deployment**
- FR39: The operator can deploy and start the whole system via `docker compose`.
- FR40: The operator can update the bridge by pulling a new image without losing configuration.
- FR41: The operator can follow a documented update procedure with a rollback point and post-update verification.

**Documentation**
- FR42: The operator can rely on a documentation set (README, config reference, versioned Sparkplug/MQTT contract, troubleshooting, update) sufficient to install and troubleshoot without reading the code.

**Broker & Versioning**
- FR43: The operator can run the bridge against either a bundled MQTT broker or an external broker, with the broker connection optionally secured (TLS and/or authentication) or plain, per configuration. *(Architecture narrows v1 to external-broker-only; see AR11.)*
- FR44: The operator can see the running application/image version in the web UI and on the health endpoint.

### NonFunctional Requirements

**Reliability & Availability**
- NFR1: Runs unattended for weeks; automatic recovery from smart-me API and MQTT broker outages without manual restart (bounded exponential backoff + jitter, e.g. 1 s → 60 s cap).
- NFR2: Per-meter staleness signalled no later than `last_success + 2×poll_interval + publish_margin`.
- NFR3: No unbounded memory/FD growth — RSS_max ≤ 100 MB; RSS slope ≤ 1 %/24 h (linear regression, RSS sampled every 60 s); FD ≤ 64 via `/proc/self/fd`.
- NFR4: Availability is best-effort; during a smart-me outage the system stays honest (quality=STALE) rather than available — integrity never traded for availability.

**Data Integrity & Correctness ("never lies")**
- NFR5: Units exactly kW/kWh, values match the meter to the digit — 0 unit/scale errors.
- NFR6: Energy counters monotonic non-decreasing except on detected reset — 0 negative deltas published as valid.
- NFR7: 0 mislabeled-identity values (serial-bound, verified at startup + periodically).
- NFR8: No value ever presented as fresh when its measurement timestamp exceeds the staleness threshold (dual-mechanism staleness).

**Performance & Footprint**
- NFR9: Idle CPU/RAM low enough to co-exist on a Raspberry Pi / NAS (RSS_max target < ~100 MB).
- NFR10: A new reading reaches MQTT within one poll cycle; read→broker-ACK latency p95 ≤ 3 s, p99 ≤ 5 s over a 24 h window under nominal load.
- NFR11: Time-to-first-value < 15 min from a clean machine, with identity binding proven within it.

**Security**
- NFR12: Credentials only in `.env`/env vars, perms 0600, never in the image, never logged (incl. rotated logs) — log-grep test.
- NFR13: All smart-me traffic over TLS; hard-fail if unavailable (esp. Basic-auth fallback).
- NFR14: Web UI exposure safe-by-default (bind/auth posture decided in architecture); credentials never re-shown in clear.
- NFR15: Explicit non-goal / threat model stated: informative-only (not metering-of-record); trusted-host/LAN assumption documented.
- NFR16: The broker connection may be secured (TLS and/or auth) or plain per config; when secured, broker credentials follow the same discipline as smart-me creds (never logged).

**Interoperability & Deployment**
- NFR17: Sparkplug B output conforms to what Ignition MQTT Engine accepts — verified by a manual pre-release contract test against a real Ignition (values, units, STALE-on-death, NCMD/Rebirth). Not in automated CI. *(Epic 1 delivered the first pass covering values, units and STALE-on-death; the NCMD/Rebirth half is Epic 4 — see the Epic 1 retrospective.)*
- NFR18: The Sparkplug/MQTT contract is a standalone versioned document; a breaking change bumps the contract version.
- NFR19: The published `sparkplug-b` crate follows semver with a stable, documented public API (no third-party types leaked), complete crate metadata, and a documented conformance scope. Publish acceptance: `cargo publish` succeeds and `cargo add sparkplug-b` in a clean project compiles an encode→decode round-trip.
- NFR20: The bridge works with either a bundled MQTT broker or an external broker.

**Maintainability & Operability**
- NFR21: Single-arch Docker Hub image with a Docker healthcheck; image-based updates preserve config (zero-config-loss integration test).
- NFR22: Test seams (`Clock`, `Source`, injectable sink) enable deterministic tests without network; property/mock-contract/chaos tests run in CI; the Ignition contract test is a manual pre-release gate; `cargo-deny` in CI enforces dependency direction and licenses.
- NFR23: Documentation sufficient for the author to install, operate, and troubleshoot without reading the code.

**Operating Assumptions**
- NFR24: Relies on an NTP-synchronized host clock (UTC); freshness/staleness guarantees assume a correct host clock. *(Architecture largely removes the host clock from the freshness path via the Date-header formula; see AR5.)*

### Additional Requirements

*Technical requirements derived from the Architecture Decision Document that shape implementation and story sequencing.*

- AR1: **Native Cargo workspace scaffolding (no starter template).** First implementation story = create the virtual workspace + 3 crates (`sparkplug-b` lib, `smart-me-client` lib, `smartme-bridge` bin), Rust edition 2024 pinned via `rust-toolchain.toml`.
- AR2: **3-crate boundary enforcement.** Dependency direction (`sparkplug-b`, `smart-me-client` → `smartme-bridge`; nothing depends on the bridge) enforced by the Cargo graph + `cargo-deny`; intra-crate purity enforced by `arch_purity.rs` (core/domain free of tokio/axum; `Measurement`→Sparkplug mapping only in `sparkplug_publisher.rs`); `sparkplug-b` crates.io purity enforced by isolated CI build + `no_context_leak.rs`.
- AR3: **[BLOCKING] `ValueDate` / HTTP `Date`-header audit spike.** Second implementation story: capture a real smart-me payload + HTTP headers, audit `ValueDate` semantics (measurement vs poll vs server time; UTC/DST across midnight + a DST change) and confirm the `Date` header is present/usable. Documented fallback: monotonic-`Instant` staleness only + documented limitation.
- AR4: **Fixtures-first.** First commits include `fixtures/smartme_sample.json` and captured HTTP-header fixtures (valid / absent / malformed / negative-skew / huge-skew), plus the checked-in Sparkplug `.proto` (via `prost`/`build.rs`).
- AR5: **Freshness formula.** `freshness = HTTP Date-header(response) − ValueDate` (removes the host clock); guards: `system_time > 2020-01-01` at boot else STALE, `age < 0` → STALE. Two distinct time uses: staleness age = monotonic `Instant`; payload timestamp written to Sparkplug = honest wall-clock (`= source ValueDate`).
- AR6: **2-task runtime + `watch` snapshot.** poll+publish task (owns the `Fresh|Stale|Failed` state machine + `last_loop_tick`) and mqtt-driver task (owns the rumqttc EventLoop + `bdSeq`), communicating over a pure `core/channel.rs` message type; UI/publisher read a coherent `watch<[MeterState; N]>` snapshot. The state machine is a pure function `(prev, tick, now) → next` and never crosses into the mqtt-driver task.
- AR7: **Broker-down policy = traced-drop (no buffer).** `try_publish` non-blocking; on full/broker-down → per-device traced drop (`readings_dropped_total{meter,reason}` + WARN with source timestamp). No persistent buffer. Anti-replay invariant: every published Sparkplug timestamp == its source `ValueDate`, verified at the down→up reconnection instant.
- AR8: **Config propagation = `ArcSwap<Config>`.** Non-structural fields (poll interval, log level, mapping labels) hot-swap per cycle; structural fields (broker host/port, meter count, secrets) are restart-required with a "pending restart" UI flag. Single-writer discipline on the config file.
- AR9: **Atomic persistence.** `persist_atomic(path, &T)`: write-temp + `fsync(file)` + `fsync(parent_dir)` + rename. `bdSeq` + minimal Sparkplug session state persisted to TOML on a mounted volume; `bdSeq` written only by the mqtt-driver task.
- AR10: **Sparkplug boot ordering.** `bdSeq → NDEATH serialized → LWT set in CONNECT → connect → SUBSCRIBE to NCMD → NBIRTH`; reconnect triggers a rebirth, and re-runs the subscribe with it. *(The NCMD step was added by Story 4.6: `tck-id-message-flow-edge-node-ncmd-subscribe`'s preamble requires the subscription **prior to sending an NBIRTH**, not merely in the same sequence. This entry was left at five steps when the code and the manual moved to six — corrected by the Story 4.6 code review, 2026-07-29.)* `sparkplug-b` exposes primitives (`publish_metric(name, value, timestamp_ms, quality)`) with a minimal `Quality { Good, Stale, Bad }` enum, `Measurement`-free.
- AR11: **External-broker-only deployment behind Traefik.** `docker compose` runs the bridge alone; joins Traefik's `external: true` named network; `expose:` only (no host `ports:`); router labels for routing + TLS + optional auth middleware; no in-app auth. A commented `docker-compose.override.yml.example` gives a `ports:` fallback for non-Traefik users. Single-arch image, `restart: unless-stopped`.
- AR12: **Liveness heartbeat healthcheck.** poll+publish loop updates `last_loop_tick` (monotonic `Instant`) at the top of every iteration before the network call; `GET /healthz` returns unhealthy only if `now − last_loop_tick > N × poll_interval` (N≈3). An honest STALE never triggers a restart; a wedged poller does.
- AR13: **Graceful shutdown must not silence the DEATH.** On SIGTERM the bridge publishes an explicit NDEATH before exit **and** drops the connection abruptly (never a clean MQTT DISCONNECT, which instructs the broker to discard the will), so the LWT fires too. Resolved 2026-07-26 (was an either/or; see ADR 0011): both mechanisms are required, not alternatives. The explicit certificate is immediate; the will alone would leave the node showing as live until the broker notices the socket — up to 1.5× keep-alive if the connection is left half-open. Measured by `chaos_sigterm_no_lie`: the will carries the connect-time stamp, the explicit death the shutdown-time stamp ~1 s later, and a consumer sees both. **Confirmed against the author's own broker on 2026-07-26** via `chaos_sigterm_no_lie_against_an_external_broker` (`#[ignore]`d, no default target, refuses the default group — the author has one broker and it is production). The consumer half — whether Ignition tolerates the double NDEATH — belongs to Story 1.15.
- AR14: **Strong domain typing.** Newtypes `MeterId`, `Serial`, `TopicPath`, `Kw(f64)`, `Kwh(f64)`; single `Quality` enum; no stringly-typed serials/topics/units/quality. Unit conversion happens in exactly one place (`SmartMeCloudSource`), fail-closed on unknown units.
- AR15: **Time discipline.** Never call `SystemTime::now()`/`Instant::now()` in logic — always the injected `Clock`. All timestamps UTC ISO-8601 on the wire, `i64` epoch-millis in Sparkplug payloads; never local time.
- AR16: **Versioned Sparkplug/MQTT contract + non-drift.** Standalone versioned `docs/mqtt-sparkplug-contract.md` encoding traced-drop, per-device staleness, anti-replay, oracle→quality mapping, cold-start STALE; `CONTRACT_VERSION` embedded in the payload/BIRTH; each contract invariant has a clause-named test; `contract_golden.rs` fails if the mapping changes without a version bump.
- AR17: **Test tiers.** Tier 1 (unit + injected-clock staleness, kWh monotonicity, identity), Tier 2 (mock cloud 401/429/500/timeout/empty/bad-unit), Tier 2b (Sparkplug seq/bdSeq/rebirth property tests + `bdSeq` crash-during-persist), Tier 2c (golden/round-trip via independent decoder), Tier 3 (manual pre-release Ignition contract test, `#[ignore]`/feature-gated), Tier 4 chaos (`chaos_stale_on_death`, `chaos_stale_on_cloud_timeout`, `chaos_broker_recovery`, `chaos_poller_wedge`, `chaos_sigterm_no_lie`), AC-LEAK-01 (100k-iteration RSS/FD stability), `config_validation_table`, `zero_config_loss_update`.
- AR18: **Cold-start = STALE-until-proven.** Before the first successful fetch, publish quality=STALE; never restore a last-known value as fresh.
- AR19: **Enriched published state for legible honesty.** Published state carries `published_at` + staleness threshold used, `last_changed_at` distinct from `last_published_at`, `culprit` (world/you/bridge) first-class + repair gesture, root-cause grouping, and the persisted *expected* mapping for Cold-Reopening reconciliation. UI consumes this state, never recomputes it.
- AR20: **Documentation set + ADR trail.** Full `docs/` tree (index, glossary, operations-quickstart, configuration-reference, mqtt-sparkplug-contract, troubleshooting, update-procedure, ignition-contract-runbook, data-flow) plus 8 ADRs (0001-sparkplug-b-v1 … 0008-three-crate-split).
- AR21: **`sparkplug-b` crates.io bar.** `#![forbid(unsafe_code)]`, semver, README + CHANGELOG, MSRV `rust-version`, complete crate metadata, documented conformance scope, no leaked third-party types. Structured now; actual publish deferred.
- AR22: **CI/CD.** GitHub Actions: `fmt` + `clippy -D warnings` + `cargo-deny` + arch-purity test + all test tiers + AC-LEAK-01; isolated `sparkplug-b` build (`--no-default-features`); Docker Hub image build/push (deferred pipeline); `sparkplug-b` crates.io publish = separate manual deferred pipeline.
- AR23: **Error taxonomy drives the state machine.** Per-crate `thiserror` enums, no leaked third-party types, no `anyhow` in libs; the bridge classifies every fallible boundary as transient (→ Stale/retry) vs fatal (→ Failed); no `unwrap()`/`expect()`/`panic!` outside tests and `main` startup.

### UX Design Requirements

*No standalone UX Design Specification exists for this project. UI/UX requirements are captured by the Functional Requirements (FR28–FR37) and the Additional Requirements AR19 (enriched published state / legible honesty). The web UI is a minimal server-rendered `axum` surface (config + live preview + diagnostics + "state of the bridge" screen), no SPA framework, assets embedded via `rust-embed`. Distinct visual vocabulary is required for empty-config vs 401 vs 403 vs timeout vs 200-empty states (never an infinite spinner) — see FR31/FR32.*

### FR Coverage Map

*Each FR is mapped to the epic that OWNS its completion ("done"). The walking skeleton (Epic 1) takes a deliberately thin vertical slice of several FRs (FR1/FR3/FR7/FR11/FR13/FR17/FR18/FR20/FR45) and proves the "never lies" principle end-to-end at iteration 1; later epics thicken behaviour along independent axes.*

- FR1: Epic 1 — connect to smart-me cloud (thin slice; credential *provisioning* via `.env` = FR23/Epic 5)
- FR2: Epic 3 — full fleet discovery by name + serial
- FR3: Epic 1 — read one meter's power + energy (configurable interval hardened in Epic 5)
- FR4: Epic 2 — automatic recovery from transient source failures (bounded backoff)
- FR5: Epic 2 — distinguish permanent auth failure from transient
- FR6: Epic 3 — meter disappears from discovery → stale/absent
- FR7: Epic 1 — kW/kWh unit explicitly attached to the value
- FR8: Epic 2 — reject unknown/mismatched source units (oracle)
- FR9: Epic 2 — serial-number identity binding + verification (oracle)
- FR10: Epic 2 — measurement timestamp UTC end-to-end + clock-skew flag
- FR11: Epic 1 — staleness detection while still broker-connected (exhaustive transitions hardened in Epic 2)
- FR12: Epic 3 — per-meter staleness isolation (needs the fleet)
- FR13: Epic 1 — signal bridge-dead to the SCADA (LWT/NDEATH)
- FR14: Epic 2 — flag values outside physical bounds (oracle)
- FR15: Epic 2 — detect energy-counter non-monotonicity (oracle)
- FR16: Epic 2 — payload completeness / numeric-domain validation (oracle)
- FR17: Epic 1 — publish in Sparkplug B form Ignition consumes
- FR18: Epic 1 — SCADA auto-discovers units from self-describing BIRTH
- FR19: Epic 4 — respond to SCADA-initiated rebirth (NCMD)
- FR20: Epic 1 — never over-claim delivery; traced drop, never silence (amended, ADR 0010)
- FR21: Epic 3 — purge orphan retained messages on mapping change *(moved from Epic 4, 2026-07-26)*
- FR22: Epic 4 — broker-outage policy (traced-drop, exhaustive)
- FR23: Epic 5 — credentials + broker details via `.env`
- FR24: Epic 5 — meter→topic/tag mapping with defaults
- FR25: Epic 5 — first-run mapping confirmation before publish
- FR26: Epic 5 — startup config validation, refuse-to-start on invalid
- FR27: Epic 5 — config persists across restarts + image updates
- FR28: Epic 6 — unified per-meter live view (value/unit/age/topic/serial/status)
- FR29: Epic 6 — independent source (smart-me) vs sink (MQTT) health
- FR30: Epic 6 — per-meter last-success / last-error / broken-link locus
- FR31: Epic 6 — actionable error messages, not stack traces
- FR32: Epic 6 — distinguish empty/unconfigured vs error vs loading
- FR33: Epic 6 — health/status endpoint for the Docker healthcheck
- FR34: Epic 6 — culprit label (world / you / bridge) per fault
- FR35: Epic 6 — auto-written timestamped config context line
- FR36: Epic 6 — "state of the bridge" orientation screen
- FR37: Epic 6 — on-demand end-to-end validation for a chosen meter
- FR38: Epic 6 — daily-rotated logs, configurable level/retention, no secrets
- FR39: Epic 7 — deploy + start via `docker compose`
- FR40: Epic 7 — update by pulling a new image without config loss
- FR41: Epic 8 — documented update procedure with rollback + verification
- FR42: Epic 8 — full documentation set (README, config ref, contract, troubleshooting, update)
- FR43: Epic 5 — external/bundled broker, optionally secured, per config
- FR44: Epic 6 — running app/image version in UI + health endpoint
- FR45: Epic 1 — cumulative energy encoded as 64-bit double (never float32)

## Epic List

*Structure adopted after a party-mode stress-test (Winston, John, Amelia, Murat): **walking-skeleton-first**. The pure "functional core" (no `tokio` in truth-deciding code) is not a temporal milestone but a **compile-time invariant** enforced across every slice from the socle onward — "no truth is ever decided inside an `async fn`". Epics 2–4 are independent thickening axes over the skeleton; each stands alone and requires no future epic to function.*

> **Execution order: 0 → 1 → 4 → 2 → 3 → 5 → 6 → 7 → 8.**
>
> **Inside Epic 4, stories 4.6 and 4.7 run before 4.5** — [ADR 0016](../../docs/adr/0016-rebirth-before-primary-host-wait.md),
> [#37](https://github.com/guycorbaz/smartme_mqtt/issues/37). Story 4.4 measured that the
> specification's motivation for waiting on a Primary Host is store-and-forward, which this bridge
> does not have, so PHID-wait alone would preserve no measurement; Rebirth is what actually restores
> a consumer's view. Story numbers, like epic numbers, are identifiers rather than sequence.
>
> Epic numbers are **identifiers, not sequence**. Epic 4 was pulled ahead of Epic 2 at the Epic 1 retrospective (2026-07-26) for three reasons: it owns the Sparkplug conformance audit, and Epic 1 demonstrated what stacking on an unverified channel costs; Epic 2 will define many oracle→quality mappings (AR16), which are cheaper to land on a settled publishing machine than to revisit after rebirth and anti-replay change republication semantics; and Epic 4 carries NFR3 / AC-LEAK-01, so a resource leak surfaces before two more epics are built on top of it.
>
> The epics were deliberately **not renumbered**: seventeen references to epic numbers live in Rust doc comments, plus the coverage map, the manual and the issue tracker. Renumbering would invalidate all of them for a cosmetic gain.
>
> *Known residual risk of the reorder:* rebirth re-declares metrics with their qualities, and Epic 2 may extend the quality set. The degradation rule (`Good` → `Stale`, never upward) would then need revisiting in the rebirth path. Small, and accepted.

### Epic 0: Socle — Workspace, CI Gates & Durability Primitive
Establish the compilable, boundary-enforced substrate every later guarantee rests on: the 3-crate Cargo workspace (edition 2024), the CI gate wall (`fmt`, `clippy -D warnings`, `cargo-deny`, `arch_purity` — which bans `tokio`/`rumqttc` imports inside `core/`), the checked-in `.proto` + fixtures, and the shared atomic-persistence primitive `persist_atomic` (write-temp + fsync(file) + fsync(dir) + rename) with its crash-injection tests. Explicit enabler epic — its value is measured in risk avoided, not user-visible features.
**FRs covered:** *(none — enabling)*
**NFR/AR:** NFR22 · AR1, AR2, AR4, AR9 *(persist_atomic primitive)* · the "no truth in async fn" invariant *(AR15/AR23)*

### Epic 1: The Walking Skeleton — One Meter → Ignition, Honest STALE
The thinnest possible vertical slice that proves the founding principle end-to-end at iteration 1: fetch one meter from smart-me → canonical `Measurement` → pure `Fresh|Stale|Failed` decision → minimal Sparkplug NBIRTH/NDATA/NDEATH → a tag moving in the author's own Ignition, with an honest quality flag. Unplug the meter → quality goes STALE while the node stays alive. Includes the **[BLOCKING] `ValueDate`/HTTP `Date`-header audit spike** (unblocks the freshness formula, done first), the 2-task runtime born small-but-whole, and the three "never lies" seams Murat flagged: **cold-start NBIRTH carries quality=STALE**, the Date-header is extracted from the real HTTP response, and NDEATH `bdSeq` == the session's NBIRTH `bdSeq`.
**FRs covered:** FR1 *(thin)*, FR3 *(thin)*, FR7, FR11, FR13, FR17, FR18, FR20, FR45
**NFR/AR:** NFR5, NFR8, NFR13, NFR17 *(first Ignition contract test)*, NFR24 · AR3 *(audit spike)*, AR5 *(freshness formula)*, AR6 *(functional-core seam + 2-task runtime)*, AR7 *(traced-drop, minimal)*, AR10 *(boot ordering + bdSeq persist)*, AR13 *(SIGTERM-NO-LIE)*, AR14, AR15, AR18 *(cold-start STALE)* · chaos_stale_on_death + chaos_stale_on_cloud_timeout

### Epic 2: Exhaustive "Never Lies" Oracles & Freshness Hardening
Thicken the skeleton's single FRESH→STALE transition into the full integrity guarantee: all quality transitions and staleness edge cases, the resilience/backoff behaviour, error taxonomy (transient→retry vs fatal→stop), and the four runtime oracles — unit rejection, serial-identity binding + verification, physical bounds, energy-counter monotonicity, plus payload completeness/numeric-domain validation and UTC-timestamp/skew handling. This is where the "never lies" invariant is proven across every failure mode, not just the happy-path unplug.
**FRs covered:** FR4, FR5, FR8, FR9, FR10, FR14, FR15, FR16
**NFR/AR:** NFR1, NFR4, NFR6, NFR7 · AR16 *(oracle→quality mapping)*, AR17 *(freshness + oracle property tests)*

### Epic 3: The Full Fleet — Multi-Meter Discovery & Per-Meter Isolation
Grow from one meter to the author's actual 4-meter fleet on a single account: full discovery by name + serial, the real Sparkplug device topology (per-meter DBIRTH/DDEATH), and per-meter staleness isolation — one silent Kamstrup flips stale individually while the other three stay fresh. Delivers Journey 2 (A Meter Goes Silent) at fleet scale.
**FRs covered:** FR2, FR6, FR12, FR21 *(moved from Epic 4 — mapping changes originate here)*
**NFR/AR:** NFR2 *(per-meter staleness latency)* · AR6 *(per-meter watch snapshot)*

### Epic 4: Sparkplug Conformance & the Exhaustive Publishing State Machine
**Runs immediately after Epic 1 — see the execution order above.** Establish what the implementation actually owes the Sparkplug B specification, then complete the publishing behaviours the skeleton stubbed: a clause-by-clause conformance audit (spec → implementation → test matrix), SCADA-initiated NCMD/Rebirth response with the Tier-3 gate extended to cover it, a decision on Primary Host / STATE, the full broker-outage traced-drop policy with anti-replay on the down→up transition, and the resource-stability guarantees (bounded growth, latency). Owns the runtime chaos suite.
**FRs covered:** FR19, FR22
**NFR/AR:** NFR3 *(AC-LEAK-01)*, NFR9, NFR10, NFR17 *(completing the Tier-3 gate — NCMD/Rebirth)* · AR7 *(full traced-drop + anti-replay)* · chaos_broker_recovery, chaos_poller_wedge

*Scope widened at the Epic 1 retrospective (2026-07-26). The conformance audit is **story 1**, and the rest of the epic is explicitly allowed to be reshaped by its findings — the audit is the artifact that tells us the size of the gap, so it cannot be planned around. Two known entries for it: NFR17 requires the Tier-3 test to verify NCMD/Rebirth and it does not; and Primary Host / STATE appears nowhere in any planning artifact while the author's broker carries live `spBv1.0/STATE` topics. Note that a Primary Host decision may force a revisit of ADR 0011, since an offline primary host changes when an edge node should stop publishing.*

*FR21 (orphan-retained purge on mapping change) moved to Epic 3: mapping changes originate with multi-meter discovery, and at one meter there is no honest way to exercise it. It is also near-moot today — the bridge publishes everything with `retain = false`, so it cannot create the orphans FR21 purges; the requirement guards against orphans left by something else.*

### Epic 5: Configuration, Secrets & Persistence
The author configures the bridge safely and confirms mappings before anything publishes: `.env` secrets discipline, external/bundled broker connection (optionally TLS/auth), meter→topic mapping with defaults, first-run mapping confirmation, startup validation with refuse-to-start, and config that survives restarts + image updates (reusing the Epic 0 `persist_atomic` primitive via `ArcSwap`). Delivers Journey 1 (First Run) configuration.
**FRs covered:** FR23, FR24, FR25, FR26, FR27, FR43
**NFR/AR:** NFR12, NFR14, NFR16, NFR20 · AR8 *(ArcSwap config)*

### Epic 6: Observability, Diagnostics & the State-of-the-Bridge UI
Single-screen confidence plus 3am re-orientation: the unified `axum` UI (live per-meter view, dual source/sink health, actionable errors, empty/error/loading distinction), the health endpoint feeding the Docker healthcheck (with the `last_loop_tick` heartbeat), culprit labels, auto-written context line, the "state of the bridge" screen, on-demand end-to-end validation, version display, and daily-rotated logging. Delivers Journeys 2 & 5.
**FRs covered:** FR28, FR29, FR30, FR31, FR32, FR33, FR34, FR35, FR36, FR37, FR38, FR44
**NFR/AR:** NFR11, NFR12 *(log-grep)* · AR11 *(axum bind)*, AR12 *(healthz heartbeat)*, AR19 *(enriched published state)*

### Epic 7: Deployment, Healthcheck & Update Lifecycle
The author deploys via `docker compose` behind Traefik and updates painlessly: single-arch Dockerfile, Traefik integration (external network, no host port) with a non-Traefik `ports:` fallback, the healthcheck semantics (restart a wedged poller, never an honest STALE), zero-config-loss image updates, and the CI/CD pipeline building/pushing the Docker Hub image. Delivers Journey 3 (Updating the Bridge).
**FRs covered:** FR39, FR40
**NFR/AR:** NFR21 · AR11 *(compose/Traefik/Dockerfile)*, AR12 *(healthcheck wiring)*, AR22 *(CI/CD)*

### Epic 8: Documentation, Versioned Contract & Crate Publishing
First-class, dual-audience documentation and the release surface: README, configuration reference, the standalone versioned Sparkplug/MQTT contract (with `CONTRACT_VERSION` + clause-named conformance tests), troubleshooting guide, update procedure with rollback, Ignition-contract runbook, data-flow diagram, the 8-ADR trail, and the `sparkplug-b` crates.io publication bar (semver, README/CHANGELOG, conformance scope). The Tier-3 Ignition contract test runs here as the final release gate (re-confirming the first pass from Epic 1). Delivers Journeys 3 & 5 (docs).
**FRs covered:** FR41, FR42
**NFR/AR:** NFR18, NFR19, NFR23 · AR16 *(contract doc)*, AR20 *(docs + ADRs)*, AR21 *(crate publish)*

## Epic 0: Socle — Workspace, CI Gates & Durability Primitive

Establish the compilable, boundary-enforced substrate every later guarantee rests on: the 3-crate Cargo workspace, the CI gate wall, the checked-in Sparkplug `.proto` + fixtures scaffolding, and the shared atomic-persistence primitive. This epic delivers no user-visible feature; its value is measured in risk avoided — it makes the "never lies" invariants mechanically enforceable before the first line of behaviour is written.

### Story 0.1: 3-Crate Cargo Virtual Workspace

As a developer,
I want a 3-crate Cargo virtual workspace pinned to a reproducible toolchain,
So that every later crate builds identically in dev, CI, and Docker.

**Acceptance Criteria:**

**Given** a clean checkout with a root `Cargo.toml` (`[workspace]`, `resolver = "2"`, `members = ["crates/sparkplug-b", "crates/smart-me-client", "crates/smartme-bridge"]`)
**When** I run `cargo build --workspace`
**Then** all three crates compile
**And** `crates/sparkplug-b` and `crates/smart-me-client` are library crates, and `crates/smartme-bridge` is a binary crate with a `lib.rs` plus a thin `main.rs` that only calls `lib::run()`.

**Given** a committed `rust-toolchain.toml` pinning the edition-2024 toolchain
**When** CI and Docker build the workspace
**Then** they resolve the identical toolchain version
**And** `Cargo.lock` is committed so dependency versions are reproducible.

**Given** the workspace dependency graph
**When** I inspect it
**Then** `smartme-bridge` depends on both library crates
**And** nothing depends on `smartme-bridge` (dependency direction is one-way).

**Given** a committed `.cargo/config.toml` with `[build] jobs = 2`
**When** any `cargo build`/`cargo test` runs in the project tree (dev, CI, or Docker)
**Then** Cargo spawns at most 2 parallel `rustc` processes
**And** the host (NAS / Raspberry Pi class) is never saturated by the build.

**Given** the same `.cargo/config.toml` selecting the `mold` linker for `x86_64-unknown-linux-gnu` (`linker = "clang"`, `rustflags = ["-C", "link-arg=-fuse-ld=mold"]`)
**When** a build links a binary on the target host
**Then** `mold` performs the linking (verifiable: the binary's `.comment` section names `mold`)
**And** the CI runner and the Docker build image install `mold` + `clang` so the linker choice holds there too.

### Story 0.2: Sparkplug `.proto` Compiled via prost/build.rs

As a developer,
I want the Sparkplug B `.proto` checked in and compiled through `prost`/`build.rs`,
So that `sparkplug-b` has reproducibly generated, typed protobuf messages.

**Acceptance Criteria:**

**Given** `crates/sparkplug-b/proto/sparkplug_b.proto` and a `crates/sparkplug-b/build.rs`
**When** I run `cargo build -p sparkplug-b`
**Then** `prost` generates the Sparkplug message types and the crate compiles.

**Given** `#![forbid(unsafe_code)]` at the top of `sparkplug-b/src/lib.rs`
**When** the crate builds
**Then** no `unsafe` code is permitted anywhere in the crate.

**Given** the `sparkplug-b` manifest
**When** I inspect its dependencies
**Then** `prost` is the only heavy runtime dependency
**And** no `smart-me`, `rumqttc`, or bridge dependency is present.

### Story 0.3: Fixtures Directory Scaffolding

As a developer,
I want the fixtures directory structure with a clearly-marked synthetic placeholder and HTTP-header slots,
So that parsing and oracle tests have a home before the real captured payload lands in Epic 1.

**Acceptance Criteria:**

**Given** `crates/smart-me-client/fixtures/smartme_sample.json` containing a synthetic placeholder payload
**When** a test loads it
**Then** it deserializes into the smart-me device model shape.

**Given** `crates/smart-me-client/fixtures/http_headers/` with named slots for `valid`, `absent`, `malformed`, `negative_skew`, and `huge_skew`
**When** Epic 1's `ValueDate`/`Date`-header audit spike runs
**Then** it replaces the synthetic placeholder with the real captured payload + headers.

**Given** the placeholder fixture
**When** any developer inspects it
**Then** a comment or README marks it explicitly as synthetic
**And** it is never mistaken for the parsing contract-of-record.

### Story 0.4: fmt + clippy CI Gate

As a maintainer,
I want `cargo fmt --check` and `cargo clippy -D warnings` enforced in CI,
So that formatting and lint regressions fail the build automatically.

**Acceptance Criteria:**

**Given** `.github/workflows/ci.yml` with a lint job
**When** a pull request runs CI
**Then** `cargo fmt --check` fails the build on any unformatted file
**And** `cargo clippy --workspace --all-targets -D warnings` fails the build on any warning.

**Given** committed `rustfmt.toml` and `clippy.toml`
**When** the lint job runs
**Then** the project's chosen formatting/lint configuration is applied.

### Story 0.5: cargo-deny Dependency & License Gate

As a maintainer,
I want `cargo-deny` enforcing external-dependency policy and licenses in CI,
So that a disallowed license or a forbidden dependency fails the build.

**Acceptance Criteria:**

**Given** a committed `deny.toml`
**When** `cargo deny check` runs in CI
**Then** a dependency with a non-MIT-compatible license fails the build.

**Given** the dependency-direction rule (pure libs must not depend on `smartme-bridge`)
**When** a hypothetical edge from `sparkplug-b` or `smart-me-client` to `smartme-bridge` is introduced
**Then** the Cargo graph resolution fails
**And** `deny.toml` scope is documented as external deps + licenses only (internal topology is guarded by the Cargo graph and `arch_purity`).

### Story 0.6: arch_purity — Functional-Core Invariant

As a maintainer,
I want an `arch_purity` test that bans async/transport imports inside the pure modules and pins the `Measurement`→Sparkplug mapping to a single file,
So that the "no truth is ever decided inside an `async fn`" invariant holds mechanically across every future slice.

**Acceptance Criteria:**

**Given** `crates/smartme-bridge/tests/arch_purity.rs`
**When** it scans `src/core/` and `src/domain/`
**Then** it fails if any file imports `tokio`, `rumqttc`, `axum`, or `reqwest`.

**Given** the mapping-ownership rule
**When** `arch_purity` scans the bridge source
**Then** it fails if the `Measurement`→Sparkplug metric mapping appears anywhere outside `src/adapters/sparkplug_publisher.rs`.

**Given** a violation is introduced (e.g. a `use tokio` added to `core/state_machine.rs`)
**When** CI runs
**Then** the build is red before merge — never discovered only at an epic boundary.

### Story 0.7: sparkplug-b Isolated Build + Context-Leak Guard

As a maintainer,
I want `sparkplug-b` built in isolation with a negative test proving no bridge context leaks,
So that the crates.io purity guarantee holds regardless of workspace feature unification.

**Acceptance Criteria:**

**Given** `.github/workflows/sparkplug-isolated.yml`
**When** it builds `sparkplug-b` alone with `--no-default-features`
**Then** the crate compiles without any workspace sibling present.

**Given** `crates/sparkplug-b/tests/no_context_leak.rs`
**When** it scans the crate's `src/`
**Then** it fails the build if the tokens `smartme`, `ignition`, or `SMARTME_` appear anywhere in the source.

### Story 0.8: persist_atomic Durability Primitive

As a developer,
I want a pure `persist_atomic` helper with crash-injection tests,
So that both `bdSeq` (Epic 1) and config (Epic 5) persist durably without a torn write, and no consumer re-implements durability.

**Acceptance Criteria:**

**Given** `crates/smartme-bridge/src/persist.rs` exposing `persist_atomic<T: Serialize>(path, &T)`
**When** it writes a value
**Then** it writes to a temp file, fsyncs the file, fsyncs the parent directory, then renames over the target path.

**Given** a crash injected between the fsync and the rename
**When** the process restarts and reloads the file
**Then** the value read is either the old or the new one, never a corrupt/torn intermediate.

**Given** `crates/smartme-bridge/tests/prop_persist_atomic.rs`
**When** it runs
**Then** it asserts fsync is invoked, the temp file is cleaned up, and the rename is atomic.

**Given** `persist.rs`
**When** `arch_purity`/import scan inspects it
**Then** it imports no domain types (generic over `T: Serialize`), confirming it is a foundational primitive with no forward dependency on any later epic.

## Epic 1: The Walking Skeleton — One Meter → Ignition, Honest STALE

The thinnest vertical slice that proves "never lies" end-to-end at iteration 1: one meter fetched from smart-me → canonical `Measurement` → pure `Fresh|Stale|Failed` decision → minimal Sparkplug NBIRTH/NDATA/NDEATH → a tag moving in the author's Ignition, with an honest quality flag; unplug the meter → quality goes STALE while the node stays alive. Stories are ordered so each compiles and tests green at its boundary (functional core first, then the 2-task async shell born whole). The purity invariant ("no truth decided inside an `async fn`") is already enforced by `tests/arch_purity.rs` (Epic 0).

### Story 1.1: Audit smart-me `ValueDate` & HTTP `Date`-header semantics

As the maintainer,
I want the real smart-me timestamp semantics audited on a captured payload,
So that the freshness formula rests on fact, not assumption.

**Acceptance Criteria:**

**Given** valid smart-me credentials for the author's account
**When** a real `GET /Devices/` (and `GET /Devices/{id}`) request is captured
**Then** the real payload replaces the synthetic `crates/smart-me-client/fixtures/smartme_sample.json`
**And** the real HTTP response headers replace the synthetic `fixtures/http_headers/*` (at least a real `valid` case).

**Given** the captured payload and headers
**When** `ValueDate` is audited (measurement vs poll vs server time; UTC/DST across midnight and a DST change) and the `Date` header's presence/format is checked
**Then** the finding is recorded in an ADR (`docs/adr/0004-freshness-date-header.md`)
**And** the ADR states either "formula `age = Date − ValueDate` confirmed" or "fallback: monotonic-`Instant` staleness only, with the documented limitation".

**Given** the audit outcome
**When** GitHub issue #1 is closed
**Then** the decision it references is the ADR, and Epic 1's state machine (1.5) builds on it.

*Note: blocked on the author configuring smart-me credentials; the capture tooling can be written first, the capture + audit run once creds exist.*

### Story 1.2: Domain model — `Measurement`, physical newtypes, `Quality`

As a developer,
I want the canonical domain types with units carried in the type,
So that no serial, topic, or physical quantity is ever a bare `String`/`f64`.

**Acceptance Criteria:**

**Given** the `domain` module
**When** it is compiled
**Then** it defines newtypes `Kw(f64)`, `Kwh(f64)`, `Serial`, `MeterId`, `TopicPath` and a canonical `Measurement { meter: MeterId, serial: Serial, power: Kw, energy: Kwh, value_date, quality: Quality }`
**And** `Quality { Good, Stale, Bad }` is a single definition aligned with the `sparkplug-b` quality enum.

**Given** the `domain` module
**When** `tests/arch_purity.rs` scans it
**Then** it imports no `tokio`/`axum`/`reqwest`/`rumqttc` (stays pure).

**Given** a raw `f64` or `String`
**When** a developer tries to use it as a serial, topic, or physical quantity
**Then** the type system rejects it (construction goes through the newtype).

### Story 1.3: `Clock` seam

As a developer,
I want time behind an injected `Clock` trait,
So that no truth is ever computed from a hardcoded `now()`.

**Acceptance Criteria:**

**Given** the `core` module
**When** it is compiled
**Then** it defines `trait Clock` exposing a monotonic instant and a wall-clock time, with a `SystemClock` production impl and a `FakeClock` test double whose time advances only on explicit calls.

**Given** any logic module
**When** `tests/arch_purity.rs` and review inspect it
**Then** no direct `SystemTime::now()`/`Instant::now()` call appears outside `SystemClock`.

### Story 1.4: `Source` seam + fake

As a developer,
I want the meter source behind a `Source` trait with a fake,
So that the staleness machine can be exercised deterministically without network.

**Acceptance Criteria:**

**Given** the `core` module
**When** it is compiled
**Then** it defines `trait Source` yielding a per-meter reading `{ value, value_date, http_date }` (or a typed error), plus a `FakeSource` that scripts `Ok` / transient error / timeout sequences.

**Given** `FakeSource`
**When** a test drives it
**Then** it can reproduce fetch success, a transient failure, and a cloud timeout without any network or tokio.

### Story 1.5: Pure `Fresh|Stale|Failed` state machine

As a developer,
I want the staleness decision as a pure, property-tested function,
So that "is this a lie?" is decided deterministically and off the network.

**Acceptance Criteria:**

**Given** `core/state_machine.rs`
**When** it is compiled
**Then** it exposes a pure `step(prev, tick, now) -> (next, effect)` over `Fresh|Stale|Failed`, importing no tokio/transport crate.

**Given** a `tick { value, value_date, http_date, now }`
**When** `step` computes freshness as `http_date − value_date`
**Then** it maps `system_time < 2020-01-01` → STALE, `age < 0` → STALE, and `age > threshold` → STALE, and only a fresh, in-bounds reading → `Fresh`.

**Given** cold start (no successful fetch yet)
**When** `step` runs before the first `Ok` reading
**Then** the result is STALE-until-proven (never a restored last-known value shown fresh).

**Given** the five header fixtures (valid/absent/malformed/negative_skew/huge_skew)
**When** each is fed through `step` with the paired `ValueDate`
**Then** each maps to its documented verdict (fresh only for `valid`; STALE for the other four)
**And** `tests/staleness_injected_clock.rs` asserts them via `FakeClock` (this is the localization twin of `chaos_stale_on_cloud_timeout`).

### Story 1.6: `smart-me-client` — GET device + `Date`-header capture

As the operator,
I want one meter read over TLS with the response `Date` captured,
So that the bridge has correct data and the freshness oracle's clock input.

**Acceptance Criteria:**

**Given** an API key (with HTTP Basic fallback)
**When** the client calls `GET /Devices/{id}` over TLS
**Then** it deserializes `ActivePower`/`ActivePowerUnit`, `CounterReading`/`CounterReadingUnit`, `ValueDate`, `Serial`, `Id`, `Name`
**And** it captures the HTTP response `Date` header alongside the body.

**Given** a non-TLS endpoint or a TLS failure
**When** the client attempts the request
**Then** it hard-fails rather than falling back to plaintext (NFR13).

**Given** the captured fixture (Story 1.1) served by an HTTP mock
**When** the client parses it
**Then** the fields and the `Date` header match the fixture (contract-of-record).

### Story 1.7: `SmartMeCloudSource` adapter + fail-closed unit conversion

As a developer,
I want smart-me types mapped to `Measurement` with units converted in one place,
So that an unknown unit is rejected rather than guessed.

**Acceptance Criteria:**

**Given** a smart-me device with `ActivePowerUnit`/`CounterReadingUnit`
**When** `SmartMeCloudSource` (impl `Source`) maps it to a `Measurement`
**Then** power is converted to `Kw` and energy to `Kwh` in exactly this adapter (nowhere else).

**Given** an unknown or mismatched source unit
**When** the adapter maps the reading
**Then** it fails closed: the `Measurement` is marked `Quality::Bad` and no guessed value is produced (thin FR8; full coverage in Epic 2).

### Story 1.8: `sparkplug-b` — encode + `seq`/`bdSeq` + NBIRTH/NDATA/NDEATH

As a developer,
I want the crate to emit a spec-correct Sparkplug flow carrying quality,
So that Ignition can consume BIRTH/DATA/DEATH with engineering units.

**Acceptance Criteria:**

**Given** a metric name, value, timestamp, and `Quality`
**When** the crate encodes a payload
**Then** it produces a valid protobuf `Payload` with `seq` (0–255 wrapping) and per-session `bdSeq`, and builders for NBIRTH, NDATA, NDEATH (and the LWT/NDEATH payload).

**Given** a cumulative energy value
**When** it is encoded
**Then** it uses the `Double` datatype (never float32), preserving full kWh resolution (FR45).

**Given** an NBIRTH
**When** it is built
**Then** it is self-describing (metric names, engineering-unit properties) so a consumer can auto-discover units (FR18).

**Given** the sequence logic
**When** `tests/prop_seq_bdseq.rs` runs
**Then** it asserts `seq` monotonic wrap 255→0 and `bdSeq` continuity across sessions.

### Story 1.9: `SparkplugPublisher` adapter — mapping + cold-start NBIRTH=STALE

As a developer,
I want the `Measurement`→metric mapping confined to one file that never births a fresh-looking lie,
So that identity/units are correct and the first message is honest.

**Acceptance Criteria:**

**Given** the bridge source
**When** `tests/arch_purity.rs` scans `adapters/`
**Then** the `Measurement`→Sparkplug metric mapping exists only in `sparkplug_publisher.rs`.

**Given** a `Measurement` with `Quality::Good`
**When** the publisher maps it
**Then** the metric carries `kW`/`kWh` engineering units, the device is keyed by `Serial`, and the payload timestamp equals the source `ValueDate`.

**Given** cold start (no successful fetch yet)
**When** the publisher emits the first NBIRTH
**Then** its metrics carry `quality = STALE` (never GOOD-by-default).

**Given** a `Measurement` with `Quality::Stale`
**When** the publisher processes it through the injectable sink
**Then** no fresh NDATA is emitted for that metric (the sink assertion holds).

### Story 1.10: `core/channel.rs` — inter-task message

As a developer,
I want the poll and mqtt tasks to communicate over one pure message type,
So that the seam is defined before either task exists (no forward reference).

**Acceptance Criteria:**

**Given** the `core` module
**When** it is compiled
**Then** `channel.rs` defines a pure message carrying `(MeterId, Measurement, Quality)` and imports no tokio/transport crate.

**Given** the message type
**When** the two tasks are later built
**Then** both reference this type and neither redefines it.

### Story 1.11: `poll_publish` task — state machine + `last_loop_tick`

As the bridge,
I want one meter polled, judged, and forwarded with a liveness heartbeat,
So that the staleness decision lives in one task and a wedge is detectable.

**Acceptance Criteria:**

**Given** the `poll_publish` task
**When** it runs a loop iteration
**Then** it updates `last_loop_tick` (monotonic) at the top of the loop before the network call, polls the meter via `Source`, runs the pure `step`, and sends the result on the channel.

**Given** the state machine
**When** `poll_publish` uses it
**Then** the machine stays entirely inside this task and never crosses into the mqtt task.

**Given** a unit test draining the channel
**When** `poll_publish` processes a scripted `FakeSource` + `FakeClock`
**Then** the emitted `(MeterId, Measurement, Quality)` matches the expected verdict — provable without the mqtt task existing.

### Story 1.12: `mqtt_driver` task — EventLoop + `bdSeq` + boot order + broker-ACK

As the bridge,
I want the Sparkplug session driven correctly and delivery never over-claimed,
So that the SCADA sees an honest, ordered lifecycle.

**Acceptance Criteria:**

**Given** startup
**When** `mqtt_driver` initializes
**Then** it follows the order `bdSeq → NDEATH serialized → LWT set in CONNECT → connect → NBIRTH`, and `bdSeq` is loaded/persisted via `persist_atomic` (Epic 0).

**Given** a message on the channel
**When** `mqtt_driver` publishes it
**Then** it uses non-blocking `try_publish`, and delivery is never over-claimed (FR20, amended — ADR 0010: Sparkplug mandates QoS 0, at which no broker ACK exists); a full/broker-down queue yields a per-device traced drop, never silence.

**Given** a transport death (LWT fires)
**When** the NDEATH is delivered
**Then** its `bdSeq` equals the session's NBIRTH `bdSeq` (asserted by `chaos_stale_on_death`).

**Given** a reconnect
**When** the broker returns
**Then** `mqtt_driver` issues a rebirth (fresh NBIRTH) with `published-ts == source ValueDate` (no replay of old values).

### Story 1.13: `supervisor` + `run()` + graceful shutdown (SIGTERM-NO-LIE)

As the operator,
I want the two tasks born whole together and death guaranteed on shutdown,
So that a clean stop never leaves the SCADA showing a stale value as live.

**Acceptance Criteria:**

**Given** `lib::run()`
**When** the process starts
**Then** it builds the tokio runtime and spawns `poll_publish` + `mqtt_driver`, wiring the channel, `Clock`, `Source`, and `SparkplugPublisher`; `main.rs` only calls `run()`.

**Given** a `SIGTERM`
**When** the process shuts down
**Then** an **explicit NDEATH is published before exit** — verified by `chaos_sigterm_no_lie`, which distinguishes it from the broker's will by its timestamp (the will is serialised before CONNECT and so can never be stamped later than the NBIRTH). The connection is then dropped rather than cleanly disconnected, so the LWT fires as well and a consumer legitimately sees two NDEATHs for the session; the will remains the sole mechanism for a hard death (crash, SIGKILL, power loss), covered by `chaos_stale_on_death`.

*Amended 2026-07-26 (ADR 0011). This AC previously read "either an explicit NDEATH ... or the connection is dropped so the LWT fires", deferring the choice of mechanism to this very test — see AR13. The test resolved it: both fire, and requiring the explicit one is what makes a planned stop immediate rather than dependent on the broker noticing a socket.*

*The clause "an independent subscriber sees no fresh DDATA survive" is **not** verified by `chaos_sigterm_no_lie` and is not claimed to be: the scenario points the bridge at an unroutable cloud, so no reading is ever obtained, and structurally no path exists by which one could follow the certificate. Proving it needs a TLS-terminating fake of the smart-me API — deferred.*

### Story 1.14: Chaos — STALE-on-DEATH + STALE-on-cloud-timeout

As the maintainer,
I want both silent-lie failure modes proven end-to-end,
So that the skeleton actually delivers the "never lies" guarantee, not just detects it.

**Acceptance Criteria:**

**Given** a running bridge against a testcontainers MQTT broker
**When** the process is killed or the bridge↔broker link is cut (`chaos_stale_on_death`)
**Then** an independent subscriber sees the affected tags marked STALE via NDEATH.

**Given** a running bridge with the broker up but the smart-me cloud unreachable (`chaos_stale_on_cloud_timeout`)
**When** the cloud fetch times out while the node stays alive
**Then** the affected metrics are published with `quality = STALE` and no frozen value is shown fresh.

### Story 1.15: Tier-3 Ignition contract test (manual) + runbook — first pass

As the maintainer,
I want a real Ignition MQTT Engine to confirm the hand-rolled protobuf,
So that the wire format is validated early, while there is budget to fix the codec.

**Acceptance Criteria:**

**Given** `crates/sparkplug-b/tests/ignition_contract.rs` (`#[ignore]`/feature-gated `ignition_contract`)
**When** it is run manually against the author's Ignition MQTT Engine
**Then** Ignition accepts the Sparkplug flow, shows correct `kW`/`kWh` values with units, and marks tags STALE on death.

**Given** `docs/ignition-contract-runbook.md`
**When** the maintainer follows it
**Then** the steps to arm, run, and interpret the Tier-3 test are documented (this is the early-discovery pass; the release-gate re-run is Epic 8).

## Epic 4: Sparkplug Conformance & the Exhaustive Publishing State Machine

Runs immediately after Epic 1. Two halves that belong together: first establish what the implementation actually owes the Sparkplug B specification — an artifact that does not exist today — then complete the publishing behaviours the skeleton stubbed. Epic 1 proved the happy path is honest; this epic proves the *protocol* is honest, and that the publisher behaves under reconnect, rebirth and backpressure.

Stories 4.1–4.3 are the audit. **They come first on purpose and the rest of the epic is explicitly allowed to be reshaped by their findings** — the audit is what measures the gap, so it cannot be planned around. Stories 4.4 onward are the known work; the audit may add more.

*Two drafting notes, applying the rule adopted at the Epic 1 retrospective (an acceptance criterion may not defer its decision to an artifact that does not yet exist):*

- *Primary Host / STATE is split into a **measurement** story and a **decision** story. Nothing here says "decide later, verified by the audit".*
- ***NFR10 is not written into an AC below.** It specifies "read→broker-ACK latency p95 ≤ 3 s", and ADR 0010 established that no broker acknowledgement exists at the QoS 0 Sparkplug mandates — the same defect FR20 had. It needs the same amendment treatment before a story can discharge it. Tracked separately; Story 4.16 is blocked on it.*

### Story 4.1: Conformance matrix — framework, namespace and topic clauses

As the maintainer,
I want a clause-by-clause record of what the topic layer owes the specification,
So that conformance is a document I can audit rather than a belief.

**Acceptance Criteria:**

**Given** the vendored specification at `docs/spec/sparkplug-b-3.0.0/` (release tag v3.0.0, EPL-2.0) and `crates/sparkplug-b/src/topic.rs`
**When** every namespace and topic-grammar clause is walked
**Then** `docs/sparkplug-conformance.md` exists with one row per clause: the `tck-id-…` identifier, requirement level (MUST/SHOULD/MAY), our behaviour, the test that proves it, and a verdict of `conformant` / `deviation` / `gap`
**And** the matrix names the specification version it was built against — a conformance claim is meaningless without one, and a version change invalidates the matrix rather than merely dating it
**And** every `deviation` row carries a rationale and a link to the ADR or deferred-work entry that records it
**And** every `gap` row carries an issue number.

**Given** a row marked `conformant`
**When** the row names no test
**Then** the row is `gap`, not `conformant` — a behaviour nothing exercises is not a proven behaviour.

### Story 4.2: Conformance matrix — payload, metrics and datatype clauses

As the maintainer,
I want the payload encoding audited against the specification,
So that the hand-rolled protobuf is trustworthy for reasons beyond "Ignition accepted it once".

**Acceptance Criteria:**

**Given** the specification's payload and metric clauses
**When** `encode.rs`, `model.rs` and `datatype.rs` are walked against them
**Then** the matrix gains rows for: metric naming, datatype codes, `is_null` semantics, property sets, timestamp units and interpretation, and the `Quality` property
**And** the known deviation "no aliases, no templates, no DataSets" is recorded as a `deviation` with its rationale, not left implicit.

**Given** the quality-code defect found in Epic 1
**When** the matrix records the `Quality` property row
**Then** it states explicitly that the *values* are host-defined and were established by measurement (`quality_code_probe`), not by reading a table — the failure mode that caused contract v1.

**Given** the 109 `tck-id-payloads-*` clauses of chapter 6 — the whole set, verified to live in that chapter and nowhere else in the vendored specification
**When** the pass ends
**Then** every one of them is accounted for by a row or by a collective block that **names its member ids**, and the arithmetic `conformant + deviation + gap + n/a = 109` is stated in the matrix
**And** a clause satisfied by construction but exercised by no named test is recorded as a `gap`, not as a `conformant`
**And** a `gap` carries an owning story or epic where one exists, and a new issue where none does
**And** the Status table row for chapter 6 is updated.

*Added 2026-07-27 while contexting the story. Story 4.1 already carries the "no test named → `gap`" rule and closed with a chapter tally, but 4.2 inherited neither, so nothing obliged this pass to report how much of the chapter it had covered — an audit could have declared itself finished after twenty clauses of a hundred and nine. No ADR: this adds a completeness obligation, it reverses no position. The 109 is a measured count, not an estimate; the enumeration command is in the story file.*

### Story 4.3: Conformance matrix — session lifecycle and host interaction

As the maintainer,
I want the lifecycle and host-facing clauses audited,
So that the gaps we suspect are counted and the ones we do not suspect are found.

**Acceptance Criteria:**

**Given** the specification's session and host-interaction clauses
**When** they are walked against the implementation
**Then** the matrix gains rows for: birth/death ordering, `seq` numbering and wrap, `bdSeq` per CONNECT, NDEATH via will and explicit publish, NCMD/`Node Control/Rebirth`, DDEATH, and the primary-host STATE mechanism
**And** the two gaps already known — NCMD/Rebirth unimplemented, STATE never considered — appear as `gap` rows pointing at Stories 4.4–4.8.

**Given** the completed matrix
**When** Epic 4's remaining stories are reviewed against it
**Then** any newly discovered `gap` is either scheduled into this epic or recorded with an issue and an owning epic — no gap is left unassigned.

**Given** the **124 clauses** that no story owns after 4.1 (chapter 4) and 4.2 (chapter 6) — chapters **1, 2, 3, 5 and 10** — verified by mechanical enumeration to be the exact remainder of the vendored specification's **303** ids
**When** the pass ends
**Then** every one of them is accounted for by a row or by a collective block that **names its member ids**, and the arithmetic `conformant + deviation + gap + n/a = 124` is stated in the matrix
**And** the matrix states the whole-specification total — `70 + 109 + 124 = 303` — so a reader can tell audited-in-full from audited-in-part
**And** a clause satisfied by construction but exercised by no named test is recorded as `gap (unproven)`, not `conformant`
**And** every `gap` carries an owning story, epic or issue
**And** the Status table rows for chapters 1, 2, 3, 5 and 10 exist and are updated.

*Added 2026-07-28 while contexting the story. This story was scoped to "chapters 2 and 5" — 103 clauses. The specification holds 303, chapters 4 and 6 account for 179, so **21 clauses (chapters 1, 3 and 10) were owned by nobody**: the chapter-1 identifier character and uniqueness rules that sit underneath chapter 4's topic grammar, and chapter 10's conformance profiles — the specification's own statement of what claiming conformance means, which is the direct input to NFR19's documented conformance scope. Leaving them out would have produced per-chapter tallies that all close over a clause set that does not, which is exactly the defect the Story 4.2 code review found in chapter 4. No ADR: this adds a completeness obligation, it reverses no position. The 124 and the 303 are measured counts; the enumeration command is in the story file.*

### Story 4.4: Primary Host / STATE — measure what the host actually does

As the maintainer,
I want to observe the real primary-host mechanism before designing for it,
So that the decision rests on this deployment's behaviour rather than on a reading of the spec.

**Acceptance Criteria:**

**Given** the author's broker, which carries live `spBv1.0/STATE/…` topics
**When** a read-only observer records the STATE traffic (topic, payload, retain flag, QoS) across an Ignition restart
**Then** the findings are recorded: whether a primary host ID is configured, what the host publishes on going online and offline, and whether it expects edge nodes to react.

**Given** the observation
**When** it is written up
**Then** it states plainly what an edge node that ignores STATE — which is what the bridge does today — actually loses in this deployment.

*Read-only: this story publishes nothing. The broker is production and the only one available.*

### Story 4.5: Primary Host / STATE — decide, and record the decision

As the maintainer,
I want STATE either implemented or ruled out in writing,
So that it stops being an omission and becomes a position.

**Acceptance Criteria:**

**Given** Story 4.4's findings
**When** the decision is taken
**Then** a new ADR recording the Primary Host / STATE decision is written, with the observed behaviour as evidence
**And** if the decision is to implement, the ADR states what the bridge does when the primary host goes offline
**And** if the decision is to rule out, the ADR states the conditions under which it would have to be revisited.

**Given** the decision interacts with graceful shutdown
**When** that ADR is written
**Then** it states explicitly whether ADR 0011 (both deaths fire on SIGTERM) still holds unchanged — a primary host going offline may change *when* an edge node should stop publishing.

> **✅ DELIVERED 2026-07-31 — [ADR 0018](../../docs/adr/0018-no-primary-host-state-the-repair-is-host-initiated.md), *Primary Host / STATE is ruled out; the repair path is host-initiated*.** Both acceptance
> criteria are met by that document: it records the decision with Story 4.4's measurements as
> evidence, states three revisit conditions, and answers the ADR 0011 question explicitly (unchanged,
> and *why* — ruling STATE out removes the host-driven shutdown path before it exists, which is the
> thing that would have forced an amendment).
>
> The four grounds, none of which depends on a future measurement: the specification says *"Specifying
> a Primary Host is not required"*; without store-and-forward the wait preserves **zero** readings;
> one broker means the stranding it guards against cannot occur; and implementing it would introduce
> a **never-births** state that the observation record shows was the real broker state on 2026-07-28.
>
> ADR 0016's ordering argument is thereby formally spent — this is the re-weighing it asked for.

*This AC named "ADR 0012" until 2026-07-28. That number was free when the epic was drafted and was taken since, by the quality-code decision. The ADR is now referenced by subject rather than by number: the story is far enough out that any number written here can be consumed before it runs — which is exactly what happened. Number it at writing time.*

*And it happened **again, the same day**. This note read "0015 is next as of 2026-07-28" until the Story 4.3 code review consumed 0015 for the language-type-invariant witness a few hours later. It was then amended to "next free is 0016" — and **0016 was consumed on 2026-07-29** by the Story 4.4 review, for the Rebirth-before-Primary-Host ordering. **Three occurrences, and the third was caused by writing the digit down for the second time.** No number is recorded here now, and none should be: reference an ADR **by subject** and read `docs/adr/` for the digit at the moment you write it. **The prediction held a fourth time:** when this story's ADR was finally written on 2026-07-31 the next free number was **0018** — 0017 having been consumed hours earlier by the retained-NCMD decision from the Story 4.7 code review. A note that predicted a failure, suffered it, and then suffered it again while documenting it is as strong an argument for the mitigation as this file is going to produce.*

### Story 4.6: NCMD subscription — plumbing that ignores safely

As the bridge,
I want to receive node commands without acting on ones I do not understand,
So that an unknown command is never mistaken for a known one.

**Acceptance Criteria:**

**Given** a connected session
**When** the driver subscribes
**Then** it subscribes to the node's NCMD topic **at QoS 1**, as the specification requires (`tck-id-message-flow-edge-node-ncmd-subscribe`, `Sparkplug_5_Operational_Behavior.adoc:158-163`: *"It MUST subscribe on this topic with a QoS of 1"*), and it does so **before** the NBIRTH is published
**And** the subscription is re-established on every reconnect.

*Amended 2026-07-29 at Story 4.6 creation, twice. The id was written `tck-id-...-edge-node-subscribe-ncmd`, which does not exist — the real one reverses the last two words. And the ordering read "as part of the same post-CONNACK sequence that publishes NBIRTH", which permits birth-then-subscribe; the clause's section preamble (`:155-156`) says **"Prior to sending an NBIRTH message"**, which does not. Both are the failure `CLAUDE.md` names: reading about the specification instead of reading it.*

**Given** the SubAck arrives
**When** the driver handles it
**Then** a refused subscription (return code `0x80`) is traced at ERROR naming the topic, and a granted QoS lower than 1 is traced at WARN naming the granted value
**And** neither aborts the session: publishing without a command path is strictly better than not publishing.

*AC added 2026-07-29 at Story 4.6 creation and carried back here on completion. It exists because the Story 4.4 review found the STATE observer discarding exactly this byte, which made a refused subscription indistinguishable from a quiet topic — the same byte, the same mistake, one file away from the code this story writes.*

**Given** an NCMD payload the bridge does not recognise
**When** it arrives
**Then** it is traced at INFO with the metric names it carried, and otherwise ignored
**And** a malformed payload is traced at WARN and ignored — never a panic, never a partial application
**And** a payload that decodes but carries no metrics is traced too, not silently dropped.

**Given** the mqtt driver task
**When** an NCMD is handled
**Then** no quality or staleness decision is taken there: the confinement guard in `arch_purity` still holds.

*Delivered 2026-07-29. The subscription and the SubAck check are proven by `chaos_ncmd_subscription`, which reads the ordering and the requested QoS out of the **broker's** verbose log — one MQTT client cannot observe another's SUBSCRIBE, so the broker is the only external witness available. `MessageType::NCmd` was added to the published `sparkplug-b` crate; `DCmd` deliberately was not, because `tck-id-message-flow-device-dcmd-subscribe` is conditional on a device supporting writable outputs and none here does. Two conformance rows moved: `-ncmd-subscribe` (ch. 5) and `topics-ncmd-topic` (ch. 4).*

### Story 4.7: `Node Control/Rebirth` — answer with a fresh birth (FR19)

As the SCADA,
I want a rebirth request answered with a complete re-announcement,
So that I can resynchronise without waiting for the bridge to reconnect on its own.

**Acceptance Criteria:**

*Three criteria were written here at drafting. **Two were amended and four added** when the story was
contexted (2026-07-30) and carried back here after implementation. The reasons are recorded in the
story file under* Dev Notes → What the epic gets wrong, and what it leaves out; *the two that matter
most are that the epic never mentioned the metric five MUST clauses require in every NBIRTH, and
that its cloud-unreachable criterion, implemented literally, would have destroyed true history.*

**AC1 — the NBIRTH declares the command** *(added)*
**Given** any NBIRTH — first birth, reconnect birth, or rebirth answer
**Then** it carries a metric named exactly `Node Control/Rebirth`, datatype `Boolean`, value `false`, and **no alias**
**And** it is on *every* NBIRTH, not only the first.
*Five MUSTs in three chapters, all unmet before this story: `tck-id-topics-nbirth-rebirth-metric`, `tck-id-payloads-nbirth-rebirth-req`, and `-rebirth-name` / `-rebirth-datatype` / `-rebirth-value`. Without the metric a host has no declared endpoint to address, so the handler is unreachable by a conformant host.*

**AC2 — a request is answered with a complete BIRTH sequence**
**Given** an NCMD carrying `Node Control/Rebirth` with boolean value `true`
**When** the driver handles it
**Then** it republishes NBIRTH followed by one DBIRTH per meter, `seq` reset to 0 and continuing 1, 2, …
**And** the answer is traced at **INFO**, naming the topic — visible under the default log filter, with no `RUST_LOG` set.

**AC3 — DATA stops on receipt and does not resume until the sequence is out** *(added)*
**Then** no DATA message is published between the request and the last DBIRTH
**And** the property is asserted by a test that would go red if a DATA could interleave — not argued from the shape of the loop.
*`-rebirth-action-1` is a MUST the epic omitted entirely. The bridge satisfies it by construction, which is exactly why it needed an assertion.*

**AC4 — the answer re-announces what is known, and never invents a reading** *(amended)*
**Given** a meter that has **never** produced a reading
**Then** its DBIRTH metrics are valueless (`Null(Double)`) with quality `Stale`, stamped with the birth's own timestamp — identical to cold start.
**Given** a meter that **has** a reading, however old
**Then** it is re-declared with its **own `ValueDate`** as the payload timestamp, never `now`, and its quality is degraded, never upgraded.
*The epic said a rebirth during a cloud outage yields metrics "with no value and quality `Stale`, exactly as at cold start". That is right for the first case and **wrong** for the second: blanking a value the bridge can account for destroys true history on the grounds that the cloud is currently down, which has no bearing on whether the last reading was real. It would also have turned an existing, correct test red.*

**AC5 — a rebirth re-announces a session, it does not open one** *(amended)*
**Given** a rebirth is answered
**Then** the NBIRTH's `bdSeq` is unchanged from the will registered at CONNECT
**And** the assertion is made **through the NCMD path**, not through the reconnect path that already has coverage.
*`-rebirth-action-3`. Concretely: `new_session()` must not be called on this path.*

**AC6 — the norm's reading, and a trace that records what actually arrived** *(added)*
**Given** `Node Control/Rebirth` whose value is boolean `false`, non-boolean, or absent
**Then** it is **not** answered — `-ncmd-rebirth-value` defines the request as carrying `true`
**And** it is traced distinctly from both an unrecognised command and an answered one, recording the metric's **datatype and value exactly as received**
**And** an alias-addressed metric with no name is not treated as a request.
*A strict matcher's failure mode is that it never fires, silently. The trace is the whole mitigation, and it is what lets the Story 4.8 run diagnose itself.*

**AC7 — every document and test that says the bridge answers no command is corrected** *(added)*
**Then** each falsified passage is amended or explicitly confirmed still-true with its reason, reported as a **per-passage table**
**And** the seven conformance rows this story owns move off `gap (unimplemented)`, with three tallies recomputed
**And** `chaos_ncmd_subscription`'s inverted assertions are re-aimed rather than deleted.

*Unblocks a recorded deviation. `tck-id-principles-rbe-recommended` says data SHOULD NOT be published periodically; the bridge publishes on every poll, and one of the author's four meters is physically unplugged, so byte-identical content goes out for it roughly 17 000 times a day. Report-by-exception could not be implemented before this story, because the periodic publish is what substituted for the missing Rebirth — without it a late-joining consumer would never learn an unchanging meter's value. **Discharged 2026-07-30 without implementing RBE:** the blocker has lifted, the deviation's verdict is unchanged, and its *reason* moved from "cannot safely be changed" to "has not been decided". The residual question — a host that never asks never learns — belongs to [#32](https://github.com/guycorbaz/smartme_mqtt/issues/32) and its own story. Recorded 2026-07-28; the PRD and architecture were corrected then, having claimed report-by-exception the bridge has never done.*

### Story 4.8: Extend the Tier-3 gate to NCMD/Rebirth — close NFR17

As the maintainer,
I want the manual contract test to exercise a real Ignition-issued rebirth,
So that NFR17 is covered by the artifact it names.

**Acceptance Criteria:**

**Given** `crates/sparkplug-b/tests/ignition_contract.rs`
**When** a step is added for rebirth
**Then** it instructs the operator to issue a rebirth from Ignition and to confirm the node re-announces, with the checklist stating what could make the step pass for the wrong reason
**And** the runbook's run table gains a column or note recording that NCMD/Rebirth was exercised.

**Given** NFR17's coverage note in `epics.md`
**When** this story is done
**Then** the note stops saying "the NCMD/Rebirth half is Epic 4" and records the version it was verified against.

### Story 4.9: Give `chaos_sigterm_no_lie` a discriminator that survives per-CONNECT `bdSeq`

As the maintainer,
I want the SIGTERM proof to stop depending on the will being stamped once,
So that Story 4.10 can change that without silently disarming the test.

**Acceptance Criteria:**

**Given** `chaos_sigterm_no_lie` distinguishes the explicit NDEATH from the will by comparing payload timestamps
**When** the will is rebuilt per CONNECT (Story 4.10)
**Then** that discriminator no longer discriminates — ADR 0011 records this explicitly.

**Given** the test
**When** a new discriminator is introduced
**Then** it does not rest on the will's timestamp
**And** it is falsified before being trusted: with the explicit publish removed, the test fails; with it restored, it passes.

*This story precedes 4.10 deliberately. Reversing the order would leave a window in which the test passes for a reason that has stopped being true.*

### Story 4.10: Own the reconnect loop — `bdSeq` per CONNECT

As the bridge,
I want a new session number on every CONNECT, as the specification requires,
So that a consumer pairing death to birth is never handed a certificate for a session that no longer exists.

**Acceptance Criteria:**

**Given** the recorded deviation in `mqtt_driver.rs` — `bdSeq` fixed for a client's lifetime because the will cannot be updated after construction
**When** the driver owns its reconnect loop and rebuilds the client per CONNECT
**Then** `bdSeq` advances on each CONNECT and the will registered in that CONNECT carries the same number
**And** the module documentation's "recorded deviation" section is replaced by a statement of the conforming behaviour.

**Given** a reconnect
**When** the new session births
**Then** the NDEATH the broker holds carries the new session's `bdSeq`, verified from an independent subscriber.

**Given** `bdSeq` is persisted
**When** it advances per CONNECT rather than per boot
**Then** the persistence path is exercised at reconnect frequency, and the deferred concern "persisted once at boot" is closed or restated.

### Story 4.11: Broker-outage policy — the traced drop, exhaustively (FR22, AR7)

As the operator,
I want every reading the bridge could not hand over to be visible,
So that a broker outage reads as loss, never as silence.

**Acceptance Criteria:**

**Given** a full outbound queue or a broker that is down
**When** a reading cannot be handed to the transport
**Then** it is counted per meter and per reason, and traced at WARN carrying the reading's source timestamp
**And** no persistent buffer is introduced: the policy is a traced drop, per AR7.

**Given** a sustained outage
**When** readings are dropped throughout
**Then** memory and file descriptors stay bounded — the drop path allocates nothing that survives it.

### Story 4.12: Anti-replay at the down→up instant

As the SCADA,
I want nothing re-timestamped when the broker comes back,
So that an outage reads as a gap rather than as a burst of fresh data.

**Acceptance Criteria:**

**Given** a broker that returns after an outage
**When** the bridge reconnects and rebirths
**Then** every published Sparkplug timestamp equals its source `ValueDate` — verified at the reconnection instant, not merely in steady state
**And** no reading acquired during the outage is published with a post-outage timestamp.

**Given** the rebirth that follows reconnection
**When** it re-declares the last known reading
**Then** that reading is degraded to `Stale` and stamped with its own `ValueDate`.

### Story 4.13: `chaos_broker_recovery`

As the maintainer,
I want the down→up transition proven from outside,
So that anti-replay is a measured property rather than a reviewed one.

**Acceptance Criteria:**

**Given** a running bridge and a broker container that is stopped and restarted
**When** an independent subscriber records the whole sequence
**Then** it observes: the node dying, the node rebirthing, and no published timestamp that post-dates its own `ValueDate`
**And** the test is falsified before being trusted — with the anti-replay stamping broken, it fails.

### Story 4.14: `chaos_poller_wedge`

As the maintainer,
I want a wedged poll loop to be detectable from outside the process,
So that lying by omission is caught the same way lying by commission is.

**Acceptance Criteria:**

**Given** a source that hangs beyond every deadline
**When** the poll loop is wedged
**Then** an independent subscriber sees the affected metrics go `Stale` rather than simply stop updating
**And** the liveness heartbeat visibly stops advancing, so an external health check can distinguish "wedged" from "idle".

### Story 4.15: `AC-LEAK-01` — resource stability under sustained load (NFR3, NFR9)

As the operator,
I want the bridge to run for weeks on a NAS without growing,
So that a leak surfaces here rather than on the fourth epic built on top of it.

**Acceptance Criteria:**

**Given** a 100k-iteration run with the transport exercised
**When** RSS is sampled every 60 s and file descriptors are counted via `/proc/self/fd`
**Then** RSS_max ≤ 100 MB, the RSS slope by linear regression is ≤ 1 %/24 h, and FD ≤ 64
**And** the measured figures are recorded, not merely asserted — a threshold met by a wide margin and one met barely are different results.

**Given** the run
**When** it is executed
**Then** it is feature-gated or `#[ignore]`d so it never runs in the ordinary `cargo test` path.

### Story 4.16: Latency budget (NFR10) — BLOCKED

As the operator,
I want a stated latency budget from reading to publication,
So that "a new reading reaches MQTT within one poll cycle" is measured rather than assumed.

**Blocked.** NFR10 specifies "read→**broker-ACK** latency p95 ≤ 3 s, p99 ≤ 5 s", and ADR 0010 established that MQTT defines no acknowledgement at the QoS 0 Sparkplug mandates — the same defect FR20 carried. The measurable analogue is read→accepted-for-transmission, which is a weaker claim and must be agreed rather than substituted quietly.

This story stays unwritten until NFR10 is amended. Recording it as blocked, rather than writing an acceptance criterion around the problem, is the rule adopted at the Epic 1 retrospective.

### Story 4.17: Fix the Will QoS — the specification says 1, we send 0

As the bridge,
I want my death certificate registered at the QoS the specification requires,
So that the broker is obliged to deliver it rather than permitted to lose it.

**Acceptance Criteria:**

**Given** the Sparkplug B specification, chapter 5
**When** `tck-id-message-flow-edge-node-birth-publish-will-message-qos` is applied — *"The Edge Node's MQTT Will Message's MQTT QoS MUST be 1"*
**Then** the will is registered at QoS 1 with retain false
**And** `qos_for` stops returning a single value for every message type, since the specification does not.

**Given** the unit test `every_edge_node_message_is_qos_zero_and_never_retained`
**When** it is revisited
**Then** it is replaced by one that encodes what the specification actually requires — QoS 0 for NBIRTH and DBIRTH, QoS 1 for the will, retain false throughout — because the current test locks in the violation and would block the fix.

**Given** the change
**When** it reaches the wire
**Then** an independent subscriber confirms the will still fires on an ungraceful disconnect, and `chaos_stale_on_death` still passes.

*Found before the audit formally began, by reading the specification in response to Guy asking whether QoS 0 was a liberty we had taken. It was not: it is mandated for births and forbidden for the will.*

### Story 4.18: Correct ADR 0010's wording — its conclusion stands, its premise is overstated

As the maintainer,
I want ADR 0010 to say something true about the specification,
So that the next person reasoning from it is not misled the way we were.

**Acceptance Criteria:**

**Given** ADR 0010, which states *"The Sparkplug B specification requires QoS 0 for **every** edge-node message (NBIRTH/NDATA/NDEATH/DBIRTH/DDATA/DDEATH). Only host STATE messages use QoS 1"*
**When** it is checked against the vendored specification
**Then** its **conclusion is confirmed**: `topics-ndata-mqtt` and `topics-ddata-mqtt` both require QoS 0 with retain false, so no broker acknowledgement exists for data and FR20 was genuinely unimplementable as written
**And** its **premise is corrected**: the blanket "every edge-node message" is wrong — the Will Message MUST be QoS 1 (chapter 5), and chapter 4 states no QoS for NDEATH at all.

**Given** the correction
**When** it is applied
**Then** ADR 0010 gains an addendum citing the tck-ids per message type rather than a blanket claim, and records that the overstatement was what made the will violation (#26) invisible for an epic.

**Given** NFR10's "read→broker-ACK latency"
**When** resolved in the same pass
**Then** it is amended to a measurable analogue (read→accepted-for-transmission), since the QoS-0 requirement for data is confirmed and no ACK will ever exist. Story 4.16 unblocks on that amendment.

*History worth keeping: this story was first written to **re-open** ADR 0010, on my claim that DATA carried no QoS requirement. That claim came from grepping chapter 5 alone and concluding from absence. Chapter 4 has the requirements. The vendored specification caught the error within minutes of being vendored — which is the argument for vendoring it.*

### Story 4.19: Finish chapter 4 — the 29 clauses Story 4.1 did not record

As the maintainer,
I want chapter 4's payload clauses audited and its tally made to close,
So that "chapter 4 is done" is a countable claim rather than a remembered one.

**Acceptance Criteria:**

**Given** `Sparkplug_4_Topics.adoc`, which carries **70** `tck-id`s, and `docs/sparkplug-conformance.md`, which records **41** of them
**When** the remaining **29** are walked against the implementation
**Then** each gains a row or a place in a collective block that **names its member ids**, under the existing chapter-4 headings
**And** the chapter-4 tally is restated so that `conformant + deviation + gap + n/a = 70`
**And** the Status table row for chapter 4 changes from **audited, not complete** to **done**.

**Given** the 29 are, by shape, chapter 4's own *payload-content* requirements — 26 `topics-*` (`-nbirth-metrics`, `-nbirth-metric-reqs`, `-nbirth-seq-num`, `-nbirth-timestamp`, `-nbirth-templates`, the three `-nbirth-bdseq-*`, `-nbirth-rebirth-metric`, and their DBIRTH/NDATA/DDATA/NCMD/DCMD/NDEATH/DDEATH counterparts) plus 3 `host-topic-phid-death-payload-timestamp-*`
**When** each is ruled on
**Then** no verdict is copied from its chapter-6 twin — the twins are cited as cross-references, and each clause is read against chapter 4's own wording
**And** `topics-nbirth-bdseq-increment` is recorded as a `deviation` owned by **Story 4.10**, not as a new finding: it is chapter 4's statement of the frozen-`bdSeq` defect chapter 6 already records at `payloads-nbirth-bdseq-repeat`
**And** `topics-nbirth-rebirth-metric` points at **Story 4.7**, and `topics-nbirth-templates` at the scope-limit deviation, rather than opening new issues for requirements already owned.

**Given** the completed chapter
**When** the whole-specification arithmetic is restated
**Then** `70 + 109 + 124 = 303` holds with every chapter fully recorded, and the matrix says so.

*Created 2026-07-28, out of the Story 4.2 code review. Story 4.1 audited the chapter's **topic grammar** and left the payload requirements the same chapter also states; nothing at the time obliged it to report how much of the chapter it had covered, since the completeness rule was only added to 4.2 and 4.3 afterwards. The count was 27 in the review's own report and is **29** on an independent recount — the review under-counted, which is worth noting about numbers produced by a single pass. Deliberately a separate story rather than re-opening 4.1: 4.1's work is correct as far as it goes and its commits are pushed; this is the remainder, and it is cheaper to schedule than to retro-fit.*
