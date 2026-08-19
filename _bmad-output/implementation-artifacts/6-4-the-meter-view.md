# Story 6.4: The meter view — one page that answers "what is happening" at three in the morning

Status: review

> **Four requirements, one page.** FR28, FR30, FR34 and FR36 describe the same table seen from
> four angles: a row per meter carrying its value and how old it is, its last success, its last
> error, and whose fault that is. Writing them as four stories would produce four screens where
> the epic asks for *"single-screen confidence"*.
>
> **Story 6.3 supplied eleven of the columns and deliberately not the value.** AR19 describes the
> state of *publication*, not the measurements. This story adds the two numbers FR28 names, and
> nothing else.

## Story

As the operator woken at three in the morning,
I want one page that tells me each meter's value, its age, whether it reached the broker, and
whose fault it is when it did not,
so that I know within seconds whether to go back to bed, fix a configuration, or look at the
world.

## What the state already answers, and what it does not

**Measured before drafting** (`MeterState` has eleven fields since 6.3):

| FR28 asks for | Where it comes from |
|---|---|
| live value | **missing** — this story adds it |
| unit | **not a field, by decision** — `Kw`/`Kwh` are domain types; kW and kWh are constants, and a per-meter copy would duplicate what the type already holds |
| freshness age | `source_value_date` against the clock, with `staleness_threshold_ms` for the judgement it was reached under |
| target topic | the configuration, through `Control::current()` — built by the same call the publisher uses, never a second spelling |
| serial | the configuration |
| published status | `published: Verdict` |

**And the two numbers are stored as numbers.** Story 6.3's AC4 forbids anything expensive under
`send_modify`, and `MeterId`/`Serial` are `String` newtypes — so storing the last `Measurement`
whole would allocate twice per meter per tick, under the lock every poll task waits on. Two
`Option<f64>` allocate nothing.

## Acceptance Criteria

**AC1 — the value reaches the state without taxing the loop.**

**Given** a published reading
**When** the fleet is read
**Then** `last_power_kw` and `last_energy_kwh` carry it
**And** no allocation is added under `send_modify` — the fields are `Option<f64>`, and the story
records why.

**AC2 — one row per meter, and every column is read rather than recomputed.**

**Given** the enriched state and the live configuration
**When** the meter view is rendered
**Then** each row shows: meter, serial, target topic, value with its unit, freshness age,
published quality, last publication, last change, and culprit
**And** **no verdict, quality or culprit is computed in the template** — AR19's rule, and the
reason 6.3 came first.

**AC3 — the three states are distinguishable at a glance (FR32).**

**Given** a bridge with no configuration, one with a fault, and one that has not completed its
first tick
**When** each is rendered
**Then** the page says which of the three it is, in words, and never shows an empty table that
could be read as "all quiet".

**AC4 — the culprit carries its repair gesture, derived at render time.**

**Given** `Culprit::{World, You, Bridge}` on a row
**When** it is rendered
**Then** the page names what to do — wait and watch, open the configuration, or read the log —
**derived from the culprit and the cause**, never stored (story 6.3 AC4).

**AC5 — a frozen meter reads differently from a quiet one.**

**Given** a meter republishing an unchanged reading for an hour
**When** its row is rendered
**Then** the page distinguishes "last published a second ago" from "last changed an hour ago",
which is the pair 6.3 built `last_changed_at` for.

**AC6 — falsification.**

**Given** each assertion about what a row says
**When** the field it reads is broken or blanked
**Then** the test goes red, and the run's output is copied next to it.

## Out of scope

**FR37 — on-demand end-to-end validation** — is its own story: it triggers network work from a
click and raises what the page shows *while waiting*, which is a different question from
rendering state that already exists. **FR35 — the auto-written configuration context line** —
touches configuration persistence more than display. Both are named so the next story inherits a
boundary rather than a surprise.

## Dev Notes

### What must not break

- **AR19's rule is the whole reason for 6.3**: the template reads, never judges. A `match` on a
  `Cause` inside a template is the second place the truth lives.
