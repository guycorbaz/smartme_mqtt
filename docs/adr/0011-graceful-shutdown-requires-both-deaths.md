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

- **⚠️ A DISCRIMINATOR THAT DEPENDS ON THIS ADR EXPIRES WITH STORY 4.10** — recorded here by
  Story 4.9, and acted on by it. *Stating the mechanism, because the conclusion alone is not
  re-derivable.*

  `chaos_sigterm_no_lie` has to tell the bridge's explicit NDEATH from the broker's will. On the
  wire the two are **byte-identical** — `tck-id-…-death-payload`
  (`Sparkplug_5_Operational_Behavior.adoc:808-812`) says the payload published on shutdown is the
  one *registered as the will* — so no content-based discriminator can exist, and any proposal to
  "tag" the explicit death would be a conformance violation invented to make a test easier.

  Until 2026-08-01 the test used `death_stamp > birth_stamp`. That was sound **only because the
  will is serialised once, inside the first CONNECT**, and so can never be stamped after any
  birth. **Story 4.10 rebuilds the will per CONNECT.** A will registered on a later connection is
  stamped later than the birth the test captured on the first one, so a *will* would satisfy the
  inequality and the test would go green on exactly the regression it guards. Nothing would
  announce it — no compile error, no edited assertion, no failing run.

  **The replacement is the count, and it reads no clock:** a graceful stop produces **two**
  NDEATHs (the bridge's own, then the will when the socket drops), and the will can fire once.
  Two therefore proves one was published by the bridge; one proves the explicit path is broken.
  This converts the measurement already recorded in the next bullet into the test's discriminator
  rather than inventing a mechanism. It does not exclude a bridge that published its explicit
  death *twice* while the will never fired — a different defect, and excluded in practice by the
  socket drop.

  Demonstrated rather than argued: with the will stamped 60 s in the future and the explicit
  publish removed, the old comparison evaluates **true** — it would have passed — while the count
  fails. The record is in the test's module docs.

- **The test suite is now stricter than the previous contract, on purpose.** An implementation
  that dropped the explicit publish and leaned on the will would have satisfied the old AC and
  will now fail `chaos_sigterm_no_lie`. That is the point of the amendment: without it, whoever
  hits that failure has to guess whether the test or the spec is authoritative, and the cheap
  answer — "fix the test" — would delete the guarantee.
- **A consumer sees two NDEATHs per graceful stop**, carrying the same `bdSeq`. Duplicate deaths
  are idempotent for a consumer that has already marked the node down, but this is a real,
  observable behaviour of the wire format and it is not obvious from the code. Story 1.15's
  Ignition contract test should confirm Ignition treats it benignly.

  > **✅ CONFIRMED against a real Ignition, 2026-07-31** (8.3.7, MQTT Engine 5.0.0-rc1), during the
  > Story 4.8 probe. Ignition tolerates it: `Node Info → Death Count` moved `0 → 2`, the node and its
  > device were marked offline, and — read out of a queried log export rather than a scrolled viewer —
  > the Engine module emitted **exactly two INFO lines in three hours of logs and no Sparkplug-side
  > WARN or ERROR at any point**:
  >
  > ```
  > 20:34:56.751  INFO  …sparkplug.SparkplugPayloadHandler  Handling LWT message for Edge Node …
  > 20:34:58.752  INFO  …sparkplug.SparkplugPayloadHandler  Handling LWT message for Edge Node …
  > ```
  >
  > One millisecond after each death reached it. No duplicate-session complaint, no error.
  >
  > **And one thing this ADR did not anticipate: Ignition calls BOTH of them an "LWT message",
  > through a single handler.** It does not distinguish an edge node's explicit certificate from the
  > broker's will — to this consumer, an NDEATH is an NDEATH. So the distinction this ADR is built on
  > is **invisible on the consumer side**. The decision stands and the two seconds of advance notice
  > are real on the wire (Engine processes the first at `:56`), but the second death overwrites
  > `Offline DateTime` with `:58`, so the immediacy the ADR claims is not observable in Ignition's own
  > record of when the node died. The benefit is narrower than the wording above implies, and anyone
  > citing "the explicit certificate is immediate" should cite it as a property of the wire, not of
  > what the host reports.
- **Confirmed against the real broker (2026-07-26).** AR13 asked for confirmation against
  *the author's broker*, and a containerised Mosquitto is a close but not identical proxy.
  `chaos_sigterm_no_lie_against_an_external_broker` — `#[ignore]`d, with no default target
  and no default group — was run against the author's own Mosquitto and passed unchanged.
  What remains for Story 1.15 is the *consumer* half: whether Ignition tolerates the double
  NDEATH, which no broker-level test can answer.
- **That test must never run by accident.** The author has exactly one broker and it is
  production, with Ignition live on it. Publishing a Sparkplug node onto such a broker makes
  every subscribed host discover and persist it, leaving a phantom device to be deleted by
  hand. Hence the ignore attribute, the absence of defaults, and the refusal to publish into
  the default `Site` group.
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
