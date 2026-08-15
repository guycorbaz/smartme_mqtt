# Story 3.5: A known meter that disappears — the topology says absence

Status: ready-for-dev

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

- [ ] **Task 1 — The disabled meter goes quiet toward smart-me** (AC1): the poll task reads
      `enabled` (hot, via the config handle) and skips fetch+publish+warn while keeping the
      task and its heartbeat alive (a disabled meter must not read as a WEDGED one — decide
      how the heartbeat represents "idle on purpose" and record it)
- [ ] **Task 2 — The surfaces follow `enabled`** (AC2): `Phase::failed_sources` and the page
      filter on it; the one-line retirement log; re-enable resumes from `State::initial()`
- [ ] **Task 3 — The certificate on the account's refusal** (AC3): the gone-latch reaches the
      publisher as a device retirement; one DDEATH; DDATA stops; surfaces keep the alarm
- [ ] **Task 4 — The three endings differ on the wire** (AC4)
- [ ] **Task 5 — ADR 0034 + manual** (AC7)
- [ ] **Task 6 — Verification sweep** (AC5)
- [ ] **Task 7 — Falsify** (AC6)
- [ ] **Task 8 — `./scripts/ci-local.sh` full run**, then `gh run list`

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

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