- **`arch_purity`**: the UI may read the domain; it may not import a fake or reach for a clock.
- **The topic is built by the publisher's own path**, not by string concatenation in a template —
  `EdgeNode::device_topic` refuses what the grammar refuses, and a page that spelled it itself
  would show a topic the bridge would never publish on.

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:307`] — FR28, FR30, FR34, FR36
- [Source: `_bmad-output/planning-artifacts/epics.md:153`] — AR19
- [Source: `crates/smartme-bridge/src/app/poll_publish.rs`] — `MeterState`'s eleven fields
- [Source: `crates/smartme-bridge/src/domain/measurement.rs:60`] — `string_newtype!`, why the value is stored as numbers
- [Source: `CLAUDE.md`] — falsify before trusting

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-19.

### Completion Notes List

**AC1 — met, as two numbers.** `MeterState` gains `last_power_kw` and `last_energy_kwh`. Storing
the `Measurement` whole would have allocated twice per meter per tick under `send_modify` —
`MeterId` and `Serial` are `String` newtypes — which story 6.3 AC4 forbids. **The unit is not a
field**: `Kw`/`Kwh` are domain types, kW and kWh are constants, and a per-meter copy would be a
second place for them to disagree.

**`Eq` left `MeterState`, and the reason is the type rather than the compiler.** A measured value
is an `f64`, and a measurement has no total equality — `NaN != NaN` is the arithmetic saying that
"unknown equals unknown" is not a question with an answer. `PartialEq` remains, every existing
comparison still works, and nothing keyed a map on this state.

**AC2, AC3, AC4, AC5 — met.** `/meters` renders one row per meter: meter, serial, topic, power,
energy, published quality with its cause, last publication, last change, and culprit with its
repair gesture. Nothing on the page is judged — every verdict comes from the state the poll loop
wrote, which is the whole reason 6.3 came first.

**THE FALSIFICATION FOUND A DEFECT IN THIS STORY'S OWN SCREEN.** Printing the full HTML on failure
showed `spBv1.0/G/DDATA/N/—` — a topic built with a dash where the serial should be, for a meter
the running configuration no longer carries. A page cannot show a destination nothing will ever
publish on. Repaired: no serial, no topic. **A test that only said "wrong" would have left it
there**, and this is the second time today that printing what was seen mattered more than the
assertion itself.

**CLIPPY REFUSED EIGHT ARGUMENTS AND THE DESIGN IMPROVED.** `record_at` now takes a `Publication`
— the instant, the threshold that judged it, the acquisition time, and the two numbers. Those five
are one event seen from five angles, and passing them separately let a caller supply the instant
without the threshold it was judged against, which is the pairing 6.3 AC1 exists to keep together.
The lint asked for a grouping; the model was wrong before it.

**Clippy also caught the screen written AFTER the test module** — code a reader finds where they
do not look for it. Moved.

**No conformance row moved and `CONTRACT_VERSION` stays at 10**: nothing here reaches the wire.

### Falsification record

| # | Mutation | Went red with |
|---|---|---|
| 1 | `last_changed_at` rendered from `last_published_at` | `a FROZEN meter must not read like a quiet one` — both columns said `1 second ago` |
| 2 | absent value rendered as `0.000 kW` | `a value nobody published must not render as a number` |
| 3 | the unconfigured phase returns a table shell | `an unconfigured bridge must SAY so … found a table instead` |

### File List

- `crates/smartme-bridge/src/app/poll_publish.rs` — modified (`Publication`, two value fields, `Eq` dropped)
- `crates/smartme-bridge/src/ui/screens.rs` — modified (`meter_view`, `repair`, `ago`)
- `crates/smartme-bridge/src/ui/mod.rs` — modified (the `/meters` route, three tests)
- `_bmad-output/implementation-artifacts/6-4-the-meter-view.md` — new
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Change Log

- **2026-08-19** — Story 6.4. FR28, FR30, FR34 and FR36 on one page, consuming the state 6.3
  built. Three mutations run; one of them found a defect in the screen itself. FR37 and FR35 stay
  out of scope, named.
