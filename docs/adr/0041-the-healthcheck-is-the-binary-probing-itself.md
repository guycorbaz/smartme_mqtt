# ADR 0041 — The healthcheck is the binary probing itself

- **Status:** accepted
- **Date:** 2026-08-20
- **Decides:** the choice [#56](https://github.com/guycorbaz/smartme_mqtt/issues/56) left to Epic 7, deliberately and on the record.
- **Issue:** [#56](https://github.com/guycorbaz/smartme_mqtt/issues/56)

## Context

The Dockerfile has carried `# NO HEALTHCHECK, and its absence is a decision` since Epic 0,
with the reason spelled out: a process-liveness probe would be decoration (the bridge stays
alive and correct through a cloud outage) and any probe failing on a stale reading would
restart the container in the one case where restarting destroys what the STALE was protecting.
The line was to land *"in the same change as the endpoint it probes"*. That endpoint —
`/healthz`, with `last_loop_tick`, the wedge allowance, and the sink's state since story 6.5 —
now exists, so this is that change.

**And [#56] found the obstacle that makes it a decision rather than a line of Dockerfile.**
The runtime image is `debian:bookworm-slim` with `ca-certificates` as its only added package:
**no `curl`, no `wget`**, and `scripts/docker-smoke.sh` already records that the image's shell
is not bash, so `/dev/tcp` is unavailable too. **Nothing inside the container can consume
`/healthz` today**, which also means the 503 path has never been exercised the way a real
deployment will exercise it.

[#56] names three ways out and says Epic 7 must choose one deliberately.

## Decision

**The binary probes itself: `smartme-bridge --healthcheck` performs one HTTP GET against its
own UI port and exits 0 or 1.** `HEALTHCHECK` invokes that.

**Why this one, and not the other two:**

- **It cannot drift from the endpoint it checks.** The probe reads the same `ui_port`
  resolution the server uses — `config.toml`, then `SMARTME_UI_PORT`, then `DEFAULT_PORT`
  ([ADR 0037]) — so a port change moves both halves at once. A `curl http://localhost:8080/…`
  in the Dockerfile hard-codes a port that ADR 0037 made configurable, and would silently
  probe the wrong one the day somebody uses that.
- **It adds no package.** `reqwest` is already linked into this binary; a `curl` in the image
  is a few MB and a new supply-chain surface for one request per thirty seconds.
- **The third option — probing from outside the container — cannot satisfy AR12 at all.**
  AR12 is "restart a wedged poller"; a probe outside Docker's healthcheck has nothing to
  restart with, so choosing it would mean recording that AR12 is not implemented rather than
  implementing it.

**What the probe must NOT do**, and this is half the decision: it reports what `/healthz`
reports and adds no opinion. `/healthz` returns non-200 in exactly one state — a publishing
bridge whose poll loop has not ticked in three periods — and that rule was argued in ADR 0027
§2 and defended by story 6.5 for the sink. The probe exits non-zero on that and on a failure to
reach the endpoint at all; **an unreachable broker, a failed source and a degraded meter are
all healthy to it**, because a restart repairs none of them and would destroy every meter's
Sparkplug session.

## Consequences

- `main.rs` grows one argument, and exactly one: `--healthcheck`. Anything else is refused
  with a message rather than ignored, so a typo in a `HEALTHCHECK` line cannot silently start
  a second bridge inside the container.
- The `HEALTHCHECK` line lands with an interval matched to the wedge allowance rather than
  guessed at, and with `start-period` covering a first run — a bridge that has never been
  configured is deliberately silent, not unhealthy.
- **The 503 path becomes reachable in a real deployment for the first time**, which is what
  [#56] says has never been true. A chaos test that wedges the poller and reads the container's
  health status is the falsification this decision asks for.
- Nothing about the wire changes. `CONTRACT_VERSION` is untouched.

[ADR 0037]: 0037-the-first-run-port-is-bootstrap-not-configuration.md
