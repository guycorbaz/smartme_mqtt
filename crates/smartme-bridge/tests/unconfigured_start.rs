//! A bridge with no configuration comes up, says so, and stays up
//! (Story 5.2 AC2, [ADR 0023] §5).
//!
//! # Why this must run the real binary
//!
//! Every setting but the credential arrives through a browser, so a bridge that
//! refused to start unconfigured could never *be* configured — the screen that
//! writes the first `config.toml` would be behind a process that exits before
//! serving it. The property is therefore about the process, not about a function:
//! it starts, it keeps running, and it puts nothing on the wire.
//!
//! # The trap this test is written around
//!
//! *"It did not exit"* is satisfied by a binary that hangs for any reason at all,
//! including one that has wedged. And *"it published nothing"* is satisfied by a
//! bridge that could never publish under any circumstances. Both are absence
//! assertions over a stream that may never have carried anything — the shape that
//! has already produced a false pass in this project, where every chaos test
//! pointed at TEST-NET-1 and *"no DATA appeared"* held over an empty stream.
//!
//! So each absence here is paired with a **presence** first: it did not exit
//! **and** it said the specific thing an unconfigured bridge says, so the
//! process is alive on purpose rather than stuck.
//!
//! # What this file does NOT test, corrected 2026-08-04
//!
//! **It never touches a broker**, and a doc comment here claimed otherwise — that
//! *"it opened no session and the same harness is shown, in `startup_banner`, to
//! observe a bridge that does reach its configuration"*. Both halves were false:
//! there was no session assertion in this file at all, and `startup_banner` is a
//! different harness that reads the stdout of a process exiting on a validation
//! fault, which demonstrates nothing about a subscriber's ability to see a
//! CONNECT.
//!
//! The wire half of that acceptance criterion lives in
//! `unconfirmed_publishes_nothing.rs`, which has a real broker and an independent
//! subscriber, and pairs its silence with a birth on the same harness.
//!
//! [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md

use std::io::Read as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// An empty state directory: no `config.toml`, which is a first run.
fn empty_state_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "smartme_unconfigured_{}_{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch dir");
    path
}

/// Spawn the bridge against `dir`, wait for it to say something, and return what
/// it said together with whether it was still running.
///
/// stdout goes to a file rather than a pipe: reading a pipe from a process that
/// is designed never to exit blocks the reader, and a test that blocks is a test
/// that reports nothing.
fn run_briefly(dir: &std::path::Path) -> (String, bool) {
    let log = dir.join("stdout.txt");
    let sink = std::fs::File::create(&log).expect("stdout sink");

    let mut child = Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("SMARTME_STATE_DIR", dir)
        // The UI port for a run with no `ui_port` in its file ([ADR 0037]).
        // NOT 8080: that is a deployment contract, and it is shared with other
        // projects on this machine. Nothing here tests the UI; the variable
        // exists so no test binary reaches for a port it does not own.
        .env("SMARTME_UI_PORT", "18100")
        // The credential is present and irrelevant: with no configuration there
        // is nothing for it to authenticate against. Supplying it rules out
        // "it stayed up because it was waiting for a credential".
        .env("SMARTME_CLIENT_ID", "x")
        .env("SMARTME_CLIENT_SECRET", "x")
        .stdout(Stdio::from(sink))
        .stderr(Stdio::null())
        .spawn()
        .expect("the bridge binary runs");

    // Poll rather than sleep-once: the assertion is about what the process is
    // doing, and a fixed sleep either wastes time or races the first write.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut said = String::new();
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        said.clear();
        if let Ok(mut f) = std::fs::File::open(&log) {
            let _ = f.read_to_string(&mut said);
        }
        if said.contains("no configuration yet") {
            break;
        }
        if child.try_wait().expect("wait").is_some() {
            break;
        }
    }

    let still_running = child.try_wait().expect("wait").is_none();
    let _ = child.kill();
    let _ = child.wait();
    (said, still_running)
}

/// FALSIFIED 2026-08-04. `run_unconfigured` mutated to return instead of
/// awaiting the shutdown signal (`let _ = runtime; Ok(())`):
///
/// ```text
/// test a_bridge_with_no_configuration_comes_up_and_stays_up ... FAILED
/// the bridge exited without a configuration. Every setting but the credential
/// arrives through the web UI, so a bridge that exits here can never be
/// configured at all. It said:
/// … INFO smartme_bridge: smartme_mqtt starting version="0.3.1" contract=3
/// … INFO smartme_bridge: no configuration yet — the bridge is up and will
/// publish nothing until one exists.
/// ```
///
/// Note what the failure output shows: the two PRESENCE assertions passed under
/// the mutation and only the absence one fell. That is the guard working — the
/// process really had started and really had said its piece, so "it exited" is
/// the only thing this is reporting.
#[test]
fn a_bridge_with_no_configuration_comes_up_and_stays_up() {
    let dir = empty_state_dir("stays-up");
    let (said, still_running) = run_briefly(&dir);

    // PRESENCE FIRST. Without this, "it did not exit" would also be true of a
    // binary wedged on something else entirely.
    assert!(
        said.contains("no configuration yet"),
        "the bridge must say why it is publishing nothing, at a level visible \
         under the DEFAULT filter — an operator who sees an empty log reads it as \
         'nothing to report' rather than 'nothing is being reported'. It said:\n{said}"
    );
    assert!(
        said.contains("smartme_mqtt starting"),
        "the banner must precede it, as in every other start:\n{said}"
    );

    // And only then the absence.
    assert!(
        still_running,
        "the bridge exited without a configuration. Every setting but the \
         credential arrives through the web UI, so a bridge that exits here can \
         never be configured at all. It said:\n{said}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// FALSIFIED 2026-08-04. `main` mutated to `store::save` a default
/// `StoredConfig` on the unconfigured path before waiting:
///
/// ```text
/// test an_unconfigured_bridge_writes_nothing_into_the_state_directory ... FAILED
/// an unconfigured start wrote a config.toml. Defaults nobody chose are still
/// nobody's configuration. It said:
/// … INFO smartme_bridge: no configuration yet — the bridge is up and will
/// publish nothing until one exists.
/// ```
#[test]
fn an_unconfigured_bridge_writes_nothing_into_the_state_directory() {
    let dir = empty_state_dir("no-writes");
    let (said, _) = run_briefly(&dir);

    // A first run must not invent a configuration. Writing defaults here would
    // make the next start "configured" with settings nobody chose, and the
    // operator's first act in the UI would be to correct a file they never wrote.
    assert!(
        !dir.join("config.toml").exists(),
        "an unconfigured start wrote a config.toml. Defaults nobody chose are \
         still nobody's configuration. It said:\n{said}"
    );

    // The guard on that absence: the directory IS writable and the process DID
    // run, so "no file appeared" is not an artefact of either.
    assert!(
        dir.join("stdout.txt").exists() && said.contains("no configuration yet"),
        "the assertion above needs a directory the process could have written to \
         and a process that actually ran:\n{said}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
