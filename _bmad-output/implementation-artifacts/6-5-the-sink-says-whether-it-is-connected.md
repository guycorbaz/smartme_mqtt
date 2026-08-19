# Story 6.5: The sink says whether it is connected — FR29, and what [#53] has been waiting for since 4 August

Status: review

> **[#53] decides the order of work, exactly as AR19 did for 6.3.** *"This issue is what is still
> missing: the sink's actual state."* The bridge knows it **intends** to publish — `/healthz` was
> corrected from `publishing` to `intends_to_publish` on 2026-08-04 for precisely that reason — and
> it does not know whether the broker is reachable. `Transport::Connected` exists in the driver
> (`mqtt_driver.rs:1489`) and is plumbed nowhere.
>
> **So FR37 cannot be written first.** An end-to-end validation whose third light was lit by an
> intention rather than a fact is the kind of lie this project exists to refuse. This story makes
> the third light possible; FR37 is the next one.

## Story

As the operator,
I want to see whether the broker is actually reachable, separately from whether the meters are
readable,
so that "nothing is being published" tells me which end to look at.

## Acceptance Criteria

**AC1 — the sink's state is observed, not inferred.**

**Given** the driver's `Transport::Connected` and `Transport::Lost` events
**When** either fires
**Then** a shared sink state records it with the instant it happened
**And** nothing infers connectivity from the poll loop's liveness, which is the source's health
and a different question.

**AC2 — `/healthz` reports it in the body and NOT in the status code.**

**Given** a bridge whose broker is unreachable
**When** `/healthz` is read
**Then** the body says the sink is disconnected, with since when
**And** **the status code is still 200**.

*[#53]'s own words, and the reason is Epic 7: an unreachable broker is an honest STALE — the
bridge is working correctly and saying so — and restarting the container fixes nothing. The rule
stays "unhealthy only for a wedged poll loop". A story that let the sink drive the code would hand
Epic 7 a restart loop triggered by somebody else's outage.*

**AC3 — the two healths are independent, and the screen shows both (FR29).**

**Given** a meter that cannot be read and a broker that cannot be reached
**When** the state screen is rendered
**Then** source health and sink health are separate lines, and either can be bad while the other
is good
**And** the page says which end is at fault rather than "something is wrong".

**AC4 — the sink state survives what it must and forgets what it should.**

**Given** a reconnect
**When** it succeeds
**Then** the state says connected, with the new instant
**And** a bridge that has never connected is distinguishable from one that has disconnected —
`None` is not `Disconnected`.

**AC5 — falsification.**

**Given** each assertion
**When** the mechanism it names is broken
**Then** a test goes red, and the run's output is copied next to it.

## Dev Notes

### What must not break

- **The status code rule.** ADR 0026, ADR 0027 and story 6.1 all say the same thing: non-200 is
  for a wedged poller. This story adds a health that must stay out of it.
- **`intends_to_publish` keeps its name.** It answers "has the bridge reached the publishing
  arm", which is still a different question from "is the broker there". Two honest fields beat one
  that tries to mean both.
- **The driver writes, the UI reads.** Same shape as `Heartbeats`: a `watch` the driver owns and
  the UI clones from. Nothing formatted crosses it (story 6.3 AC4).

### References

- [Source: `https://github.com/guycorbaz/smartme_mqtt/issues/53`] — the issue this closes, and its guard on the status code
- [Source: `_bmad-output/planning-artifacts/prd.md:308`] — FR29
- [Source: `crates/smartme-bridge/src/app/mqtt_driver.rs:1489`] — `Transport::Connected`, already emitted
- [Source: `crates/smartme-bridge/src/app/supervisor.rs:115`] — `Control`, the established way to hand a live handle to the UI
- [Source: `CLAUDE.md`] — falsify before trusting

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-19.

### Completion Notes List

**AC1 — met.** `SinkHealth` is a `watch` the driver owns and every surface clones from — the same
shape as `Heartbeats`, for the same reason. It is written in the `Transport::Connected` and
`Transport::Lost` arms, which fire on every CONNACK and every drop, so the first connect and every
reconnect are covered by one call site each.

**AC2 — met, and it is the criterion this story exists to protect.** `/healthz` gains
`sink_connected` and `sink_since_ms` **in the body**, and the status code is untouched. Falsified:
letting a disconnected sink drive the code yields `503`, and the test refuses it with the reason —
Epic 7 wires non-200 to a container restart, which would kill every meter's Sparkplug session over
somebody else's outage.

**AC3 — met.** The meter page names which end is at fault, with since when, and says that
restarting the bridge repairs nothing — the same judgement `/healthz` makes by staying at 200,
expressed in the words an operator reads.

**AC4 — met.** `None` is not `Disconnected`. A bridge that has never connected has not lost
anything, and falsifying it — reporting `false` before the first connect — goes red.

**THE `#[allow]` NAMED ITS OWN REVISIT TRIGGER, AND THE TRIGGER FIRED.** `mqtt_driver::run`
carried *"eight parameters, and the revisit trigger is the ninth"* since Epic 1, under Epic 3's
action D4. Story 6.5 needed the ninth. **The revision was honoured rather than the count raised**:
the meters' health and the broker's went into `Health`, because they are the pair FR29 requires to
be reported independently and they genuinely travel together. The parameter count is unchanged at
eight and the next trigger is the ninth again. *An `#[allow]` that carries its trigger is worth
writing: this one was written in July and paid in August.*

**Seven integration files needed updating for the signature change**, and two of my replacements
were wrong — a qualified path overwritten, and `poll_publish::run` altered when it takes the
heartbeats alone. The compiler caught both, which is what a traversing type change is supposed to
cost.

**[#53] closes on this.** It was opened by the story 6.1 review on 2026-08-04 with the sentence
this story implements: *"This issue is what is still missing: the sink's actual state."*

### Falsification record

| # | Mutation | Went red with |
|---|---|---|
| 1 | a disconnected sink drives the status code | `AN UNREACHABLE BROKER IS AN HONEST STALE … left: 503, right: 200` |
| 2 | never-connected reports `false` | `a bridge that has never connected has not lost anything` |
| 3 | the unreachable branch renders as "connected" | `an unreachable broker must be named as such, with since when` |

### File List

- `crates/smartme-bridge/src/app/mqtt_driver.rs` — modified (`SinkState`, `SinkHealth`, `Health`, both transport arms, `run`'s signature)
- `crates/smartme-bridge/src/app/supervisor.rs` — modified (`Control::sink`, the handle, `detached_with_sink`)
- `crates/smartme-bridge/src/ui/mod.rs` — modified (`/healthz` body, two tests)
- `crates/smartme-bridge/src/ui/screens.rs` — modified (the sink line on the meter page)
- seven integration test files — modified (the `Health` argument)
- `_bmad-output/implementation-artifacts/6-5-the-sink-says-whether-it-is-connected.md` — new
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Change Log

- **2026-08-19** — Story 6.5. The sink's state is observed rather than inferred, reported in the
  body and never in the status code, and named on the page with the gesture it calls for. [#53]
  closes. FR29 delivered; FR37 is now writable.
