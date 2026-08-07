# Story 3.2: A meter's silence is published, not withheld

Status: review

## Story

As the SCADA host,
I want a meter that has stopped answering to be told to me,
so that I stop displaying its last value as though it were still true.

## Why this exists

[ADR 0027](../../docs/adr/0027-a-failed-source-is-a-fault-the-screen-must-name.md) decided the rule
and left the mechanism with Epic 2, which has no story. Story 3.1 opened the fleet, and the fleet is
what makes the gap urgent: **three silences hide behind the one meter that works.**

The defect, measured on 2026-08-06: `poll_publish` sends a `MeterUpdate` only when the fetch
**succeeded**. On a failure it traces the verdict and sends nothing, and the only DDATA publish site
is fed by that channel. No update, no DDATA — and no DDEATH either, since device certificates come
only from `Control::apply`.

**Silence on a Sparkplug wire is not a statement.** It is indistinguishable from *"nothing has
changed"*, which is why a host goes on displaying what it last received. A verdict the bridge has
reached and does not publish is a verdict it has **withheld**, and withholding it is the failure
this project is named for.

## The case that is already honest, and the case that is not

Worth separating, because they look alike and only one is a defect:

- **A meter that has never answered** — Guy's permanently unplugged fourth. Its DBIRTH declares the
  tag set with **no value and quality `Stale`** (`cold_start_metrics`), and no DDATA ever follows.
  The host shows a stale, valueless tag from birth. **That is true, and this story does not change
  it.**
- **A meter that answered and then stopped.** Its last DDATA said `Good`. Then nothing. The host
  keeps showing that value at `Good`, indefinitely, and nothing on the wire contradicts it. **That
  is the lie**, and it is the whole of this story.

## Decisions taken at drafting

**1. A failed tick republishes the LAST KNOWN value with the new verdict — it does not invent one.**

