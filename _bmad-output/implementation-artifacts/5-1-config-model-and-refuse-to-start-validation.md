# Story 5.1: One configuration model, validated as a whole, refusing to start rather than starting partially

Status: ready-for-dev

## Story

As the operator,
I want the bridge to check its **entire** configuration before it does anything, and to tell me
everything that is wrong in one go,
so that bringing it up is one read-fix-restart cycle instead of one per mistake.

## Acceptance Criteria

The epic states FR26. **It stands, and six are added.** Three (AC2, AC5, AC7) because the epic's
wording hides a false pass; one (AC1) because
[ADR 0021](../../docs/adr/0021-configuration-is-editable-from-the-ui.md) gave this story a second
consumer — the UI — that it did not have when the epic was written; and two (AC4, AC6) because Guy
set the multi-meter scope on 2026-08-03, after this file was first drafted.

**AC1 — one model, one validation, no second opinion**

**Given** `BridgeConfig` (`app/supervisor.rs:28`), built today by six `env::var` reads scattered
through `main.rs`
**When** this story lands
**Then** there is exactly **one** function that turns untrusted input into a validated
configuration, and it is the only way to obtain one
**And** `main.rs` calls it rather than assembling the struct field by field.

> The second consumer is the UI (FR46). If validation lives in `main.rs`, the form will grow its own
> copy, and the two will disagree the first time a bound changes — which is the shape of the defect
> this project has logged seven times. The type is the enforcement: **a `BridgeConfig` that exists
> is a `BridgeConfig` that was validated.**

**AC2 — every problem is reported, not the first** *(added)*

**Given** a configuration with three faults
**When** the bridge starts
**Then** the operator is told about **all three**, in one message, with the variable or field name
against each
**And** the process exits non-zero without opening a socket, publishing anything, or writing state.

