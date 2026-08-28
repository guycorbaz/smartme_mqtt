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

- **`NodeSession::start` takes an `Option<BdSeq>`, and `BdSeq::before_first` is gone.**
  *Breaking.* `None` means this node has never connected, and its first session is numbered **0**
  — the *start at zero* half of `tck-id-topics-nbirth-bdseq-increment`, which this crate did not
  honour: the sentinel returned 0 and `start` advanced past it, so a brand-new node published 1.
  The absence of a previous session cannot be spelled as a number without replaying one, so it is
  now spelled as the absence it is. `Some(previous)` behaves exactly as before. ADR 0042, issue
  [#100]. Confirmed against a live Ignition on 2026-08-22.
- **`decode` returns this crate's own `DecodeError`** instead of `prost::DecodeError`.
  *Breaking for anyone matching on the error type.* The reason: a borrowed error type made every
  consumer a `prost` consumer, at the version this crate pins, and a `prost` major release broke
  their code as well as ours. The message is preserved verbatim inside it.

### Added

- **`SeqCounter::give_back` and `SessionEncoder::give_back_seq`.** Give back the sequence number
  the last message took, when that message **never reached the wire**. A `seq` jump is a
  lost-message condition to a Sparkplug host — it issues a Rebirth Request or marks the node stale
  — so repairing the hole a refused message would leave is worth doing. **Replaying a number that
  DID reach the wire is worse than the hole**, so exactly one condition makes this sound, and a
  caller who cannot state it must not call it: *a single message was in flight and the transport
  refused it*. It does not hold for a partly-refused BIRTH sequence. ADR 0046, issue [#92].



- `README.md` and this file, and the crate metadata crates.io requires — the publication bar
  NFR19 describes. **Publication itself is decided against** — not deferred — by ADR 0045 on
  2026-08-22: a crates.io version cannot be withdrawn, only yanked; nothing yet feeds `decode`
  hostile bytes, which is where a stranger arrives; and the conformance scope this README states
  rests on a matrix with citations that no longer point at code. The bar stands on its own merits:
  it closed a real leak and made the README something `cargo test` fails on.
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
