# ADR 0034 — A device the account refuses ends with a certificate

- **Status**: accepted
- **Date**: 2026-08-15
- **Deciders**: Guy Corbaz (the epic-level reservation, 2026-08-06); drafting and mechanism,
  story 3.5
- **Amends in part**: the latched-device wire behaviour established by stories 2.6 and 3.2.
  **Leans on**: ADR 0027 §3 (its own licence), ADR 0012 (the contrast it must argue against),
  ADR 0009 ("stop + surface").
- **Issue**: [#65](https://github.com/guycorbaz/smartme_mqtt/issues/65) (the disable half),
  story 3.5 (the disappearance half)

## Context

Three different events end a device's flow of DDATA, and until this decision the wire could not
tell two of them apart — nor could the operator's screens tell the third apart from a fault:

1. **The bridge dies.** The NODE dies with it: the will (NDEATH) says so. Decided long ago.
2. **The operator disables the meter.** A DDEATH already goes out (`classify_meters`,
   `2a4d5ca`) — but the poll task kept calling the smart-me API every period for ever, filled
   the log with one warn per discarded reading, and `Phase::failed_sources` kept naming the
   meter's old fault on `/` and `/healthz` until a container restart. [#65] holds all three:
   nobody decided them; they fell out.
3. **The account refuses the device id** — smart-me's `404`, latched `Fatal` since story 2.6.
   The device then published `Bad` DDATA every period, for ever: a device that is *not there*
   rendered indistinguishable from one that is misbehaving, while the API was hammered with a
   question whose answer cannot change an absorbing latch.

ADR 0012 chose `Bad_Stale` DDATA over DDEATH for a SILENT meter, and that stands: silence is
usually transient, a DDEATH destroys the device's online state in the host, and the next good
poll does not undo it. But a latch is not a silence. It is absorbing by design (ADR 0009), so
"may recover on its own" — the premise of ADR 0012's choice — is false for it by construction.

**What the norm says, cited rather than remembered** (added by the story's review — the first
draft argued from internal ADRs alone, which is the exact habit CLAUDE.md's opening rule
exists to break):

- Publishing the DDEATH is the Edge Node's job when a device *"becomes unavailable for any
  reason"* — `docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_4_Topics.adoc:458-461`. A device the account
  refuses is unavailable.
- The host-side consequence the Consequences section relies on is mandated, not hoped:
  `tck-id-operational-behavior-edge-node-termination-host-action-ddeath-devices-offline` and
  `tck-id-operational-behavior-edge-node-termination-host-action-ddeath-devices-tags-stale`
  (`docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_5_Operational_Behavior.adoc:503-510`; the
  two clauses sit under the edge-node-termination heading, their text covering any DDEATH
  reception) — on DDEATH the host marks the device
  offline and its tags STALE, keeping last values. "DEAD, stale, last values kept" is the
  norm's own description.

## Decision

**The epic's 2026-08-06 reservation is honoured: DDEATH is the ending for disable and for
disappearance, and an ending is followed by silence, not by an endless `Bad`.**

1. **Disable**: the DDEATH continues to go out (unchanged). New with story 3.5: the poll task
   reads `enabled` every tick — a disabled meter's task stays bound (re-enable is a DBIRTH,
   not a restart) and keeps its heartbeat, but it fetches nothing, publishes nothing, warns
   about nothing, **and its fault is retired from the operator surfaces**, said once in the
   log. Disabling a broken meter is the obvious operator gesture; it now quietens the alarm it
   is aimed at. Re-enabling judges afresh from Stale-until-proven.
2. **Disappearance**: the `404` latch gets its own name — `device-not-in-account`, split out
   of `configuration-contradicted` exactly as story 2.6 split the refusals
   (`CONTRACT_VERSION` 9 → 10, additive). It is the one latch that is *evidence about the
   device*, so the device ends: **one DDEATH after the latch verdict, then no further DDATA
   and no further fetches** while the latch holds. The licence is ADR 0027 §3's own text —
   *"every poll cycle publishes a verdict for every enabled meter, **or a device
   certificate**; never silence"* — the certificate IS the publication.
3. **The asymmetry is the design.** Disable retires the operator's alarm (they said "stop");
   disappearance KEEPS it — `failed_sources` and `/` name the meter and its repair for as
   long as the latch holds, because the account saying "gone" is a fault someone must fix.
   The certificate retires the wire's device, never the operator's alarm.
4. **Only this latch ends a device.** A credential or base-URL latch is evidence about the
   ASKING, not about the device; a certificate there would declare dead a device nobody has
   evidence about. Those latches keep today's behaviour.

## What this does NOT decide

- **No listing-comparison loop.** Detection rides on the per-device fetch whose `404` the
  account itself pronounces. A `GET /Devices` diff would rest on unobserved API behaviour
  (what does absence from the listing mean while the device endpoint still answers?); it
  reopens the day the wire shows a disappearance the fetch cannot see.
- **Non-gone latches keep fetching.** A credential latch still calls the API every period
  although the answer cannot change the verdict — a discovered adjacent defect, recorded as
  its own issue rather than absorbed here.
- **Mistyped vs removed.** One `404` covers both; the refusal's text names both origins and
  claims no discrimination.
- **The observation grain.** The `enabled` level (and the row's existence) is read once per
  tick. A disable-and-re-enable completed WITHIN one poll period is unobserved — like any
  level read — so the reset gesture takes effect at the first tick that sees the level
  changed. Making the gesture edge-triggered needs an eventing path from `reconfigure` to the
  poll tasks that does not exist; recorded as an issue, not absorbed.
- **Repeat burials.** A gone-then-disabled meter produces the poll task's certificate and
  then `classify_meters`' disable-Death for an already-dead device; a re-enable of a
  still-gone meter births and re-buries within a tick. Each certificate is a truthful event
  about the state at its instant, re-burial is idempotent for a host, and serialising the two
  senders would couple the poll loop to the reconfigure path for a cosmetic gain.

## Consequences

- A Sparkplug host shows a gone device DEAD (stale, last values kept by the host) instead of
  an endlessly refreshed `Bad` — and the `device-not-in-account` cause on the latch verdict
  names the row or the account rather than the file.
- The three endings are distinguishable on the wire: NDEATH (bridge death), DDEATH after
  `device-not-in-account` (account refusal, alarm held), DDEATH from disable (operator's
  hand, alarm retired).
- The smart-me API stops being called for meters that are disabled or pronounced gone —
  [#65]'s quota leak closed for both cases.
