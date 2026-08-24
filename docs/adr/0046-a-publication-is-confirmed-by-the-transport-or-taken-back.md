# ADR 0046 — A publication is confirmed by the transport, or taken back

- **Status:** accepted
- **Date:** 2026-08-24
- **Decides:** when the publisher's state may move, and what happens to a sequence number the transport refused.
- **Issue:** [#92](https://github.com/guycorbaz/smartme_mqtt/issues/92)
- **Builds on:** [ADR 0029](0029-the-declared-serial-is-checked-against-the-one-smart-me-reports.md), whose enforcement mechanism this reuses deliberately.

## Context

`SparkplugPublisher::publish` takes the next `seq`, encodes the DDATA, hands it to the sink and
writes the reading into `declared` — all before anything has touched a socket. The transport then
drains the sink and may refuse. Two things follow, and nothing repaired either:

**The sequence has a hole.** A `seq` jump is not "one message missing" to a Sparkplug host. The
specification makes it a *lost-message condition*: the host issues a Rebirth Request or marks the
node stale. So one counted drop escalates into a session-level event that `dropped_readings` does
not mention — and it is the arm that fires *most* during an outage, because that is exactly when
the outbound queue is full.

**And the refused reading is still what a rebirth re-declares as last published.** `/healthz` says
the reading was lost while the publisher's own memory says it was delivered. Both halves predate
story 4.11; what 4.11 changed is that the disagreement became *visible*.

**The ordering is the defect.** Not the counting, not the queue: state that describes what the host
has received is written before the host can possibly have received it.

## Decision

**Publishing is split in two, and the type makes the second half unforgettable.**

`publish` prepares — it takes the `seq`, encodes, and queues — and it no longer touches `declared`.
It returns a `#[must_use]` `Pending` that only `confirmed()` or `refused()` consume. `confirmed`
records the reading as the last published; `refused` gives the sequence number back and leaves
`declared` as it was.

**The enforcement is copied from ADR 0029 on purpose.** There, `fetch_once` cannot return a
`Reading` — only an `UnverifiedReading` that `verify` opens — because `fetch` has two success paths
and a check on one of them would have held until a token first expired and then silently stopped
holding. The same shape of risk is here: a caller that forgets to confirm produces exactly the
defect this ADR repairs, silently, on whichever branch was forgotten.

**And the first version of this section claimed a guarantee that does not exist.** It said
`#[must_use]` plus the gate's `clippy -D warnings` would catch an omission. That was written from
memory of what `must_use` does, and the measurement says otherwise: the omission was written out —
`deliver` reading `outcome()` and never answering — and `clippy -D warnings` reported **nothing**.
The value is bound and read, so it counts as used; `must_use` only ever spoke about a value nobody
touched. *This is the same failure `CLAUDE.md` records against ADR 0010: a decision resting on a
premise nobody measured.*

So the guarantee is made real. `Pending` implements `Drop`, and dropping one that was never
answered trips a `debug_assert` — which fires in every test and every gate run, and compiles out of
the released image, because a bridge must not gain a new way to panic in production and the
omission would have been caught a hundred times before it could ship. `#[must_use]` is kept for the
one case it does cover: a call whose result is discarded outright.

### Giving a sequence number back is the dangerous half, and it is bounded by its call site

`SeqCounter` gains `give_back`, which is the sharpest operation in `sparkplug-b`: **replaying a
number that did reach the wire is worse than the hole it repairs** — a host would see a duplicate
where it can only conclude corruption.

It is legitimate here for one reason, and only where that reason holds: **when a single message is
in flight and the transport refused it, the number never reached the wire, so there is no hole to
leave and nothing to replay.** The continuity a consumer sees is the truth of what was sent.

That condition holds in `deliver`, which queues one DDATA and drains it immediately. **It does not
hold in `announce`**, where a BIRTH sequence is queued and partly refused: some messages went out,
so the counter has advanced for reasons the refusal does not undo. `announce` therefore does not
call it, and [#88]'s withdrawal remains its answer.

### What is NOT decided here

The third half of [#92] — a republication counted as a fresh loss, so a single stale value is
counted N times under a simultaneous source and broker failure — changes what `dropped_readings`
*counts* rather than when state moves. It touches the surface [#90] has just delivered and is left
to its own change.

## Consequences

**A refused reading now leaves no trace at all**, which is the correct account: the host was not
told, so the bridge does not remember telling it. `dropped_readings` and the rebirth path agree
where they used to contradict each other.

**A consumer sees an unbroken sequence across a queue-full outage**, and stops issuing Rebirth
Requests for messages that were never sent. This is a behaviour change on the wire in the sense
that a hole disappears — no message changes, and no host that was working stops working.

**`sparkplug-b` gains a public operation whose misuse is worse than the defect.** Its documentation
says so in those words, and names the one condition that makes it sound. This is the cost of the
decision, accepted knowingly.

**Every caller of `publish` must now confirm or refuse**, and that is a compile error rather than a
review item.

## Falsification

Recorded with the tests, all run 2026-08-24:

- **`refused` without `give_back_seq`** — the state before this ADR: RED, the message after a
  refusal carries `3` where `2` is owed.
- **`refused` behaving as `confirmed`**: RED — and on the sequence assertion, which comes first,
  never reaching the memory assertion it was written for. Caught, for a reason other than the
  intended one, and the test says so rather than claiming the intended one.
- **`confirmed` recording nothing**: RED on the memory assertion, `None` where `0.019` is owed.
  The only mutation that reaches that half, which is what gives the control a subject.
- **`give_back` as a no-op, and as `saturating_sub`**: both RED in `sparkplug-b` — the second is the
  plausible one-character difference, and it replays `0`, the value a BIRTH claims.
- **The omission itself** — `deliver` preparing a publication and never answering: `clippy -D
  warnings` says nothing, `debug_assert` panics with the message above. That measurement is why
  this ADR's decision section was rewritten rather than kept.