> **AMENDED 2026-08-06 — [ADR 0026](../../docs/adr/0026-a-configuration-it-cannot-use-stops-the-bridge-publishing-not-serving.md),
> [#57](https://github.com/guycorbaz/smartme_mqtt/issues/57).** The process **no longer exits**. It
> stays up, opens no socket, publishes nothing and writes no state — every guarantee this criterion
> was written to buy — and serves the configuration screen so the fault can be repaired in a
> browser. The exit was taking down the only repair path there is on a deployment with no shell.
> FR26 is amended in the same motion: *refuse to start* → *refuse to publish, and say so on the
> screen*.

> Today `require()` (`main.rs:17`) returns on the first missing variable. With six required values
> and no example that fills them, a first run is up to six edit-restart cycles, each revealing
> exactly one more thing. This is the acceptance criterion the story exists for; FR26's *"refuse to
> start"* is already half-true and would have let the story pass without touching it.

**AC3 — the bounds from ADR 0020 are enforced here, not at the form** *(added)*

**Given** the publish period
**When** it is validated
**Then** a value below **5 s**, above **300 s**, or expressing "off" (`0`, empty, absent-meaning-
disabled) is **rejected**, with a message that says why the bound exists
**And** the default when unset is **30 s**, which is today's hard-coded value.

> Today the period is not configurable at all: `Duration::from_secs(30)` at `main.rs:237`, reachable
> by no variable. This story is where it becomes one. The bound must live in the model because
> [ADR 0018](../../docs/adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) made the
> periodic publish the trigger for host-initiated recovery — a bound enforced only in a form is a
> bound a `.env` bypasses.

**AC4 — the model holds MANY meters, each with an enabled flag** *(added; scope set by Guy
2026-08-03)*

**Given** `BridgeConfig::meter` is singular today (`supervisor.rs:36`) and Guy has **four** real
meters, one of them not currently connected
**When** this story lands
**Then** the configuration holds a **collection** of meters, each carrying its identity and an
**enabled** flag — the unconnected fourth is expressible as *configured but not enabled*
**And** the runtime is **not** changed: serving more than one meter is later work.

> **Why the shape moves now and the runtime does not.** Guy's order is *web UI first, then multiple
> meters*. The meter list is the central object of the configuration screen, so a form built against
> a singular field would be built twice. Reshaping the model costs a few lines and is invisible at
> runtime; reshaping a form is not. This is the cheap half of multi-meter, taken early precisely so
> the expensive half can stay late.

**AC5 — topic uniqueness is checked for real, with two meters that collide** *(added)*

**Given** the collection from AC4
**When** two meters resolve to the same topic, or share a serial
**Then** validation rejects the configuration and names **both** offenders
**And** the test constructs the collision rather than asserting over a one-element list.

> Until AC4 this could not be written: a uniqueness check over one element is green forever and
> proves nothing. Four Epic 1 tests passed for that class of reason, and the conformance matrix's
> `n/a` column still has 144 clauses nobody has ever falsified. AC4 is what makes FR26's uniqueness
> clause a real requirement instead of a vacuous one.

**AC6 — more enabled meters than the runtime can serve is REFUSED, not truncated** *(added)*

**Given** the model accepts N meters while the runtime still serves one
**When** a configuration enables more than the runtime supports
**Then** the bridge **refuses to start**, with a message naming the limit and the story that lifts it
**And** it never starts serving a subset.

> This is the AC that keeps AC4 from being dangerous. Without it, Epic 6 lets Guy add four meters in
> a browser, the bridge starts happily, and three of them silently never publish — a healthy-looking
> node with missing devices, which is the same failure shape as a serial with a leading zero
> (`DroppedUndeclaredDevice`) and just as hard to notice. **Silent truncation of a configuration is
> the exact lie this product exists to prevent.**

**AC7 — a serial is validated as an identity, not as a string** *(added)*

**Given** the serial is the Sparkplug device identifier
**When** it is validated
**Then** the rules that make it an identity are checked — at minimum **no leading zero**, and
whatever `check_identifier` already enforces for the topic grammar
**And** the failure message names the consequence, not the rule.

> **What this rule actually is, decided 2026-08-03.** The real requirement is *the serial must be the
> one smart-me reports*, which cannot be checked offline. The leading zero is a **proxy** for it,
> generalised from a single incident, so it can in principle refuse a legitimate serial and there is
> deliberately no override. Guy confirmed none of his four meters carries one and chose the hard
> refusal over a warning — because the failure it prevents is silent, and a startup WARN would drown,
> which [#44](https://github.com/guycorbaz/smartme_mqtt/issues/44) had just demonstrated about
> warnings nobody can see. A meter with a genuine leading zero would be a code change, and that is
> the accepted cost rather than an oversight.
>
> This is not hypothetical tidiness. Serial `9202685` written as `09202685` drops **every reading**
> as `DroppedUndeclaredDevice`: the bridge runs, the node births, the tags exist, and no value ever
> arrives. A validation error at startup costs a minute; this failure mode costs however long it
> takes to notice that a healthy-looking bridge publishes nothing.

## Tasks / Subtasks

- [ ] **Task 1 — read before writing** (AC: all)
  - [ ] `crates/smartme-bridge/src/main.rs` in full — the six `env::var` reads, `require()` at :17,
        and the `BridgeConfig` literal beginning at :196. That assembly is what this story replaces.
  - [ ] `crates/smartme-bridge/src/app/supervisor.rs:28` — `BridgeConfig` as it stands.
  - [ ] `.env.example` — the variable names are the operator-facing contract and must not drift.
  - [ ] `crates/sparkplug-b/src/topic.rs::check_identifier` — what is already enforced, so **AC7**
        adds rather than duplicates. Note it implements **Sparkplug's** wildcard rule, not MQTT's
        character set ([#34](https://github.com/guycorbaz/smartme_mqtt/issues/34)).

- [ ] **Task 2 — the model and its one constructor** (AC: 1)
  - [ ] A `RawConfig` (everything optional, no rules) and a validated `BridgeConfig`, with a single
        fallible conversion between them.
  - [ ] `main.rs` reads the environment into `RawConfig` and nothing else. No `expect`, no
        field-by-field assembly.

- [ ] **Task 3 — accumulate, do not short-circuit** (AC: 2)
  - [ ] Validation returns a **collection** of faults. `?` on the first error is the defect.
  - [ ] Falsify: a config with three faults must produce three lines. Run it against a
        short-circuiting version first and record that it reported one.

- [ ] **Task 4 — the publish period** (AC: 3)
  - [ ] New variable, named in `.env.example` with its bounds and the reason for them.
  - [ ] `main.rs:237`'s hard-coded `30` is **deleted**, not left as a fallback beside the default —
        two sources for one value is how they diverge.

- [ ] **Task 5 — the meter collection** (AC: 4, 6)
  - [ ] `BridgeConfig::meter` becomes a collection, each entry carrying its identity and an
        `enabled` flag. **Do not touch the runtime** — `supervisor` keeps serving what it serves.
  - [ ] The refusal of AC6, with the limit and the lifting story named in the message. Falsify by
        enabling two and confirming the process exits without publishing.

- [ ] **Task 6 — uniqueness and serials** (AC: 5, 7)
  - [ ] Uniqueness over the collection, tested with a constructed collision, naming both offenders.
  - [ ] Serial rules, with the `DroppedUndeclaredDevice` consequence in the message.

- [ ] **Task 7 — the record**
  - [ ] Falsification notes copied from the runs, next to each test.
  - [ ] `./scripts/ci-local.sh` — full, not `--fast`.

## Dev Notes

### What this story does NOT do

**No persistence, no reload, no file format.** Configuration still arrives from the environment; this
story only makes it *validated*. Persistence and `ArcSwap` hot reload are Story 5.2, and **where a
secret rests on disk is architecture open item 5, still open** — ADR 0019 sharpened it and did not
settle it. Writing a config file here would pre-empt that decision by accident.

**No UI.** Per ADR 0021 this epic owns the model and Epic 6 owns the screens. Everything above is
testable without a line of HTML, which is the entire reason for the split.

### The secrets rule applies already

ADR 0019 requires that secrets are never traced. Validation is exactly where that gets broken: an
error message that formats the config to say *"this field is wrong"* prints the ones that are right.
The faults collection must carry **field names, never values** — and the test for it is worth
writing here rather than in Epic 6, because a `Debug` derive on `RawConfig` would defeat it silently
and no template is involved.

### Where the first run has no configuration

FR23 stays the bootstrap path and must remain sufficient alone (ADR 0021). So this story's validation
runs on environment input, and Epic 6's form later feeds the *same* constructor. If the two paths
ever have different rules, the bug will be that a value accepted in a browser refuses to boot.

### The `.env.example` contract

Every variable this story adds or renames must appear there in the same change. Yesterday's integrity
check found `SMARTME_LOG_DIR` and `SMARTME_LOG_KEEP` read by `main.rs` since v0.2.0 and documented
**nowhere** — not in `.env.example`, not in the manual. That is the cheapest possible defect to avoid
and it still happened.
