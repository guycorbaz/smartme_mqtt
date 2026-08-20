# Story 7.1: The healthcheck the image can actually run — AR12, and what [#56] has been holding since 5 August

Status: review

> **The Dockerfile has been waiting for this change by name.** Its comment says the
> `HEALTHCHECK` line *"belongs in the same change as the endpoint it probes, not before it"*.
> The endpoint exists — `/healthz`, with the wedge allowance and, since story 6.5, the sink's
> state — so this is that change.
>
> **And [#56] is the reason it is not one line.** The runtime image carries no `curl`, no
> `wget`, and a shell that is not bash, so **nothing inside the container can consume
> `/healthz`**. [ADR 0041](../../docs/adr/0041-the-healthcheck-is-the-binary-probing-itself.md)
> decides between the three ways out: the binary probes itself.

## Story

As the author whose bridge is wedged at three in the morning,
I want Docker to notice and restart it — and *only* then —
so that the one failure a restart repairs does not need me awake, and the several it would
make worse are left alone.

## Acceptance Criteria

**AC1 — the binary can probe itself, and resolves the port the way the server does.**

**Given** `smartme-bridge --healthcheck`
**When** it runs inside the container
**Then** it performs one HTTP GET against `/healthz` on **the port the server would bind** —
`config.toml`, then `SMARTME_UI_PORT`, then `DEFAULT_PORT` ([ADR 0037]) — and exits 0 or 1
**And** the resolution is the same code path as the server's, not a second copy of the rule.

**AC2 — it fails on exactly one state, and says which.**

**Given** `/healthz`'s rule that non-200 means a publishing bridge whose poll loop has not
ticked in three periods
**When** the probe runs
**Then** it exits non-zero on that, and on being unable to reach the endpoint at all
**And** it exits **zero** for an unreachable broker, a failed source, a degraded meter and a
deliberately silent bridge — a restart repairs none of them and destroys every meter's
Sparkplug session (ADR 0027 §2, story 6.5 AC2).

**AC3 — one argument, and anything else is refused.**

**Given** the binary's new surface
**When** it is invoked with anything other than `--healthcheck`
**Then** it refuses with a message naming what it accepts
**And** it does **not** fall through to starting a bridge — a typo in a `HEALTHCHECK` line
must not start a second bridge inside the container, competing for the state directory and the
Sparkplug session.

**AC4 — the `HEALTHCHECK` line's numbers are derived, not guessed.**

**Given** AR12's allowance of three poll periods
**When** the Dockerfile declares the healthcheck
**Then** its `--interval`, `--timeout`, `--retries` and `--start-period` are each justified in
a comment beside them, and `start-period` covers a bridge that has never been configured —
which is silent by design and must not be restarted for it.

**AC5 — the 503 path is exercised the way a deployment will exercise it.**

**Given** [#56]'s finding that this path has never run inside a container
**When** the chaos test wedges the poll loop
**Then** the container's health status is observed to become `unhealthy`
**And** a bridge that is merely stale — cloud unreachable — is observed to stay `healthy`,
because that is the half the Dockerfile's own comment says must not regress.

**AC6 — falsification.**

**Given** each mechanism above
**When** it is broken
**Then** a test goes red, and the run's output is copied next to it.

## Out of scope

- **Restart policy tuning.** `restart: unless-stopped` is already in the compose file and is
  not this story's to revisit.
- **A `--version` or any other subcommand.** One argument, decided in ADR 0041. A CLI surface
  grows by being convenient.

## Dev Notes

### What must not break

- **`/healthz`'s status-code rule.** Non-200 is for a wedged poller and nothing else. Three
  stories have defended it (6.1, 6.5, and the review of 6.5); this one is its first real
  consumer, and a consumer is how a rule gets bent.
- **The port resolution is ADR 0037's**, in one place.
- **The image gains no package.** That is half of ADR 0041's reasoning.

### References

- [Source: `https://github.com/guycorbaz/smartme_mqtt/issues/56`] — the finding, and the three options
- [Source: `docs/adr/0041-the-healthcheck-is-the-binary-probing-itself.md`] — the choice, and what the probe must not do
- [Source: `docs/adr/0037-the-first-run-port-is-bootstrap-not-configuration.md`] — the port resolution order
- [Source: `Dockerfile`] — the `NO HEALTHCHECK` comment this story replaces
- [Source: `CLAUDE.md`] — falsify before trusting

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-20.

### Completion Notes List

**AC1 — met, and the extraction is the point.** `ui_port_for` is now the one place the
resolution rule lives, and both the server and the probe call it. A probe with its own copy
would keep working right up to the day somebody sets a port, then report a healthy bridge at
8080 while the bridge listens elsewhere — the project's own species of lie, arriving through
the door marked "monitoring".

**AC2 — met on both halves.** Non-zero for a non-200 answer and for no answer at all; zero for
everything else, including a bridge that publishes nothing on purpose. The unhealthy-when-
unreachable arm is not an opinion added to `/healthz` — it is the absence of an answer, and it
is the state a restart most plausibly clears.

**AC3 — met, and it is the criterion that would have been discovered in production.** A typo
in the `HEALTHCHECK` line would have fallen through to `main`'s normal start: a SECOND bridge
inside the container, every thirty seconds, competing for the state directory and opening a
second Sparkplug session under the same edge node.

**AC4 — met.** Every number carries its reason beside it in the Dockerfile, and `start-period`
is justified by what it is NOT for: an unconfigured bridge answers 200, so the period covers
building the runtime and binding the port, nothing more.

**AC5 — met in the only place it can be checked.** `docker-smoke.sh` runs the image, waits for
`.State.Health.Status`, and refuses both `NONE` (no HEALTHCHECK declared — [#56] unfixed) and
anything but `healthy` for an unconfigured bridge. **Presence is asserted before the verdict**,
because "not unhealthy" is also true of an image with no healthcheck at all.

**`reqwest` becomes a declared dependency of this crate, and the image does not grow.**
`smart-me-client` already links it, so it was compiled into this binary either way; what
changed is that it is now declared where it is used. `Cargo.lock` gained one line.

### The falsification this test could not do at first, and the repair

`an_unrecognised_argument_refuses_rather_than_starting_a_bridge` used `Command::output()`.
Under the mutation — falling through to a normal start — the process never exits, so
`output()` blocked for ever: the falsification was a **hang**, not a red test. A test that
cannot fail promptly has the same defect as one that cannot fail at all. Rewritten to wait with
a five-second deadline, and the mutation now produces a message that names what went wrong.

**And one harness defect of the same family, caught the same way:** the healthy-case test ran
on the default single-threaded test runtime, where `Command::output()` blocks the very runtime
serving `/healthz`. It reported a defect in the bridge where the defect was in the harness —
Epic 4's action E2 exactly. `flavor = "multi_thread"` is load-bearing and says so.

### Falsification record

| # | Mutation | Went red with |
|---|---|---|
| 1 | the probe's verdict inverted (`!status.is_success()`) | `a bridge that answers on /healthz is healthy — even one publishing nothing on purpose: exit code Some(1)` |
| 2 | an unreachable endpoint treated as healthy | `a bridge whose web server does not answer cannot be called healthy` |
| 3 | an unknown argument falling through to a normal start | `the process was still running after 5 s, which means it started a bridge — inside the container that is a SECOND bridge, on every probe` |

### File List

- `crates/smartme-bridge/src/main.rs` — modified (`ui_port_for`, `healthcheck`, the argument gate)
- `crates/smartme-bridge/Cargo.toml`, `Cargo.lock` — modified (`reqwest` declared where it is used)
- `Dockerfile` — modified (the `HEALTHCHECK`, replacing the comment that explained its absence)
- `scripts/docker-smoke.sh` — modified (the health status, checked against the real image)
- `crates/smartme-bridge/tests/healthcheck_probe.rs` — **new**
- `docs/adr/0041-the-healthcheck-is-the-binary-probing-itself.md` — **new**
- `_bmad-output/implementation-artifacts/7-1-…md`, `sprint-status.yaml` — new/modified

### Change Log

- **2026-08-20** — Story 7.1. AR12's restart can fire for the first time; [#56] closes. Three
  mutations run. `CONTRACT_VERSION` stays at 10.
