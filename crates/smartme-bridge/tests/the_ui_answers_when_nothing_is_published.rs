//! Story 6.1 AC1/AC3/AC4 — the UI answers in the states where the bridge is
//! silent, and `/healthz` does not mistake that silence for a failure.
//!
//! # Why these are the states that matter
//!
//! Since [ADR 0023] every setting but the credential arrives through a browser.
//! So the two states in which nothing is published — no configuration, and an
//! unconfirmed mapping — are exactly the two in which an operator must be able
//! to reach a screen. A server that only ran alongside a healthy session would
//! be absent from every situation it exists for.
//!
//! # And why `/healthz` is tested here rather than as a unit
//!
//! Epic 7 will wire it to a Docker healthcheck that **restarts the container**.
//! A `/healthz` that reported unhealthy merely because nothing was configured
//! would put a fresh deployment into a restart loop and destroy, every few
//! seconds, the screen needed to configure it. That failure is only visible from
//! outside the process, over HTTP, which is what this file does.
//!
//! [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[path = "common/port_lock.rs"]
mod port_lock;

/// A port nobody else in this test binary is using.
///
/// Fixed per case rather than allocated: the bridge reads its port from
/// `config.toml`, so the test has to choose it before the process starts.
fn port_for(case: u16) -> u16 {
    18080 + case
}

fn state_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("smartme_ui_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch dir");
    path
}

/// The smallest HTTP client that can answer the question, so the test does not
/// acquire a dependency to prove a page exists.
fn get(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut body = String::new();
    stream.read_to_string(&mut body).ok()?;
    Some(body)
}

