# Stories 1.11, 1.12, 1.13: the async shell (poll task, mqtt driver, supervisor)

Status: 1.11 done · 1.12 done (FR20 amended, ADR 0010) · 1.13 done (2026-07-26)

Issues [#13](../../issues/13) (1.11), [#14](../../issues/14) (1.12), [#15](../../issues/15) (1.13).
Delivered together: the epic specifies the 2-task shell is "born whole".

## Honest status

The adversarial review of this batch was the harshest of the sprint and it was right.
Two ACs were left unmet rather than signed off; both have since been resolved — one by
amendment, one by the test that was missing:

- **FR20 "published ✓ only on broker ACK" (1.12) — RESOLVED by amendment (ADR 0010).** The
  review established that this was not merely unimplemented but *unimplementable as written*:
  Sparkplug mandates QoS 0 for every edge-node message, and MQTT defines no acknowledgement at
  QoS 0. Guy approved re-scoping (issue #19, closed): FR20 now reads "never over-claims
  delivery — reported published only once accepted for transmission, with a per-device traced
  drop rather than silence", which is exactly what the driver implements. PRD, epics and
  architecture amended.
- **`chaos_sigterm_no_lie` verification (1.13) — NOW MET (2026-07-26, issue #15).** The test
  exists and the death path is verified end to end. See below for what it proves and what it
  still does not.

## Critical defects the review caught, and what was done

- **The explicit DEATH never reached the wire.** `try_publish` only enqueues; the EventLoop was
  never polled again after the loop broke, so the certificate died in a channel and the socket
  closed. Worse, the `client.disconnect()` that followed would have told the broker to DISCARD
  the will — had it ever been pumped, BOTH mechanisms would have failed. Now: the death is
  queued, the transport keeps pumping for `death_flush`, and the connection is DROPPED (never a
  graceful DISCONNECT, which suppresses the will).
- **Two bugs that cancelled each other.** The will is registered once, at client construction,
  and rumqttc re-registers that same will on every internal reconnect — while
  `new_session_if_reconnecting` could never fire, because the transport error that precedes a
  reconnect always cleared the flag first. The reviewer's warning was exact: *fixing either one
  alone* would leave the broker holding a death certificate for a session that no longer exists,
  and a consumer pairing death to birth by `bdSeq` would ignore the death and keep showing a
  frozen value as live. Resolved by making the session number FIXED for a client's lifetime
  (matching the will it registered), removing the dead code path, and documenting the deviation
  from "increment per CONNECT" plus what it would take to do properly (owning the reconnect loop).
- **`EventLoop::poll()` inside `select!` is not cancellation-safe.** Every inbox message could
  abandon a half-finished CONNECT — after the broker had registered the will, which then fires
  against a node that never birthed. The EventLoop now runs alone in its own task and reports
  through a channel.
- **`client.disconnect().await` could deadlock shutdown forever** with the broker down (blocking
  send onto a full, undrained channel). Removed.
- **Rebirth re-asserted `Good` on values it had not re-judged** — a 45-minute outage came back as
  a fresh-looking lie. Re-declared readings are now degraded (`Good` → `Stale`, never upgraded)
  and the DBIRTH payload is stamped with the reading's own `ValueDate`, not `now`.
- **QoS was wrong in both directions**: QoS 1 on births violates Sparkplug; QoS 0 on data makes
  FR20 impossible. Now uniformly QoS 0 / retain false, per spec, with #19 raised.
- **The heartbeat-ordering test was vacuous** (the fake clock never advanced, so the assertion
  held whichever side of the fetch the touch sat on — and its comment said the opposite). The
  clock now advances first, so the test discriminates.
- **State-machine confinement had no mechanical guard** while every other invariant did.
  `arch_purity` now bans the mqtt task from even naming the machine, and bans `.step(` anywhere
  outside the poll task (the supervisor may still pass `Policy` through as configuration).
- **An illegal serial left the node connected, unborn and silent forever.** Validated at startup
  instead — refusing to start beats starting wrong.
- `RUST_LOG` was silently ignored (`env-filter` feature missing); a typo'd broker port silently
  fell back to 1883. Both fixed.

### Security finding (cargo-deny)

`rumqttc` 0.25.1's default `use-rustls` pulls `rustls-webpki` 0.102.8 — RUSTSEC-2026-0049/0098/
0099/0104, unreachable past rumqttc's `^0.102`. Disabled `default-features`: the broker is on the
local container network, so the vulnerable stack was dead weight we would still have shipped.
The smart-me API's TLS is unaffected (separate, current rustls in reqwest — NFR13 holds). Issue
**#20** tracks re-enabling broker TLS.

## Closing 1.13: `chaos_sigterm_no_lie` (2026-07-26)

The AC was left unmet because the death path had been *fixed* but never *observed*. It is now
observed from outside, on the real binary, against a real broker.

**Why it needed its own test rather than an extension of `chaos_stale_on_death`.** That test
kills the bridge outright, which proves the BROKER's mechanism: the will fires. A graceful stop
is the case where the bridge does have a chance to speak, and must not fall back on the broker
noticing a dropped socket. Those are different mechanisms and only one of them was covered.

**The problem the test had to solve.** On the wire an explicit NDEATH and a will are
indistinguishable — same topic, same payload shape. Timing does not separate them either: the
driver drops the transport rather than sending DISCONNECT (deliberately, so the will survives as
the fallback), and a dropped socket makes the broker fire the will immediately, not after the
keep-alive. So a test that merely waits for an NDEATH passes whether or not the bridge published
anything, which is exactly the bug the review had found.

What does separate them is the **timestamp**. The will is serialised and handed to the broker
inside CONNECT, so it carries a connect-time stamp; the explicit death is stamped when the
shutdown is handled. A death stamped at or after the instant the SIGTERM was sent can only be
the one the bridge published itself. The test sleeps one second between birth and signal so the
two stamps are separated by far more than clock granularity.

**Run as a real process.** Every other test drives `app::run` in-process, where no signal can
land. This one spawns `CARGO_BIN_EXE_smartme-bridge` and sends a genuine SIGTERM, so
`main.rs → run() → shutdown_signal()` is exercised — the only coverage that path has.

**Both new assertions were falsified before being trusted**, since a test written against
already-fixed code proves nothing by passing:

- Replacing `publish(&client, publisher.will(...))` with a discard makes the will arrive instead,
  and the test fails on the timestamp with the connect-time stamp in the message. This is the
  precise regression the review caught, now guarded.
- Flipping `qos_for` to `retain = true` makes a late subscriber receive a retained NBIRTH for a
  process that has already exited, and the test fails naming the topic.

Both probes were reverted; `mqtt_driver.rs` is byte-identical to before.

**What it does not prove.** The AC's "no fresh DDATA survives" clause is **vacuously satisfied,
not verified** — the honest word is unverified, and an earlier draft of this section overstated it
as "asserted in its general form". Two independent reasons: the cloud is unroutable in every chaos
scenario so no DDATA is ever produced, and structurally `supervisor::run` signals the death then
aborts the poll task while the driver has already left its `select!` loop, so no path exists by
which a reading could follow the certificate. The post-death drain is a guard against a future
ordering regression, not evidence about today's behaviour. Proving the clause needs a
TLS-terminating fake of the smart-me API (HTTPS is mandatory — `client.rs` rejects any non-`https`
scheme — and webpki rejects self-signed certs). Recorded in `deferred-work.md`.

## Review of the closing work (2026-07-26)

Three adversarial layers ran against the diff. Two independently reproduced both falsification
probes, confirming the test is a real oracle. They also found defects, and the test was hardened
before being signed off:

- **The discriminator spanned two clocks.** The original assertion compared the death's stamp
  against the instant the *test* sent the SIGTERM. A backward NTP step during the run — a mode
  this project explicitly models (`FakeClock::set_wall` documents "an NTP step, forward or
  backward") — would let the will satisfy it, passing off the exact regression the test exists to
  catch. Now compared against the *birth's* stamp: both come from the bridge's own clock, so a
  step perturbs them together and only in the conservative direction. Re-falsified after the
  change — the probe shows the will stamped 1 ms *before* the birth, against a 1 s margin the
  other way.
- **A buffered will could cause a false failure.** A transport blip before the signal fires the
  will, which then sits in the observer's queue; `wait_for` would pop that one and blame the
  production code for a certificate it did publish. The stream is now drained immediately before
  the signal.
- **The `bdSeq` pairing was a tautology.** A fresh state dir made `load_bd_seq` fall back to the
  sentinel, so birth and death both carried the same low constant every run and a hard-coded
  number would have passed. The test now seeds the persisted number (41 → session 42) and asserts
  the bridge adopted it. *This flaw is inherited from `chaos_stale_on_death`, where it still
  stands.*
- **The post-death drain ran after the exit was confirmed**, where a dead process cannot publish
  and the check could not fail for any reason. Moved before the exit wait, and `try_recv().ok()`
  replaced with an explicit match so a closed channel is not silently read as silence.
- **Startup failures were undiagnosable.** Both streams went to `/dev/null`, so a bridge that
  refused its config produced a 30 s hang and a message blaming the subscriber. The log is now
  captured and its tail included in the panic, alongside the child's actual exit status.
- **Proxy variables were inherited**, which would route the "unroutable" request to a host that
  answers — and a proxy named by hostname resolves on the blocking pool, which `poll.abort()`
  cannot cancel, stalling the very shutdown being measured. Now removed from the child's env.
- **The state dir leaked on every failing path** while the child had a `Drop` guard. It has one now.

**Resolved 2026-07-26 — ADR 0011, approved by Guy.** The AC read as a disjunction ("either an
explicit NDEATH ... **or** the connection is dropped so the LWT fires") while the test enforces
only the first branch. Investigating it showed the disjunction was never a settled decision:
AR13 and architecture item ⑧ both deferred the choice of mechanism *to this very test*
("confirmed against the author's broker via the `SIGTERM-NO-LIE` chaos test"), and the test had
not existed until now.

Measuring settled it — the implementation does not choose, it does **both**:

| Message | Timestamp | vs NBIRTH |
| --- | --- | --- |
| Will (broker) | 1785059893052 | −1 ms (stamped before CONNECT) |
| NBIRTH | 1785059893053 | — |
| Explicit NDEATH (bridge) | 1785059894099 | +1046 ms (stamped at shutdown) |

The explicit certificate is immediate; the will follows when the socket closes at exit. Requiring
the explicit branch is what keeps a *planned* stop from depending on the broker noticing a socket
— up to 1.5× keep-alive (45 s at the configured 30 s) if the connection is left half-open.
Amended in `epics.md` (AR13 + the AC) and `architecture.md` (the shutdown decision, open item ⑧
now closed, and the test-tier description). FR13 is stated as an outcome and needed no change.

## Remaining known gaps (deferred)

`poll_publish::run` (the loop itself) is untested — only `step_once` is; bdSeq is persisted once
at boot rather than per session; a corrupt bdSeq file restarts at 1 rather than refusing;
reconnect backoff is fixed 1 s with no jitter; no NCMD subscription. See deferred-work.md.

## File List

- `crates/smartme-bridge/tests/chaos_sigterm_no_lie.rs` (new)
- `crates/smartme-bridge/tests/common/mod.rs` (modified: `named_subscriber` extracted so a
  second observer can use a distinct client id — a broker evicts the older session when a
  client id reconnects, so two observers sharing a name would silently unplug each other)

## Change Log

- 2026-07-25: Implemented; combined review found 26 findings incl. 4 critical; the critical and
  high-severity correctness defects are fixed, two ACs are recorded as unmet with issues #19/#20
  raised. 144 workspace tests green; clippy, fmt and cargo-deny green.
- 2026-07-26: Closed 1.13's last AC — `chaos_sigterm_no_lie` added (issue #15). No production
  code changed. 147 workspace tests green (was 146); fmt and clippy `-D warnings` green;
  cargo-deny unaffected (no manifest change).
- 2026-07-26: Adversarial review of the closing work; seven defects fixed in the test itself
  (cross-clock discriminator, buffered-will false failure, tautological `bdSeq` pairing, a drain
  that ran where it could not fail, undiagnosable startup failures, inherited proxy env, leaking
  state dir). Documentation corrected: the DDATA clause is recorded as vacuously satisfied rather
  than "asserted". Four new items in `deferred-work.md`, and one spec amendment left for Guy.
  Re-falsified after the changes; 147 tests, fmt and clippy still green.
