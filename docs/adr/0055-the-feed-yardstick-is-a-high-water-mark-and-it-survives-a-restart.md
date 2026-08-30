# ADR 0055 — The feed yardstick is a high-water mark, and it survives a restart

- **Status:** accepted
- **Date:** 2026-08-30
- **Decides:** what `MeterMemory::last_http_date` remembers, and how a reference file written before
  today is loaded.
- **Issue:** [#80](https://github.com/guycorbaz/smartme_mqtt/issues/80)
- **Related:** ADR 0040 (the first schema migration — the posture this one declines), story 2.7,
  FR15.

## Context

`feed_is_advancing` is the only judgement in this bridge whose input is a relation between two
responses rather than a fact about one. It exists because a replayed response is internally
consistent: its age looks plausible, and every other check passes it.

**The oracle was always right.** It refuses `current <= before` — frozen and backwards alike, and
its doc says why. What was wrong was the yardstick it was handed, in two ways [#80] named together
and asked to be repaired together.

## Decision 1 — the mark only ever moves forward

Every `Date` was written straight into the memory, **including one that went backwards**. So a
response with an older header dragged the mark back, and the next replay — still behind the newest
answer genuinely seen — then compared favourably and read as *advancing*. **The memory was rewindable
by the very thing it exists to catch.**

It is now the maximum: *the newest answer we have ever seen*, not *the last one we happened to
receive*.

**The cost is stated rather than discovered.** A cloud whose clock is corrected backwards reads as a
stalled feed until it catches up. That is honest-pessimistic — by this yardstick the feed is not
advancing, and the operator is told exactly that — and it is the safe direction for a bridge whose
motto is that it does not lie. The alternative recovers a tick sooner and can be walked backwards by
a replay.

**The non-adoption rule is untouched.** A header is still recorded when the *reading* was refused,
because this yardstick is about the response and not about the value; refusing to record a header
because the numbers were stale would make a stale meter look like a frozen cloud. Story 2.7's
reasoning stands; only the direction of travel is constrained.

## Decision 2 — the mark is persisted, beside the energy reference

Before this, `last_http_date` was in memory only. So on the first tick after **every** restart the
oracle compared against `None`, which it answers `Good`, and the meter-replacement exemption's
voucher degenerated to *"the response carries a Date header"*. A replayed older answer served exactly
on that tick could be vouched for and **rewind the persisted energy reference** — after which the
next genuine reset composes `Good`. One tick per restart, and it is the FR15 defeat.

It rides in the file the energy reference already uses. **The file's name does not change**, and that
is a compatibility constraint rather than a description: `energy-reference-<id>.toml` exists on a
deployment reachable only over a file share, and renaming it would silently abandon every meter's
reference. The struct is renamed `PersistedMemory` so that at least the code stops saying the file
holds one thing.

**What it costs.** The file is written when either memory moves. For a consuming meter the energy
index rises on nearly every tick, so this already wrote once per meter per period and the mark rides
along for free. What is new is a meter whose index is *not* moving — a quiet night — where the header
still advances: one write per period where there were none. Bought deliberately, because a quiet
meter is exactly where the stalled-feed oracle earns its keep.

**One case is left honest rather than closed:** a meter that has never had an energy reference has no
file to carry the mark, so its own first tick after a restart still vouches on *"carries a Date"*.

## Decision 3 — no schema version, and the reason the attribute was removed

An old file must load and keep its reference. **A schema version and a migration, as ADR 0040 did for
the configuration, buys nothing here**: the absence of this field is not an unknown state needing
interpretation, it is one the module already names and handles — *"as if it had never been read"* —
and it is exactly the state every meter was in before today.

**And the mechanism is the `Option`, not an attribute — which falsification, not reasoning,
established.** The field was written `#[serde(default)]` and the guard's recorded mutation was
"remove it". Run, that mutation **stayed green**: serde already treats a missing `Option` as `None`,
so the attribute was inert and the sentence calling it the compatibility mechanism was false.

The attribute is **removed** rather than kept as a harmless restatement. On a non-optional field it
would be actively harmful — turning a missing value into a silent `0`, an epoch mark presented as a
real one, which is the shape of lie this bridge exists to refuse.

The guard was then falsified by the mutation that is the ordinary shape of this fault: **adding a
required field**, which is what anyone writes who is not thinking about the files already on disk.
Red, with the reference coming back absent.

## Consequences

- **The one-tick-per-restart window is closed**, and with it the only path by which a replay could
  rewind a persisted reference.
- **A backwards clock at the source now costs a degraded verdict** until it catches up. Recorded in
  the operator manual beside the cause itself, not only here.
- **The reference file gains a field and keeps its name**, so a deployment upgrades without an
  operator action and without losing a yardstick.
- **A predicted falsification that did not hold is recorded next to its test**, because a guard whose
  stated mutation does not fail it is a guard nobody has checked.
