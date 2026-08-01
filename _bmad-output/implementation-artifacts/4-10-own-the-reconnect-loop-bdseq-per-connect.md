# Story 4.10: Own the reconnect loop — `bdSeq` per CONNECT

Status: ready-for-dev

## Story

As the bridge,
I want a new session number on every CONNECT, as the specification requires,
so that a consumer pairing death to birth is never handed a certificate for a session that no
longer exists.

## Acceptance Criteria

The epic states three. **All three stand.** Four are added, for reasons in *Dev Notes*.

**AC1 — `bdSeq` advances per CONNECT, and the will agrees**

**Given** the recorded deviation in `mqtt_driver.rs` — `bdSeq` fixed for a client's lifetime because
the will cannot be updated after construction
**When** the driver owns its reconnect loop and rebuilds the client per CONNECT
**Then** `bdSeq` advances on each CONNECT and the will registered in that CONNECT carries the same
number
**And** the module documentation's *"recorded deviation"* section is replaced by a statement of the
conforming behaviour.

**AC2 — verified from outside**

**Given** a reconnect
**When** the new session births
**Then** the NDEATH the broker holds carries the new session's `bdSeq`, verified from an independent
subscriber.

**AC3 — the persistence path is re-examined at its new frequency**

**Given** `bdSeq` is persisted
**When** it advances per CONNECT rather than per boot
**Then** the persistence path is exercised at reconnect frequency, and the deferred concern
*"persisted once at boot"* is closed or restated.

**AC4 — the reconnect POLICY is decided here, not inherited** *(added)*

**Given** owning the loop means owning what `rumqttc` was doing
**When** the loop is written
**Then** the backoff, its floor, its ceiling and its jitter are stated in the code with the reason
**And** a broker that refuses every connection cannot make the bridge spin, nor grow memory.

> Architecture: *"bounded resilience (backoff + jitter; no unbounded growth)"*. Today that behaviour
> is `rumqttc`'s and undocumented by us. After this story it is ours, and an unstated policy is how
> a reconnect storm gets written by accident.

**AC5 — Story 4.9's discriminator is confirmed still armed** *(added)*

**Given** Story 4.9 replaced a timestamp comparison precisely because this story breaks it
**When** this story lands
**Then** `chaos_sigterm_no_lie` is run and passes **for the right reason** — and a mutation removing
the explicit death is re-run to show it still goes red under per-CONNECT wills.

> 4.9 was sequenced first to close a window, not to tick a box. Not re-running its falsification here
> would leave the window open in fact while closed on paper.

**AC6 — the conformance matrix moves, and this story earns the move** *(added)*

**Given** `-will-message-payload-bdSeq` is a `deviation` **because the value never increments**, and
`payloads-nbirth-bdseq-repeat` is a `deviation` that holds *"only because neither ever changes"*
**When** the behaviour becomes conformant
**Then** both rows move, the tallies are recomputed, and the prose at
`sparkplug-conformance.md:473-485` explaining *why* they were deviations is rewritten rather than
left contradicting the rows above it.

> Unlike Story 4.5, this story **may** move verdicts: it changes the code the verdicts describe,
> rather than arguing about rows on the strength of a document it wrote itself.

**AC7 — every passage asserting the old behaviour is amended** *(added)*

**Given** this project's most repeated defect
**When** the behaviour changes
**Then** a **per-passage table** is produced — not a grep result — covering at minimum the driver's
module docs, the manual's `bdSeq` row (*"Fixed per MQTT client, not incremented per CONNECT. A
deliberate trade"*), the conformance prose above, and `chaos_sigterm_no_lie`'s module docs.

## Tasks / Subtasks

- [ ] **Task 1 — read before writing**
  - [ ] `mqtt_driver.rs`, `run()` in full, and the module section *"Session identity, and a recorded
        deviation"* — that prose is part of what this story deletes.
  - [ ] `sparkplug_publisher.rs:202-232` — `new`, `bd_seq`, `new_session`, `will`.
  - [ ] `docs/adr/0011-graceful-shutdown-requires-both-deaths.md`, including the Story 4.9 note.

