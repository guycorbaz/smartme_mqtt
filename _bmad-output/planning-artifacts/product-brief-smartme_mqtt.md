---
title: "Product Brief: smartme_mqtt"
status: "complete"
created: "2026-07-24"
updated: "2026-07-24"
inputs:
  - "User brief (autonomous launch args)"
  - "Web research: smart-me API, MQTT/SCADA integration norms"
---

# Product Brief: smartme_mqtt

## Executive Summary

**smartme_mqtt** is an open-source bridge that connects [smart-me](https://smart-me.com) cloud energy meters to any MQTT-based monitoring system. It polls a user's smart-me devices through the official smart-me REST API, normalizes the readings, and publishes instantaneous power and cumulative energy to an MQTT broker — where a SCADA, HMI, or home-automation platform can consume them in real time.

Today, smart-me owners who want their metering data inside a SCADA or industrial dashboard have no purpose-built path. They must hand-roll REST polling in Node-RED, wire up brittle Home Assistant REST sensors, or write throwaway scripts — none of which speak the MQTT language that SCADA/HMI systems expect. **smartme_mqtt closes that gap** with a single, self-hosted service: configure it once through a web interface, `docker compose up`, and your energy data flows onto MQTT with SCADA-friendly topics and units.

Built in **Rust** for a small, dependable footprint, distributed as source on **GitHub** and as a ready-to-run image on **Docker Hub**, it is designed to run unattended 24/7 on modest hardware (a NAS, a Raspberry Pi, a small VM) — exactly where energy-monitoring gateways live.

## The Problem

A smart-me meter measures your electrical reality every second — active power, consumed energy, voltage, current — and exposes it through a clean cloud API. But the systems people actually use to *watch* that reality — SCADA platforms, industrial HMIs, home-automation dashboards — don't speak REST-with-polling. They speak **MQTT**.

So owners are stuck bridging the gap themselves:

- **Node-RED / custom scripts** — every user reinvents the same polling loop, error handling, and topic layout, then maintains it forever.
- **Home Assistant REST sensors** — fragile, still using deprecated Basic Auth, and awkward to map onto industrial tag structures.
- **No standard topics or units** — is power in W or kW? Is the energy counter monotonic? Each ad-hoc solution answers differently, so nothing is reusable or trustworthy across sites.

The cost is real: hours of glue-code per deployment, silent data staleness when the API hiccups, and no clean way to feed a SCADA historian. There is currently **no dedicated smart-me→MQTT bridge on GitHub** — the need is unmet, not merely underserved.

## The Solution

smartme_mqtt is a single self-hosted service that does one job well:

1. **Poll** the smart-me REST API on a configurable interval, using a modern **API Key** (with Basic Auth as a fallback for legacy accounts).
2. **Normalize** each device's readings — `ActivePower` → power, `CounterReading` → energy — with explicit, consistent units and the meter's own `ValueDate` timestamp for staleness detection.
3. **Publish** to MQTT using a stable, human-readable, SCADA-friendly topic structure (e.g. `smartme/<site>/<serial>/power`), with retained last-value messages so a freshly-connected dashboard sees current data immediately, and a Last-Will `status` topic so consumers can flag an offline bridge.
4. **Configure, preview & diagnose** everything through a **built-in web interface** — set smart-me credentials, device selection, poll interval, broker details, and topic layout; preview live values as they're read; and inspect diagnostics (connection state, last poll, errors) without leaving the browser.

The whole thing starts with `docker compose up`, optionally bundling a Mosquitto broker for turnkey deployments. Secrets (the API key) stay outside the image, injected at runtime. One instance serves **one smart-me account** (with all its devices); users needing multiple accounts simply run multiple instances. It writes **daily-rotated log files** so any issue can be traced after the fact.

## What Makes This Different

- **Purpose-built, not glue** — the only tool that targets the smart-me *cloud* API specifically and speaks MQTT natively. No scripting, no Node-RED flow to maintain.
- **SCADA-first data model** — deliberate topic hierarchy, explicit engineering units, retained state, ISO-8601/UTC timestamps, and staleness signalling. Data a historian can trust, not just raw JSON dumped on a topic.
- **Interoperability out of the box** — optional **Home Assistant MQTT discovery** for instant auto-registration in home setups, on top of a clean, SCADA-mappable topic layout.
- **Operationally boring, on purpose** — a compact Rust binary in a small container, low memory/CPU, no runtime VM. It's meant to run for months untouched.
- **Genuinely open** — **MIT-licensed**, public image on Docker Hub, so anyone can self-host or extend it commercially.

## Who This Serves

- **Primary — the technical smart-me owner / integrator** running a home lab, a building-monitoring setup, or a small industrial site. They already have a SCADA/HMI or Home Assistant and want smart-me data in it without writing code. Success = data visible in their dashboard within minutes of `docker compose up`.
- **Secondary — energy / facility engineers** who need a reliable, auditable feed of power and energy into a historian for trending and reporting.
- **Tertiary — the open-source / self-hosting community** who will star, fork, and extend the project, contributing new publish modes and device coverage.

## Success Criteria

- **Time-to-first-value:** a new user gets live power + energy on an MQTT topic in **under 15 minutes** from a clean machine.
- **Reliability:** runs **weeks unattended**; API/broker outages recover automatically without manual restart; staleness is signalled, never silently wrong.
- **Correctness:** published units and timestamps match the meter; energy counters behave as monotonic accumulators downstream.
- **Adoption signals:** GitHub stars/forks, Docker Hub pulls, and community-contributed integrations (HA discovery confirmed working, first Sparkplug B user).
- **Footprint:** idle resource usage low enough to co-exist on a Raspberry Pi alongside other services.

## Scope

**In (v1):**
- REST polling of smart-me devices for **one smart-me account** (API Key auth; Basic Auth fallback).
- Publishing instantaneous power and consumed energy to MQTT with configurable, retained, SCADA-friendly topics + LWT status.
- Web UI for **configuration, live value preview, and diagnostics** (connection state, last poll, errors).
- **Daily-rotated log files** for after-the-fact debugging.
- Docker image on Docker Hub + `docker compose` deployment, optional bundled Mosquitto.
- Home Assistant MQTT discovery.
- **MIT license.**

**Out (deferred / not planned):**
- smart-me **Realtime/Webhook** (protobuf push) ingestion — considered for v2 as an alternative to polling.
- Device **control/actions** (switching outputs) — read-only in v1.
- **Sparkplug B** — out of scope for now; a possible future addition (the publish layer should stay open to it, see Technical Approach).
- **Multi-account in a single instance** — served by running one instance per account instead.
- Long-term storage / built-in historian / charting — that's the SCADA's job, not ours.
- Multi-tenant / hosted SaaS offering — this is self-hosted software.

## Vision

Start as the definitive smart-me→MQTT bridge; grow into the **reference open-source gateway for cloud energy meters onto industrial and home messaging fabrics**. With webhook ingestion for sub-second data and additional metering back-ends behind the same normalized MQTT model, smartme_mqtt becomes the boring, trusted piece of infrastructure that quietly makes energy data available everywhere it's needed — the "Zigbee2MQTT of cloud energy meters."

---

## Technical Approach (high level)

- **Language:** Rust — compact static binary, low idle footprint, no GC pauses, strong fit for a long-running 24/7 container.
- **Likely building blocks:** `tokio` async runtime; `reqwest` for the smart-me REST client; `rumqttc` for the MQTT publisher; `axum` (+ a lightweight embedded UI) for the configuration web interface; `serde` for (de)serialization; `config`/env-var layering for secrets.
- **Deployment:** multi-stage Docker build → small runtime image on Docker Hub (pinned semver + `latest`); `docker-compose.yml` wiring the bridge and an optional Mosquitto broker; API key injected at runtime, never baked into the image.
- **Extensibility:** put the publish path behind an abstract `Publisher` trait (v1 ships a plain-MQTT implementation) so an optional **Sparkplug B** publisher can be added later without reworking the core.
- **Key design risks to resolve in architecture:** graceful handling of smart-me API rate limits and outages; correct W-vs-kW unit handling; monotonic energy-counter/rollover semantics; safe storage of credentials entered via the web UI; and the polling-vs-webhook decision boundary.
