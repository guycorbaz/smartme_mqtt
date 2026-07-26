# ADR 0011 — A graceful stop requires the explicit NDEATH, not either mechanism

- **Status:** Accepted
- **Date:** 2026-07-26
- **Related:** Story 1.13 (`supervisor`), `chaos_sigterm_no_lie`, AR13, architecture open item ⑧,
  FR13 (signal bridge-dead to the SCADA)
- **Amends:** AR13 and Story 1.13's second acceptance criterion, both of which read
  "*either* an explicit NDEATH is published before exit, *or* the connection is dropped so the
  LWT fires".

## Context

The bridge has two ways to tell a SCADA host that its node is gone:

1. **The broker's last will**, serialised and handed over inside the CONNECT packet, published by
   the broker when it decides the connection died.
2. **An explicit NDEATH**, published by the bridge itself before it exits.

AR13 and Story 1.13's AC deliberately left the choice open, and named the instrument that would
settle it: *"confirmed against the author's broker via the `SIGTERM-NO-LIE` chaos test."*
Architecture item ⑧ recorded the same question. That test did not exist until 2026-07-26, so the
question stayed open through the whole of Epic 1.

Writing it exposed why the disjunction cannot stand. Two findings:

**The two branches are not equally prompt.** The will only fires once the broker concludes the
connection is dead. A dropped socket usually makes that immediate, but not always: a half-open
connection — a container network namespace torn down without a FIN, a NAT or firewall dropping
the flow silently — is invisible to the broker until the keep-alive elapses. Mosquitto waits
1.5× keep-alive, which at the configured 30 s is **up to 45 seconds** of a dead node displayed as
live. On an *unplanned* death that is the best anyone can do. On a *planned* stop, accepting it
means an operator restarting the bridge depends on the broker noticing a socket — which is
precisely the class of silent lie this project exists to prevent.

**The implementation does not choose; it does both.** Measured from an independent subscriber
against Mosquitto 2 (`chaos_sigterm_no_lie`):

| Message | Timestamp | Relative to the NBIRTH |
| --- | --- | --- |
| Will (published by the broker) | 1785059893052 | −1 ms — stamped before CONNECT |
| NBIRTH | 1785059893053 | — |
| Explicit NDEATH (published by the bridge) | 1785059894099 | +1046 ms — stamped at shutdown |

The explicit certificate arrives at once; the will follows when the socket closes at process exit,
because the driver drops the connection rather than sending a clean MQTT DISCONNECT (which would
instruct the broker to *discard* the will). The two are complementary, not alternative.

## Decision

**AR13 and Story 1.13's AC now require the explicit NDEATH on a graceful stop.** The will remains
mandatory as well — it is the sole mechanism for a hard death (crash, SIGKILL, power loss) and is
covered by `chaos_stale_on_death`.

Architecture item ⑧ is resolved: **both, not either.**

## Consequences

- **The test suite is now stricter than the previous contract, on purpose.** An implementation
  that dropped the explicit publish and leaned on the will would have satisfied the old AC and
  will now fail `chaos_sigterm_no_lie`. That is the point of the amendment: without it, whoever
  hits that failure has to guess whether the test or the spec is authoritative, and the cheap
  answer — "fix the test" — would delete the guarantee.
- **A consumer sees two NDEATHs per graceful stop**, carrying the same `bdSeq`. Duplicate deaths
  are idempotent for a consumer that has already marked the node down, but this is a real,
  observable behaviour of the wire format and it is not obvious from the code. Story 1.15's
  Ignition contract test should confirm Ignition treats it benignly.
- **The mechanism is confirmed against Mosquitto 2, not against Ignition's broker.** AR13's
  original wording asked for confirmation against *the author's broker*; a containerised
  Mosquitto is a close but not identical proxy. Story 1.15 closes that gap.
- **The distinguishing signal is a timestamp, and that is fragile in one specific way.** The will
  and the explicit death are byte-identical in shape; only the stamp separates them, and only
  because the will is serialised once, before CONNECT, and re-registered verbatim by `rumqttc` on
  every internal reconnect. Should the driver ever own its reconnect loop and rebuild the client
  per CONNECT (a deferred item), the will would be re-stamped and `chaos_sigterm_no_lie` would
  lose its discriminating power. Whoever does that work must give the test a new discriminator
  first.
- **Not amended:** FR13 ("the bridge can signal to the SCADA when the bridge itself is no longer
  alive") is stated as an outcome and is satisfied by either mechanism, so it needs no change.
- **Unaffected by this ADR:** the AC's other clause, "an independent subscriber sees no fresh
  DDATA survive", remains **unverified** — the chaos scenario has no reachable cloud, so no
  reading exists to survive. Proving it needs a TLS-terminating fake of the smart-me API and is
  recorded in `deferred-work.md`.
