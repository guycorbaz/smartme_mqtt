# Story 2.7: A feed that stopped moving stops being called fresh, and a clock that is wrong is not called old

Status: done (2026-08-15) — all ACs met; independently reviewed the same day (three findings
confirmed and triaged — one repaired and falsified, one reworded, one to arbitration [#79] —
and one refuted; see Review Findings)

## Story

As the operator,
I want a source that keeps handing back the same response to be refused rather than believed,
so that a frozen cloud cannot look exactly like a working one, and so that a meter whose clock is wrong is not reported as a meter whose data is stale.

## Why this exists, and two thirds of FR10 may already be met

FR10 asks three things. **Two of them look done, and this story's first job is to verify that
rather than build it again** — the discipline story 2.6 used for NFR1's backoff, which turned out
to exist already for the broker.

| FR10's clause | Where it stands, to be verified rather than assumed |
|---|---|
| *attach the meter's measurement timestamp to each value* | `Measurement::value_date` becomes the Sparkplug metric timestamp (`sparkplug_publisher::millis`) |
| *treat all timestamps as UTC end-to-end* | `UtcMillis` is the only time type in the domain; `parse_value_date` requires a trailing `Z`; `parse_imf_fixdate` requires a literal `GMT`; no `chrono`, no local timezone anywhere |
| *flag abnormal source clock skew* | **This is the gap.** `TimestampsDisagree` fires only on a NEGATIVE age. A meter whose clock runs behind by a constant produces a positive age that grows into `ReadingTooOld` — **a wrong clock reported as old data**, which sends an operator to the wrong place |

**And the parked oracle is here.** `Policy::step`'s own doc has carried this since Epic 1:

> *Known accepted limitation (deferred oracle): a byte-identical replayed response — `http_date`
> frozen WITH `value_date` — keeps a plausible age and stays Fresh; detecting it needs cross-tick
> state (`http_date` monotonicity), an additive Epic 2 oracle.*

**A frozen cloud is indistinguishable from a working one today**, because every freshness guard
compares two timestamps *inside one reading* and a replay keeps both consistent. This is the last
oracle Epic 2 owes, and it is the one the freshness formula cannot reach by construction.

## Acceptance Criteria

**AC1 — A feed that stopped moving is refused, and the refusal is about the FEED rather than the value.**

**Given** two consecutive successful fetches whose `http_date` has not advanced
**When** the second is judged
**Then** the verdict is non-good with a cause naming a stalled feed, applied to the reading as a
whole — both metrics, because a frozen response says nothing about either number in particular
**And** a feed whose `http_date` advances is untouched, whatever the values did
**And** it is falsified by removing the comparison: a test driving two identical responses must go
red naming what was believed.

*Decided at drafting: the reference is `http_date`, not `value_date`.* A meter that genuinely stops
reporting keeps its `value_date` frozen while the cloud's `Date` header advances — that is ordinary
staleness, already handled, and it must not be reported as a replay. Only the CLOUD's own clock
standing still means the response is not being regenerated.

**AC2 — Skew is distinguished from staleness structurally, not by a threshold.**

**Given** readings whose age is large but stable while `value_date` ADVANCES
**When** they are judged
**Then** the cause says the source's clock disagrees with the cloud's, not that the reading is old
**And** readings whose `value_date` does NOT advance keep `ReadingTooOld` — they are old
**And** the discrimination uses no magnitude threshold: what separates the two is whether the
meter is still producing new measurements, which is a fact rather than a number

*Decided at drafting, and it is the reason this criterion is expressible at all.* A skew threshold
would be a number nobody measured, which story 2.2 AC4 refused for the tolerance band and ADR 0033
refused for physical bounds. **The structural question — is the meter still measuring? — needs no
threshold**, and it is the one that tells an operator whether to fix a clock or a meter.

**AC3 — UTC end-to-end is VERIFIED and recorded, not rebuilt.**

**Given** FR10's *"treat all timestamps as UTC end-to-end"*
**When** the path is examined
**Then** a test asserts the two parsers refuse anything that is not explicitly UTC — a `ValueDate`
without its `Z`, an HTTP date without its `GMT` — and the assertion says why refusing is right
**And** the absence of any local-timezone or `chrono` dependency is asserted mechanically rather
than by inspection, in the spirit of `arch_purity`
**And** if a gap is found, it is fixed here; if none is, that is recorded as the finding.

**AC4 — Both new oracles carry cross-tick state, and the third one is the moment to stop threading parameters.**

**Given** `step_once` already carries `last`, `energy_reference` and `rate_limited_until`
**When** a fourth memory is added
**Then** they are bundled into one per-meter memory type rather than a fifth parameter
**And** the refactor changes no verdict: the assertions that exist before it must pass unchanged,
the proof stories 2.1, 2.3 and 2.5 all used.

*Decided at drafting: this is the story that pays that debt.* Three carried values were tolerable;
a fourth makes every call site a place to pass them in the wrong order, and `step_once` already has
six parameters.

**AC5 — [#69] closes here, or the reason it cannot is recorded.**

**Given** a replayed response, which the SOURCE marks `Good` and this story's oracle refuses
**When** the adoption rules run
**Then** neither `last` nor `energy_reference` adopts it, and the OLD guard
(`reading.value.quality != Quality::Bad`) would have adopted it — the two rules disagree
observably for the first time
**And** a test drives that disagreement and [#69] is closed in the same commit, story 2.3's AC3
marked met
**And** if the disagreement turns out not to be observable, that is recorded on [#69] instead of
being asserted.

**AC6 — Falsified before trusted, and RUN before recorded** — the criterion story 2.6 added.

**AC7 — `CONTRACT_VERSION` moves 8 → 9, additive**, with the golden written out, the manual, the
runbook, and the mechanical grep.

**AC8 — No verdict that is correct today changes**, apart from the cases AC1 and AC2 name.

## Tasks / Subtasks

- [x] **Task 1 — Bundle the per-meter memory** (AC4) — 2026-08-13
  - [x] `MeterMemory` carries `last`, `energy_reference`, `rate_limited_until`
  - [x] Every existing assertion passes unchanged: 221 bridge tests, none edited for
        content, 17 call sites rewritten mechanically
- [x] **Task 2 — The stalled-feed oracle** (AC1) — 2026-08-13
  - [x] `http_date` of the previous SUCCESSFUL fetch — not the previous *accepted* one;
        the difference is decided and argued on `MeterMemory::last_http_date`
  - [x] Reading-scoped judgement, composed like the others
- [x] **Task 3 — Skew told apart from staleness** (AC2) — 2026-08-15
  - [x] The structural discrimination; no threshold — `Policy::step_remembering`,
        fed by `MeterMemory::last_value_date`; the cause is `timestamps-disagree`,
        REUSED rather than minted (see the note below), so the contract does not move
- [x] **Task 4 — Verify UTC end-to-end** (AC3) — 2026-08-15: two why-tests at the
      parsers, one mechanical guard across all three crates, one finding recorded
- [x] **Task 5 — Close [#69]** (AC5) — 2026-08-15: the feed gate on both adoption
      rules; the disagreement driven on the wire in
      `a_replayed_response_rewinds_neither_memory`
- [x] **Task 6 — Contract 8 → 9** (AC7) — 2026-08-13: golden, manual, runbook, grep
- [x] **Task 7 — Falsify, running each mutation BEFORE writing its note** (AC6) —
      2026-08-15, nine mutations, tables below; one of them found the test wanting
      and is recorded as such
- [x] **Task 8 — `./scripts/ci-local.sh` full run**, then `gh run list` — 2026-08-15.
      Every step reproduced green EXCEPT the two tests that bind port 8080
      (`from_an_empty_directory_to_publishing…`, `with_no_configuration_the_ui_answers…`):
      the port is held by another project on this machine (the response carries mybibli's
      session cookie — verified, not assumed), the standing impediment of 2026-08-13.
      Image build + smoke tests green; the two port tests are verified on GitHub after
      the push, as on 2026-08-13.

## Dev Notes

### The trap this story is most likely to fall into

**Two oracles in one story make the first failure ambiguous.** Story 2.1 refused to ship a layer
and an oracle together for exactly this reason. They are together here because both need the same
cross-tick state and both are FR10 — but **each must fail on its own assertion**, and a mutation to
one must leave the other's tests green. If that cannot be arranged, they are two stories.

**And the second trap is AC2's.** The temptation is to reach for a magnitude — *"more than N
seconds of stable age is skew"*. That number would be nobody's measurement, and this epic has
refused it twice: story 2.2 AC4 for the tolerance band, ADR 0033 for physical bounds. The
structural question — *is `value_date` advancing?* — is the one that needs no number.

### Where the code lives

- `crates/smartme-bridge/src/core/state_machine.rs:~150` — the parked-oracle note this story
  closes, and `judge_reading`'s guards
- `crates/smartme-bridge/src/app/poll_publish.rs` — `step_once`'s six parameters and the three
  carried memories AC4 bundles
- `crates/smart-me-client/src/types.rs` — `parse_value_date`, which requires the `Z`
- `crates/smart-me-client/src/http_date.rs:16` — `parse_imf_fixdate`, which requires the `GMT`
- `crates/smartme-bridge/src/core/clock.rs` — the only `SystemTime` use, behind the clock seam

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:280`] — FR10
- [Source: `_bmad-output/implementation-artifacts/2-2-energy-counter-monotonicity.md`] — AC4's
  refusal of a number nobody measured, which AC2 here follows
- [Source: `docs/adr/0033-fr14-is-withdrawn-physical-plausibility-is-not-the-bridge-s-to-judge.md`]
  — the same refusal, applied to an entire requirement
- [Source: `_bmad-output/implementation-artifacts/2-6-error-taxonomy-and-bounded-backoff.md`] —
  AC6's ordering rule, and the precedent of verifying a requirement instead of rebuilding it

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

**2026-08-13 — AC4, the refactor that pays for the two oracles.**

`MeterMemory` replaces three threaded parameters. `step_once` goes from six parameters
to four, and the two memories AC1 and AC2 need have somewhere to live that is not a
seventh and eighth.

**The proof is that nothing moved**: all 221 bridge tests pass with no assertion edited
— only the 17 call sites, mechanically. No falsification is recorded here and that is
deliberate rather than an omission: the repository's rule governs *new tests asserting
an invariant*, and this task added none. A refactor's evidence is the unchanged suite.

**One behaviour did change, in the tests only, and it is an improvement.** Several call
sites passed `&mut None` for a memory, so every write to it was discarded and the next
call started fresh. Sharing one `MeterMemory` makes it persist — which is what
production always did. No test depended on the reset; the harness is now the more
faithful of the two.

### File List

- `crates/smartme-bridge/src/app/poll_publish.rs` — `MeterMemory` (AC4, 2026-08-13:
  +`last_http_date`; 2026-08-15: +`last_value_date`), the feed oracle's composition (AC1),
  the memory threading and recording (AC2), the feed gate on both adoption rules (AC5);
  pipeline tests for AC1, AC2 and AC5
- `crates/smartme-bridge/src/core/oracle.rs` — `Cause::FeedNotAdvancing` and
  `feed_is_advancing` (AC1); `TimestampsDisagree`/`ReadingTooOld` docs widened (AC2)
- `crates/smartme-bridge/src/core/state_machine.rs` — `Policy::step_remembering` and the
  over-age discrimination (AC2), with its unit tests; the parked-oracle note replaced (AC1)
- `crates/smart-me-client/src/types.rs` — `a_value_date_that_does_not_declare_utc_is_refused` (AC3)
- `crates/smart-me-client/src/http_date.rs` — `a_date_header_that_is_not_gmt_is_refused` (AC3)
- `crates/smartme-bridge/tests/arch_purity.rs` — `utc_is_the_only_time_domain` (AC3)
- `crates/smartme-bridge/tests/contract_golden.rs`, `docs/manual/chapters/05-mqtt-sparkplug-contract.tex`,
  `docs/ignition-contract-runbook.md` — contract 8 → 9 (AC7, 2026-08-13)
- `_bmad-output/implementation-artifacts/2-3-the-oracle-layer-finished.md` — AC3 marked met
  the day its subject arrived (AC5)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — status trail

**2026-08-13 — AC1, the oracle parked since Epic 1.**

`Cause::FeedNotAdvancing`, `feed_is_advancing` in the pure core, composed in `step_once` as a
reading-scoped judgement beside freshness. `CONTRACT_VERSION` 8 → 9, additive.

**AC1 lives OUTSIDE `Policy::step` and AC2 will live inside it, and that split is deliberate.**
The story warns that two oracles in one story make the first failure ambiguous. Putting them in
different places makes a mutation to one structurally unable to touch the other — verified by
running M2 below, which reddened only its own test and left the monotonicity suite green.

**The memory does NOT follow the adoption rule, and that is the design decision here.** `last` and
`energy_reference` refuse a reading the oracles refused (story 2.3 AC4) because they are yardsticks
for a VALUE. `last_http_date` is a yardstick for the RESPONSE: the question is whether the cloud is
still rebuilding its answer, which has nothing to do with whether we trusted the numbers inside.
Recording it only on adopted readings would make a stale meter look like a frozen cloud one tick
later — the exact confusion AC1 chose `http_date` over `value_date` to avoid.

**`<=` rather than `==`**: a header going backwards is not evidence of a working feed either, and
"the feed is not advancing" is true of both. No second cause for a distinction that changes no
repair.

**The state stays `Fresh` on a replay, which is correct, and the first draft of the test asserted
otherwise.** `State` judges the timestamps INSIDE one reading and a replay's are impeccable. What
an operator reads is the composed verdict, and `FleetState::degraded` filters on
`published.quality()`, so the meter is reported degraded with its cause. The two answers differ on
purpose and only one of them is a surface. What would be a defect is a latch — a frozen feed must
not need a restart to clear — and that is what the test asserts instead.

### The fixtures were modelling the fault they were asserting health against

Three existing tests failed when the oracle landed, and none of them was a defect in the oracle:
every reading fixture pinned `value_date` to `BASE` and `http_date` to `BASE + age`, so a sequence
of ticks handed back a **byte-identical response**. The oracle read that as a frozen cloud, and it
was right. **Our own tests could not tell a working feed from a replay** — which is precisely the
blind spot `Policy::step`'s parked-oracle note described, reappearing one layer up. Fixed with a
`later(reading, ticks)` helper that advances both timestamps while holding the age, and a `tick`
parameter on the NFR2 test's fixture.

### An observed-once flake, recorded rather than dismissed

`a_payload_the_bridge_could_not_read_names_its_field_to_the_operator` (story 2.6's) failed once
during a full workspace run on a loaded machine and **did not reproduce in 17 later runs**. The
mechanism that fits: the fetch's 2 s deadline elapsing before the fake source was polled, making
the tick a `Timeout` that never carried the decode failure. Unconfirmed. The test now asserts
against that line explicitly and says so, so the next occurrence explains itself instead of looking
like the property failing — the repository's rule about a flake that impersonates a real bug.

**2026-08-15 — AC2, the discrimination that needs no threshold.**

`Policy::step_remembering(prev, tick, now, previous_value_date)` — the memory arrives as a
parameter so `Policy` stays a pure function of its inputs, the same reason `now` does. The
three-argument `step` remains and delegates with `None`, so every assertion kept verbatim since
story 2.1 still calls exactly what it always called, and the no-memory path IS the pre-2.7
behaviour — which is also what the first tick after a restart honestly deserves: on one reading,
nobody can tell a wrong clock from old data.

**The cause is `timestamps-disagree`, reused rather than minted, and that is a decision.** A
negative age is the meter's clock ahead of the cloud's; a large age over an advancing
`value_date` is the same clock behind it. Same disagreement, same repair — the operator goes to a
clock either way — and story 2.6 set the rule: no second cause for a distinction that changes no
repair. What it buys concretely: the cause vocabulary does not change, so `CONTRACT_VERSION`
stays at 9 and the golden agrees mechanically. What it costs: a consumer cannot tell ahead from
behind by the token alone — it can by the sign of `age`, which the wire carries in the two
timestamps themselves.

**The memory records only plausible timestamps.** Story 1.7 pins an unparseable `ValueDate` to
the epoch, and a sentinel entering `last_value_date` would make the next real reading look like
production resuming. The floor guard sits at the recording site in `step_once`, beside the same
rule `last_http_date` already follows (recorded on every successful fetch, adoption rules
deliberately not consulted — both memories are yardsticks for the FEED, not for the values).

**2026-08-15 — AC3, verified rather than rebuilt, and the finding.**

The two parsers already refused everything that does not declare UTC — the grammar tests said
*that*, and AC3's ask was the *why*. Two new tests carry it: `a_value_date_that_does_not_declare_utc_is_refused`
(no marker, explicit offsets, lowercase `z`) and `a_date_header_that_is_not_gmt_is_refused`
(`UTC`, `UT`, `gmt`, a named zone, offsets, no zone). Both say the same why: the freshness
formula subtracts two stamps, and timezone arithmetic on a guess shifts the age by whole hours
against a 90-second allowance.

The mechanical half is `utc_is_the_only_time_domain` in `arch_purity`: no source line in any of
the three crates may name a zoned time capability (`use chrono`, `chrono::`, `OffsetDateTime`,
`FixedOffset`, `with_timezone`, `Local::now`, `localtime` — tokens, not substrings, because
"synchronous" contains "chrono"), and no manifest may declare `chrono` or `time`.

**THE FINDING, recorded as AC3 asks: no gap in the code, one surprise in the lockfile.**
`Cargo.lock` DOES list `chrono` 0.4.45. It arrives through `testcontainers` (a dev-dependency of
the chaos tests) via `serde_with`'s feature union, and `cargo tree -i chrono` prints nothing —
the lockfile is the union of everything that COULD be built, not what is. No crate of ours
requests it, no source names it, and the guard now keeps both true.

**2026-08-15 — AC5, [#69] closes, and the exemption was the door.**

The gate is one line read twice: `feed_refused = feed.quality() != Quality::Good`, required by
both `reference_adoptable` and `last_adoptable`. What it closes is not hypothetical:

- **the yardstick**: a replayed OLDER response carries a lower index, the monotonicity oracle
  duly says `counter-went-backwards` — and the METER-REPLACEMENT EXEMPTION adopted it. The
  reference rewound by a replay, so the next genuine backwards reading was judged against an
  index from the past and passed as `Good`. FR15 defeated by the exemption that exists to serve
  it, one oracle later.
- **the buffer**: a replayed EQUAL response is refused only by the feed (`Stale`, not `Bad`), so
  `last` adopted it — same numbers, minute-old `value_date` — and the next silent cloud
  republished a reading re-dated to a moment the bridge never accepted.

The disagreement [#69] waited for is now on the wire: the fixture is asserted `Quality::Good` —
the SOURCE's opinion — and the composed verdict refuses it. Swapping the gate away (the old
source-quality rule's answer) turns the test red twice, once per memory. Story 2.3 AC3 is
therefore MET, its rule observable at last; recorded there and on the issue.

### Falsification — AC2/AC3/AC5, every mutation RUN before its note (2026-08-15)

| mutation | result |
|---|---|
| AC5: `!feed_refused` dropped from `reference_adoptable` | RED on the tick-3 assertion — *"under the source-quality guard the replayed 850 000 became the reference and this genuine backwards reading passed as Good"*, `left: Good` |
| AC5: `!feed_refused` dropped from `last_adoptable` | **GREEN on the first run, and that is the instructive one.** The original scenario's replay carried a lower index, so monotonicity already refused it `Bad`-with-value and `last` never adopted it — the gate was unwitnessed. The test gained an EQUAL-index replay (refused by the feed alone) and a `value_date` assertion on the republish; re-run: RED — *"republishing it re-dates the reading to a moment the bridge never accepted"*, `left: UtcMillis(…640000)`. AC6's ordering rule caught a test that would have recorded a falsification it never ran |
| AC2: discrimination neutered (`meter_still_measuring = false`) | RED on BOTH layers — the unit test and the pipeline test — while every AC1 feed test stayed green, which is the mutation-independence the story demanded of its two oracles |
| AC2: `>` loosened to `>=` | RED on `a_meter_that_stopped_measuring_keeps_reading_too_old` and the pipeline's stopped-meter half — a frozen `value_date` counted as production |
| AC2: memory unthreaded in `step_once` (`None` passed) | RED on the PIPELINE test only, every unit test green — the layer-above test earning its place, against exactly the Epic 2 failure shape |
| AC3: the `Z` made optional (`strip_suffix('Z').unwrap_or(s)`) | RED — *"does not explicitly declare UTC and must be refused"* (and the older grammar test too) |
| AC3: any zone token accepted (`"GMT"` → `_zone`) | RED — *"is not the literal GMT and must be refused"* |
| AC3: `"with_timezone"` planted on a non-comment source line | RED naming the file and the line |
| AC3: `chrono = "…"` planted in a manifest (`[package.metadata]`) | RED naming the manifest — the scan reads the file, not cargo's opinion of it |

## Review Findings (2026-08-15, independent pass, fresh context)

Four candidate findings, each adversarially verified by its own agent; three CONFIRMED, one
REFUTED. Triaged the way this repository triages: repair, reword, arbitrate — nothing dismissed
silently.

1. **CONFIRMED, REPAIRED — a replay with its `Date` header stripped walked through AC5's gate.**
   The feed oracle answers `good()` on a missing header (right for judging: no header, no
   question), so `!feed_refused` was true of a headerless replay and the meter-replacement
   exemption rewound the reference through the door the gate had just closed for headered ones.
   Repaired with two rules, each falsified (mutations run first, outputs in the test doc):
   the exemption now requires the feed to VOUCH — a `Date` seen and not refused — because
   re-baselining FR15's yardstick is the single most trusting act in the loop and deserves
   positive evidence, not absence of objection; and `last` refuses any candidate older than
   what it holds, closing the header-stripped door for the buffer by the buffer's own
   definition. Ordinary adoption still needs only non-refusal: a merely headerless reading
   keeps its republication.
2. **CONFIRMED, ARBITRATION [#79] — the over-age cause flaps with the polling phase** when the
   meter's cadence is slower than the poll period (the realistic regime: ~60 s cadences
   observed, 30 s default poll). A wrong-clock meter alternates `timestamps-disagree` /
   `reading-too-old` as the same measurement is re-served. Three candidate designs, each
   touching the epic's threshold-refusals; sibling of [#75] (the cause depends on the last
   tick), left to the same arbitration rather than half-decided here. The quality half never
   moves — both causes publish `Stale` — so the wire stays fail-safe throughout.
3. **CONFIRMED, REWORDED — the docs claimed a culprit the bridge cannot assert.** A wrong meter
   clock and a cloud ingesting late produce the same signature (`value_date` advancing, age
   large), and the doc comments said "its clock is wrong / the operator goes to a clock". The
   cause's documentation now asserts the DISAGREEMENT and names the two-stop repair path
   (clock, then ingestion latency); the mechanism is untouched — the verifier's own conclusion
   was that the discrimination is right and only the narrative overclaimed.
4. **REFUTED — the same-second `Date` block.** Two fetches inside one second would read equal
   truncated `Date`s and be refused as a replay — but the scenario is not constructible:
   `PERIOD_MIN` is 5 s and refused (not clamped) below, the period field cannot even express
   sub-seconds, the hot-reconfigure first tick is explicitly consumed, and there is no
   retry-within-tick. The residual trigger (a server clock stepping back) is the fault the
   oracle exists to report. Kept here so the next reader does not re-derive it.

### Falsification — AC1, both mutations RUN before this note

| mutation | result |
|---|---|
| the comparison in `feed_is_advancing` neutered (`false &&`) | RED on **both layers** — the unit test and `a_replayed_response_is_refused_on_both_metrics`, which is the pipeline proof |
| the reference switched from `http_date` to `value_date` | RED on `a_silent_meter_behind_a_live_cloud_is_not_called_a_replay` only — *"the FEED advanced; it is the METER that went quiet, and the two send an operator to different places"* — and the monotonicity tests stayed green, which is the independence the story asked for |
