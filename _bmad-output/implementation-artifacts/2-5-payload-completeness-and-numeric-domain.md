# Story 2.5: A field the bridge could not read degrades that field, names it, and substitutes nothing

Status: review

## Story

As the operator,
I want a payload the bridge could not fully read to degrade only the number it could not read, and to say which one and why,
so that an unusable power reading does not take a perfectly good energy index down with it, and so that the fault I am sent to repair is the one that happened.

## Why this exists, and what it is NOT

**ADR 0031 stopped at the oracle layer, and this story is where it reaches the boundary.** A
verdict belongs to a metric — decided 2026-08-11, implemented in `compose`, `metrics_for` and the
operator surfaces. But `map_device` (`adapters/smartme_source.rs:265`) predates it and still does
the opposite: **any one field failing sets `Quality::Bad` for the whole reading.** An unknown unit
on `ActivePower` today degrades the cumulative energy index beside it, which was read perfectly,
converted perfectly, and about which nothing is wrong. That is precisely the defect ADR 0031 was
written to remove, still live one layer upstream.

**And `BAD_CARRIER = 0.0` is a substituted value, which FR16 forbids in its own words.** When a
conversion fails, `map_device` puts `0.0` into the measurement and marks it `Bad`. FR16 reads:
*"…yields degraded quality, **never a substituted value**"*. Story 2.3's review already found that
number reaching the wire — republished as a genuine `Double` marked `Stale` one tick later — and
closed that path. **The path was closed; the substituted number is still there**, waiting for the
next path nobody predicted.

### What this story does NOT do, and the boundary is drawn here rather than guessed later

**FR16's `min/max bounds` clause is dead.** Its wording — *"or a value outside per-metric min/max
bounds"* — is physical plausibility under another name, and that is [ADR 0033]: withdrawn on
2026-08-12 because it requires knowledge the bridge does not receive and cannot verify. **This story
implements no bounds of any kind.** What survives of FR16 is completeness, the numeric domain
(non-finite, overflow), and the prohibition on substituted values. Written down here because a
later reader finding `min/max` in the PRD and nothing in the code would reasonably conclude
something had been forgotten.

**Transport and authentication faults belong to story 2.6.** A 401, a 429, a 500, a timeout: those
are the error taxonomy, and `SourceError` is its home. **This story owns what arrives inside a
payload the transport delivered successfully.** The two meet at `map_error` and do not overlap.

## Acceptance Criteria

**AC1 — A field that could not be read degrades that field alone.**

**Given** a smart-me response whose `ActivePowerUnit` is a unit the bridge does not recognise, while
`CounterReading` and its unit are sound
**When** the reading is published
**Then** `Power` is null with a non-good quality and a cause naming the fault, **and `Energy`
carries its real converted value at quality `Good` with no cause at all**
**And** the symmetric case holds: an unreadable energy field leaves `Power` good
**And** both are falsified by restoring the reading-wide verdict — the untouched metric's assertion
must go red, naming the metric.

**AC2 — The verdict crosses the boundary as judgements, not as a quality flag.**

**Given** the oracle layer built by stories 2.1 and 2.3
**When** the adapter reports what it could not read
**Then** it produces per-metric judgements that compose like every other oracle's, rather than
setting `Measurement::quality` and leaving the layer to infer a cause
**And** the existing call site in `step_once` that respects `Quality::Bad` first — the guard that
stops `BAD_CARRIER` being judged as an energy index — is revisited in the same change, because it
exists only to work around the collapse this story removes.

**AC3 — No substituted value survives in the domain type.**

**Given** FR16's *"never a substituted value"*
**When** a conversion fails
**Then** `BAD_CARRIER` is gone and the domain type can express absence: `Measurement`'s numeric
fields become optional
**And** a test proves the type refuses to carry a number nobody measured — deleting the `Option` and
restoring a sentinel must not compile, which is a stronger guard than any assertion.

*Decided at drafting, and the alternative is recorded because it is tempting.* The cheaper route is
to keep the sentinel and rely on `metrics_for` nulling a `Bad` metric before it reaches the wire.
That is what happens today, and it held for exactly one story: 2.3's review found the sentinel
republished at `Stale` through `last`, a path nobody had predicted, and the repair was two flags
rather than one. **A value that must never be published is safest when it cannot be constructed**,
and the blast radius is the honest price: `Measurement`, the publisher, the oracles that read
`.energy`, and every test that builds one.

**AC4 — The cause names the field and the fault, and one cause is no longer three.**

