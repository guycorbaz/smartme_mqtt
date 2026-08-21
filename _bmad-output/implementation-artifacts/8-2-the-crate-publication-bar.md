# Story 8.2: The publication bar for `sparkplug-b`

Status: done

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
of more. A test scans every hand-written public signature, and the generated `protobuf` module
is exempt by name because it lives in `OUT_DIR` and has no author to decide anything.

*Amended by the review below, 2026-08-21: the scan as written here searched each `pub` **line**
for the literal `prost::`, which caught the shape the defect happened to have and no other. The
design of `DecodeError` stands; the guard was rewritten.*

**AC2 — met, and the dry run is the arbiter rather than my reading of the rules.**
`cargo publish --dry-run -p sparkplug-b` packages 18 files and verifies them. `repository`,
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

## Review, 2026-08-21

Three of the five criteria did not hold. **Neither failure was a wrong judgement**: the
design of AC1's error and of AC3's README were right. What had not been done was to attack
the two mechanisms that carry the promises — the guard and the example — with the shapes
that break them. AC2 and AC4 were re-verified and hold; the dry run was run, not read.

### AC1 and AC5 — the guard was awake only to the accident

The test searched each `pub` line for the literal string `prost::`. That is the shape the
original defect happened to have and no other. Two mutations went past it with the suite
green:

- **`use prost::DecodeError as ProstDecodeError;` and the name used bare.** This is how a
  Rust author *normally* writes an imported type. The guard saw no `prost::` on the line.
- **A signature wrapped across lines.** `) -> Result<Payload, prost::DecodeError> {` does
  not start with `pub fn`, so the line was skipped — and rustfmt wraps every signature past
  the line width, so this is the ordinary shape of a long one, not an exotic one.

AC5 followed from AC1: the single recorded mutation restored precisely the one-line,
fully-qualified form, so the falsification exercised the guard's only open eye.

**Repaired.** The scan now joins a declaration into a whole statement before reading it, and
judges **names in scope** rather than spellings: a file's `use` lines say what came from
another crate, and those names may not appear in a public declaration under *any* spelling.
The list is now an allow-list of what is ours (`crate`/`self`/`super`/`std`/`core`/`alloc`,
the crate's own module names read from the file stems, and `protobuf` — AC1's exemption, by
name) rather than a deny-list of five crates it had to have thought of in advance. Two
further holes closed on the way: a glob import from a foreign crate is refused at the import,
because after one nothing in the file can be judged at all; and the walk over `src/` is
recursive, so a future `src/foo/mod.rs` cannot go unscanned in silence.

The old comment claiming the deny-list held "the dependencies this crate actually has" is
gone with the deny-list. It was not true: `prost` is the only dependency, `rumqttc` and
`tokio` are *dev*-dependencies, and `bytes` and `serde` are neither.

**The falsification is now permanent rather than a one-off.** A second test runs the scanner
over a fixture carrying all four shapes and requires it to still catch them — and requires it
to leave `fmt::Result` alone, so it proves discrimination and not merely noise. A mutation
performed once by hand proves the guard only for the day it was performed.

### AC3 — the README's example did not compile, and nothing compiled it

Three defects in the eight lines a reader meets first on crates.io: `NodeSession::new` does
not exist (it is `NodeSession::start`), `BdSeq` has no `From`, so `0.into()` resolves to
nothing, and `Metric::new` was called with two of its three arguments. The hidden
`# Ok::<(), sparkplug_b::TopicError>(())` line shows it was *written* as a doctest — it was
simply never included anywhere, and a README is a file rather than a module, so `cargo test`
never read it.

**Repaired at the mechanism, not at the text.** `src/lib.rs` now carries

```rust
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct Readme;
```

which compiles and runs every fence in the README with `cargo test` — the command CI already
runs — while `cfg(doctest)` keeps the README's prose out of the rendered documentation. The
example itself was rewritten to be worth compiling: it asserts the topic it builds, that the
will carries no `seq` and that the BIRTH carries `seq` 0.

**Falsified**: putting `NodeSession::new(0.into())` back gives
`error[E0599]: no associated function or constant named 'new' found for struct 'NodeSession'`,
and `test crates/sparkplug-b/src/lib.rs - Readme ... FAILED`.

### Falsification record, review

| # | Mutation | Went red with |
|---|---|---|
| 2 | `use prost::DecodeError as ProstDecodeError;` + `pub fn decode_aliased(…) -> Result<Payload, ProstDecodeError>` | `encode.rs:655: `ProstDecodeError` is brought in by a `use` from another crate` |
| 3 | `pub fn decode_wrapped(` / `bytes: &[u8],` / `) -> Result<Payload, prost::DecodeError> {` | `encode.rs:659: `prost::…` is another crate's path` |
| 4 | `pub struct Leaky { pub inner: prost::DecodeError }` | `encode.rs:666: `prost::…` is another crate's path` |
| 5 | `use prost::*;` | `encode.rs:653: `prost::*` puts unnameable foreign types in scope` |
| 6 | the README's `NodeSession::start(BdSeq::before_first())` → `NodeSession::new(0.into())` | `crates/sparkplug-b/src/lib.rs - Readme … FAILED` — `E0599` |
| — | *(recorded)* mutations 2 and 3 are the two that the **first** version of the guard let through with the suite green. That run is what this review is; they are kept in the fixture so the guard is re-falsified on every CI pass. |

### File List

- `crates/sparkplug-b/src/encode.rs` — modified (`DecodeError`, and `decode`'s signature)
- `crates/sparkplug-b/src/lib.rs` — modified (the new type is exported; **review**: the
  `cfg(doctest)` hook that compiles the README)
- `crates/sparkplug-b/Cargo.toml` — modified (`repository`, `readme`, `keywords`, `categories`)
- `crates/sparkplug-b/README.md`, `CHANGELOG.md` — **new** (**review**: the example rewritten
  so that it compiles, and the CHANGELOG says so)
- `crates/sparkplug-b/tests/public_api_is_our_own.rs` — **new** (**review**: rewritten — whole
  statements, names in scope, and a fixture that re-falsifies the guard on every run)
- `_bmad-output/implementation-artifacts/8-2-…md`, `sprint-status.yaml` — new/modified

### Change Log

- **2026-08-20** — Story 8.2. NFR19's bar, met and guarded. Publication itself stays deferred
  ([#3]). One mutation run. `CONTRACT_VERSION` untouched — this crate does not know what a
  contract version is.
- **2026-08-21** — Review. AC2 and AC4 re-verified and hold. AC1, AC3 and AC5 did not: the
  guard caught only the one shape the original defect had, and the README's example did not
  compile and nothing compiled it. Both repaired at the mechanism. Five further mutations run,
  four of them now permanent. `CONTRACT_VERSION` still untouched.
