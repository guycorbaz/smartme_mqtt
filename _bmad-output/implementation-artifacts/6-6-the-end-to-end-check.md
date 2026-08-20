# Story 6.6: The end-to-end check — three links, one screen, and nothing published to make them light up

Status: ready-for-dev

> **Story 6.5 is what made this story writable, and it said so.** Until the sink's state was
> observed, an end-to-end validation whose third light was lit by an *intention* — the bridge
> having reached its publishing arm — would have been the kind of lie this project exists to
> refuse. `SinkHealth` now carries a fact, and FR37 can be answered without inventing one.
>
> **The temptation this story has to refuse, said before any code:** the obvious way to prove
> the sink is to publish something. It is also the one thing this bridge may not do. A DDATA
> carrying a test value is indistinguishable, in the historian, from a measurement — nobody
> downstream can tell that a number came from a button rather than from a meter — and a topic
> outside the Sparkplug grammar is refused by `EdgeNode::device_topic` anyway. **The check
> observes; it never injects.**

## Story

As the operator who has just changed something,
I want to point at one meter and ask the bridge to show me the whole chain — did the source
answer, what is being published for it, and is the broker there —
so that I learn which link is broken from one screen instead of correlating three.

## The decision this story takes at drafting, and why it is not the implementer's

**The check does NOT judge the reading it fetches, and the reason is `MeterMemory`.**

The instinct is to run the freshness and quality oracles over the reading the button just
fetched, so the middle light is about *this* reading. Walking the code refuses it:
`step_once` judges against a per-meter memory — `energy_reference`, `last_http_date`,
`last_value_date` — and three oracles read it. A validation that judged would either

- **mutate that memory**, and then a click changes what the loop judges next: a counter
  compared against a reference the button moved reports `counter-went-backwards` on the next
  real tick, and `feed_is_advancing` gets vouched for by a reading no publication ever
  carried; or
- **judge without it**, which is a second assembly of judgements — a second place the truth
  lives, which is the exact defect AR19's *"UI consumes this state, never recomputes it"*
  exists to prevent, and which story 6.3 was ordered before 6.4 to avoid.

**So the three links report three facts, each from its own owner:**

| Link | What it reports | Whose fact it is |
|---|---|---|
| source | a real `get_device` for this meter's `device_id`, now, with its latency | the check's own call |
| value | the verdict, cause and culprit **in force**, with the instant they were reached | the poll loop, through `FleetState` |
| sink | connected / never-connected / unreachable-since, plus this meter's last publication and its drop counters | the driver, through `SinkHealth` and `FleetState` |

This is more useful than a second judgement, not less: the middle light shows what the SCADA
is being told *right now*, and the operator sees the gap between "the source answers" and
"the bridge still publishes Bad" — which is precisely the state a latched meter is in, and
the one a check that re-judged would paper over.

## Acceptance Criteria

**AC1 — one meter, chosen, and the call is real.**

**Given** a configured meter
**When** the operator triggers the check for it
**Then** the bridge performs a real `get_device` against the saved `api_base` with the
credential from the environment, for that meter's `device_id`
**And** the answer is reported with the time it took
**And** nothing is written to the fleet state, the energy reference, or any other memory the
poll loop reads — the check is observable in the log and nowhere else.

**AC2 — the second link is the published verdict, not a fresh opinion.**

**Given** the reading the check just fetched
**When** the result is rendered
**Then** the value link shows the verdict, cause and culprit **in force** for that meter with
`last_published_at` beside them, taken from the fleet state
**And** the page says in words that this is what the host is being told, as against what the
source just answered — the two being different is a fact worth seeing, never an error.

**AC3 — the third link is observed, and nothing is published to light it.**

**Given** the sink's state
**When** the result is rendered
**Then** the sink link shows connected / never connected / unreachable-since, this meter's
last publication instant, and its drop counters by reason
**And** **no MQTT message of any kind is produced by the check** — asserted by a test that
fails if one reaches the outbox.

**AC4 — a failure at any link names the link, the culprit and the gesture.**