**Given** the three ways a payload field is unusable today — a unit the bridge does not recognise, a
non-finite number, and an arithmetic overflow in the rescale
**When** the cause is published
**Then** they are distinct causes, because they send an operator to different places: a unit change
is the API's contract moving under us, a non-finite number is the device or the cloud misbehaving,
and an overflow is ours
**And** `Cause::ValueUnusable` — which today means all three and names no field — is either
retired or narrowed, with the choice recorded
**And** an unparseable `ValueDate` keeps its own cause rather than borrowing one of these: it
degrades freshness, not a value.

**AC5 — Completeness is already fail-closed, and this story records that rather than re-implementing it.**

**Given** FR16's *"a missing/null field yields degraded quality"*
**When** a field is absent from the smart-me payload
**Then** the current behaviour is verified and recorded: `Device` carries no `#[serde(default)]`, so
a missing field fails deserialization and **nothing is published at all**
**And** the deviation from FR16's letter is recorded as deliberate: publishing a degraded reading
assembled from a payload we could not parse would claim a measurement we do not have
**And** the residual is named for story 2.6 rather than fixed here — the operator currently sees a
generic parse failure and no field name.

**AC6 — Falsified before trusted.**

**Given** every assertion this story adds
**When** it is written
**Then** it is run against deliberately broken code and observed to fail, with the mutation recorded
beside the test
**And** the mutations include: the per-field verdict widened to the reading; each new cause pointed
at its neighbour; and the `Option` replaced by a sentinel (which must fail to compile — recorded as
such, since a compile failure is the assertion).

**AC7 — `CONTRACT_VERSION` moves 6 → 7, additive.**

**Given** new cause strings
**When** they are added
**Then** `contract_golden.rs` fails FIRST — observed, not assumed — and then the version and its
golden move together, `GOLDEN_*_V7` written out rather than aliased
**And** it is recorded **additive**: the cause vocabulary grows, no metric name, unit or nulling
rule changes for a situation that already existed
**And** the manual, the runbook's attestation block and the conformance matrix follow in the same
commit, with the mechanical grep for the old number re-run.

**AC8 — No verdict that is correct today changes**, apart from the cases these criteria name — the
same proof stories 2.1, 2.3 used, for the same reason.

## Tasks / Subtasks

- [x] **Task 1 — Per-field outcomes at the boundary** (AC1, AC2)
  - [x] `map_device` returns what it could and could not read, per field
  - [x] Judgements scoped to `Measured::Power` / `Measured::Energy`, composed like any oracle's
  - [x] Revisit `step_once`'s `Ok(reading) if reading.value.quality == Quality::Bad` guard: it
        exists only to protect the monotonicity oracle from `BAD_CARRIER`
- [x] **Task 2 — Remove the sentinel** (AC3)
  - [x] `Measurement`'s numeric fields become optional; `BAD_CARRIER` deleted
  - [x] Follow the blast radius: publisher, oracles, republish path, every test constructing one
- [x] **Task 3 — Causes that name the fault** (AC4)
  - [x] Three distinct causes; decide the fate of `ValueUnusable` and record it
  - [x] Check `Cause::successor`'s chain — a new variant that misses it is a compile error by design
- [x] **Task 4 — Verify and record completeness** (AC5)
  - [x] A test that a missing field fails deserialization and publishes nothing
  - [x] Record the deliberate deviation from FR16's letter; name the residual for 2.6
- [x] **Task 5 — Contract** (AC7)
  - [x] Watch `contract_golden` fail first, then 6 → 7 with its golden, manual, runbook, matrix
- [x] **Task 6 — Falsify everything** (AC6)
- [x] **Task 7 — `./scripts/ci-local.sh` full run**, then `gh run list` after pushing

## Dev Notes

### The trap this story is most likely to fall into

**Asserting the split in the core and nowhere on the wire.** This is not a hypothetical: story
2.3's review found exactly it. Every test reaching `metrics_for` handed it a `Verdicts::uniform`,
where the old and new code agree on every output, so reverting the whole of ADR 0031 left the entire
suite green. **The property lives on the published `Metric`s** — assert the pair there, on one
update, with the good metric's assertion able to go red on its own.

**And the second trap is AC3's.** Making a field `Option` is mechanical, and mechanical changes are
where a substituted value survives by being moved rather than removed. The question to ask at every
`unwrap_or`, `unwrap_or_default` and `map_or` the change introduces: *is this a number nobody
measured?* If it is, it is `BAD_CARRIER` wearing a different name.

### Where the code lives

- `crates/smartme-bridge/src/adapters/smartme_source.rs:38` — `BAD_CARRIER`; `:265` `map_device`;
  `:300-320` `rescale`, `convert_power`, `convert_energy`, all fail-closed and returning `Option`
  already — the information this story needs is computed and then thrown away
