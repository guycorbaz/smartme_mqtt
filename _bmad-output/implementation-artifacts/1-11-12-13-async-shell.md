# Stories 1.11, 1.12, 1.13: the async shell (poll task, mqtt driver, supervisor)

Status: 1.11 done · 1.12 **partially done** · 1.13 **partially done**

Issues [#13](../../issues/13) (1.11), [#14](../../issues/14) (1.12), [#15](../../issues/15) (1.13).
Delivered together: the epic specifies the 2-task shell is "born whole".

## Honest status

The adversarial review of this batch was the harshest of the sprint and it was right.
Two ACs are **not** met and are recorded as such rather than signed off:

- **FR20 "published ✓ only on broker ACK" (1.12) — NOT MET.** The review established that this
  is not merely unimplemented but *unimplementable as written*: Sparkplug mandates QoS 0 for
  every edge-node message, and a QoS-0 publish is never acknowledged. Raised as issue **#19**
  for a correct-course decision by Guy. What IS implemented is the strongest honest claim
  available at QoS 0: non-blocking `try_publish` with a per-device traced drop.
- **`chaos_sigterm_no_lie` verification (1.13) — NOT MET.** The AC names a chaos test that is
  Story 1.14's deliverable. The death path was fixed (below) but is not yet verified end to end.

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

## Remaining known gaps (deferred)

`poll_publish::run` (the loop itself) is untested — only `step_once` is; bdSeq is persisted once
at boot rather than per session; a corrupt bdSeq file restarts at 1 rather than refusing;
reconnect backoff is fixed 1 s with no jitter; no NCMD subscription. See deferred-work.md.

## Change Log

- 2026-07-25: Implemented; combined review found 26 findings incl. 4 critical; the critical and
  high-severity correctness defects are fixed, two ACs are recorded as unmet with issues #19/#20
  raised. 144 workspace tests green; clippy, fmt and cargo-deny green.