- [ ] **Task 2 — own the loop**
  - [ ] Wrap the connect/serve cycle in an outer loop that, on each iteration, calls
        `publisher.new_session()`, persists, rebuilds `MqttOptions` **with the new will**, and
        constructs a fresh `AsyncClient`.
  - [ ] `inbox` and `shutdown` must survive across iterations — they are the process's, not the
        connection's.
  - [ ] Decide and state the backoff (AC4).

- [ ] **Task 3 — prove it from outside** (AC: 2)
  - [ ] Extend or add a chaos test that forces a reconnect and asserts the second birth carries
        `bdSeq + 1` **and** that a death after that reconnect carries the new number. An independent
        subscriber, as always — the bridge agreeing with itself proves nothing.
  - [ ] Falsify it: pin `bdSeq` across reconnects and watch it go red.

- [ ] **Task 4 — re-run 4.9's falsification** (AC: 5)

- [ ] **Task 5 — the record** (AC: 6, 7)
  - [ ] `./scripts/ci-local.sh`, not `--fast`.

## Dev Notes

### The seam already exists, unused

`SparkplugPublisher::new_session()` (`sparkplug_publisher.rs:225`) advances the session exactly as
this story needs. **It has no production caller today** — `grep new_session()` finds one hit, in a
unit test. The publisher half was built for this story and left waiting; what is missing is entirely
on the driver side.

### Why the deviation exists, in the code's own words

> *"`rumqttc` reconnects internally and rebuilds the CONNECT packet from the `MqttOptions` captured
> at construction — so the registered will can never be updated. The session number is therefore
> FIXED for the lifetime of one client […] advancing it here would leave the broker holding a death
> certificate for a session that no longer exists, and a consumer that pairs death to birth by
> `bdSeq` would IGNORE the death and keep showing a frozen value as live."*

That reasoning is sound and is exactly why the fix is *own the loop*, not *advance the counter*.
**Advancing `bdSeq` without rebuilding the will is worse than the deviation** — it produces a will
the consumer will discard. Any implementation that does the first without the second must fail a
test rather than pass one.

### The persistence question AC3 asks, with the numbers

`persist_atomic_bytes` does write → `fsync(file)` → `rename` → `fsync(dir)`. Today that runs **once
per process start**. After this story it runs **once per CONNECT**, i.e. at reconnect frequency —
which on a flapping network or a restarting broker is unbounded from the disk's point of view unless
the backoff bounds it.

Two things follow, and AC4 is one of them:

- The backoff **floor** is what bounds the write rate. State it as such in the code, because that is
  a durability property, not just a politeness to the broker.
- Persisting **before** connecting is not negotiable: the point of the file is that a crash between
  connect and next boot cannot replay a session number. Batching or deferring the write reintroduces
  exactly the failure it exists to prevent.

The deployment writes this file to a bind-mounted directory on a NAS ([#41](https://github.com/guycorbaz/smartme_mqtt/issues/41)),
so "how often do we fsync" is a real question there and not a theoretical one.

### What this story must not break

`chaos_sigterm_no_lie` was rewritten by **Story 4.9** specifically because this story invalidates a
timestamp-based discriminator. It now counts certificates and reads no clock, so it survives — but
AC5 requires demonstrating that rather than assuming it. 4.9's record contains the mutation to
re-run.

### Wire impact, and why it is cheap right now

This changes what a consumer sees across a reconnect. Guy declared the pre-production Sparkplug
identity **disposable** on 2026-08-01 and the window for wire-breaking changes runs until the
web-configurable release (end of Epic 6), so the cost is currently zero — but `v0.2.0` is **running
in production on panoramix**, so the change must ship as a version bump and a deliberate redeploy,
not as a silent `latest`.