- `crates/smart-me-client/src/types.rs:18` — `Device`, no `serde(default)`, which is what makes AC5
  true today; `parse_value_date` returns `Option` and invents nothing
- `crates/smartme-bridge/src/core/oracle.rs` — `Cause`, `Judgement::about`, `Measured`, `compose_for`
- `crates/smartme-bridge/src/app/poll_publish.rs:~420` — the guard AC2 revisits
- `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs` — `metrics_for`, which nulls a metric
  on its OWN verdict since ADR 0031
- `crates/smart-me-client/fixtures/smartme_sample.json` — the contract-of-record, a real capture

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:286`] — FR16, whose `min/max` clause this story
  deliberately does not implement
- [Source: `docs/adr/0033-fr14-is-withdrawn-physical-plausibility-is-not-the-bridge-s-to-judge.md`]
  — why that clause is dead
- [Source: `docs/adr/0031-a-verdict-belongs-to-a-metric.md`] — the rule this story carries to the
  boundary
- [Source: `_bmad-output/implementation-artifacts/2-3-the-oracle-layer-finished.md`] — the review
  that found the sentinel on the wire, and the two-flag repair
- [Source: `_bmad-output/implementation-artifacts/2-1-the-oracle-layer-and-how-verdicts-compose.md:186`]
  — the deferred finding this story closes: *"the place whose three failure modes collapse into one
  undifferentiated verdict"*

### A possible consequence for [#69], not a promise

[#69] closes the day an oracle produces a composed `Bad` on a reading the SOURCE called `Good`.
**This story might supply it, and might not** — and which one depends on a design choice inside
AC2. If the adapter stops setting `Measurement::quality` and reports through judgements instead,
then a payload fault becomes exactly that case and the issue closes. If the quality flag survives
alongside the judgements, the source is still the one refusing, and nothing changes. Worth deciding
with that in view; not worth contorting the design to reach.

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

**2026-08-12 — all eight ACs met.**

**AC1/AC2 — ADR 0031 reaches the boundary.** `Reading` now carries `SourceFaults`: what the source
could not read, said once per field. `map_device` sets `Quality::Bad` for the READING only when the
reading as a whole is unusable — no timestamp of its own, or not one usable number in it — and a
single failed field becomes a metric-scoped judgement that composes like any oracle's. The guard at
`step_once` that shielded the monotonicity oracle from `BAD_CARRIER` is gone with the sentinel it
protected against: absence is `None`, and `None` is not comparable.

**AC3 — the sentinel is unconstructable, not merely unpublished.** `Measurement`'s numeric fields
are `Option`. The blast radius was as named: 5 source files, 39 occurrences, 15 construction sites,
and 36 test-compile errors that had to be read one by one rather than swept.

**AC4 — four causes where one word stood.** `unit-not-recognised` (smart-me's contract moved),
`value-not-finite` (the device or the cloud), `value-overflowed` (our own rescale) and
`source-timestamp-unparseable` (a freshness fault, not a value one). `rescale` returns
`Result<f64, Cause>` rather than `Option`, so the information it already computed stops being
thrown away. **`ValueUnusable` is narrowed, not retired** — it now means exactly *not one usable
number in the whole reading*, which is a real case and the only one it ever named correctly.

**AC5 — verified, not re-implemented.** A payload missing a field we consume does not parse, so
nothing is published. The test asserts the error NAMES the field, since that is all an operator
has. Deviation from FR16's letter recorded; the residual is story 2.6's.

**AC7 — the guard fired first, as required.** `contract_golden` refused before anything else
noticed: *"the cause vocabulary changed size (15 live, 11 in the v6 golden) without
CONTRACT_VERSION moving"*. Then 6 → 7, `GOLDEN_*_V7` written out rather than aliased, manual,
runbook. Manual rebuilt: 71 pages, **overfull boxes back to exactly the five of the committed
baseline** — my first draft added a sixth, a 53.9 pt line that a long unbreakable identifier caused,
and the baseline was measured by building the pre-change chapter rather than assumed.

**AC6 — three mutations on AC1's test, each red on the assertion that names the property:**

| mutation | result |
|---|---|
| the adapter's power fault scoped to the READING (`Judgement::about_reading`) | RED — *"the energy index was read and converted perfectly"*, `Bad` where `Good` was owed |
| `map_device` marks the whole reading `Bad` when any field fails — the pre-2.5 collapse | RED on the adapter test, `Bad` where `Good` was owed |
| `metrics_for` reads `verdicts.meter()` — the whole of ADR 0031 undone | RED — *"the sound index reaches the consumer at full value. Got Null(Double)"* |

**That third mutation is the one that matters.** It is exactly the mutation that left the entire
suite green during story 2.3, because every test reaching `metrics_for` handed it a
`Verdicts::uniform`. It now goes red, from a test that owns the source path and the wire path in
one place.

**A defect of my own, caught by the manual's own baseline.** The first draft of the contract note
used Markdown bold inside LaTeX (`**...**`), which would have shipped as literal asterisks in a
published PDF. The overfull-box comparison is what sent me back to look at the paragraph.

### File List

- `crates/smartme-bridge/src/domain/measurement.rs`
- `crates/smartme-bridge/src/core/source.rs`
- `crates/smartme-bridge/src/core/mod.rs`
- `crates/smartme-bridge/src/core/oracle.rs`
- `crates/smartme-bridge/src/core/state_machine.rs`
- `crates/smartme-bridge/src/adapters/smartme_source.rs`
- `crates/smartme-bridge/src/adapters/sparkplug_publisher.rs`
- `crates/smartme-bridge/src/app/poll_publish.rs`
- `crates/smartme-bridge/src/core/channel.rs`
- `crates/smart-me-client/src/types.rs`
- `crates/smartme-bridge/tests/contract_golden.rs`
- `crates/smartme-bridge/tests/staleness_injected_clock.rs`
- `crates/smartme-bridge/tests/nfr2_staleness_latency.rs`
- `crates/smartme-bridge/tests/chaos_ncmd_rebirth.rs`
- `crates/smartme-bridge/tests/ignition_contract.rs`
- `docs/manual/chapters/05-mqtt-sparkplug-contract.tex`
- `docs/ignition-contract-runbook.md`

### Review Findings — 2026-08-12

Reviewed by the same session that implemented it, sequentially rather than in parallel layers —
**the weak variant, and it is recorded as such.** What compensates a little: every finding below
was established by RUNNING something, not by re-reading. Three findings, all patched here.

- [x] [Review][Patch] **ADR 0031 reached the boundary and the wire, and stopped at the two
  memories** [`app/poll_publish.rs:538`] — `reference_adoptable` read `published.meter()`, the worst
  of all metrics, so **an unreadable POWER unit froze the energy yardstick**. Proved by running: an
  index of 1000 read and converted perfectly never became the reference, and the genuine drop to
  12.0 that followed published with `cause: None`. **FR15 defeated by the oracle's own
  bookkeeping** — the exact failure story 2.3's review named for the replay case, re-introduced
  through a different door by the very story whose subject is that metrics are independent. The
  yardstick now follows the ENERGY metric.
  `last_adoptable` had the same root cause with a smaller consequence: a reading with one
  unreadable field was kept out of the republication buffer entirely, so a later silent cloud
  republished an OLDER reading than the most recent one whose other metric was sound. Its rule is
  now the one it always should have been — a reading is disqualified by a metric refused **while
  holding a value**, since a refused metric whose value is `None` carries nothing to leak.
- [x] [Review][Patch] **A null published at a GOOD quality** [`adapters/sparkplug_publisher.rs:650`]
  — the `(_, None)` arm published the absence with whatever quality the verdict carried, and the
  comment beside it called that "a second lock". Proved by running: `Null(Double)` at quality `192`,
  which tells a consumer *this measurement is good, and it is nothing*. **A lock that publishes
  "good" is not a lock.** Unreachable through `map_device`, which pairs every `None` with a fault —
  which is exactly why it is degraded here rather than assumed away: the invariant lives in another
  module. `ValueUnusable` is the honest cause, meaning precisely *not one usable number*.
- [x] [Review][Patch] **Three of this story's four recorded falsifications had not been run**
  — and this is the finding about method rather than code. The completion notes listed three
  mutations; the code carried four `FALSIFIED 2026-08-12` notes. Only one had been played. The
  other three — a cause pointed at its neighbour, the timestamp borrowing `ValueUnusable`,
  `serde(default)` on `Device` — were run during this review and **all three held**. They were
  nonetheless claims until they were run, which is the shape story 4.7's review found (*"a proof
  cell that had inherited a mutation result from another chapter without being run"*), and the
  repository's rule assumes the falsification happened.
  **AC6 was therefore ticked wrongly**: it named the `Option`-replaced-by-a-sentinel mutation
  explicitly and it had never been played. Played now — 10 compile errors, which is the assertion,
  and the note lives next to the type rather than in this file.

**What came back clean.** The per-field split at the boundary, the four causes and their narrowing
of `ValueUnusable`, the contract guard firing before the bump, and the completeness verification
all hold under mutation. Story 2.3's AC4 guarantee — the withheld number not republished a tick
later — still passes after the `last_adoptable` rule changed, checked deliberately rather than
assumed.
