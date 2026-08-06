# Story 6.1: The server exists, and it exists in every state the bridge can be in

Status: review

## Story

As the operator,
I want the web UI to answer in every state the bridge can be in — including the states where it is
publishing nothing —
so that the one screen I need most is not the one that disappears when something is wrong.

## Why this is Epic 6's first story, and why it is urgent

**Since [ADR 0023](../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md)
landed on 2026-08-04, a fresh deployment cannot be configured at all except by hand-writing
`config.toml`.** Every setting but the credential arrives through a browser, and the browser has
nothing to talk to. Stories 5.2 and 5.3 both wrote *"comes up and serves the web UI"* into their
acceptance criteria and both delivered only the first half — deliberately, and recorded as such, but
the debt is now the product's shortest path to being usable.

That is what makes the **server**, not a screen, the first story. And it dictates the shape: the
states in which the bridge publishes nothing are exactly the states in which the UI matters most, so
a server that only runs alongside a healthy session would be useless in all three of them.

## Acceptance Criteria

**AC1 — the server answers in all four startup states**

**Given** each of the four states Stories 5.2 and 5.3 established
**When** the bridge is started in it
**Then** the HTTP surface is up and answers

| state | server | what it must be able to say |
| --- | --- | --- |
| no `config.toml` | **up** | there is no configuration, and where the file will be written |
| present, invalid | *see the note* | which settings are at fault |
| present, valid, unconfirmed | **up** | the mapping, for confirming |
| ready | **up** | everything |

