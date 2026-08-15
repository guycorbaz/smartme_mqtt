# Story 3.5: A known meter that disappears — the topology says absence

Status: review — implemented 2026-08-15, the day it was written; awaiting the independent pass

## Story

As the operator,
I want a meter that is no longer part of the fleet — disabled by me, or refused by the account —
to END on the wire with a device certificate and to stop occupying my screens and my API quota,
so that absence is published as absence, and so that the obvious operator gesture (disabling a
broken meter) actually quietens the alarm it is aimed at instead of surviving it.

## Why this exists, and what already exists

FR6 asks that *"a known meter that disappears from discovery"* be marked *"stale/absent, never a
silent disappearance"*. Two mechanisms already carry most of the honesty, and this story's first
duty is to verify rather than rebuild them (the 2.6/2.7 discipline):

- **A device the account refuses already latches loudly** (story 2.6, `b47c73f`): a `404` is
  `UnknownDevice` → `Fatal`, `configuration-contradicted` naming the id, `failed_sources`
  carrying it, values nulled. The *detection* half of FR6 exists.
- **A DDEATH on disable already goes out** (`classify_meters`, `2a4d5ca`), and the design
  *"disabling does NOT unbind its task"* is deliberate — re-enabling is a DBIRTH, not a restart.

What does NOT exist is everything that should follow, and [#65] holds the list nobody decided:
the smart-me API keeps being called every period for a meter the operator removed; every
discarded reading logs one warn per period (noise that trains an operator to stop reading
warnings); and `Phase::failed_sources` never reads `enabled`, so **a disabled meter's fault
stays on `/` and `/healthz` until a container restart** — which kills the session for every
other meter. The operator does the right thing and the surface keeps accusing them.

And the DISAPPEARANCE case ends wrong: after the latch, the device publishes `Bad` DDATA for
ever — a device that is *not there* rendered indistinguishable from one that is misbehaving.
The epic decided on 2026-08-06 that **DDEATH is reserved for disable and for
disappearance-from-discovery**; this story is where that reservation is honoured.

## Decisions taken at drafting (no later story may re-choose them)

1. **There is NO listing-comparison loop, and the reason is the epic's own rule.** FR6's title
   says "disappears from discovery", and the tempting build is a periodic `GET /Devices` diff.
   Every parameter of that loop — its period, and above all what a device's absence from the
   LISTING means while its own endpoint still answers — would be an assumption about API
   behaviour nobody has observed (the description declares only `200`s; no deletion has ever
   been captured). Detection therefore rides on the fact in hand: **the per-device fetch, whose
   `404` the account itself pronounces.** The listing loop reopens the day the wire shows a
   disappearance the fetch cannot see, and not before.
2. **The bridge does not pretend to tell "mistyped" from "removed", because it cannot.** Both
   reach the same `404`; story 2.6's refusal text already names both origins. One behaviour for
   both, honestly worded — inventing a discrimination would be claiming a diagnosis we do not
   have (the `CounterWentBackwards` precedent).
3. **Absence ends in a certificate, and then in silence — and that needs an ADR.** For a
   disabled meter and for a latched-gone device alike, the end state is: one DDEATH, no further
   DDATA. ADR 0027 §3 already provides the licence (*"every poll cycle publishes a verdict for
   every enabled meter, or a device certificate; never silence"*) — the certificate IS the
   honest publication for a device that is not part of the fleet, where an endless `Bad` says
   "misbehaving" about something that is *gone*. This changes what the wire does in the latched
   case, so it is an ADR (0034), with the manual and (if the golden demands it) the contract
   moved together, per the repository's decision rule.
4. **The ops surfaces follow `enabled`, and retiring a fault is not erasing history.** A
   disabled meter leaves `failed_sources` and `/` (the alarm the operator aimed at is
   quietened); the log says ONCE that it was disabled and why its fault is retired. Re-enabling
   re-evaluates from scratch: fresh DBIRTH, fresh state (`State::initial()` — Stale until
   proven, as ever).

## Acceptance Criteria

**AC1 — Disabling a meter stops asking smart-me and stops the noise.** ([#65] items 1 and 2)

**Given** a meter whose row is disabled (hot, via the configuration screen)
**When** the poll cycles continue
**Then** no fetch is made for that meter and no per-period warn is emitted
**And** the task stays bound (the deliberate design keeps: re-enable needs no restart)
**And** the DDEATH that already goes out on disable still goes out — verified, not rebuilt.

**AC2 — A disabled meter's fault is retired from the operator surfaces.** ([#65] item 3)

**Given** a meter in `Failed` (say ADR 0029's identity latch) that the operator disables
**When** `/` and `/healthz` render
**Then** the meter is no longer named in `failed_sources` nor accused on the page
**And** the retirement is said once in the log (disabled by the operator, fault retired with it)
**And** re-enabling brings the fault machinery back from `State::initial()` — a latch is not
carried across the operator's explicit removal-and-return, because the configuration may be
exactly what they changed.

**AC3 — A device the account refuses ENDS with a certificate.** (FR6, the epic's DDEATH
reservation, ADR 0034)

**Given** a meter whose fetch latches `Fatal` on the account's own refusal of the device id
**When** the latch takes effect
**Then** ONE DDEATH goes out for that device after the latch is published
**And** no further DDATA follows while the latch holds — the certificate is the publication,
per ADR 0027 §3
**And** `failed_sources` and `/` keep naming the meter and its repair for as long as the latch
holds (the certificate retires the WIRE's device, never the operator's alarm — the exact
opposite of AC2's disable, and the asymmetry is the design: disable is the operator saying
"stop", disappearance is the account saying "gone", and only the first quietens the screen)
**And** other latches (credential, base URL) get NO DDEATH: the device may be fine, it is the
asking that is broken — a certificate there would declare dead a device nobody has evidence
about.

**AC4 — The end states are distinguishable ON THE WIRE from a silence.**

**Given** the three ways a device stops producing DDATA — disabled, latched-gone, or the
bridge dying
**When** a Sparkplug host reads the session
**Then** disabled and latched-gone both show the device DEAD (DDEATH seen), while a bridge
death kills the NODE (NDEATH/will) — and a test walks the three and asserts the wire artifacts
differ.

**AC5 — Verified, not rebuilt**: the existing detection (2.6's latch, its cause, its surfaces)
and the existing disable-DDEATH are pinned by tests that name this story, or the existing tests
are cited in the record — nothing re-implemented.

**AC6 — Falsified before trusted, and RUN before recorded** (C3).

**AC7 — ADR 0034 written; manual and conformance/golden moved with it if any pinned surface
changes; `./scripts/ci-local.sh` full run; `gh run list`.** [#65] closes with this story.

## Tasks / Subtasks

- [x] **Task 1 — The disabled meter goes quiet toward smart-me** (AC1) — 2026-08-15. The loop
      reads `enabled` from the config handle every tick; a MISSING row reads as enabled (the
      operator's "stop" is `enabled: false` on a row that exists — removal is a
      ProcessRestart, and harnesses abbreviate). "Idle on purpose" decided: the heartbeat
      keeps beating (the loop IS alive), so the wedge detector needs no special case
- [x] **Task 2 — The surfaces follow the retirement** (AC2) — 2026-08-15, and more cheaply
      than drafted: `Heartbeats::retire` clears the meter's recorded OPINION, and a cleared
      cell already reads as "no opinion yet" to `failed()`/`degraded()` — no surface grew a
      filter, which means no third caller can forget one. One info line at the transition;
      re-enable resets to `State::initial()` (memory kept: the yardstick must survive a
      disable, FR15)
- [x] **Task 3 — The certificate on the account's refusal** (AC3) — 2026-08-15: contract
      9 → 10 (additive): `device-not-in-account` splits out of `configuration-contradicted`
      exactly as 2.6 split the refusals (`Refusal::DeviceNotInAccount`, latching, Bad); the
      loop certifies ONCE on that cause and then goes silent toward both the API and the
      wire, the cell keeping its `Failed` so the alarm stays
- [x] **Task 4 — The three endings differ on the wire** (AC4) — 2026-08-15: the certificate
      pinned here (`DeviceCommand::Death` after the gone-latch, and NONE on disable — the
      disable DDEATH is `classify_meters`', pinned by `chaos_device_certificates`); the
      bridge-death ending is the will's (story 4.17's QoS-1 tests). Each ending pinned where
      it is produced, cited in the test's module doc
- [x] **Task 5 — ADR 0034 + manual** (AC7) — 2026-08-15: ADR written; manual ch5 v10 row +
      prose; runbook v10 row; golden v10 (its own copy, v9 kept as the record it is)
- [x] **Task 6 — Verification sweep** (AC5) — 2026-08-15: the 2.6 latch and its naming
      verified as the detection (tests updated for the split, not rebuilt); the disable
      DDEATH verified as existing (`classify_meters`, chaos) and deliberately NOT resent by
      the poll task (two senders for one ending would race — asserted)
- [x] **Task 7 — Falsify** (AC6) — 2026-08-15, six mutations, table in the notes, every one
      run before its note
- [x] **Task 8 — `./scripts/ci-local.sh` full run** — 2026-08-15, EXIT=0 end to end
      (chaos and image included, `chaos_device_certificates` among them — the disable
      DDEATH verified against a real broker), then `gh run list`

## Dev Notes

### The traps this story is most likely to fall into

1. **Quietening the wrong thing.** AC2 retires a fault because the OPERATOR removed the meter;
   AC3 keeps the fault because the ACCOUNT removed the device. Collapsing the two ("gone is
   gone") would either leave the operator's alarm shouting after their explicit fix, or
   silence a real configuration fault — [#62]'s family, from a new door.
2. **A DDEATH for every latch.** Credential and base-URL latches say nothing about the
   device's existence. The certificate is only for the refusal that names the device itself.
3. **The heartbeat.** A meter that deliberately does not fetch must not trip the wedge
   detector, and must not need a special case scattered through `/healthz` — decide once where
   "idle on purpose" lives.
4. **Rebuilding 2.6.** The latch, the cause, the naming — all exist. This story routes an
   existing fact to the publisher; if a diff touches `map_error` or the cause vocabulary,
   something has gone wrong (contract 9 should survive this story untouched unless the golden
   proves otherwise).

### Where the code lives

- `crates/smartme-bridge/src/app/poll_publish.rs` — the loop that never reads `enabled`;
  `step_once`; the warn; the heartbeat touch
- `crates/smartme-bridge/src/app/reconfigure.rs` — `classify_meters`, the disable-DDEATH that
  exists, the device channel plumbing
- `crates/smartme-bridge/src/app/mqtt_driver.rs` — device birth/death execution
- `crates/smartme-bridge/src/ui/mod.rs` — `Phase::failed_sources` (the [#62]/[#65] surface)
- `crates/smartme-bridge/src/adapters/smartme_source.rs` — the `UnknownDevice` latch (AC3's
  input; do not touch)
- `docs/adr/` — 0027 (the licence AC3 leans on), 0012 (why silence chose Bad_Stale — the
  contrast AC3's ADR must argue against for the GONE case), 0029

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:274`] — FR6
- [Source: `_bmad-output/planning-artifacts/epics.md:300`] — the DDEATH reservation, decided
  2026-08-06
- [Source: [#65]] — the three never-decided consequences, verbatim
- [Source: story 3.4's drafting decision 2] — discovery is on demand; the loop this story
  REFUSES is the same loop, refused again for the same reason
- [Source: `epic-2-retro-2026-08-15.md`] — C1–C5, binding

[#65]: https://github.com/guycorbaz/smartme_mqtt/issues/65
[#62]: https://github.com/guycorbaz/smartme_mqtt/issues/62

## Review Findings (2026-08-15, independent pass, five finder angles)

The richest haul of the epic — five finders, strong convergence on the gravest item — triaged
into six repairs, one citation debt paid, two issues, two ADR notes. Every repair falsified
where a falsification was possible, run before its note.

**REPAIRED:**

1. **The wedge on idle (three finders converged).** Both idle `continue`s sat ABOVE the
   period-rebuild block, so a hot interval change left the ticker pacing the OLD period while
   every touch recorded the NEW one — `loop_age` divides age by the recorded period and takes
   the WORST meter, so one meter idling on purpose read the whole bridge as wedged for most of
   every window, and Epic 7 will wire wedged to a restart that kills the session fleet-wide.
   The re-arm is hoisted above every skip: an idle loop still re-paces.
2. **The certificate named the DESIRED serial, not the in-force one.** `Control::apply` stores
   a serial edit that `reconfigure` classified ProcessRestart — not in force until the restart
   — so the Death could bury a device the wire never birthed while the born one stayed alive
   for ever. `PolledMeter` bundles the meter with its SPAWN-TIME serial (ADR 0029's pair,
   seen from the wire's side); every certificate names the device the DBIRTH used.
3. **The wire order of verdict and certificate was a coin toss**: both queued in one tick on
   sibling channels into the driver's `select!`, so a Death winning dropped the final
   `device-not-in-account` verdict as undeclared. The certificate now goes out ONE TICK after
   the latch verdict (`gone_pending`) — a full period of separation is what makes "the
   verdict, then the certificate" true on the wire and not merely in queueing order.
4. **`certified_gone` was set before the send was known to succeed** — a failed send (or the
   old missing-row arm) entered a permanent silence nothing ever ended. Certified only on a
   successful send; a failure retries next tick. The missing-row arm is GONE entirely: the
   spawn-time serial needs no row.
5. **A stale fixture staged an impossible pairing** (`Refusal::Configuration` + a 404's
   message, which `map_error` can no longer produce) — the fixture-models-the-impossible
   class; updated to the refusal the 404 actually yields.
6. **The runbook's scope heading said v3 → v9 while its table said v10** — the
   attestation-drift its own preamble exists to prevent; heading and count move with the
   table now, and the note says who caught it.

**THE CITATION DEBT:** ADR 0034 decided Sparkplug wire behaviour citing internal ADRs alone —
the exact habit CLAUDE.md's opening rule exists to break, flagged by a reviewer who then
VERIFIED the claims hold: `Sparkplug_4_Topics.adoc:458-461` (the DDEATH is the edge node's job
when a device "becomes unavailable for any reason") and
`tck-id-operational-behavior-edge-node-termination-host-action-ddeath-devices-offline` /
`-ddeath-devices-tags-stale` (the host-side consequence). Cited now, in the ADR.

**ISSUES:** [#82] — the enabled level is observed at tick granularity, so a
disable-and-re-enable inside one poll period is a silent no-op (the reset gesture is an EVENT
only `reconfigure` sees; an eventing path to the poll tasks is the fix direction). And the
"unreachable missing row" comment was FALSE — `Control::apply` stores `new.meters` wholesale,
so removing a served row is reachable through Save; the comment now tells the truth (the
pre-existing zombie polls until the restart the classifier demanded, loudly).

**ADR NOTES:** the observation grain and the repeat-burial sequences (gone→disable→re-enable
produces Death/Death/Birth/Death — each truthful at its instant, re-burial idempotent),
recorded in "What this does NOT decide".

**The two missing falsifications, found by a reviewer reading the falsification TABLE against
the assertions** (the story claimed the two-sender race "asserted" while no mutation had ever
made that assertion fire): the send-path deleted goes RED on *"the certificate follows the
latch: Elapsed"*, and a disable-branch Death goes RED on *"the poll task sends NO certificate
on disable"*. Both run, both now in the table below.

### Falsification — the review round's additions (2026-08-15, run before their notes)

| mutation | result |
|---|---|
| the certificate never sent (send deleted, certify anyway) | RED — *"the certificate follows the latch: Elapsed"* |
| the disable branch sends a Death (a second sender) | RED — *"the poll task sends NO certificate on disable — two senders for one ending would race"* |

## Dev Agent Record

### Agent Model Used

claude-fable-5 (same session as 3.4; C1–C5 binding).

### Debug Log References

### Completion Notes List

**2026-08-15 — the whole story, one sitting; four drafting decisions held, one discovery.**

- **The discovery**: a latched meter keeps FETCHING today — for every latch. 2.6's "latches
  instead of being polled for ever" latched the VERDICT; the loop still asks the API a
  question whose answer cannot change an absorbing state, every period, including with a
  rejected credential (the exact hammering 2.6's own doc warns about). This story stops it
  for the GONE latch (the certificate ends the asking); the credential/identity latches keep
  today's behaviour, recorded as an adjacent issue rather than absorbed — ADR 0027 requires
  their verdicts to keep publishing, so stopping their fetch needs a publish-without-fetch
  path this story has no criterion for.
- **`retire` clears the opinion, not the meter**: `last_tick` stays (idling is not wedging),
  and a cleared cell reads exactly like "no tick yet", which `failed()`/`degraded()` already
  treat as absent — the AC2 surface change cost zero filter edits, so no third caller can
  forget one.
- **The missing-row rule**: a served meter without a configuration row reads as ENABLED. The
  operator's stop is an explicit `enabled: false`; absence is a harness's abbreviation or a
  future hot-removal, and reading it as "stop" would silently idle a task the classifier
  promised to restart. (Found the hard way: the first rule idled every nfr2 harness meter.)
- **First-tick fatal publishes nothing on the outbox** (story 3.2 AC4: never answered →
  nothing to republish) — the latch is attested on the snapshot, where the surfaces read it.
  The integration test learned this before the implementation did anything wrong.
- **`run` gained its eighth parameter** (the device channel) with an `#[allow]` and a written
  revisit trigger, not a bundling struct: these are wiring concerns threaded once, not values
  that travel together (the 2.7 bundling precedent is about the latter).

### Falsification — six mutations, each RUN before its note (2026-08-15)

| mutation | result |
|---|---|
| the `enabled` flag ignored | RED — the queued Good readings surface while disabled: *"a disabled meter publishes nothing — and the Good readings queued in the script prove no fetch happened either"* |
| `pulse.retire` skipped | RED — *"the alarm the operator aimed at is retired with the meter ([#65] item 3)"* |
| the latch carried across a disable (`state` not reset) | RED — *"left: Bad, right: Good"*: a carried `Failed` makes the post-re-enable Good impossible by construction |
| the post-certificate `continue` removed | RED — *"after the certificate, silence IS the publication (ADR 0027 §3)"* |
| every latch certified (`published.latches()` instead of the one cause) | RED at test 1's premise — *"the first tick publishes: Elapsed"* — and that premise failure IS the harm surfacing: a credential-latched meter went silent after one tick, the ADR 0027 violation this guard exists to prevent. Recorded as observed rather than re-staged onto the named assertion (the phase.rs "mutation B" lesson, inverted: here the precondition failure is the property) |
| the heartbeat untouched while disabled | RED — *"idling on purpose is not wedging"* |

### File List

- `crates/smartme-bridge/src/core/source.rs` — `Refusal::DeviceNotInAccount` (AC3)
- `crates/smartme-bridge/src/core/oracle.rs` — `Cause::DeviceNotInAccount`: ALL, successor,
  string, quality, latch (AC3)
- `crates/smartme-bridge/src/adapters/smartme_source.rs` — `map_error` split; tests updated
- `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs` — `CONTRACT_VERSION` 10
- `crates/smartme-bridge/src/app/poll_publish.rs` — the enabled read, `Heartbeats::retire`,
  the certification and its silence, the device channel parameter (AC1–AC3)
- `crates/smartme-bridge/src/app/supervisor.rs` — `device_tx` threaded to the poll tasks
- `crates/smartme-bridge/tests/a_meter_that_leaves_the_fleet.rs` — the loop-level proofs
  (AC1–AC4)
- `crates/smartme-bridge/tests/contract_golden.rs` — golden v10
- `crates/smartme-bridge/tests/nfr2_staleness_latency.rs` — call-site update
- `docs/adr/0034-a-device-the-account-refuses-ends-with-a-certificate.md` — the decision
- `docs/manual/chapters/05-mqtt-sparkplug-contract.tex`, `docs/ignition-contract-runbook.md`
  — v10 rows
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — status trail
