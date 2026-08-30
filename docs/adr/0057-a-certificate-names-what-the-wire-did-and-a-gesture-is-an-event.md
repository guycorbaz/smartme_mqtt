# ADR 0057 — A certificate names what the wire did, and a gesture is an event

- **Status:** accepted
- **Date:** 2026-08-30
- **Decides:** which serial a device certificate undeclares, and how the poll loop learns that an
  operator moved a meter's switch.
- **Issues:** [#83](https://github.com/guycorbaz/smartme_mqtt/issues/83),
  [#82](https://github.com/guycorbaz/smartme_mqtt/issues/82)
- **Related:** ADR 0034 (the certified-gone latch and the gesture that clears it), ADR 0049 (the
  device is named by its measuring point), ADR 0052 / [#27] (published names are unique), story 3.5.

## Context — one subject seen twice

Both issues come from the review of story 3.5, and both are the same confusion: **the stored
configuration is what the operator wants; the running bridge is what the wire was told.** Where the
two are read as one, the bridge acts on an intention the host has never seen.

- **[#83]** — `reconfigure::classify_meters` builds certificates from the STORED row, and a serial
  edit is `Cost::ProcessRestart`, so the row can carry a serial the runtime has never used.
- **[#82]** — the poll loop reads `enabled` as a LEVEL, once per tick, so a gesture completed
  between two ticks never happened as far as the loop is concerned.

## Decision 1 — a DDEATH undeclares the device the WIRE knows

`SparkplugPublisher::device_death` resolves the declaration by the **published name**, which is what
the topic carries, and removes that entry — whatever serial the caller handed in.

It is sound because published names are unique: `config::validate` refuses two meters sharing a
`meter_id`, which is the verified half of [#27] (ADR 0052). One name, at most one declaration.

**The consequence had moved since [#83] was written, and that is worth recording.** The issue was
raised when the serial *was* the device level of the topic, so it read as *a death addressed to the
wrong device*. ADR 0049 made the device level the published name — which a serial edit does not
touch — so the DDEATH is now correctly addressed, and the failure moved one line down:

`declared` is keyed by serial, so `remove(S2)` removed **nothing**. The host was told the device was
offline while the publisher still held it as born, and the next reading — carrying the poll task's
spawn-time `S1` — was published for a device the host had just buried. **DDATA after DDEATH**, which
`device_death`'s own doc promises cannot happen.

That is worse than the issue predicted, and it is the failure this project is named for: the wire
saying something untrue. `a_death_undeclares_the_device_the_wire_knows_even_after_a_saved_serial_edit`
is falsified against restoring the caller's serial — `left: Emitted, right:
DroppedUndeclaredDevice`.

## Decision 2 — the switch moving is an EVENT, carried out and counted

`Plan` gains `toggled`, filled by `classify_meters` at the flip — the one place a flip is visible.
`Control::apply` bumps a per-meter counter in the fleet state; the poll loop compares that count with
what it held a tick ago and, on a difference, performs the same reset a disable performs:
`State::initial()`, `certified_gone` and `gone_pending` cleared.

**Why not derive it from `births`/`deaths`.** Those also carry removals and re-adds. A task resetting
itself for those would judge a meter afresh because somebody else's row was deleted — so the guard
asserts both halves, and the mutation that wires the event to the removal arm is red.

**Why a count and not a nudge.** A per-meter channel can lose a wake-up; a monotonic count cannot.
The loop compares rather than receives, so the width of the gesture stops mattering — which is the
whole of [#82].

**Why it lives in the fleet state and not on `MeterConfig`.** What the operator *did* is not part of
what the operator *configured*, and a count written to the file would come back after a restart as a
toggle nobody performed.

**The counter is kept in step on the slow path too.** A disable that lasts a tick is handled by the
level branch; the loop then records the current count, so the meter is not re-announced as a fast
toggle when it comes back.

## Consequences

- **The gesture ADR 0034 documents now works however fast it is performed.** Before, it worked only
  if the disable outlasted a polling period — up to five minutes — while the screen said it had
  worked either way, which is a surface reporting a success that did not happen.
- **A certified-gone device re-added to the account can be recovered by the documented gesture.**
  Before, a quick toggle left it freshly announced on the wire and silent for ever.
- **A saved-but-unrestarted serial edit no longer leaves a buried device publishing.** The restart
  the classifier demands is still owed; what is repaired is what happens before it.
- **The manual drops the workaround** — *hold the disable for one period* — instead of documenting a
  gesture with a hidden condition.
- **One thing is NOT decided here:** whether a serial edit should be a certificate rather than a
  restart. It stays `ProcessRestart`, for the reason `classify_meters` already gives — the DDATA
  serial comes from the smart-me response, never from the file, so re-declaring under a new one
  silences the meter until a restart.
