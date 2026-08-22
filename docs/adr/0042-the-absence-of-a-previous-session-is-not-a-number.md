# ADR 0042 — The absence of a previous session is not a number

- **Status:** accepted
- **Date:** 2026-08-22
- **Decides:** how a node that has never connected numbers its first session, and it does NOT move `CONTRACT_VERSION`.
- **Issue:** [#100](https://github.com/guycorbaz/smartme_mqtt/issues/100)

## Context

`tck-id-topics-nbirth-bdseq-increment` (`Sparkplug_4_Topics.adoc`) states **two** obligations in
one sentence:

> *The bdSeq number MUST **start at zero** and increment by one on every new MQTT CONNECT packet.*

Story 4.10 delivered the increment, proven by `chaos_bd_seq_advances_on_every_connect`. **The
start was not satisfied**, and story 4.19 found it by reading chapter 4's own wording instead of
copying its chapter-6 twin's verdict — chapter 6 gives the increment no id of its own and says
nothing about a zero start, so a matrix row built from chapter 6 alone is silent on this half.

The cause was a sentinel that stood for two different things. `BdSeq::before_first()` returned
`0`, and `NodeSession::start(previous)` advanced past whatever it was given, so a bridge with an
empty state directory published **`bdSeq = 1`** in its first BIRTH and its first will. **Measured,
not deduced**: story 4.13's falsification printed *"born 1, reborn 1"* — the first session number
ever observed on this wire, by an independent subscriber, is 1.

The same sentinel was also the answer for an **unreadable** state file, which is a different
situation entirely: there, the node HAS connected and we cannot tell under which number.

## Decision

**The absence of a previous session is spelled as an absence, at the one place that can know it.**

- `NodeSession::start` takes `Option<BdSeq>`. `None` — this node has never connected — produces
  `bdSeq = 0`. `Some(previous)` advances, exactly as before.
- `BdSeq::before_first()` is **removed**. It existed only to let a caller spell "nothing" as a
  number, which is the defect.
- `load_bd_seq` returns `Option<BdSeq>` and **distinguishes the two failures it used to
  conflate**: `NotFound` answers `None`; any other error answers `Some(BdSeq::new(0))` and warns,
  which continues from 0 and opens session 1 — **exactly today's behaviour, deliberately
  unchanged**.

### What was rejected, and why

**Changing the persisted file's meaning from "the number used" to "the next number to use".** It
makes the missing case fall out arithmetically, and it is wrong here: an existing state file
would be read under the new meaning and replay a number a consumer has seen. It also drags in a
schema migration (ADR 0040's machinery) for what is a two-state distinction.

**Refusing to start on an unreadable file.** It is the safer answer and this ADR does not take
it. Continuing from 0 replays numbers a long-lived consumer may already have seen — the code has
said so since it was written — but that is a configuration-validation decision, it is not what
[#100] is about, and taking it here would hide a behaviour change inside a conformance repair.

## `CONTRACT_VERSION` does NOT move, and this is the reasoning

The constant's own doc states what it protects: *"two runs sharing a version number attest to the
same tag set"*. Story 5.2 is the precedent — the bridge began emitting DDEATH, a message type no
consumer had seen from this node, and the number did **not** move, because the tag set was
untouched.

Here, less moves than that. No metric name, unit, datatype or quality code changes; the `bdSeq`
metric is present in NBIRTH and NDEATH exactly as before. What changes is **the value one metric
carries in the first session of a node that has never connected** — and [#100] itself records
what that costs a consumer: *"A consumer pairs a DEATH to a BIRTH by matching `bdSeq` values, and
that works from any starting number."*

`contract_golden.rs` agrees by construction: it pins names, datatypes and the cause vocabulary,
and none of them moved.

## Consequences

- **The conformance matrix's chapter-4 row moves from `deviation` to `conformant`.**
- **The public API of `sparkplug-b` breaks** — `start` takes an `Option`, `before_first` is gone.
  Free today: the crate is not published, and on 2026-08-22 Guy decided it will not be.
- **An existing deployment sees nothing.** Its state file is present and readable, so it takes the
  `Some` path it always took. Only a fresh install is affected, which is the only case the clause
  is about.
- **Two guards, for the two halves**, and they are separate on purpose: a publisher built with no
  previous session must birth under 0 (`a_bridge_that_has_never_connected_births_under_bd_seq_zero`,
  asserting on the number the *wire* carries), and a missing file must answer `None` rather than a
  number (`bd_seq_survives_a_round_trip_and_a_missing_file`). Neither catches the other's mutation,
  which is why both exist.

## Falsification

| # | Mutation — the ordinary shape of the fault | Went red with |
|---|---|---|
| 1 | `None => BdSeq::new(0).next_session()` — the sentinel restored inside `start`, which is how anyone would re-introduce it | `prop_bdseq_is_continuous_across_sessions`: *"a first session starts at zero"*, **and** the README doctest, which asserts the same thing in the eight lines a consumer reads first |
| 2 | the `NotFound` arm deleted, so absent and corrupt answer alike — the exact code that stood here until this ADR | `bd_seq_survives_a_round_trip_and_a_missing_file`: *"a missing file means this node has never connected — not a number"*, `left: Some(BdSeq(0))`, `right: None` |
| — | *(recorded)* mutation 2 leaves `a_bridge_that_has_never_connected_births_under_bd_seq_zero` **green**, because that test constructs the publisher with `None` directly and never touches storage. The wiring is what mutation 2 breaks, and only the driver test sees it. |
