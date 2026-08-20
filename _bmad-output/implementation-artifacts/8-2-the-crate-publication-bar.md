# Story 8.2: The publication bar for `sparkplug-b`

Status: review

> **NFR19 names five things and the crate has three.** *"Semver with a stable, documented
> public API (no third-party types leaked), complete crate metadata
> (`license`/`description`/`repository`), a README and a CHANGELOG."* Today: `license`,
> `description` and `rust-version` are there; **there is no README, no CHANGELOG, no
> `repository`** — and `cargo publish` refuses a crate with no README path when one is
> declared, which is the least of it.
>
> **And a third-party type does leak**, found by reading the signatures rather than the
> intent: `pub fn decode(bytes: &[u8]) -> Result<Payload, prost::DecodeError>`. A consumer
> matching on that error has to depend on `prost`, at the exact version this crate does, for
> ever.

## Story

As someone who might one day depend on `sparkplug-b` from crates.io,
I want its public surface to be its own and its documents to exist,
so that using it does not silently make me a consumer of this bridge's dependency choices.

## Acceptance Criteria

**AC1 — no third-party type in the signatures this crate writes, and a guard that keeps it
that way.**

*The criterion is narrowed at drafting rather than at implementation, because the strict reading
is already false and the crate says so.* `protobuf::Payload` is generated **here**, by this
crate's own `build.rs`, from the specification's committed `.proto` — it is the Sparkplug
payload itself, not a borrowed type — and `encode.rs` records the consequence in as many words:
*"`prost` is therefore a PUBLIC dependency of this crate, and a major `prost` bump is a
breaking change here too."* Pretending otherwise would be a story asserting something the code
has already refuted.

**Given** every `pub` signature this crate writes by hand
**When** it is read
**Then** no type from another crate appears in it — `prost::DecodeError` becomes the crate's own
error, so a consumer matching on a failure does not thereby depend on `prost` at the exact
version this crate pins
**And** the rule is enforced mechanically, on the `arch_purity` pattern
**And** the generated `protobuf` module is exempt by name, and the README states the public
dependency plainly rather than leaving a consumer to discover it from a compiler error.

**AC2 — the metadata is complete, and `cargo publish` says so.**

**Given** `cargo publish --dry-run -p sparkplug-b`
**When** it runs
**Then** it succeeds
**And** `repository`, `readme`, `keywords` and `categories` are present alongside what is there
already — the dry run is the arbiter, because the rules are crates.io's rather than ours.

**AC3 — a README that says what the crate is and what it is not.**

**Given** a reader who has just found the crate
**When** they read the README
**Then** it says what it does, what it deliberately does not do, and **which parts of the
Sparkplug specification it implements and which it does not** — NFR19's *documented conformance
scope*, without which "Sparkplug B library" is a claim nobody can check.

**AC4 — a CHANGELOG that starts where the versions actually are.**

**Given** the crate has shipped inside this bridge since v0.1
**When** the CHANGELOG is written
**Then** it records the versions that exist rather than inventing a history
**And** it says plainly that the crate's version tracks the workspace's today, and what that
means for a consumer.

**AC5 — falsification.**

**Given** AC1's guard
**When** a third-party type is put back into a public signature
**Then** it goes red, and the run's output is copied next to it.

## Out of scope

- **Actually publishing.** [#3] defers the crates.io push, and it stays deferred: publishing is
  a decision with a name attached to it, not a step in a story.
- **Stabilising the API at 1.0.** The version tracks the workspace's, which is recorded in AC4
  rather than changed here — changing it is a decision about two products' release cadences.

## Dev Notes

### What must not break

- **`protobuf::Payload` is the crate's own**, generated from the committed `.proto` by this
  crate's `build.rs`. It is not a third-party type, and AC1's guard must not mistake it for one.
- **The isolated build** (`cargo build -p sparkplug-b --no-default-features`) stays green: it is
  what proves the crate does not depend on the bridge.
- **`#![forbid(unsafe_code)]`** stays where it is.

### References

- [Source: `_bmad-output/planning-artifacts/prd.md:367`] — NFR19, in full
- [Source: `crates/sparkplug-b/src/encode.rs:208`] — the leak
- [Source: `https://github.com/guycorbaz/smartme_mqtt/issues/3`] — publication itself, deferred
- [Source: `CLAUDE.md`] — falsify before trusting

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-20.

### Completion Notes List

**AC1 — met.** `decode` returns the crate's own `DecodeError`, which keeps `prost`'s message
verbatim and stops handing out its type. The error is **opaque on purpose**: there is exactly
one way to fail — the bytes are not this protobuf — so an enum with one arm would be a promise
of more. A test scans every hand-written public signature against the crate's actual
dependencies, and the generated `protobuf` module is exempt by name because it lives in
`OUT_DIR` and has no author to decide anything.

**AC2 — met, and the dry run is the arbiter rather than my reading of the rules.**
`cargo publish --dry-run -p sparkplug-b` packages 17 files and verifies them. `repository`,
`readme`, `keywords` and `categories` join the metadata that was already there.

**AC3 and AC4 — met.** The README says what the crate does, what it deliberately does not
(no transport, no host-application side, no DCMD, no aliases), and **which specification
clauses are in scope and which are recorded as out of it** — without that section, "Sparkplug B
library" is a claim nobody can check. The CHANGELOG starts where the versions actually are and
**invents no history**: it says the crate has shipped inside the bridge since v0.1.0 and that
its version tracks the workspace's, which is the fact a consumer most needs.

### A factual error caught before it shipped

The README's first draft said the licence was **EPL-2.0**. It is **MIT** — the workspace's.
EPL-2.0 is the licence of the *specification* this library is written against, a copy of which
is committed for citation. The two had been conflated because the specification is the thing
this crate quotes most. Corrected, and the distinction is now stated in the README rather than
left to be re-conflated.

### Falsification record

| # | Mutation | Went red with |
|---|---|---|
| 1 | `decode` returns `prost::DecodeError` again (signature and body) | `a public signature hands a consumer a type from another crate … encode.rs:214: pub fn decode(bytes: &[u8]) -> Result<Payload, prost::DecodeError>` |
| — | *(recorded)* changing only the signature does not compile, so the mutation had to restore the body too — the type system catches half of this on its own, and the test catches the half it does not |

### File List

- `crates/sparkplug-b/src/encode.rs` — modified (`DecodeError`, and `decode`'s signature)
- `crates/sparkplug-b/src/lib.rs` — modified (the new type is exported)
- `crates/sparkplug-b/Cargo.toml` — modified (`repository`, `readme`, `keywords`, `categories`)
- `crates/sparkplug-b/README.md`, `CHANGELOG.md` — **new**
- `crates/sparkplug-b/tests/public_api_is_our_own.rs` — **new**
- `_bmad-output/implementation-artifacts/8-2-…md`, `sprint-status.yaml` — new/modified

### Change Log

- **2026-08-20** — Story 8.2. NFR19's bar, met and guarded. Publication itself stays deferred
  ([#3]). One mutation run. `CONTRACT_VERSION` untouched — this crate does not know what a
  contract version is.
