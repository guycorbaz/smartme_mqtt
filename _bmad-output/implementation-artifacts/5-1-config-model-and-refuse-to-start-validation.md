# Story 5.1: One configuration model, validated as a whole, refusing to start rather than starting partially

Status: ready-for-dev

## Story

As the operator,
I want the bridge to check its **entire** configuration before it does anything, and to tell me
everything that is wrong in one go,
so that bringing it up is one read-fix-restart cycle instead of one per mistake.

## Acceptance Criteria

The epic states FR26. **It stands, and four are added** — three because the epic's wording hides a
false pass, one because [ADR 0021](../../docs/adr/0021-configuration-is-editable-from-the-ui.md)
gave this story a second consumer it did not have when the epic was written.

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

**AC4 — "topic uniqueness" must not pass vacuously** *(added)*

**Given** FR26 requires validating topic uniqueness
**When** the walking skeleton serves **exactly one** meter (`BridgeConfig::meter`, singular)
**Then** the uniqueness check is either **written against a collection that can hold duplicates and
tested with two**, or **recorded as unmet with an issue** — not implemented over a single-element
field and reported as satisfied.

> A uniqueness check over one element is green forever and proves nothing. Four Epic 1 tests passed
> for this class of reason, and the `n/a` column of the conformance matrix has 144 clauses that have
> never been falsified. Decide at drafting: *if* the model does not yet hold more than one meter,
> this AC is **unmet**, and it says so.

**AC5 — a serial is validated as an identity, not as a string** *(added)*

**Given** the serial is the Sparkplug device identifier
**When** it is validated
**Then** the rules that make it an identity are checked — at minimum **no leading zero**, and
whatever `check_identifier` already enforces for the topic grammar
**And** the failure message names the consequence, not the rule.

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
  - [ ] `crates/sparkplug-b/src/topic.rs::check_identifier` — what is already enforced, so AC5 adds
        rather than duplicates. Note it implements **Sparkplug's** wildcard rule, not MQTT's
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

- [ ] **Task 5 — serial and uniqueness** (AC: 4, 5)
  - [ ] Serial rules, with the `DroppedUndeclaredDevice` consequence in the message.
  - [ ] Decide and record AC4 either way. If unmet, open the issue in the same change.

- [ ] **Task 6 — the record**
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