> **The invalid case is the one that has to be DECIDED here rather than discovered.** Today the
> bridge refuses to start on an invalid configuration, so there is no process to serve anything —
> and the screen that would repair it is behind the process that refused. That is a real corner and
> this story owns it. **Decide one of:** (a) keep refusing, and treat a hand-edit as the documented
> repair — the fault list already goes to `stderr` and `docker compose logs`; or (b) serve a
> read-only fault page and stay up.
>
> **DECIDED 2026-08-04: (a), keep refusing.** Written into `main.rs` beside the refusal, and into
> the manual. The reasoning below is what was weighed.
>
> **SUPERSEDED 2026-08-06 by [ADR 0026](../../docs/adr/0026-a-configuration-it-cannot-use-stops-the-bridge-publishing-not-serving.md)
> ([#57](https://github.com/guycorbaz/smartme_mqtt/issues/57)): (b), serve the screen.** The
> decision was sound when taken and its strongest reason expired. *"Somebody may believe it is
> running"* was a hazard exactly as long as no surface could say otherwise — story 6.2 built that
> surface, and the correction round of 2026-08-05 built `Lifecycle::Misconfigured`, whose headline
> is *"The saved configuration is not usable"* and which already answers `/healthz` with 200 so
> Epic 7 cannot restart-loop it away. The objection that (b) *"needs the server to start before the
> configuration is read"* turned out not to bite either: the port is read from the file when there
> is one and defaults when there is not, which this story itself established. What decided it in
> the end is that the commonest first-run mistake — a state directory nobody `chown`ed — reached
> this arm on the **first** turn, and so was a restart loop with no browser path out.
>
> **Recommendation: (a), keep refusing.** The refusal is Story 5.1's whole point and FR26's, the
> faults are already legible without a browser, and a bridge that stays up on a configuration it has
> rejected is one restart away from somebody believing it is running. (b) also needs the server to
> start before the configuration is read, which inverts the ordering `main.rs` was just given.
> **But it must be written down, and the manual must say which it is.**

**AC2 — the bind posture is the one already decided, and it is asserted**

**Given** [ADR 0019](../../docs/adr/0019-no-auth-on-the-config-ui-secrets-are-write-only.md) and
`architecture.md:203`
**When** the server starts
**Then** it binds `0.0.0.0:PORT` **inside the container**, with **no in-app authentication**
**And** the container publishes no host port — Traefik is the only ingress, over its shared Docker
network
**And** a test asserts the bind address rather than a comment claiming it.

> **The trust boundary is Traefik, not the app.** That was decided on 2026-08-01 and recorded in
> ADR 0019 after a BasicAuth-at-Traefik position was withdrawn. **Do not add a login here**, and do
> not "improve" the bind to loopback: loopback would make the container unreachable from Traefik,
> which is the only thing that can reach it.
>
> The port is a setting and therefore belongs in `config.toml` under Story 5.2's rules — which makes
> it a **new-session-class** change by the same argument as the broker: a listener cannot move
> without dropping what is connected to it. Add the row to that table; the table is compiler-checked
> and will not let this be forgotten.

**AC3 — `/healthz` is what the Docker healthcheck consumes, and it cannot lie**

**Given** **FR33** and **AR12** (the `last_loop_tick` heartbeat)
**When** the healthcheck asks
**Then** the answer distinguishes *the process is alive* from *the bridge is working*
**And** a bridge that is deliberately not publishing — unconfigured, or unconfirmed — is **not
reported unhealthy**.

> **This is the acceptance criterion most likely to be got wrong, and the failure is expensive.**
> Epic 7 wires this to a Docker healthcheck that RESTARTS the container. A `/healthz` that reports
> unhealthy because no configuration exists would put a fresh deployment into a restart loop — and
> the screen needed to configure it would be destroyed every few seconds by the very mechanism meant
> to protect it.
>
> The existing note in Epic 7 says it exactly: *"restart a wedged poller, never an honest STALE"*.
> Extend that: never a deliberate silence either.

**AC4 — the version is served, and it is the one that is running**

**Given** **FR44**
**When** the operator looks at the UI or the health endpoint
**Then** both carry the application version and the contract version
**And** they are the values the **binary** was compiled with, not a tag it is wearing.

> `CARGO_PKG_VERSION` and `CONTRACT_VERSION` are already resolved at compile time and already in the
> startup banner, for exactly this reason — the publish workflow carries a tag-vs-version guard
> because the two can drift. Serve the same constants; do not read a tag.

**AC5 — the server never takes the bridge down with it**

**Given** a UI that is a diagnostic aid
**When** the listener cannot bind, or a handler panics
**Then** publishing continues
**And** the failure is traced, loudly, at a level the default filter shows.

> The same rule file logging already follows: *"a bridge that stops publishing because it cannot
> write a log file has turned a diagnostic aid into an outage."* A port already in use must not cost
> the meters.

## Tasks / Subtasks

- [x] **Task 1 — read before writing**
  - [x] `main.rs`'s four-state block — the server has to exist in three of them, so it is started
        *before* that match, not inside one arm.
  - [x] `lib.rs::run_without_publishing` — two of the three states already wait there.
  - [x] `app/supervisor.rs::Control` — `current()`, `apply()`, and `Plan::cost()` are the API the
        screens will use. **They exist and have no caller**; this story is where that stops.
  - [x] `app::config::mapping_preview` — already returns exactly what a confirmation screen renders.
  - [x] ADR 0019 in full, including what was *withdrawn*.

- [x] **Task 2 — the server** (AC: 1, 2, 5)
  - [x] **Stage `Cargo.lock` and the manifests by name** — never `git add` a directory after a
        dependency change. `ci-local.sh` caught the lock file unstaged, which is what it is for.
  - [x] Started before the state match; every state that stays up gets it.
  - [x] The bind asserted by a test, not by a comment.
  - [~] **A panicking handler must not stop the poll loop — NOT ASSERTED.** Half of AC5 is proven:
        a listener that cannot bind degrades to "no UI", says so, and the bridge keeps publishing
        (`a_taken_port_does_not_cost_the_meters`). The panic half is not, because asserting it needs
        a route that panics, which means shipping a panicking handler in the binary or a test-only
        route that is not the code under test. **Recorded as unmet rather than ticked**, with
        [#51](https://github.com/guycorbaz/smartme_mqtt/issues/51). What holds it up meanwhile is
        structural rather than tested: the server is a spawned task, so a panic in it kills that task
        and nothing else.

- [x] **Task 3 — `/healthz`** (AC: 3, 4)
  - [x] Alive vs working, distinguishable, with the heartbeat.
  - [x] **A deliberately silent bridge is healthy.** Falsify this one against the actual Docker
        healthcheck semantics Epic 7 will use, not against a unit test's idea of them.

- [x] **Task 4 — the state model the screens read**
  - [x] One source of truth for source/sink/bridge state (**FR29** says *"a single internal source
        of truth"*). Do not let a template compute it.

- [x] **Task 5 — falsification** (AC: all)
  - [x] Bind to loopback and confirm the test catches it — that is the plausible "hardening" a
        future reader will try.
  - [x] Report unhealthy when unconfigured, and confirm the test catches it.
  - [x] Panic in a handler and confirm publishing survives.
  - [x] `./scripts/ci-local.sh`, **full**. This story changes how the binary starts, and the image
        smoke tests are where that has been caught five times.

- [x] **Task 6 — the consequences**
  - [x] `docker-smoke.sh`: the UI answers in the unconfigured and unconfirmed states.
  - [x] The manual gains the UI's address and the bind posture; chapter 6 is a stub today.
  - [x] Story 5.2's change-cost table gains the UI port.
  - [x] The compose example, when Epic 7 gets there — **not here**.

## Dev Notes

### What this story does NOT do

**No configuration screen, no editing, no confirmation button.** Those are 6.2 and after. This story
delivers the surface they need and proves it exists in the states they will be used in — which is
precisely the sequencing error to avoid, because a beautiful form that only renders when the bridge
is already working would be useless on a first run.

**No authentication.** Decided, recorded, and withdrawn once already. See ADR 0019.

### The trap this epic inherits

Five times on 2026-08-04, a change to how the binary starts made a check go **quiet rather than
red** — two chaos tests timing out, `startup_banner` hanging, an image smoke test running 26 minutes
against a 3 minute baseline. This story changes how the binary starts. Read
[`scripts/docker-smoke.sh`](../../scripts/docker-smoke.sh) before writing, add to it while writing,
and run the full `ci-local.sh` — `--fast` does not build the image and would not have caught any of
the five.

### Why `/healthz` is called out so hard

Because its failure mode is a **restart loop that destroys the repair path**, and because the
requirement that creates it (FR33) lives in this epic while the mechanism that acts on it (the
Docker healthcheck) lives in Epic 7. A criterion whose consequence lands in another epic is exactly
the kind this project has already had to go back for.
