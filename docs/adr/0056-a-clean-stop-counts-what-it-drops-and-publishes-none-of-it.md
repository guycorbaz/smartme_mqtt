# ADR 0056 — A clean stop counts what it drops, and publishes none of it

- **Status:** accepted
- **Date:** 2026-08-30
- **Decides:** what happens to readings still in the inbox when the bridge is asked to stop.
- **Issues:** [#87](https://github.com/guycorbaz/smartme_mqtt/issues/87),
  [#91](https://github.com/guycorbaz/smartme_mqtt/issues/91)
- **Related:** AR13 / SIGTERM-NO-LIE, ADR 0011 (the two deaths), ADR 0027 §2 (the status code stays
  200), story 4.11.

## Context

`DropReason` asserted its six variants were exhaustive. They were not.

When `SessionEnd::Shutdown` fired, `mqtt_driver::run` returned without draining `inbox`. The 64-slot
channel was dropped with its receiver; the poll tasks were aborted by the supervisor and never
observed `Closed`. **So no reason fired, no line was logged, and `dropped_readings` reported zero**
for up to sixty-four judged readings. A clean stop — a container restart, the ordinary gesture of
this deployment — swallowed them in silence.

[#87] filed it rather than fixing it in story 4.11, and said why: *"counting them without publishing
is cheap and honest and is probably the answer, but it is a decision rather than an oversight."*

## Decision — they are counted, under their own reason, and not published

`count_undrained` drains the inbox on the way out and counts each reading as
**`undrained-at-shutdown`**, a seventh `DropReason` whose culprit is `Bridge`.

**Draining to PUBLISH would be the wrong repair, and it is why this waited for a decision.**
Publishing after the shutdown decision is precisely what the death sequence exists to bound: AR13
requires the bridge to stop saying things once it has announced it is going. **Counting is not
publishing.** These readings are lost either way; what changes is that the bridge names them instead
of losing them quietly.

**The signature is the guarantee, not a comment.** `count_undrained` is handed no sink and no
client, so publishing is unrepresentable inside it — the same enforcement `SparkplugPublisher::publish`
gets from being handed no clock. An edit that wanted to publish would have to add a parameter, which
is a change a reviewer sees.

It is a function rather than four lines in `run`'s tail for the reason `count_loss` is one: inside
that tail no test can reach it, and a drain that works perfectly with a caller that forgets to count
is the defect being repaired with every other test green.

## Decision — every drop row carries when it last moved, per reason

[#91] is the same surface's other half. The counts are cumulative for the process lifetime and the
status code deliberately stays 200, so **a bridge that had published nothing for six hours and one
that lost three readings at start-up rendered the same body**. An operator reading `/healthz` once
could not tell them apart; only a scraper diffing successive polls could.

`last_at_ms` joins each row. It is the same argument that produced `degraded_meters` after [#62],
where a meter froze for ten hours and every surface said the fleet was healthy: the difference
between a surface that **names** a fault and one that merely **contains** it.

**Per reason, not per meter**, and that is the whole of the decision. A meter can be losing readings
now for one reason while another reason's count is a week-old scar; one instant per meter reports the
scar as fresh — the confusion this exists to remove, reintroduced one level up, and the cheaper
implementation anyone reaches for first. The guard is falsified against exactly that.

**The instant is passed in, never read here.** `arch_purity` confines raw time sources to their own
modules, and a counter that reads a clock is a counter no test can pin.

## What this does NOT close

**[#85] stays open, and this repair narrows where its measurement must be taken.** `try_publish`
answers `Ok` on entering `rumqttc`'s request channel, not on leaving the socket, so a reading counted
as handed over can still be discarded when the `EventLoop` is dropped.

`chaos_broker_recovery` was the natural place to look and **it does not exercise that window**: its
readings are sent once `stop_with_timeout` has returned, so the socket is already dead and every run
measured `("garage", "before-birth", 3)` — counted, not silently lost. The window needs a connection
that is **alive but dying**, which is a different experiment from a broker that is already down.

## Consequences

- **The seventh path is named on the wire's own surface**, so a container restart that loses a minute
  of the fleet says so instead of reporting zero.
- **`DropReason::ALL` widens to seven**, and the counter arrays widen with it — the array is the
  single source of both count and index, so this could not be added without them moving.
- **`/healthz` gains a field.** Additive: a reader that does not know it is unaffected.
- **The manual carries both**, beside the counts themselves rather than only here.
