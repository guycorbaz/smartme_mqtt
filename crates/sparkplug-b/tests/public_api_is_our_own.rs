//! Story 8.2 AC1 — nothing this crate writes by hand may hand a consumer somebody
//! else's type.
//!
//! **Why a test and not a review.** `decode` returned `prost::DecodeError` from the
//! day it was written until 2026-08-20, through every review this crate has had. It
//! was not an oversight anybody could see: the signature reads perfectly well, and
//! the cost is invisible until a consumer tries to match on the error and finds
//! themselves pinned to this crate's `prost` version.
//!
//! # Why this test was rewritten on 2026-08-21
//!
//! Its first version searched each `pub` line for the literal `prost::`. That is the
//! shape the original defect happened to have, and **only** that shape. The review of
//! story 8.2 put two mutations past it and the suite stayed green:
//!
//! ```text
//! use prost::DecodeError as ProstDecodeError;
//! pub fn decode_aliased(bytes: &[u8]) -> Result<Payload, ProstDecodeError> { … }
//!
//! pub fn decode_wrapped(
//!     bytes: &[u8],
//! ) -> Result<Payload, prost::DecodeError> { … }
//! ```
//!
//! The first is how a Rust author normally writes an imported type — bare, with the
//! path at the top of the file. The second is what rustfmt does to any signature past
//! the line width. So the guard was blind to the ordinary case and awake only to the
//! accidental one.
//!
//! It now reads a declaration as a whole statement rather than a line, and it judges
//! **names in scope** rather than spellings: what a file's `use` lines bring in from
//! another crate may not appear in a signature at all, under any spelling.
//!
//! # The one exemption, and why it is not a loophole
//!
//! `protobuf::Payload` and its neighbours are generated **in this crate**, by its own
//! `build.rs`, from the specification's committed `.proto`. They are the Sparkplug
//! payload itself. They implement `prost::Message`, which makes `prost` a **public
//! dependency** — a fact the README states plainly rather than leaving a consumer to
//! discover from a compiler error. The generated module lives in `OUT_DIR`, is named
//! in [`OUR_ROOTS`] and is never scanned here: it is not hand-written, and there is
//! nothing for its author to decide.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Path roots that are nobody else's crate.
///
/// `protobuf` is the generated module — AC1's exemption, by name. Everything else
/// here is either the language's own prelude of paths or the standard library, which
/// no consumer can be pinned to by us. **A crate's own module names are added to this
/// set at run time from the file stems**, so a new module needs no edit here — and a
/// new *dependency* needs none either, because this is an allow-list of what is ours
/// rather than a deny-list of the world. The first version was the other way round,
/// which meant it could only catch a leak somebody had already thought of.
const OUR_ROOTS: &[&str] = &[
    "crate", "self", "super", "Self", "std", "core", "alloc", "protobuf",
];

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("the crate's source")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            // Recursive on purpose: the first version read the top level only, so a
            // future `src/foo/mod.rs` would have gone unscanned in silence.
            rust_files(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

/// What one `use` line brings into a file: the crate it comes from, and the names it
/// makes available without a path.
struct Import {
    root: String,
    names: Vec<String>,
    glob: bool,
}

fn leaf(item: &str) -> String {
    let item = item.trim();
    // An alias is the name that ends up in scope: `X as Y` puts `Y` there, not `X`.
    let item = match item.rsplit_once(" as ") {
        Some((_, alias)) => alias.trim(),
        None => item,
    };
    item.rsplit("::").next().unwrap_or(item).trim().to_string()
}

fn parse_use(line: &str) -> Option<Import> {
    let rest = line.trim().strip_prefix("pub ").unwrap_or(line.trim());
    let rest = rest
        .strip_prefix("use ")?
        .trim()
        .trim_end_matches(';')
        .trim();
    let root = rest
        .split("::")
        .next()
        .unwrap_or(rest)
        .trim()
        .trim_start_matches('{')
        .trim()
        .to_string();
    let (list, prefix) = match (rest.find('{'), rest.rfind('}')) {
        (Some(open), Some(close)) if close > open => (&rest[open + 1..close], &rest[..open]),
        _ => (rest, ""),
    };
    let mut glob = false;
    let mut names = Vec::new();
    for item in list.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if item.ends_with('*') {
            glob = true;
            continue;
        }
        if item == "self" {
            // `use std::fmt::{self, Display}` — `self` is the module itself.
            names.push(leaf(prefix.trim_end_matches("::")));
            continue;
        }
        names.push(leaf(item));
    }
    Some(Import { root, names, glob })
}

