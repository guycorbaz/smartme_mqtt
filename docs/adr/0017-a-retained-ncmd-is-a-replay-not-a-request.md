# ADR 0017 — A retained NCMD is a replay, not a request, and is refused

- **Status:** Accepted
- **Date:** 2026-07-31
- **Related:** Story 4.7, Story 4.6, `docs/sparkplug-conformance.md`, `deferred-work.md` (2026-07-29
  packet-size deferral), `tck-id-payloads-ncmd-retain`
- **Supersedes nothing.** It settles a question Story 4.7 created and did not notice.

## Context

Story 4.7 made the bridge act on an inbound message for the first time. Everything it publishes had
always been governed by a rule about the retain flag —
`every_edge_node_message_is_qos_zero_and_never_retained` pins it — but nothing looked at the flag on
the way **in**. `pump_transport` destructured `topic` and `payload` from the inbound `Publish` and
dropped the rest.

The code review of Story 4.7 found what that costs.

MQTT retains a message **on the broker** and replays it to every client that subscribes to a
matching filter, at the moment it subscribes. The bridge subscribes to its NCMD topic on every
CONNACK (`mqtt_driver.rs`, the `Transport::Connected` arm — the ordering is Story 4.6's AC1). So a
single publish, by any client, of

```
topic:  spBv1.0/<group>/NCMD/<node>
retain: true
payload: a conformant Node Control/Rebirth = true
```

makes the broker hand that request to the bridge **on every connect and every reconnect, for as long
as the retained message exists**. The bridge would answer every one.

Three properties make this worse than an ordinary unwanted birth:

1. **It needs no attacker after the first second.** Publish once and leave. The bridge becomes its
   own amplifier.
2. **It is invisible.** The answer trace is identical to the one a real Host Application provokes.
   An operator reading the log sees a host asking for rebirths and goes looking for a host.
3. **It survives everything except a deliberate cleanup.** Restarting the bridge does not clear it;
   restarting the broker does not, if persistence is on. It is cleared only by publishing an empty
   retained payload to that exact topic — which nobody will think to do, because nothing points at
   the cause.

The broker on this deployment is **unauthenticated** (some meters cannot present credentials), so
any client on the LAN can publish it.

## Decision

**A `Node Control/Rebirth` that arrives with the MQTT retain flag set is not answered. It is
classified as a near miss and traced at WARN, naming the flag, the clause, and the cure.**

`classify` takes the flag as an argument and cannot reach `Inbound::Rebirth` when it is set; the
trace carries `reason=Retained` and says how to clear the retained message.

## Why this costs nothing

`tck-id-payloads-ncmd-retain` (`Sparkplug_6_Payloads.adoc:1421`):

> *"NCMD messages MUST be published with the MQTT retain flag set to false."*

A conformant Host Application therefore **never** sends one. Refusing a retained NCMD cannot reject
anything the specification permits a host to send, so the usual argument against strictness — that a
strict matcher silently declines a request a real host meant — does not apply. There is no
conformant sender to lose.

That clause is a rule for publishers, and the bridge is not a publisher of NCMD; the conformance
matrix records it and eight neighbours as `n/a` on exactly that reasoning. The `n/a` stands. What
Story 4.7 changed is that the bridge became a **consumer**, and a clause that binds the sender is
still evidence about what a legitimate message looks like. The matrix now says so.

## Consequences

- **The live delivery is still answered, and must be.** MQTT sets the retain flag on *delivery* only
  when the message is sent in response to a new subscription. A client that publishes with
  `retain: true` while the bridge is already subscribed produces a delivery with `retain: 0`, and
  that is an ordinary request someone is making now. Both behaviours are asserted in
  `chaos_ncmd_subscription`: the live delivery IS answered, the replay after a forced reconnect is
  NOT. The first version of that test conflated the two and failed against correct code.
- **The near-miss detector gains a third reason.** It already distinguished a wrong value from a
  nearly-right name; `Retained` joins them, and the three traces are held apart by
  `an_answered_a_missed_and_an_unknown_command_do_not_read_alike`.
- **The 2026-07-29 oversized-packet deferral loses its bounding argument, and is not re-decided
  here.** That deferral rested on the vector being *"a disruption, not a lie … the bridge re-births
  ~1 s later"*, and handed the residue to Story 4.13 as *"a sustained attack would churn death/birth
  at roughly 1 Hz"*. A **retained** oversized frame needs no sustained attacker: it is redelivered on
  every reconnect and kills the connection again. This ADR does not close that, and cannot — the
  frame is rejected by `rumqttc` before any of this bridge's code sees it, so the guard above never
  runs. What changed is the brief handed to Story 4.13: *"1 Hz with an attacker"* becomes
  *"indefinite without one"*. `deferred-work.md` records it.
- **The incoming packet bound is now stated in this repository** rather than inherited from
  `rumqttc`'s `Default`, at the same value. `AC-LEAK-01` depends on it for bounded memory, and a
  bound that can move under a dependency bump with nothing failing is not a bound.
- **This is the shape the next command inherits.** The planned meter relay switches physical
  hardware. A relay command replayed from a retained message on every reconnect would not cost an
  idempotent extra birth; it would actuate. The rule is established here, on the harmless command,
  before it is needed on the harmful one.
