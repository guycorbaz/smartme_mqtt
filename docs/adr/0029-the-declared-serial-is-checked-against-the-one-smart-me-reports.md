# ADR 0029 — The declared serial is checked against the one smart-me reports

- **Status**: accepted
- **Date**: 2026-08-09
- **Deciders**: Guy Corbaz
- **Supersedes**: nothing. **Amends in part**: Epic 2's ownership of the runtime oracles.
- **Issue**: [#61](https://github.com/guycorbaz/smartme_mqtt/issues/61)

## Context

Two identifiers name the same physical meter on one configuration row, and they are used at
opposite ends of the bridge:

- `meters[].serial` is what `supervisor::run_with_control` **births the Sparkplug device
  under** (`node.device_topic(DBirth, meter.serial)`).
- `meters[].device_id` is the smart-me UUID the poll task **actually fetches**.

Every DDATA is then routed by the serial the smart-me **response** carries, never by the
configured one (`SparkplugPublisher::publish`). Until this decision, nothing anywhere compared
the two.

When they disagree, the bridge behaves as follows, and every line of it is worth reading
because the whole point is that none of it looks wrong:

- the fetch **succeeds**, so the oracle judges the reading `Fresh`;
- the heartbeat ticks, so `/healthz` reports `wedged: false` and answers `200`;
- `failed_sources` is empty, so `/` names no meter and says the bridge is publishing what it
  reads;
- the tags appear in the SCADA host's browse tree, because the DBIRTH went out normally;
- and **every reading is discarded** as `DroppedUndeclaredDevice`, behind one `warn` per poll
  period.

A bridge that looks perfect on every surface it offers and puts nothing on the wire. That is
the exact failure this project exists to prevent, reached without a single component
malfunctioning.

**This has already happened here.** `config::check_serial` refuses a serial with a leading
zero for precisely this reason, and its comment states the real rule in as many words:

> KNOW WHAT THIS RULE ACTUALLY IS. The real requirement is *the serial must be the one
> smart-me reports*, which cannot be checked offline. The leading zero is a proxy for it,
> generalised from a single incident.

The sentence stops one step short. The rule cannot be checked **offline** — but the bridge
holds the answer in its hand on every successful fetch, and had never looked at it.

Three things made this worth deciding now rather than with Epic 2:

1. **Story 3.1 multiplied the occasions by four.** One row was one chance to get the pair
   wrong; four rows are four, and one of the deployment's meters is permanently unplugged, so
   an operator has an independent reason to be editing those rows.
2. **ADR 0023 made the web form the only supported way to configure the bridge**, and the
   serial is typed by hand there until discovery (story 3.4) exists.
3. **Guy's gate for the panoramix deployment test runs through that form.** The first
   configuration written on that machine is the one most likely to carry a transposition, and
   the least likely to be noticed, because nobody yet knows what a working bridge looks like
   there.

## Decision

**A reading whose reported serial is not the declared one is refused as
`SourceError::Fatal`, on every fetch, at the source adapter.**

Four sub-decisions, taken here rather than deferred:

### 1. At the source adapter, not at the publisher

The publisher already *notices* — that is what `DroppedUndeclaredDevice` is — but it notices
in the one place that must not decide anything (AR6: the state machine decides truth, the mqtt
task transports it). A drop there is a transport fact reported to a log. The state machine is
what the screen, the health endpoint and the wire all read, so the verdict has to be reached
where a verdict is reached.

### 2. `Fatal`, so it latches

A serial does not come back on its own. `Transient` would poll a misconfigured meter for ever
while publishing `Stale`, which reports a configuration fault as weather. `Fatal` latches
`State::Failed`, which names the meter on `/` and in `failed_sources` and clears only on a
restart.

That latch costs nothing that was not already owed: `reconfigure::classify_meters` classifies a
serial change as `ProcessRestart`, so the repair and the latch ask the operator for the same
action.

### 3. Enforced by a type, and the guarantee is stated exactly

`fetch` has two success paths — the ordinary one and the refresh-and-retry after a 401. A check
written on one of them would hold until a token first expired and then silently stop holding,
which is the shape `node_metrics` was restructured to remove in the publisher.

So `fetch_once` cannot return a `Reading` at all; it returns an `UnverifiedReading`, and
`verify` is the only thing that opens one. Dropping the call **does not compile** — measured,
not assumed.

**The guarantee is that and no more.** The field is private to the module, not to the type, so
`unverified.0` compiles; this was measured too. What is closed is the *forgotten branch*. A
deliberate unwrap remains possible and is one word long in a diff. Recorded rather than
overclaimed, because the previous sentence in the source said the compiler put the check on
both paths, and that was more than the measurement supports.

### 4. The offline proxy stays

`check_serial`'s leading-zero refusal is **not** withdrawn. The two guards refuse at different
moments and that difference is their whole value: the offline one costs no API call and stops
the bridge **before it births anything** into a namespace a SCADA host persists; the online one
catches every other way the pair can disagree, but only once the node and its devices are
already on the wire. Removing the cheap, certain case because a general one now exists would
trade a refusal for a burial.

## Consequences

### What this does not catch, and why FR25 is untouched

The check binds a reading to **the device the configuration declares**. It cannot bind it to
**the meter the operator meant**. A row naming the cellar's serial *and* the cellar's device id
under the label `garage` is self-consistent, passes this check, and files the cellar's
readings under the garage for as long as it runs.

That case is FR25's, and FR25 is the only guard against it. The manual's warning has been
amended to say which half is now machine-checked and which half is still the human's —
weakening that warning into "the bridge checks the mapping" would be the worse outcome of this
change, and is the thing to watch for in the screens Epic 6 still owes.

### The screen and the manual had to move with it

The caveat on `/` asserted a single cause (*"the smart-me cloud refused or could not
answer"*), which would have sent an operator to look at their credentials for a typo in their
own form. It also promised that *"the last known values are still published, marked
not-good"* — which was **false before this ADR and independently of it**: `Failed` publishes
`Quality::Bad`, and `metrics_for` publishes `Null` for `Bad`, so no value goes out at all. Both
were corrected in the same commit, in the page and in chapter 6.

### It anticipates Epic 2

Epic 2 owns four runtime oracles — unit rejection, **serial-identity binding**, physical bounds
and energy-counter monotonicity — and has no story written. This implements the second of them,
for the deployment reason above. The other three stay with Epic 2, and Epic 2 should treat this
as done rather than re-deciding it: a second implementation of the same rule in a different
place is how two answers to one question start disagreeing.

### Cost

One `String` comparison per fetch. No new dependency, no new message on the wire, no change to
the topic grammar or to any metric — `CONTRACT_VERSION` is untouched, and deliberately: a
consumer sees nothing new.

### Amended 2026-08-11 — this is the first instance of the LATCHING half of the oracle rule

Story 2.1 (2026-08-10) built the oracle layer that this decision anticipated, and with it a rule
this ADR could not have stated because the vocabulary did not exist yet: **a verdict either
LATCHES or merely DEGRADES.** A latching cause takes the meter off the wire until the process
restarts; a degrading one marks one reading and lets the next one recover on its own.

**The check decided here is the canonical latching case, and it is worth saying why**, because the
rule is otherwise easy to read as arbitrary. The serial mismatch is a statement about *identity*:
this topic is not the meter you configured. Nothing about the next reading can repair that — the
same wrong meter will answer the same wrong serial thirty seconds later, and a bridge that
recovered by itself would simply resume mislabelling values. So it raises `SourceError::Fatal`,
which `Policy::step` absorbs into `State::Failed`, from which there is no exit but a restart.

Contrast with the degrading half, whose first instance is story 2.2's energy-counter monotonicity:
a counter that went backwards is a statement about one *value*. The reading after it may be
perfectly sound, and latching there would take a healthy meter off the wire for an event that has
already passed.

**Identity latches; value degrades.** The rule now lives in `core/oracle.rs`; this ADR is named
there as its first instance. Recorded here because the review of story 2.1 found the link existed
only in the code — a reader of this ADR still saw an isolated decision, and the repository's rule
is that an architectural position is recorded where decisions are recorded.
