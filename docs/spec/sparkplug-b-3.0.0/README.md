# Sparkplug B specification, v3.0.0 — vendored

**This directory is third-party content and is NOT covered by this project's MIT licence.**

| | |
| --- | --- |
| Upstream | <https://github.com/eclipse-sparkplug/sparkplug> |
| Version | **v3.0.0** (release tag, not `master`) |
| Licence | **EPL-2.0** — see `LICENSE` and `NOTICE` in this directory |
| Copyright | © 2016–2021 The Eclipse Foundation, Cirrus Link Solutions, and others |
| Retrieved | 2026-07-26 |

Every `.adoc` file keeps its original EPL-2.0 header. Nothing here has been modified.

## Why it is vendored

The conformance audit (Epic 4, stories 4.1–4.3) walks the specification clause by clause against
the implementation. That needs a source that is **greppable** and, more importantly, **pinned**:
"conformant with Sparkplug B v3.0.0" is a claim someone can check, while "conformant with the
specification" is not — the document moves.

It also removes a failure mode this project has already hit twice. The quality-code defect
(contract v1) and the QoS misunderstanding in ADR 0010 both came from reading a *vendor's*
documentation or a summary table instead of the normative text. The normative statements carry
`tck-id-…` identifiers; cite those, not prose.

The release tag was chosen over `master` deliberately: `master` carries post-release edits that
correspond to no published version, so a conformance claim against it would be unverifiable.

## What is here and what is not

Present: the full AsciiDoc source of the specification — `sparkplug_spec.adoc` and
`chapters/*.adoc` — plus `LICENSE` and `NOTICE`.

Absent: `assets/` (diagrams and images) and the build tooling. This copy exists to be read and
searched, not rendered. The rendered specification lives at <https://sparkplug.eclipse.org>.

## Refreshing it

Fetch the same paths at a new tag, replace this directory, and update the version and retrieval
date above. **Do not** silently move to a newer version: a conformance matrix is only meaningful
against the version it was built from, so a version change invalidates it and must be treated as
an audit task, not a chore.

## A trademark caveat worth knowing before Epic 8

`Sparkplug®`, `Sparkplug Compatible` and the Sparkplug logo are **trademarks of the Eclipse
Foundation**, and there is a formal compatibility programme with a TCK behind those terms.

The PRD promises a "public conformance guarantee" for the `sparkplug-b` crate when it is
published. Describing the crate as *implementing the Sparkplug B specification*, with a stated
conformance matrix and its known deviations, is a factual claim we can support. Describing it as
*Sparkplug Compatible*, or using the marks as branding, is a different claim and needs the
programme behind it. Worth settling before the crates.io publish rather than after.