The publisher already does exactly this on a rebirth (`birth()`'s re-declaration path), and the
reasoning there transfers unchanged: *"a re-declared reading has NOT been re-judged against now, so
it is never re-asserted as Good: it is true history, published stale, stamped with its own
ValueDate."* A republish carrying `now` as its timestamp would turn a 45-minute outage into a
fresh-looking lie the moment the value was re-sent.

`publish()` already stamps `millis(update.measurement.value_date)`, so the rule is enforced by the
code that already exists rather than by a new one that must remember it.

**2. DDATA with a non-good quality, not a DDEATH.**

The norm allows the DDEATH (`Sparkplug_4_Topics.adoc:460` makes it the edge node's job on behalf of
an unavailable device), and it is rejected here: [ADR 0012](../../docs/adr/0012-quality-codes-spec-versus-host.md)
chose `Bad_Stale` for this exact case and verified it against a live Ignition across a
`Good`→`Stale` transition. A DDEATH additionally destroys the device's **online** state in the host
— a heavier claim than *"this reading is old"*, and one the next good poll does not undo. DDEATH
stays reserved for disable (story 5.2's certificate) and for disappearance-from-discovery (3.5).

**3. The poll task keeps the last measurement; the publisher is not asked to remember for it.**

The publisher's `declared` map already holds the last update per device, so the republish could be
driven from there. It is not, and the reason is the seam: **the state machine decides truth and the
mqtt task decides nothing** (AR6). A driver that substituted a value of its own choosing when the
poll task said nothing would be deciding, and the one place truth is decided would stop being one
place. The poll task therefore carries its own `last: Option<Measurement>` and sends a complete
update every tick.

**4. This does not implement report-by-exception, and does not make the deviation worse.**

`tck-id-operational-behavior-data-publish-dbirth-change` says DDATA SHOULD only be published when
metrics change; the bridge already publishes every poll, and that deviation is recorded
([#32](https://github.com/guycorbaz/smartme_mqtt/issues/32)). A republish adds no new *kind* of
message — it fills the gap where a poll produced none.

## Acceptance Criteria

**AC1 — a meter that stops answering is published stale, not withheld**

**Given** a meter that has published at least one `Good` reading
**When** its next fetch fails or times out
**Then** a DDATA is published for that meter carrying the state machine's verdict — a non-good
quality — and the last known value
**And** the payload timestamp is the value's own `ValueDate`, not now.

**AC2 — the value is never re-asserted as Good**

**Given** the republish of AC1
**When** the published quality is read off the wire
**Then** it is whatever `Policy::step` returned, and it is never `Good`
**And** the assertion names the quality, not merely "a DDATA was emitted".

**AC3 — one meter's silence does not publish another meter's verdict**

**Given** four enabled meters, one failing and three answering
**When** three periods elapse
**Then** each meter's DDATA carries **its own** serial and **its own** verdict
**And** the failing meter's messages are counted separately from the others'.

> The trap this AC exists for: story 3.1's first cadence test counted `[9,0,0]` because the shared
> fixture hard-codes one meter id. An assertion about "the right verdict" is worthless if every
> message is labelled with the same device.

**AC4 — a meter that has never answered is not given a value it never had**

**Given** a meter whose every fetch has failed since start-up
**When** the wire is read
**Then** its DBIRTH declared it with no value and a non-good quality, and **no DDATA claims one**
**And** the bridge does not synthesise a zero, a default, or a neighbour's reading.

> This is the state of Guy's fourth meter, permanently. The DBIRTH is already honest; the risk is a
> republish path that reaches for *something* to send.

**AC5 — the screen and `/healthz` agree with the wire** *(ADR 0027 §1, the honesty half of FR28)*

**Given** a meter whose source is in `Failed`
**When** the operator opens `/`
**Then** the page does not describe the bridge as *"polling the meters and publishing what it
reads"* without qualification
**And** `/healthz` still answers `200` (ADR 0027 §2 — a restart cannot clear a rejected credential)
but distinguishes a fault from a deliberate silence.

## What is done, 2026-08-07

**AC1, AC2 and AC4 are implemented and falsified.** The poll task carries its own `last`
measurement and publishes a verdict every tick; a meter that has never answered still gets
nothing. Two mutations, both red at the assertion under repair: withholding the republish gives
`left: 1, right: 2` — the premise reading alone, so the mutation removed exactly the republish —
and inventing a value for a meter that never answered gives `left: 1, right: 0`.

**AC5 is done** (2026-08-07, second pass). The oracle verdict is recorded in the same per-meter
cell the heartbeat lives in, so the page and `/healthz` read what the task writes rather than a
second opinion. `/` names the failed meters — *"one meter is not being read: cellar"* — and
`/healthz` carries `failed_sources`, an empty list when all is well. The status code stays `200`
per ADR 0027 §2. Falsified by making `failed_sources` return nothing: the page reverts to its
unqualified claim and the test dies on it.

That test also introduced a `body()` helper that reads the **rendered bytes**. Every existing
`/healthz` test asserted only a status code, and the one test that did look at content used
`format!("{response:?}")`, which prints `body: Body(UnsyncBoxBody)` and never the content — the
hollow assertion found on 2026-08-05.

**AC3 is done** (2026-08-07, third pass), at the publisher — where routing by serial and the
published quality meet, and where a shared fixture would hide the defect. Four meters, three
answering and one silent, each update carrying its own serial; the emitted messages are indexed
**by topic** into a map and the whole map is asserted, because a count would be satisfied by four
messages addressed to one device.

Falsified by routing every DDATA to the first declared device. The result is worth keeping:
`left: {"30000004": 192}` — one entry, because all four landed on one device, and the silent
meter's `Bad_Stale` vanished from the wire entirely. The harm the assertion names, produced
rather than argued.

## Falsification

- **AC1's mutation is to restore the `if let Ok(reading)` guard.** Assert it fails on the count of
  DDATA for the failing meter, not on a later quality assertion.
- **AC2's mutation is to publish `Quality::Good` in the republish.** If the test still passes, it is
  asserting emission rather than content.
- **Prove the stream flows before asserting anything about its shape.** Every absence and every
  count in this story is meaningless over a wire nothing reached; story 3.1's cadence test needed
  three attempts for exactly this reason, and one of its mutations compiled and stayed green.
