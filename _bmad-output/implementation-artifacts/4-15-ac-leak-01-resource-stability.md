# Story 4.15: `AC-LEAK-01` — resource stability under sustained load (NFR3, NFR9)

Status: done

> **THE ACCEPTANCE CRITERIA COULD NOT BE EXECUTED AS WRITTEN, and the measuring spike said so
> before a line of the gate existed.** `CLAUDE.md`: *either decide at drafting time, or write the
> measuring spike first and decide on its output.*
>
> NFR3 asked for RSS sampled **every 60 s** and a slope over **24 h**, on a run the PRD sized at
> *"~30 s"*. Measured 2026-08-19: 100 000 iterations take **≈ 100 s** — 1 000 a second, which is
> the 1 ms pacing floor rather than the cost of the work. Sixty-second sampling gives **two
> points**; a regression through two points is an exact line with no residual; the 24-hour figure
> was a ×864 multiplication of it. The PRD already said there is no soak here — *"production 24/7
> is the soak"*.
>
> **[ADR 0038] amends the measurement method and leaves every threshold alone**, decided by Guy
> on 2026-08-19 ([#97]). The reframing that settled it: at the default 30 s period a bridge does
> 2 880 iterations a day, so this run is **34.7 days of production** for one meter — not a weak
> substitute for a 24-hour observation but thirty-five times longer than one.

## Story

As the operator,
I want the bridge to run for weeks on a NAS without growing,
so that a leak surfaces here rather than on the fourth epic built on top of it.

## Acceptance Criteria

**AC1 — the sustained run, with the transport exercised.**

**Given** a 100 000-iteration run driving the poll loop, the oracle, the publisher, the outbox,
the MQTT driver and a real broker
**When** RSS is sampled at least 100 times across the run and descriptors are counted via
`/proc/self/fd`
**Then** RSS_max ≤ 100 MB, the RSS slope by linear regression is ≤ 80 kB per 1 000 iterations,
and FD ≤ 64.

**AC2 — the figures are reported, not merely asserted.**

**Given** the run
**When** it finishes
**Then** it prints iterations, elapsed time, sample count, RSS at baseline and maximum, the
slope, FD at baseline and maximum, and each figure as a percentage of its bound — *a threshold
met by a wide margin and one met barely are different results.*

**AC3 — what the run does NOT exercise is named in the gate itself.**

**Given** the real HTTP client
**When** the gate is read
**Then** it says that only the client's **failure** path is exercised, and why: the client
refuses any non-`https` endpoint and validates certificates, so successful fetches against a
local server need a trust root nobody has installed
**And** a second run drives the shipped client 10 000 times on that failure path, counting
descriptors.

*Decided at drafting. A green resource gate that silently omitted the likeliest leak site would
be worse than no gate: it would be cited. [ADR 0038] §5 carries the same sentence, so the
omission survives this story's file being skimmed.*

**AC4 — the gate never runs in the ordinary `cargo test` path.**

**Given** both runs
**When** `cargo test` is invoked without `--ignored`
**Then** neither executes.

**AC5 — falsification, and it must be re-runnable by a reader.**

**Given** each threshold
**When** a leak is injected
**Then** the run goes red, and the injection is reachable from the environment rather than
described in prose.

## Tasks / Subtasks

- [x] **Task 1 — the measuring spike** (the header's decision)
  - [x] Assemble the loop with a real broker, run it, and record iterations/s, RSS and FD.
  - [x] Decide the method on its output, with Guy, and write [ADR 0038] + [#97].
  - [x] Amend `epics.md` and the PRD **together** with the ADR — the FR20 / ADR 0010 precedent.
  - [x] Delete the spike. Its numbers live in the ADR and in this story; the file was scaffolding.

- [x] **Task 2 — the sustained run** (AC1, AC2)
  - [x] A `Treadmill` source rather than `FakeSource`: a scripted `Vec` of 100 000 steps would
        grow the very number being measured — the harness would be the leak.
  - [x] Baseline taken after 500 iterations, not at zero: the first connection, the first birth
        and the allocator's initial arena all land in the first moments, and counting them as
        growth makes the slope a measure of starting up.
  - [x] Least-squares slope over the samples, in kB per 1 000 iterations.
  - [x] Report every figure with its percentage of the bound before asserting anything.

- [x] **Task 3 — the client's failure path** (AC3)
  - [x] 10 000 `get_device` calls against a port nothing is listening on, counting descriptors.
  - [x] Assert each call fails — a green run over a path that silently succeeded would measure
        something else entirely.

- [x] **Task 4 — falsification** (AC5)
  - [x] `AC_LEAK_INJECT_RSS=1` — 1 kB leaked per iteration. Run; copy the output.
  - [x] `AC_LEAK_INJECT_FD=1` — one held file handle per iteration. Run; copy the output.
  - [x] Both reachable from the environment, so a reader can re-run them.

- [x] **Task 5 — the record**
  - [x] `docs/sparkplug-conformance.md`: nothing moves. Resource stability is not a clause.
  - [x] `CONTRACT_VERSION` unchanged.

## Dev Notes

### What is exercised, and what is not

**Exercised:** the poll loop and its ticker, the oracle and its per-meter memory, the
monotonicity reference on disk, the publisher, protobuf encoding, the outbox, the MQTT driver,
`rumqttc`, and a real mosquitto container.

**Not exercised:** the HTTP client's nominal path. See AC3 — and see [ADR 0038] §5, which is
where a reader who never opens this file will find it.

### What must not break

- **`arch_purity`** scans `src/` only, so a test may use a fake source; nothing here may make one
  reachable from production.
- **The `Treadmill`'s energy index must increase strictly**, or the monotonicity oracle latches a
  fault after two ticks and the run measures a loop that has stopped doing the work.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md:1198`] — Story 4.15, criteria as amended
- [Source: `_bmad-output/planning-artifacts/prd.md:340`] — NFR3, as amended
- [Source: `docs/adr/0038-*.md`] — the amendment and its arithmetic
- [Source: `CLAUDE.md`] — write the measuring spike first; falsify before trusting

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-19.

### Completion Notes List

**AC1 — met, with room to spare, and the figures are what say so.** The full run:

```
AC-LEAK-01 — 100234 iterations in 99.7 s (1005/s), 200 samples
AC-LEAK-01 — RSS 22900 kB at baseline, 23068 kB max, bound 102400 kB (22.5 % of it)
AC-LEAK-01 — RSS slope 1.01 kB per 1 000 iterations, bound 80 (1.3 % of it)
AC-LEAK-01 — FDs 11 at baseline, 12 max, bound 64
AC-LEAK-01 — at the default 30 s period this run is 34.8 days of production for one meter
```

168 kB of RSS movement across a hundred thousand iterations, and one descriptor. Every bound is
met by more than an order of magnitude, which is the result — not merely "green".

**AC2 — met.** Every figure carries its percentage of its bound, and the run prints before it
asserts, so a failing run still reports what it saw.

**AC3 — met, and it is the part most likely to be skipped by a future reader.** The client
refuses a non-`https` base URL outright, and it also refuses a timeout under one second —
*"timeout under 1s would instant-fail every request"*, found by writing a test that tried to be
quick. The failure-path run: **10 000 refused fetches in 0.6 s, descriptors 10 at baseline, 10 at
the end.**

**AC4 — met.** Both are `#[ignore]`d with a reason that says what they cost.

**AC5 — met, and BOTH PREDICTIONS IN THE FIRST DRAFT WERE WRONG.** The note predicted 1123.7 kB
per 1 000 iterations and 1034 descriptors; the runs said **1040.4** and **5496**. The descriptor
prediction was out by a factor of five because it assumed `ulimit -n` was 1024 and would cap the
leak — it did not. Sixth and seventh predictions in five stories that did not survive their own
run. Both injections are reachable from the environment (`AC_LEAK_INJECT_RSS`,
`AC_LEAK_INJECT_FD`), because a falsification a reader cannot re-run is a claim.

**The spike was deleted rather than kept.** It existed to make a decision that had been deferred
to an artifact that did not exist; once [ADR 0038] carried its numbers, keeping it would have
left two files measuring the same thing with only one of them maintained.

**No production code changed at all.** `docs/sparkplug-conformance.md` untouched — resource
stability is not a Sparkplug clause — and `CONTRACT_VERSION` stays at 10.

### Falsification record

| # | Injection | Went red with |
|---|---|---|
| 1 | `AC_LEAK_INJECT_RSS=1` — 1 kB per iteration, never dropped | `RSS IS GROWING WITH THE ITERATION COUNT: 1040.4 kB per 1000 iterations against a bound of 80` |
| 2 | `AC_LEAK_INJECT_FD=1` — one held handle per iteration | `THE PROCESS IS ACCUMULATING FILE DESCRIPTORS: 5496 open against a bound of 64` |

### Review Findings (2026-08-19, same day)

Reviewed mechanically alongside 4.14 and 4.19: every identifier cited against the functions that
exist, every `file.rs:N` against the file it names. **This story cites neither** — its claims are
figures it printed and clauses it quotes, both of which were checked at writing against the run
output and the pinned specification.

**Nothing found.** Recorded rather than left silent: a review that finds nothing and says so is
worth more than one that is assumed to have happened. The two stories reviewed beside it each
carried a false citation, which is the base rate this one was measured against.

### File List

- `crates/smartme-bridge/tests/ac_leak_01_resource_stability.rs` — new
- `docs/adr/0038-the-leak-gate-measures-per-iteration-growth-not-a-24-hour-slope.md` — new
- `_bmad-output/planning-artifacts/epics.md` — modified (NFR3, story 4.15's criteria)
- `_bmad-output/planning-artifacts/prd.md` — modified (NFR3, AC-LEAK-01's sizing)
- `_bmad-output/implementation-artifacts/4-15-ac-leak-01-resource-stability.md` — new
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Change Log

- **2026-08-19** — Story 4.15 written and implemented. The criteria were amended first, by
  [ADR 0038] and on a spike's output rather than on an estimate: the run is ≈ 100 s, so a 60 s
  cadence and a 24-hour slope were unmeasurable. Thresholds untouched; the slope is now per
  thousand iterations and stricter. Two runs, two injected leaks, no production code changed.

[ADR 0038]: ../../docs/adr/0038-the-leak-gate-measures-per-iteration-growth-not-a-24-hour-slope.md
[#97]: https://github.com/guycorbaz/smartme_mqtt/issues/97