/// Poll until the server answers, or give up.
fn wait_for_ui(port: u16, child: &mut Child) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(answer) = get(port, "/") {
            return Some(answer);
        }
        if child.try_wait().expect("wait").is_some() {
            return None; // it exited; nothing will ever answer
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

fn spawn(dir: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("SMARTME_STATE_DIR", dir)
        .env("SMARTME_CLIENT_ID", "x")
        .env("SMARTME_CLIENT_SECRET", "x")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the bridge binary runs")
}

/// As [`spawn`], but capturing what the bridge says.
///
/// Separate rather than piping in `spawn` itself: an unread pipe fills and blocks
/// the child, and every other test here kills its bridge without reading a word.
fn spawn_listening(dir: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("SMARTME_STATE_DIR", dir)
        .env("SMARTME_CLIENT_ID", "x")
        .env("SMARTME_CLIENT_SECRET", "x")
        // The subscriber's fmt layer writes to STDOUT, so that is where the
        // failure is said.
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the bridge binary runs")
}

fn write_config(dir: &std::path::Path, port: u16, confirmed: bool) {
    std::fs::write(
        dir.join("config.toml"),
        format!(
            "schema_version = {}\n\
             group_id = \"Plant\"\n\
             node_id = \"Bridge01\"\n\
             # TEST-NET-1 (RFC 5737): unroutable, so nothing can connect even if\n\
             # the guard under test failed open.\n\
             broker_host = \"192.0.2.1\"\n\
             broker_port = 1883\n\
             publish_period_secs = 30\n\
             api_base = \"https://192.0.2.1\"\n\
             mapping_confirmed = {confirmed}\n\
             ui_port = {port}\n\
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
}

/// AC1 — a first run, which is the whole reason this story came before a screen.
#[test]
fn with_no_configuration_the_ui_answers_and_says_so() {
    // The lock exists for this port and this test did not take it.
    //
    // At least four other test binaries spawn an unconfigured bridge, which also
    // binds `DEFAULT_PORT`. Without the lock, the loser's bridge logs "the web UI
    // could NOT start" — the exact AC1 failure — and this test is then answered by
    // the OTHER bridge, sees "Not configured yet", and reports green. A test for
    // "the server exists" passing while the server under test did not start.
    let _lock = port_lock::PortLock::acquire(smartme_bridge::ui::DEFAULT_PORT);
    let dir = state_dir("unconfigured");
    // No config.toml at all, so the port must come from the default.
    let mut child = spawn(&dir);
    let answer = wait_for_ui(smartme_bridge::ui::DEFAULT_PORT, &mut child);
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);

    let answer = answer.expect(
        "the UI must answer on the DEFAULT port when there is no configuration — \
         that run has no file to read a port from, and it is the run that needs \
         the screen most",
    );
    assert!(
        answer.contains("Not configured yet"),
        "the page must say which state it is in:\n{answer}"
    );
    assert!(
        answer.contains(env!("CARGO_PKG_VERSION")),
        "AC4: the running version must be on the page:\n{answer}"
    );
}

/// [ADR 0026] — **the state that had no screen at all until 2026-08-06.**
///
/// A configuration that exists and is refused used to kill the process before the
/// server was spawned, so the screen that repairs it was behind the refusal. On a
/// deployment with a restart policy that is a loop, and the commonest way to
/// reach it is the `chown` everybody forgets.
///
/// The assertions are on the FIRST answer the bridge gives, not on a polled one:
/// `wait_for_ui` returns the earliest response it gets. A helper that retried
/// until the page said the right thing would hide a bridge that answered
/// something else first — which is exactly how a "Not configured yet" on the
/// first request survived five green tests and two full local CI runs.
///
/// FALSIFIED 2026-08-06 by restoring the exit — `Decision::Invalid(errors) if
/// first_turn => return Err(...)` — in `main.rs`. Copied from the run:
///
/// ```text
/// test an_invalid_configuration_still_serves_the_screen_that_repairs_it ... FAILED
///
/// thread 'an_invalid_configuration_still_serves_the_screen_that_repairs_it' (251) panicked at
/// crates/smartme-bridge/tests/the_ui_answers_when_nothing_is_published.rs:230:25:
/// ADR 0026: a configuration that cannot be used must not take the screen down with
/// it — the process exited instead of serving
///
/// test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 5 filtered out
/// ```
///
/// It dies in 0.10 s rather than hanging, because `wait_for_ui` gives up the
/// moment the child exits. That is deliberate on its part and worth keeping: the
/// same mutation against a helper that only polled would have gone quiet.
///
/// [ADR 0026]: ../../../docs/adr/0026-a-configuration-it-cannot-use-stops-the-bridge-publishing-not-serving.md
#[test]
fn an_invalid_configuration_still_serves_the_screen_that_repairs_it() {
    let dir = state_dir("misconfigured");
    let port = port_for(7);
    // Parses and matches the schema, so `ui_port` is readable and this test does
    // not have to fall back to the shared default port. Refused by `validate`,
    // because the Sparkplug identity is empty.
    std::fs::write(
        dir.join("config.toml"),
        format!(
            "schema_version = {}\n\
             group_id = \"\"\n\
             node_id = \"\"\n\
             broker_host = \"192.0.2.1\"\n\
             broker_port = 1883\n\
             publish_period_secs = 30\n\
             mapping_confirmed = true\n\
             ui_port = {port}\n\
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

    let mut child = spawn(&dir);
    let answer = wait_for_ui(port, &mut child);
    let health = get(port, "/healthz");
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);

    let answer = answer.expect(
        "ADR 0026: a configuration that cannot be used must not take the screen \
         down with it — the process exited instead of serving",
    );
    assert!(
        answer.contains("The saved configuration is not usable"),
        "the page must name this state, and not borrow another's words:\n{answer}"
    );
    assert!(
        !answer.contains("Not configured yet") && !answer.contains("confirm the meter mapping"),
        "a refused configuration is neither absent nor merely unconfirmed, and an \
         operator who cannot tell the three apart cannot act:\n{answer}"
    );
    assert!(
        !answer.contains("still running on the configuration it started with"),
        "this state is reached at STARTUP now, where nothing was ever published — \
         the old wording described a bridge that was running:\n{answer}"
    );

    let health = health.expect("/healthz must answer in this state too");
    assert!(
        health.contains("200 OK"),
        "ADR 0026: Epic 7 wires this endpoint to a container restart, and a restart \
         cannot repair a configuration fault — a 503 here would destroy the screen \
         that can, every few seconds:\n{health}"
    );
    assert!(
        health.contains("\"intends_to_publish\":false"),
        "a 200 must not be mistaken for a bridge that is working: the body has to \
         say nothing is being published:\n{health}"
    );
}

/// AC1 — and the unconfirmed state must not describe itself in the same words.
#[test]
fn with_an_unconfirmed_mapping_the_ui_answers_differently() {
    let dir = state_dir("unconfirmed");
    let port = port_for(1);
    write_config(&dir, port, false);
    let mut child = spawn(&dir);
    let answer = wait_for_ui(port, &mut child);
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);

    let answer = answer.expect("the UI must answer while the mapping is unconfirmed");
    assert!(
        answer.contains("confirm the meter mapping"),
        "the page must say what is missing, and it is not the configuration:\n{answer}"
    );
    assert!(
        !answer.contains("Not configured yet"),
        "the two silent states must not share a message — an operator who cannot \
         tell them apart cannot act, and only one is a click from fixed:\n{answer}"
    );
}

/// AC3 — **the expensive one.** A deliberate silence must not be reported as a
/// failure to something that restarts containers.
#[test]
fn healthz_does_not_call_a_deliberate_silence_unhealthy() {
    let dir = state_dir("healthz");
    let port = port_for(2);
    write_config(&dir, port, false);
    let mut child = spawn(&dir);

    // The premise: the server is up at all.
    let up = wait_for_ui(port, &mut child).is_some();
    let health = get(port, "/healthz");
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(up, "the premise of this test is a server that answers");
    let health = health.expect("/healthz must answer");

    assert!(
        health.contains("200 OK"),
        "an unconfirmed bridge must answer 200. Epic 7 wires this to a Docker \
         healthcheck that RESTARTS the container: reporting unhealthy here would \
         put a fresh deployment in a restart loop and destroy, every few seconds, \
         the screen needed to configure it. Answer was:\n{health}"
    );
    // And the body still says it is not doing anything — the status code answers
    // "is this process worth keeping", the body answers "is it working".
    assert!(
        health.contains("\"intends_to_publish\":false"),
        "200 must not mean 'working': the body has to say the bridge intends to \
         publish nothing, or the endpoint has collapsed two different questions \
         or the endpoint has collapsed two different questions into one:\n{health}"
    );
    assert!(
        health.contains("\"status\":\"unconfirmed\""),
        "and it must say WHICH silence:\n{health}"
    );
    // AC4's machine-facing half, unasserted anywhere until 2026-08-05: deleting
    // both fields from the JSON left the whole suite green.
    //
    // Named with their keys, deliberately. `CONTRACT_VERSION` is 3 and the
    // package version is 0.3.1, so a naive `contains("3")` would pass on the
    // version string alone — the hollow-assertion shape this file already
    // carries three records of.
    assert!(
        health.contains(&format!("\"version\":\"{}\"", env!("CARGO_PKG_VERSION"))),
        "AC4: /healthz must carry the running version:\n{health}"
    );
    assert!(
        health.contains(&format!(
            "\"contract\":{}",
            smartme_bridge::adapters::sparkplug_publisher::CONTRACT_VERSION
        )),
        "AC4: and the contract version, which is what a consumer actually sees \
         when a tag looks wrong in Ignition:\n{health}"
    );
}

/// AC5, half of it — and the honest half.
///
/// A port already taken must cost the UI and nothing else. The bridge keeps
/// running, says why on the console, and an operator loses a screen rather than
/// their meters: the same rule file logging follows, where a diagnostic aid that
/// can stop the bridge has stopped being an aid.
///
/// **The other half of AC5 — a panicking handler — is NOT asserted here**, and
/// is recorded as unmet in the story rather than implied by this test. Proving it
/// needs a route that panics, which means shipping one. What holds it up
/// meanwhile is structural: the server is a spawned task, so a panic there kills
/// that task alone.
#[test]
fn a_taken_port_does_not_cost_the_meters() {
    let dir = state_dir("port-taken");
    let port = port_for(3);
    // Hold the port first, so the bridge cannot have it.
    let squatter = std::net::TcpListener::bind(("0.0.0.0", port)).expect("hold the port");

    write_config(&dir, port, true);
    let mut child = spawn_listening(&dir);

    // The premise: the process is up despite the port being unavailable. It is
    // asserted by outliving a wait long enough for a bind to have failed.
    std::thread::sleep(Duration::from_secs(3));
    let still_running = child.try_wait().expect("wait").is_none();

    let _ = child.kill();
    let output = child.wait_with_output().expect("wait");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    drop(squatter);
    let _ = std::fs::remove_dir_all(&dir);

    // AC5's SECOND clause, unasserted until 2026-08-05: the failure must be
    // traced, at a level the default filter shows. `stderr` went to /dev/null,
    // so deleting the `tracing::error!` kept the suite green — and the operator
    // would meet a dead URL with an empty log, which is the whole point of the
    // clause.
    assert!(
        said.contains("the web UI could NOT start"),
        "the bridge must SAY the UI is absent, or an operator meets a dead \
         address and no explanation. stderr was:\n{said}"
    );
    assert!(
        still_running,
        "the bridge exited because the web UI could not bind. A diagnostic aid \
         that can stop the meters has stopped being an aid — the UI must degrade \
         to absent and say so, never propagate"
    );
}

/// AC1's fourth row — **untested until a review pointed it out on 2026-08-04**.
///
/// Three tests fetched from the two silent states and one squatted the port, so
/// nothing had ever asked a RUNNING bridge for a page. `Lifecycle::Running`'s
/// text was covered only by a string-distinctness unit test, and `/healthz`'s
/// running body by nothing at all — which is part of why it shipped claiming to
/// be publishing.
///
/// The broker is unroutable on purpose: the bridge reaches Running, tries to
/// connect, and fails. That is the case the endpoint must describe honestly.
#[test]
fn a_running_bridge_serves_its_page_and_an_honest_health_body() {
    let dir = state_dir("running");
    let port = port_for(4);
    write_config(&dir, port, true);
    let mut child = spawn(&dir);

    let page = wait_for_ui(port, &mut child);
    let health = get(port, "/healthz");
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);

    let page = page.expect("a running bridge must serve its page too");
    assert!(
        page.contains("Running"),
        "the page must say which state it is in:\n{page}"
    );

    let health = health.expect("/healthz must answer");
    assert!(
        health.contains("200 OK"),
        "a bridge whose broker is unreachable is working correctly and saying so \
         — an honest STALE, which must never trigger a restart:\n{health}"
    );
    assert!(
        health.contains("\"status\":\"running\""),
        "and it must say which state:\n{health}"
    );
    assert!(
        !health.contains("\"publishing\":true"),
        "the endpoint must not claim to be PUBLISHING. The broker here is \
         unroutable and nothing has reached the wire; what the bridge can \
         honestly assert is what it INTENDS. Answer was:\n{health}"
    );
    assert!(
        health.contains("\"intends_to_publish\":true"),
        "and it must assert that much:\n{health}"
    );
}
