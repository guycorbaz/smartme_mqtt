# ADR 0015 — A language-level type invariant is admissible conformance evidence, under ADR 0014's test

- **Status:** Accepted
- **Date:** 2026-07-28
- **Related:** [#36](https://github.com/guycorbaz/smartme_mqtt/issues/36), [ADR 0014](0014-schema-as-conformance-evidence.md), `docs/sparkplug-conformance.md` §"How to read this", Story 4.3
- **Extends:** ADR 0014, which admitted exactly one non-test witness — the vendored protobuf schema.

## Context

Chapter 1 of the specification states three clauses about the *type* of an identifier:

> `tck-id-intro-group-id-string` — *"The Group ID MUST be a UTF-8 string and used as part of the
> Sparkplug topics"* (`Sparkplug_1_Introduction.adoc:304-306`); likewise
> `-edge-node-id-string` (`:313-315`) and `-device-id-string` (`:322-324`).

`EdgeNode` stores `group` and `node` as `String` and `device_topic` takes a `&str`
(`topic.rs:99-100, 140`). In safe Rust a `String` **is** UTF-8: the invariant is enforced by the
standard library's constructors, and there is no program we can write that puts invalid UTF-8 in
one. The violation is not untested, it is unrepresentable.

**The Story 4.3 pass marked all three `conformant` and cited the topic-grammar tests as proof.**
That was wrong in two ways, and the code review caught both:

1. `node_topics_follow_the_namespace_grammar` and `device_topics_append_the_device_identifier` test
   topic *grammar*. Neither exercises UTF-8 validity. Delete the clause's subject matter and they
   pass unchanged — the "assertion adjacent to the clause" failure this project has already been
   bitten by. The Proof column was padded with test names to make the rows look as though they
   cleared the bar the ordinary way.
2. The real argument — the type invariant — was an **extension of ADR 0014 made silently, inside a
   document, mid-audit**. Which is precisely the failure ADR 0014 was written to correct, committed
   again in the same epic, one chapter later.

ADR 0014 also anticipated this exact move and warned against it:

> *"The narrowness is what keeps this honest. A wider version of this rule — 'the compiler proves
> it' — would swallow half the matrix and turn correct-by-construction into conformant, which is the
> downgrade the chapter-6 pass performed eight times on purpose."*

So the question is not whether "the compiler proves it" is a good rule. It is not. The question is
whether *this* witness passes the test ADR 0014 actually set.

## Decision

**ADR 0014's admissibility test is a property, not an artifact — as ADR 0014 itself says: the
witness qualifies by "compile-time unrepresentability, not the file's location". A type invariant
enforced by the language or its standard library therefore qualifies, under three conditions that
are jointly necessary.**

A row may cite a language-level type invariant as its proof when **all three** hold:

1. **The clause is about a type**, not a value and not a behaviour. *"MUST be a UTF-8 string"*
   qualifies. *"MUST be 3"* does not, and never will — ADR 0014's value boundary is untouched.
2. **The enforcing invariant is not ours to change.** `String`'s UTF-8 guarantee belongs to the
   standard library; no edit in this repository can weaken it. This is the condition that keeps the
   rule narrow, and it is the one that does the work below.
3. **The row says so explicitly**, naming the type and the invariant, and does **not** pad the Proof
   column with tests that exercise something adjacent.

**The three chapter-1 `-string` rows stay `conformant` on this basis, with their Proof cells
rewritten.**

### The boundary, stated as the cases it excludes

Condition 2 is the whole rule. Without it "the compiler proves it" swallows the matrix, exactly as
ADR 0014 predicted:

- **`operational-behavior-primary-application-state-with-multiple-servers-single-server` stays
  `gap (unproven)`.** A second concurrent session is unreachable because `MqttConfig` holds one
  `host` and one `port` — **our** type, which Story 4.5 will change when it adds a server list. Our
  own code shape, excluded by ADR 0014 and excluded again here.
- **The property-set array-length clauses stay `gap (unproven)`.** `encode_properties` pushes in
  pairs because our loop does; nothing in the type system says the two `Vec`s agree.
- **A `#[repr(u32)]` discriminant equalling the literal a clause names is still a value claim**, and
  still needs a test — the `Int32 = 3` defect the Story 4.2 review found stays found.

### One case this ADR deliberately does not resolve

`SeqCounter` holds a `u8`, and the matrix calls 0–255 "a type invariant, not a check"
(`payloads-sequence-num-req-nbirth`). `u8`'s range is the language's, but the clause is about a
**value range**, so condition 1 excludes it. Those rows are unaffected because they name property
tests as well — they never rested on the invariant alone. Flagged rather than settled: a future
pass should decide whether a value-range clause can ever be discharged by a type, and this ADR's
answer is provisionally *no*.

## Consequences

- **Three chapter-1 rows keep `conformant`; no tally moves.** Chapter 1 stays `3 · 0 · 4 · 1 = 8`
  and the story total stays `26 · 3 · 31 · 64 = 124`.
- **Their Proof cells no longer name unrelated tests.** The witness is stated as what it is.
- **The witness class is now defined by a property with three conditions**, so the next pass meeting
  a correct-by-construction clause has a test to apply rather than a precedent to stretch.
- **`CLAUDE.md`'s rule held, late.** The extension reached an ADR only because an adversarial review
  demanded one. That is the mechanism working, and it is also the second time in one epic that a
  standing rule was amended inside a document first and justified afterwards.

## What this ADR is really correcting

The same thing ADR 0014 was: not the exception, which survives scrutiny, but the way it arrived.
ADR 0014 closes by observing that "a reader agreeing with the conclusion never audits the premise".
Story 4.3 read that sentence, agreed with it, and then did it again — which is worth recording,
because the lesson evidently does not transfer by being written down once.
