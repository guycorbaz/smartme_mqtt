//! Story 5.2 AC6 — the smart-me secret reaches no log line, from the real binary.
//!
//! # Why the real binary, and not a unit test
//!
//! `app::config` already asserts that no *fault* carries the secret, and
//! `app::store` asserts that no *file* does. Neither covers the case that
//! actually leaks: a `tracing::debug!(?something)` on a struct that happens to
//! hold the credential, anywhere in the process, on a path no unit test walks.
//! Story 1.6's review found exactly that — a derived `Debug` printing
//! `client_secret: Some("…")` in a panic message — and it was found by looking,
//! not by a test.
//!
//! So this runs the whole binary with a distinctive secret and searches
//! everything it says.
//!
//! # The guard, which is the point of the file
//!
//! *"The secret does not appear in the output"* is an absence assertion, and this
//! project has shipped ones that held over an empty stream. Two guards, both
//! required before the absence is allowed to mean anything:
//!
//! 1. **The output is non-empty and recognisable** — the process really ran and
//!    really logged.
//! 2. **The search finds the secret when it IS there.** The same matcher is run
//!    over the same text with one line of deliberate leak spliced in, and must
//!    find it. A matcher that cannot find a secret in a document containing one
//!    proves nothing about a document that does not.

use std::io::Read as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Distinctive enough that a match cannot be a coincidence, and shaped like a
/// real credential rather than like a word that might occur in prose.
const SECRET: &str = "s3cr3t-QZX7-do-not-print-9f41";

/// The one matcher, used for both the absence and its guard.
///
/// A separate matcher for each would be two things that can disagree — and the
/// one that only ever runs against clean output would never be exercised.
fn leaks(haystack: &str) -> bool {
    haystack.contains(SECRET)
}

fn state_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("smartme_secret_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch dir");
    path
}

/// Run the bridge for a few seconds against an unroutable broker and cloud, and
/// return everything it wrote to stdout and stderr.
///
/// TEST-NET-1 (RFC 5737) for the cloud so no request can succeed, and a broker
/// that does not answer — the interesting paths for a leak are startup,
/// configuration, and the failures that follow, all of which happen here.
fn everything_it_said() -> String {
    let dir = state_dir("log");
    std::fs::write(
        dir.join("config.toml"),
        format!(
            "schema_version = {}\n\
             group_id = \"Group\"\n\
             node_id = \"Node\"\n\
             broker_host = \"192.0.2.1\"\n\
             broker_port = 1883\n\
             publish_period_secs = 5\n\
             api_base = \"https://192.0.2.1\"\n\
             \n\
             [[meters]]\n\
             meter_id = \"garage\"\n\
             device_id = \"a1a1a1a1-b2b2-c3c3-d4d4-000000000001\"\n\
             serial = \"9202685\"\n\
             enabled = true\n",
            smartme_bridge::app::store::SCHEMA_VERSION
        ),
    )
    .expect("write config");

    let out_path = dir.join("out.txt");
    let out = std::fs::File::create(&out_path).expect("stdout sink");
    let err = out.try_clone().expect("stderr sink");

    let mut child = Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("SMARTME_STATE_DIR", &dir)
        .env("SMARTME_CLIENT_ID", "client-id-not-secret")
        .env("SMARTME_CLIENT_SECRET", SECRET)
        // TRACE, deliberately: a leak that only shows at a level nobody runs is
        // still a leak, and this is the one place it can be looked for cheaply.
        // Running at the default filter would test the filter, not the code.
        .env("RUST_LOG", "trace")
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .expect("the bridge binary runs");

    // Long enough for a connect attempt and a poll tick to fail.
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
        if child.try_wait().expect("wait").is_some() {
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();

    let mut said = String::new();
    std::fs::File::open(&out_path)
        .expect("open output")
        .read_to_string(&mut said)
        .expect("read output");
    let _ = std::fs::remove_dir_all(&dir);
    said
}

/// FALSIFIED 2026-08-04 by reproducing the historical defect exactly: the
/// hand-written `Debug` on `Credential` replaced by a derive, plus the one line
/// somebody plausibly adds next to it —
/// `tracing::debug!(?credential, "credential read from the environment")`.
///
/// ```text
/// test the_smart_me_secret_appears_in_nothing_the_bridge_says ... FAILED
/// the smart-me client secret reached the log.
/// DEBUG smartme_bridge: credential read from the environment
///   credential=Credential { client_id: Some("client-id-not-secret"),
///                           client_secret: Some("s3cr3t-QZX7-do-not-print-9f41") }
/// ```
///
/// Note that NEITHER half leaks on its own: the derive alone is harmless while
/// nothing formats the struct, and the trace alone is harmless while the
/// hand-written `Debug` stands. That is why this is a test about the process and
/// not a rule about a derive.
#[test]
fn the_smart_me_secret_appears_in_nothing_the_bridge_says() {
    let said = everything_it_said();

    // GUARD 1 — the process ran and logged. Without this the absence below
    // would hold over an empty file.
    assert!(
        said.contains("smartme_mqtt starting"),
        "the bridge produced no recognisable output, so the absence assertion \
         below would hold over nothing. It said:\n{said}"
    );
    assert!(
        said.len() > 200,
        "only {} bytes of output; a leak hides in the paths this run never \
         reached:\n{said}",
        said.len()
    );

    // GUARD 2 — the matcher works. Same function, same text, one line of
    // deliberate leak spliced in.
    let leaked = format!("{said}\nDEBUG pretend_module: raw config: client_secret={SECRET}\n");
    assert!(
        leaks(&leaked),
        "the search cannot find the secret in a document that CONTAINS it, so \
         its failure to find one in the real output means nothing"
    );

    // And only now the assertion this file exists for.
    assert!(
        !leaks(&said),
        "the smart-me client secret reached the log. ADR 0019 makes 'never \
         rendered, never traced' a property of the product, not a habit of \
         whoever writes the next error message — and a derived Debug on any \
         struct holding it is enough to break it silently. Output:\n{said}"
    );
}
