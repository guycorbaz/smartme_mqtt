# ADR 0019 — The configuration UI carries no authentication; secrets are write-only

- **Status:** Accepted
- **Date:** 2026-08-03 *(recording a decision Guy took on 2026-08-01)*
- **Implements:** **NFR14** — *"Web UI exposure safe-by-default (bind/auth posture decided in
  architecture); credentials never re-shown in clear"*. The write-only rule below is not a new
  requirement; it is NFR14's second clause made testable, and the *"decided in architecture"* of its
  first clause is what this ADR finally decides. `prd.md:183` flagged the same thing as a domain
  constraint and deferred it here.
- **Related:** architecture open items **2** (*"Web UI network bind + auth posture"*) and **5**
  (*"Broker/token secrets-at-rest boundary … Coupled with item 2"*), Epics 5 and 6,
  [ADR 0009](0009-smartme-auth-client-credentials.md)
- **Supersedes** the BasicAuth-at-Traefik position taken earlier on 2026-08-01 and never written
  down. It stood for a few hours; it is recorded here only because a reader finding Traefik
  middleware in an old note deserves to know it was withdrawn.

## Context

Epics 5 and 6 add a configuration web UI at `smartme.home.arpa`, fronted by the homelab's Traefik.
It will own the values that today live in twelve raw environment variables — including the smart-me
`SMARTME_CLIENT_SECRET` and, if the broker ever gains credentials, `SMARTME_BROKER_PASSWORD`.

Open item 2 framed the question as *loopback-default versus documented trusted-network*. It has sat
open since the architecture was written, and the UI cannot be built without answering it: the answer
decides whether there is a login form, a session store, a password reset, and a second secret to
manage.

The deployment is a single-user homelab. The broker on the same LAN is **already unauthenticated**,
deliberately and permanently — some devices on it cannot present credentials at all — so the network
this UI would sit on is one where an attacker already has the meter data by subscribing to MQTT.

## Decision

**No authentication.** The UI is served without a login, on the trusted LAN, behind Traefik. No
session store, no credential of its own, no password to rotate.

**And the rule that makes it survivable — secrets are write-only.** A secret may be *submitted*
through the UI and *stored*; it may never travel outwards:

1. never rendered in any HTTP response, including as a `value=` attribute on a form field or a
   masked placeholder derived from the real value;
2. never present in the enriched published state (the Sparkplug payload the bridge publishes);
3. never traced — not at any level, not in a debug struct, not in an error message that formats a
   config object.

A field that holds a secret renders **empty**, and an empty submission means *unchanged*, not
*cleared*.

## Why the rule is the load-bearing half

Removing authentication makes open item 5 **more** load-bearing, not less. With a login, a rendered
secret is exposed to whoever got past the login; without one, it is exposed to whoever can reach the
host. So the decision to skip authentication is only defensible if the UI never becomes a *read*
oracle for the credentials it holds.

The project has been caught by exactly the failure this rule prevents. A `sed` mask written for
`KEY=value` was applied to `docker compose config`'s YAML, matched nothing, and printed both
credentials in full — see the *never render secrets* rule. **A mask is a claim about output format,
and it fails silently when the format changes.** Not rendering at all cannot fail that way.

## A question this ADR surfaced — SETTLED the same day by [ADR 0021](0021-configuration-is-editable-from-the-ui.md)

> **Resolved 2026-08-03.** Guy added **FR46** (configuration is editable from the UI, FR23 rescoped
> to the bootstrap path) and split ownership: **Epic 5 owns the configuration model, Epic 6 owns the
> screens.** The deciding argument was this ADR's own rule — the write-only property becomes
> falsifiable in Epic 5, at the process boundary, before any form exists. The finding as first
> written follows.

**No epic owned configuration *editing* in the UI.** The PRD is unambiguous that it exists — the
product is *"configured, previewed, and diagnosed through a built-in web UI"* (`prd.md:28`), and
Journey 1 has Guy clicking *"Test connection"* — but the epic split does not reflect it. Epic 5 is
described in terms of *"`.env` secrets discipline"*, and Epic 6 is *Observability, Diagnostics & the
State-of-the-Bridge UI*, which is a **read** surface. FR23 still says credentials arrive *"via
environment/`.env`"*, and no FR says the operator may change configuration through a browser.

This ADR assumes a form that submits a secret. That assumption is sound against the PRD's prose and
NFR14, and unowned by any epic. **Settling it is a scope decision, not an architectural one**, so it
is recorded here and left to Guy rather than answered by an ADR that would be inventing requirements
it also depends on.

## Consequences

- Epic 5's acceptance criteria must carry the write-only rule as a testable property, not as
  guidance. The natural form is an automated test that submits a secret, requests every page, and
  asserts the value appears in no response body — armed by first making it fail.
- **The bind address becomes the whole of the network posture**, since nothing else stands in front
  of the UI. It is a Traefik-fronted service, so the container should not publish the port to the
  LAN itself; the reverse proxy is the only ingress.
- Two of the six unfilled `.env` values disappear — the UI basicauth pair is gone.
- `SMARTME_CLIENT_SECRET` moves from an environment variable to stored configuration, which is what
  open item 5 is about. **This ADR does not settle where it is stored** (same file, separate file,
  `0600`, Docker secret) — that remains open item 5, and it is now unblocked rather than *"coupled
  with item 2"*.
- **This is reversible and cheaply.** Adding authentication later costs a form and a session; the
  write-only rule is what would be expensive to retrofit, and it is being adopted now.

## What this decision does NOT claim

It does not claim the LAN is trustworthy in general. It claims that *this* UI adds no exposure the
broker does not already have, which is a narrower and checkable statement. If the broker ever gains
authentication ([#20](https://github.com/guycorbaz/smartme_mqtt/issues/20) proposes TLS), that
premise weakens and this ADR should be re-weighed — the way ADR 0016's ordering argument was
re-weighed once its premise expired.
