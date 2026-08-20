//! Story 8.2 AC1 — nothing this crate writes by hand may hand a consumer somebody
//! else's type.
//!
//! **Why a test and not a review.** `decode` returned `prost::DecodeError` from the
//! day it was written until 2026-08-20, through every review this crate has had. It
//! was not an oversight anybody could see: the signature reads perfectly well, and
//! the cost is invisible until a consumer tries to match on the error and finds
//! themselves pinned to this crate's `prost` version.
//!
//! # The one exemption, and why it is not a loophole
//!
//! `protobuf::Payload` and its neighbours are generated **in this crate**, by its own
//! `build.rs`, from the specification's committed `.proto`. They are the Sparkplug
//! payload itself. They implement `prost::Message`, which makes `prost` a **public
//! dependency** — a fact the README states plainly rather than leaving a consumer to
//! discover from a compiler error. The generated module lives in `OUT_DIR` and is
//! never scanned here, because it is not hand-written and there is nothing for its
//! author to decide.

use std::fs;
use std::path::{Path, PathBuf};

/// Crates whose types must not appear in a public signature this crate writes.
///
/// Not an allow-list of the world: these are the dependencies this crate actually
/// has, so this is the complete set of types it *could* leak today. A new
/// dependency adds a row here — and that addition is the moment to ask whether it
/// belongs in the public surface.
const FOREIGN: &[&str] = &["prost::", "bytes::", "rumqttc::", "serde::", "tokio::"];

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn no_public_signature_hands_out_a_foreign_type() {
    let mut files: Vec<PathBuf> = fs::read_dir(src())
        .expect("the crate's source")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    assert!(
        files.len() >= 5,
        "found only {files:?} — the scan is broken and this test would prove nothing"
    );

    let mut offenders = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("source");
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("#") {
                continue;
            }
            // A signature line: something public is being declared on it.
            let declares_public = trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub const fn ")
                || trimmed.starts_with("pub struct ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("pub type ")
                || trimmed.starts_with("pub use ")
                || (trimmed.starts_with("pub ") && trimmed.contains(':'));
            if !declares_public {
                continue;
            }
            for foreign in FOREIGN {
                if trimmed.contains(foreign) {
                    offenders.push(format!(
                        "{}:{}: {trimmed}",
                        file.file_name().expect("named").to_string_lossy(),
                        number + 1
                    ));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a public signature hands a consumer a type from another crate. Matching on \
         it — or naming it — makes them depend on that crate at the version this one \
         pins, and a major release there breaks their code as well as ours:\n{}",
        offenders.join("\n")
    );
}
