# ADR 0047 — An unchanged reading is not republished, because the session is stateful

- **Status:** accepted
- **Date:** 2026-08-24
- **Decides:** whether the bridge publishes on every poll. Amends [ADR 0027](0027-a-failed-source-is-a-fault-the-screen-must-name.md) §3.
- **Issue:** [#92](https://github.com/guycorbaz/smartme_mqtt/issues/92) for the counting it interacts with, [#32](https://github.com/guycorbaz/smartme_mqtt/issues/32) for this.

## Context

The specification asks for report-by-exception and says why:

> *"Because of the stateful nature of Sparkplug sessions, data SHOULD NOT be published from Edge
> Nodes on a periodic basis and instead SHOULD be published using a RBE based approach."*
> — `tck-id-principles-rbe-recommended`, `Sparkplug_2_Principles.adoc:50`

It also leaves a door open, in the prose immediately above the clause: *"Sparkplug does not require
that RBE be used in all cases. This is to account for special circumstances that may require
periodic reporting."* So keeping periodic publishing is legal — it is not the door that is missing,
it is a reason to walk through it.

**And ADR 0027 §3 says the opposite, for a reason that is the whole of this project:**

> *"Silence on a Sparkplug wire is not a statement. It is indistinguishable from 'nothing has
> changed', which is why a host goes on displaying what it last received. A verdict the bridge has
> reached and does not publish is a verdict it has withheld."*

Both cannot stand. This ADR decides which, and the decisive fact is that **one of ADR 0027's
premises stopped being true.**

### What changed under ADR 0027

When it was written, the bridge did not answer a Rebirth Request. [#32] recorded that consequence
precisely: *"the periodic publish is currently substituting for the missing Rebirth. Implementing
RBE first would mean a new consumer never learns the unplugged meter's value — a functional
regression wearing conformance as a costume."*

Story 4.7 landed. The bridge answers a Rebirth Request with a full BIRTH sequence under the same
`bdSeq`. The substitution is no longer needed, and [#32]'s own stated condition for revisiting is
met.

### What the periodic publish costs today

The fixture meter `30000003` — the physically unplugged one — reads `0.0 kW` with a `ValueDate` of
2026-04-20. Byte-identical content goes out for it roughly **17 000 times a day**, indefinitely.
That is the case the clause exists for.

For an active meter it costs almost nothing, because almost nothing is suppressed: cumulative energy
advances at the published precision on nearly every poll.

## Decision

**A reading identical in every published respect to the last one CONFIRMED for that device is not
published.** Everything else is, exactly as before.

Identical means `MeterUpdate == MeterUpdate`: the measurement — serial, power, energy, unit,
`ValueDate`, source quality — and the verdicts, which carry the per-metric quality **and the cause**
that contract v12 publishes as `Cause/Power` and `Cause/Energy` (ADR 0044). A comparison narrower
than the payload would suppress a message that differed from its predecessor.

So the rule is not *"suppress what has not changed"* in some interpreted sense. It is: **suppress
exactly the messages that carry no information the host does not already hold, and nothing else.**

### ADR 0027 §3 is amended, not overturned

Its rule read: *every poll cycle produces a published verdict for every enabled meter — a value with
its quality, or no value with a non-good quality — or it produces a device certificate. Not
silence.*

It becomes: **every CHANGE in what a meter publishes — value, quality, or cause — produces a
published verdict. An unchanged reading produces silence, because the session is stateful and a
late-joining consumer asks.**

The reasoning ADR 0027 gave survives intact where it applies. *A verdict the bridge has reached and
does not publish is a verdict it has withheld* — and a verdict identical to the one the host is
already displaying has not been withheld from anybody. What made the sentence load-bearing was the
case where the verdict **differs**: the meter froze, the quality went to `Bad_Stale`, and silence
would have left a fresh-looking value on screen. That transition publishes, and this ADR does not
touch it.

**A quality or cause transition always publishes**, which is the half [#32] warned must not be lost:
suppress on value alone and staleness becomes unobservable.

### What the host is relied on to do

Nothing it is not required to do. The session is stateful by the specification's own design, the
host holds the last value it received, and a consumer that needs to relearn issues a Rebirth
Request — which this bridge answers, and which `chaos_rebirth` exercises against a real broker.

**If a host neither retains nor rebirths, it was already broken** under any Sparkplug producer, and
the bridge cannot repair it by shouting.

## Consequences

**A frozen meter goes quiet after its first frozen publication**, and its screen keeps the quality
it was last told — which is the true statement. The bridge's own `/` and `/healthz` keep saying when
it last published, which is now a fact about the meter rather than about the poll loop.

**The historian stops receiving identical points.** That is the change that gets cheaper the earlier
it is made, and the reason [#32] was the one issue whose deferral was paid in data.

**A suppressed publication is not a loss and is not counted.** [#92]'s republication counter counts
readings the transport refused; this suppresses readings the bridge chose not to send. Conflating
them would report a fault where there is a decision.

**`last_published_at` keeps advancing for a quiet meter, and that is a residual this ADR does not
close.** The first draft of this section claimed the opposite, and reading the code refuted it:
`record_at` is called by the poll loop when it hands a reading to the channel, before the driver has
decided anything, so a suppression cannot reach it. The field therefore continues to mean *when the
loop last handed a reading over*, which is what it has always meant and is no longer quite what its
name suggests once suppression exists.

It is left as is rather than repaired here, deliberately: moving that write behind the transport's
confirmation is the same reordering ADR 0046 performed for `declared`, and doing it in the same
change would mix a decision about the wire with a change to what the screens measure. Recorded as
its own issue instead of being carried in prose.

## Falsification

Recorded with the tests: suppressing on the value alone lets a quality transition go unpublished and
turns the staleness guard red; comparing against the last *attempted* rather than the last
*confirmed* publication suppresses a message the host never received; and removing the suppression
restores the identical-message stream the fixture meter produces.