/// Every identifier in `text`, with what sits either side of it.
fn identifiers(text: &str) -> Vec<(String, bool, bool)> {
    let bytes: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_alphabetic() || bytes[i] == '_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_alphanumeric() || bytes[i] == '_') {
                i += 1;
            }
            let name: String = bytes[start..i].iter().collect();
            let preceded = start >= 2 && bytes[start - 1] == ':' && bytes[start - 2] == ':';
            let followed = i + 1 < bytes.len() && bytes[i] == ':' && bytes[i + 1] == ':';
            out.push((name, preceded, followed));
        } else {
            i += 1;
        }
    }
    out
}

#[derive(PartialEq)]
enum Kind {
    Use,
    Item,
    Field,
}

fn declaration_kind(trimmed: &str) -> Option<Kind> {
    if trimmed.starts_with("pub use ") {
        return Some(Kind::Use);
    }
    const ITEMS: &[&str] = &[
        "pub fn ",
        "pub const fn ",
        "pub async fn ",
        "pub unsafe fn ",
        "pub struct ",
        "pub enum ",
        "pub union ",
        "pub trait ",
        "pub type ",
        "pub const ",
        "pub static ",
    ];
    if ITEMS.iter().any(|item| trimmed.starts_with(item)) {
        return Some(Kind::Item);
    }
    // A public field: `pub inner: Whatever,`
    if trimmed.starts_with("pub ") && trimmed.contains(':') {
        return Some(Kind::Field);
    }
    None
}

/// The public declarations in one file, each joined into a single statement.
///
/// **Joined, not read line by line** — that is the repair of 2026-08-21. rustfmt puts
/// a long return type on its own line, and a guard that only looks at the line
/// carrying `pub fn` never sees it.
fn public_declarations(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("//") || trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        let Some(kind) = declaration_kind(trimmed) else {
            i += 1;
            continue;
        };
        let first = i;
        let mut head = String::new();
        // 40 lines is far more than any signature here and stops a malformed file
        // from running the scanner off the end.
        for _ in 0..40 {
            if i >= lines.len() {
                break;
            }
            let line = lines[i].trim();
            i += 1;
            if line.starts_with("//") {
                continue;
            }
            head.push_str(line);
            head.push(' ');
            let done = match kind {
                // Until the semicolon: a `pub use` may span lines inside braces.
                Kind::Use => line.ends_with(';'),
                // Until the body opens, or the declaration ends without one.
                Kind::Item => head.contains('{') || line.ends_with(';'),
                Kind::Field => true,
            };
            if done {
                break;
            }
        }
        out.push((first + 1, head.trim().to_string()));
    }
    out
}

/// Reports every public declaration in `text` that names a type this crate does not own.
fn offenders_in(text: &str, our_roots: &BTreeSet<String>) -> Vec<(usize, String, String)> {
    let mut foreign_names = BTreeSet::new();
    let mut our_names = BTreeSet::new();
    let mut offenders = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let Some(import) = parse_use(line) else {
            continue;
        };
        let ours = our_roots.contains(&import.root);
        if import.glob && !ours {
            // A glob from another crate puts names in scope that no reading of this
            // file can enumerate, so nothing below can be judged. It is refused at
            // the import rather than guessed at in the signatures.
            offenders.push((
                number + 1,
                line.trim().to_string(),
                format!(
                    "`{}::*` puts unnameable foreign types in scope",
                    import.root
                ),
            ));
        }
        for name in import.names {
            if ours {
                our_names.insert(name);
            } else {
                foreign_names.insert(name);
            }
        }
    }

    for (line, head) in public_declarations(text) {
        let mut why: Option<String> = None;
        for (name, preceded, followed) in identifiers(&head) {
            if preceded {
                // A segment inside a path; its root has already been judged.
                continue;
            }
            if followed {
                let first_is_lower = name.chars().next().is_some_and(char::is_lowercase);
                if first_is_lower && !our_roots.contains(&name) && !our_names.contains(&name) {
                    why = Some(format!("`{name}::…` is another crate's path"));
                    break;
                }
            }
            if foreign_names.contains(&name) {
                let source = "brought in by a `use` from another crate";
                why = Some(format!("`{name}` is {source}"));
                break;
            }
        }
        if let Some(why) = why {
            offenders.push((line, head, why));
        }
    }
    offenders
}