**Given** a credential the account refuses, an unreachable API, a meter absent from the
account, and an unreachable broker
**When** each is checked
**Then** the page names which link failed, carries `Culprit::{World, You, Bridge}` and the
repair gesture, and never shows a stack trace or a raw error type (FR31's rule on this page)
**And** the wording is `SmartMeError`'s own — story 2.6 AC5 wrote a repair into each variant's
`Display`, and this page adds no second opinion.

**AC5 — the check cannot become a way to hammer smart-me.**

**Given** an operator clicking repeatedly, and [#77]'s finding that a 429 on the token
endpoint arms no wait
**When** a check is already in flight for a meter, or one completed less than the configured
poll period ago
**Then** the second is refused **in words on the page**, naming when it may be run again
**And** the refusal is never silent, and never a spinner that resolves into the previous
result.

**AC6 — waiting is a state the page renders, not a page that hangs.**

**Given** a source that does not answer
**When** the check is run
**Then** it is bounded by the same timeout discovery uses, and the expiry is reported as a
result — *"no answer in N seconds"* — rather than as a broken page
**And** the three states FR32 separates stay separable: a check never run, a check running,
and a check that answered.

**AC7 — falsification.**

**Given** each assertion above
**When** the mechanism it names is broken
**Then** a test goes red, and the run's output is copied next to it.

## Out of scope, named rather than left to be inferred

- **FR35 — the auto-written configuration context line.** It is the other unwritten
  requirement of this epic and it touches configuration persistence: nothing in `store.rs`
  records when the configuration was created or last changed, so FR35 begins by adding those
  facts, not by drawing a screen. Its own story.
- **[#103] — the repair gesture per cause.** AC4 asks the check's failures to be actionable,
  which the three culprit sentences already are for this page's four cases. The general table
  — one gesture per `Cause`, pinned on the `qos_for` pattern — stays [#103]'s, and this story
  must not grow a second half-table beside it.
- **Lifting a latch.** A meter in absorbing `Failed` stays there whatever the check finds; the
  page says which gesture clears it ([#81] is the standing question of whether it should keep
  fetching, and it is not decided here).

## Dev Notes

### What must not break

- **The check writes nothing.** Not the fleet state, not `MeterMemory`, not the energy
  reference file, not the outbox. The table above is the whole reason this story exists in
  this shape.
- **`arch_purity`.** The UI may read the domain; it may not reach for a clock of its own or
  import a fake.
- **The credential never reaches the page.** [ADR 0023](../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md) keeps it in the environment; the check
  reads it the way `fetch_listing` does and renders nothing of it, not even its length.
- **The origin guard.** Every mutating route carries `origin::refusal` ([ADR 0024](../../docs/adr/0024-the-config-ui-refuses-submissions-from-other-origins.md), story 6.2
  AC5). This one is a POST, so it carries it too — even though it mutates nothing, because the
  guard is about who may make the bridge call smart-me.

### The paths this story reuses rather than re-spells

- `screens::fetch_listing` — how the UI already reaches smart-me: base from the saved
  configuration, credential from the environment, its own timeout, `SmartMeError`'s wording on
  failure. The check is the same shape with `get_device` in place of `get_devices`.
- `screens::sink_health_line` — the sink's words, already shared by `/` and `/meters` since the
  review of story 6.5. A third spelling here would be a third place for the truth to live.
- `Culprit::as_str` and `repair` — the culprit and its gesture, derived at render time
  (story 6.3 AC4).
- `FleetState::meters` — `published`, `culprit`, `last_published_at`, `dropped`.

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:316`] — FR37
- [Source: `_bmad-output/planning-artifacts/epics.md:153`] — AR19, *"UI consumes this state, never recomputes it"*
- [Source: `crates/smartme-bridge/src/app/poll_publish.rs:801`] — `step_once`, and the `MeterMemory` this story refuses to touch
- [Source: `crates/smartme-bridge/src/ui/screens.rs:850`] — `fetch_listing`, the UI's existing path to smart-me
- [Source: `crates/smartme-bridge/src/app/mqtt_driver.rs:1094`] — `SinkState`, the third link's fact
- [Source: `docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md`] — no credential on any screen
- [Source: `docs/adr/0024-the-config-ui-refuses-submissions-from-other-origins.md`] — the origin guard on POST routes
- [Source: `CLAUDE.md`] — falsify before trusting; decide at drafting, never defer to an artifact that does not exist
