# Changelog

All notable changes to `sparkplug-b`.

**This crate's version tracks the workspace's**, not a release cadence of its own — it lives
inside [smartme_mqtt](https://github.com/guycorbaz/smartme_mqtt) and ships when the bridge
ships. **What that means if you depend on it**: a version bump does not imply a change here at
all, and this file is the only place that says whether one happened. It is written so that a
consumer can tell "the bridge released" from "the library changed".

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the project follows
semantic versioning as far as the shared version number allows.

## [Unreleased]

### Changed

- **`decode` returns this crate's own `DecodeError`** instead of `prost::DecodeError`.
  *Breaking for anyone matching on the error type.* The reason: a borrowed error type made every
  consumer a `prost` consumer, at the version this crate pins, and a `prost` major release broke
  their code as well as ours. The message is preserved verbatim inside it.

### Added

- `README.md` and this file, and the crate metadata crates.io requires — the publication bar
  NFR19 describes. Publication itself remains deferred (issue #3).
- **The README's example is compiled and run by `cargo test`**, through a `cfg(doctest)` hook in
  `src/lib.rs`. Its first draft did not compile — it named a constructor this crate does not
  have — and nothing read it, a README being a file rather than a module. The example on the
  front page is now the one piece of documentation that cannot go stale in silence.

## Earlier versions

The crate has shipped inside the bridge since **v0.1.0** (2026-07), and its history until now is
the bridge's own: see that repository's log and its ADR trail. The parts that were built, in
order — topic grammar and validation, payload and metric encoding with datatypes and quality,
`seq`/`bdSeq` sequencing and persistence across restarts, the BIRTH/DATA/DEATH lifecycle and the
will, the rebirth answer — are each covered by tests that cite the specification clause they
enforce.

**No entry is invented for those versions.** A changelog that reconstructs a history nobody
recorded is worth less than one that says where it starts.
