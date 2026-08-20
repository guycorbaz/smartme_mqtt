# Story 6.6: The end-to-end check — three links, one screen, and nothing published to make them light up

Status: done

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
- [Source: `crates/smartme-bridge/src/ui/screens.rs:876`] — `fetch_listing`, the UI's existing path to smart-me
- [Source: `crates/smartme-bridge/src/app/mqtt_driver.rs:1094`] — `SinkState`, the third link's fact
- [Source: `docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md`] — no credential on any screen
- [Source: `docs/adr/0024-the-config-ui-refuses-submissions-from-other-origins.md`] — the origin guard on POST routes
- [Source: `CLAUDE.md`] — falsify before trusting; decide at drafting, never defer to an artifact that does not exist

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-20.

### Completion Notes List

**AC1 — met, and the proof is the whole snapshot.** `run_check` writes one thing: the check
registry, which lives on `UiState` and which nothing outside `ui` can read. The test compares
`FleetState` before and after a full check — `generation` included, because that counter moves
on every `send_modify`, so a write that happened to store the same value is still visible.

**AC2 — met.** The middle link reads `published`, `culprit` and `last_published_at` from the
fleet state, and the page says in words that the check did not re-judge, and that the two links
disagreeing is the useful part rather than a contradiction.

**AC3 — met, and held structurally rather than by observation.** `arch_purity` gains
`the_end_to_end_check_cannot_publish`: `ui/check.rs` may not mention `Outbox`, `outbox`,
`Publisher`, `publish(` or `Publication` outside comments. A runtime assertion could only ever
say that the paths a test happened to walk sent nothing; this says the module has no way to.

**AC4 — met, and it produced the story's one refactor.** `SourceError::cause` is extracted from
`Policy::step_remembering` into `core/source.rs`, and the state machine now reads it. The check
needed the same mapping outside the poll loop; a copy would have been a second place the truth
lives, which is the very thing this story's shape exists to avoid. What stayed in the state
machine is which *state* each error lands in — `Fatal` latches `Failed`, everything else is
`Stale` — because that is the machine's own business and not the table's.

**AC5 — met.** One check in flight per meter, and no second inside the poll period, refused in
words that name when it may be run again. The rule is a pure function so a test can walk all
four of its cases.

**AC6 — met.** `POST` starts the check and redirects; `GET` reports it. So the three states are
rendered by the server, the running one carries a two-second `meta refresh`, and a reload
re-reads the answer rather than re-asking smart-me. The bound is `CHECK_TIMEOUT`, five seconds,
the same as discovery's.

**A defect this implementation avoided by measuring rather than assuming.** `axum`'s `Query`
extractor is behind a feature this workspace does not enable — the build said so. The query
string is read from the `Uri` with a six-line decoder rather than by turning on a feature and
enlarging the dependency surface for one parameter.

**The base is the RUNNING configuration's, not the saved file's.** Discovery reads the file
because it runs before any configuration is in force; a check only exists once there is a poll
loop, and the base it must exercise is the one that loop is using. There is therefore no
unreadable-file case here, and the variant that would have carried it was removed rather than
left unconstructed.

### The trap these tests could have fallen into, named

`a_check_writes_nothing_the_poll_loop_reads` runs a real `run_check`, and **would have passed
for two different reasons**: with no credential in the environment the check never builds a
client, and with one — other tests in this binary set `SMARTME_CLIENT_*`, and environment
variables are per-process — it would have sent a real request to smart-me and blocked on the
timeout. The fixture's `api_base` is therefore `http://127.0.0.1:1`: `SmartMeClient::new`
refuses any scheme but `https` before opening a socket, so the test is deterministic and off
the network whatever the environment holds. Verified by running it with
`SMARTME_CLIENT_ID`/`SMARTME_CLIENT_SECRET` set: 0.00 s.

### Falsification record

| # | Mutation | Went red with |
|---|---|---|
| 1 | `control.heartbeats().retire(&meter)` before asking — the "clean slate" edit | `a check must not move the fleet state: generation 1 became 2 … left: 1, right: 2` |
| 2 | the `TooSoon` arm deleted (a finished check always re-runs) | `a second check inside the poll period must be refused: the button would out-poll the poll loop` |
| 3 | link 2 rendered without the verdict's cause | `the page must report the verdict IN FORCE, cause included` — with the page dumped beside it |
| 4 | the `meta refresh` dropped from the running branch | `a page that says "asking" must come back for the answer` — and the dump showed the page still *saying* it refreshes itself |
| 5 | `is_fatal()` forced false (a refused credential treated as passing trouble) | `left: SourceUnreachable, right: CredentialRejected` |
| 6 | `use crate::app::poll_publish::Publication;` added to the module | `the end-to-end check must not be able to publish (story 6.6 AC3)` |

### File List

- `crates/smartme-bridge/src/core/source.rs` — modified (`SourceError::cause`, extracted)
- `crates/smartme-bridge/src/core/state_machine.rs` — modified (reads the table instead of repeating it)
- `crates/smartme-bridge/src/ui/check.rs` — **new** (the module, its rate rule, its one call, its page)
- `crates/smartme-bridge/src/ui/mod.rs` — modified (`checks` on `UiState`, the two routes, five tests)
- `crates/smartme-bridge/src/ui/screens.rs` — modified (`page`, `repair`, `ago` opened to the module next door; the link from `/meters`)
- `crates/smartme-bridge/tests/arch_purity.rs` — modified (the guard AC3 asks for)
- `_bmad-output/implementation-artifacts/6-6-the-end-to-end-check.md` — modified
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Change Log

- **2026-08-20** — Story 6.6. FR37, as three facts from three owners rather than one
  re-judgement. Six mutations run. `CONTRACT_VERSION` stays at 10 — nothing here reaches the
  wire, which is the point.

### Review — 2026-08-20

**Every acceptance criterion holds.** AC1 is proven on the whole snapshot rather than on the
fields a test remembered to look at; AC3 is structural, which is the only form of "publishes
nothing" that a test can actually hold; AC4's mapping goes through `SourceError::cause`, so
the check and the poll loop cannot drift apart.

**One defect found and repaired: `/check` was reachable from `/meters` alone.** A screen
nothing links to does not exist, and the page an operator opens first is `/`. Repaired, with
both halves pinned — the running phase offers the two screens, and a silent phase offers
neither, because a bridge with no poll loop has nothing to check and the way out is the
configuration. Falsified.

**One claim in the implementation report was wrong, and it is corrected here rather than left
standing.** The report flagged the `meta http-equiv=refresh` as living in the body and
therefore non-conformant. It does not: `page()` emits `<!doctype>`, the two metas, the title
and the style, and this module's `{refresh}` is inserted before the first content element, so
the HTML5 parser keeps it in the implicit `head`. Nothing to repair, and the doubt is recorded
so the next reader does not re-open it.

**Two residues, both named and neither this story's:**

- **[#103]** — the repair gesture derives from the culprit alone, so the check's failure
  wording is the three-sentence table rather than a per-cause one. FR31's subject.
- **[#77]** — a 429 on the token endpoint still arms no wait. AC5's rate rule keeps the button
  from being the cause of one; it does not fix the underlying gap, and it does not claim to.