#[test]
fn no_public_signature_hands_out_a_foreign_type() {
    let mut files = Vec::new();
    rust_files(&src(), &mut files);
    assert!(
        files.len() >= 5,
        "found only {files:?} — the scan is broken and this test would prove nothing"
    );

    let mut our_roots: BTreeSet<String> = OUR_ROOTS.iter().map(|r| (*r).to_string()).collect();
    for file in &files {
        // The crate's own modules are named by their files, and `lib.rs` refers to
        // them without a `crate::` prefix.
        our_roots.insert(
            file.file_stem()
                .expect("named")
                .to_string_lossy()
                .into_owned(),
        );
    }

    let mut examined = 0;
    let mut offenders = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("source");
        examined += public_declarations(&text).len();
        let name = file
            .file_name()
            .expect("named")
            .to_string_lossy()
            .into_owned();
        for (line, head, why) in offenders_in(&text, &our_roots) {
            offenders.push(format!("{name}:{line}: {why}\n    {head}"));
        }
    }

    // A parser that silently stopped working would otherwise report a clean crate.
    assert!(
        examined >= 30,
        "only {examined} public declarations were read; this crate has many more, so \
         the scanner is broken and its silence means nothing"
    );

    assert!(
        offenders.is_empty(),
        "a public declaration hands a consumer a type from another crate. Matching on \
         it — or naming it — makes them depend on that crate at the version this one \
         pins, and a major release there breaks their code as well as ours:\n{}",
        offenders.join("\n")
    );
}

/// The three shapes the first version of this guard let through, kept as a fixture.
///
/// **This is the falsification, run on every CI pass rather than once by hand.** The
/// project's rule is that a test which cannot be made to fail is not yet a test; a
/// mutation performed once proves that only for the code as it stood that day. Here
/// the scanner is asked, every time, to still catch what it was blind to — and `d`
/// proves it has not simply started shouting at everything.
const EVASIONS: &str = r#"
use prost::DecodeError as ProstDecodeError;
use prost::*;
use std::fmt;

pub fn a(bytes: &[u8]) -> Result<Payload, ProstDecodeError> {
}

pub fn b(
    bytes: &[u8],
) -> Result<Payload, prost::DecodeError> {
}

pub struct C {
    pub inner: prost::DecodeError,
}

pub fn d(text: &str) -> fmt::Result {
}
"#;

#[test]
fn the_scanner_still_catches_what_it_was_blind_to() {
    let our_roots: BTreeSet<String> = OUR_ROOTS.iter().map(|r| (*r).to_string()).collect();
    let offenders = offenders_in(EVASIONS, &our_roots);
    let found: Vec<&str> = offenders.iter().map(|(_, head, _)| head.as_str()).collect();

    assert_eq!(
        offenders.len(),
        4,
        "expected the glob import, the aliased import, the wrapped signature and the \
         public field to be caught, and `fmt::Result` — a standard-library alias — to \
         be left alone; got {found:#?}"
    );
    assert!(
        found.iter().any(|h| h.contains("use prost::*")),
        "the glob import went unseen: {found:#?}"
    );
    assert!(
        found.iter().any(|h| h.contains("pub fn a")),
        "the aliased import went unseen: {found:#?}"
    );
    assert!(
        found.iter().any(|h| h.contains("pub fn b")),
        "the wrapped signature went unseen: {found:#?}"
    );
    assert!(
        found.iter().any(|h| h.contains("pub inner")),
        "the public field went unseen: {found:#?}"
    );
    assert!(
        !found.iter().any(|h| h.contains("pub fn d")),
        "`fmt::Result` is the standard library, not a crate a consumer can be pinned \
         to: {found:#?}"
    );
}
