# Story 5.2: Configuration persists across restarts and image updates, and takes effect without one

Status: ready-for-dev

## Story

As the operator,
I want a setting I change to survive a restart and an image update, and to take effect without
either,
so that reconfiguring the bridge is not a deployment.

## Acceptance Criteria

The epic contributes **FR27** (*"persist configuration across restarts and image updates"*) and
**AR8** (*ArcSwap config*). Both stand. **Five are added**, for reasons in *Dev Notes*.

**AC1 — configuration survives a restart and an image update**

**Given** a configuration written to the state directory
**When** the container is restarted, and separately when the image is replaced with a newer tag
**Then** every setting is still in force
**And** the test exercises the **image-update** path, not only the restart path.

> These are two different claims and only one of them is cheap to test. A restart re-reads a file
> the same process wrote; an image update replaces the binary that reads it. The second is what
> FR40 promises the operator and what a schema change would break — see AC5.

**AC2 — the split from ADR 0022, with the mode VERIFIED** *(added)*

**Given** [ADR 0022](../../docs/adr/0022-secrets-rest-in-a-separate-0600-file.md)
**When** the bridge starts
**Then** non-sensitive settings live in `config.toml` and secrets in a **separate** `secrets.toml`
created `0600`
**And** the bridge **stat**s that file and **refuses to start** if it is readable by group or other,
naming the file.

> Verified, not assumed. [#41](https://github.com/guycorbaz/smartme_mqtt/issues/41) already produced
> a case in this deployment where the mode bits read `drwxrwxrwx` while a Synology ACL denied uid
> 10002 — **the displayed mode was not the enforced permission.** A mode this process set at
> creation says nothing about what a restore, a remount, an `umask` or a `docker cp` did afterwards.

**AC3 — the two files may desynchronise, and that is a validation fault** *(added)*

**Given** two files that are written separately
**When** one names a meter the other does not, or one is missing entirely
**Then** it is reported through the **same fault collection as Story 5.1**, with the file named
**And** the bridge refuses to start rather than running with a meter whose credential it cannot find.

> This is the cost ADR 0022 accepted in exchange for a diagnosable `config.toml`. Accepting a cost
> means handling it, not hoping it does not happen — and a missing credential must not surface later
> as an authentication error that looks like a smart-me outage.

**AC4 — a change takes effect without a restart, and the meter set is a special case** *(added)*

**Given** AR8's `ArcSwap`
**When** a setting changes
**Then** the running bridge picks it up without restarting the process
**And** for the **meter set** specifically, enabling a meter publishes a **DBIRTH** and disabling one
publishes a **DDEATH**, under the same `bdSeq` — no new session, no NBIRTH, no interruption to the
other meters.

> A meter is a Sparkplug *device*, not a node metric, so this costs a device-level certificate rather
> than a rebirth. What the norm warns about
> (`Sparkplug_5_Operational_Behavior.adoc:863`) is the opposite: DDATA carrying a metric that was not
> in the previous DBIRTH — which publishing the DBIRTH **first** avoids. Order matters and is the
> testable part.
>
> **Not every setting is hot-swappable and the story must say which are not.** Changing the group or
> node identity changes the topic namespace and the will registered at CONNECT; that is a new
> session by definition. Decide per field at drafting: hot, or requires-reconnect. **No field may be
> left undecided** — an unlisted field will be assumed hot by whoever adds the form.

**AC5 — an older file must not be silently misread by a newer binary** *(added)*

**Given** an image update where the configuration schema has changed
**When** the new binary reads the old file
**Then** it either migrates it or refuses to start, and **never** starts with defaults substituted
for fields it did not understand
**And** the file carries a version so that "did not understand" is detectable at all.

> Serde's default behaviour is the trap: unknown fields are ignored and missing fields take their
> `Default`. A renamed field would therefore read as *absent*, take its default, and the bridge would
> start **happily on a configuration the operator never wrote** — publishing at 30 s because the
> period silently reverted. That is the same class as `bdSeq`'s corrupt-file fallback, which this
> project already handles by falling back to a sentinel **and logging it**.

**AC6 — no secret reaches the log, the error, or the state** *(added)*

**Given** a fault in `secrets.toml`
**When** it is reported
**Then** the message carries the **field name and the file**, never the value
**And** a test asserts the secret string appears in no log line, no error, and no published payload.

> ADR 0019's rule, tested here rather than in Epic 6, because this is where the value enters the
> process. A `Debug` derive on the secrets struct would defeat it silently, and no template is
> involved — which is the whole argument for the Epic 5 / Epic 6 split.

## Tasks / Subtasks

- [ ] **Task 1 — read before writing**
  - [ ] `crates/smartme-bridge/src/persist.rs` in full — `persist_atomic` is TOML + temp + fsync +
        rename + fsync(dir), and it already exists. Do not write a second writer.
  - [ ] `main.rs:233` — the state directory and its `/data` default.
  - [ ] Story 5.1's fault collection: this story adds to it, it does not start a second one.
  - [ ] ADR 0022, including *The blocker*.

- [ ] **Task 2 — the two files** (AC: 2, 3)
  - [ ] `secrets.toml` created `0600` **before** the first secret is written to it, not tightened
        afterwards — creating `0644` and fixing it leaves a window.
  - [ ] The startup `stat` check, with its refusal.
  - [ ] Cross-file consistency into 5.1's faults.

- [ ] **Task 3 — versioned schema** (AC: 5)
  - [ ] A version field, and `deny_unknown_fields` — the default of ignoring them is the defect.
  - [ ] Decide migrate-or-refuse **now**, not when the first schema change happens.

- [ ] **Task 4 — hot reload** (AC: 4)
  - [ ] `ArcSwap` — **a new dependency.** It reaches `Cargo.lock` and `deny.toml`; stage those files
        by name. Never `git add` a directory after a dependency change.
  - [ ] The per-field table: hot vs requires-reconnect, every field listed.
  - [ ] DBIRTH on enable, DDEATH on disable, same `bdSeq`, DBIRTH before any DDATA.

- [ ] **Task 5 — falsification** (AC: all)
  - [ ] Mode check: create the file `0644` and confirm the bridge refuses. Copy the message.
  - [ ] Schema: rename a field in the file and confirm refusal rather than a silent default.
  - [ ] Secrets: assert absence from logs by **searching for the value**, having first confirmed the
        search finds it when deliberately leaked — an absence assertion over a stream that never
        carried it proves nothing.
  - [ ] `./scripts/ci-local.sh`, full.

## Dev Notes

### The blocker this story inherits

**ADR 0022 is accepted; its prerequisite is not met.** `/data` on the deployment is world-writable
([#41]). A `0600` file inside a directory anyone can write to is a claim about one inode in a
directory where files can be replaced. **This story cannot be closed as done while that holds**, and
the honest handling is to record the AC as unmet with the issue rather than to weaken the AC.

### What this story does NOT do

**No UI.** Per ADR 0021 the screens are Epic 6. Everything above is testable without HTML — the
write-only rule (AC6) especially, which is the reason the split exists.

**No multi-meter runtime.** Story 5.1's AC6 keeps the bridge refusing to serve more meters than it
can. AC4's DBIRTH/DDEATH applies to the meter the runtime does serve, and generalises when the
fan-out lands.

### The absence assertion, again

AC5's secret test is an absence assertion, and this project has been caught by one: every chaos test
pointed at TEST-NET-1, so *"no DATA appeared"* held over an empty stream. **Prove the stream carries
something first**, then prove the secret is not in it.
