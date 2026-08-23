//! Story 7.1 — `--healthcheck`: the probe [#56] said this image could not run.
//!
//! **What [#56] found on 2026-08-05**: the runtime image has no `curl`, no `wget`
//! and a shell that is not bash, so nothing inside the container could consume
//! `/healthz`. AR12's restart could not fire in a real deployment, and the non-200
//! path had never been exercised the way a deployment exercises it.
//!
//! [ADR 0041] decides that the binary probes itself. These tests run the REAL
//! binary — `CARGO_BIN_EXE_smartme-bridge` — against a real `ui::serve`, because a
//! unit test of the probe function would assert against nobody's deployment, which
//! is the defect [#56] recorded in the first place.
//!
//! [ADR 0041]: ../../../docs/adr/0041-the-healthcheck-is-the-binary-probing-itself.md

mod common;

use std::process::Command;
use std::sync::Arc;

use smartme_bridge::ui;

fn probe(port: u16, state_dir: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .arg("--healthcheck")
        .env("SMARTME_UI_PORT", port.to_string())
        .env("SMARTME_STATE_DIR", state_dir)
        .output()
        .expect("the probe runs")
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("smartme_probe_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The harness guard: something holding the port is NOT the UI answering.
///
/// # Why this is the assertion that matters
///
/// The bind race is closed by retrying, and a retry is only as good as the test
/// that decides an attempt failed. The obvious test — `TcpStream::connect`
/// succeeds — is the one that cannot work here, because the whole failure being
/// guarded against is *another process holding this port*: it accepts the
/// connection, never answers, and a connect-based wait declares victory and
/// hands back a port the UI is not on. That is the same false pass the old wait
/// loop in this file could produce.
///
/// A bare listener that never accepts is exactly that squatter: the kernel
/// completes the handshake into its backlog, so the connection succeeds and no
/// byte ever comes back.
///
/// **FALSIFIED 2026-08-23** — `ui_answers` rewritten as
/// `std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()`, which is how one
/// would ordinarily write "is it up yet": RED here, and green on every other test
/// in this file. The mutation is the shipped shape of the wait it replaces.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_holding_the_port_is_not_the_ui_answering() {
    // Bound and never accepted, and kept alive by this binding for the whole test.
    let squatter = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("the kernel has a port");
    let port = squatter
        .local_addr()
        .expect("a bound listener has an address")
        .port();

    assert!(
        !common::ui_answers(port, std::time::Duration::from_secs(2)).await,
        "a port that accepts connections and answers nothing must NOT count as \
         the UI being up: taking it for the UI is how the harness hands a test a \
         port the bridge was never on, and every assertion afterwards indicts the \
         bridge for it"
    );
}

/// **AC1 and AC2's healthy half** — a bridge that answers is healthy, including
/// one that is deliberately publishing nothing.
///
/// **The silent phase is the case worth testing, not an afterthought.** A bridge
/// with no configuration publishes nothing by design; a probe that called that
/// unhealthy would put a first run into a restart loop and destroy the very screen
/// the operator needs to configure it.
///
/// FALSIFIED 2026-08-20 — mutation RUN, output copied: making the probe exit 1 on
/// any status but 200 **including** 200 (`!status.is_success()`) goes red with
///
/// ```text
/// a bridge that answers on /healthz is healthy — even one publishing nothing on
/// purpose: exit code Some(1), stderr: ""
/// ```
/// **`multi_thread` is load-bearing.** `Command::output()` blocks the calling
/// thread, and on the default single-threaded test runtime it would block the very
/// runtime serving `/healthz` — the probe would then fail to connect and the test
/// would report a defect in the bridge where there was one in the harness. That is
/// the harness failure Epic 4's action E2 was written about.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bridge_that_answers_is_healthy_even_when_it_publishes_nothing() {
    let dir = scratch("healthy");
    // THROUGH THE HARNESS, and the reason is on the record: this test went red on
    // the pre-push hook of 2026-08-23 and green on every re-run, blaming the
    // bridge for a port the harness had lost between `an_unused_host_port` and
    // `ui::serve`. `serve_ui_on_a_free_port` retries and waits for an ANSWER
    // rather than for a connection — a squatter accepts the connection too.
    let dir_for_state = dir.clone();
    let port = common::serve_ui_on_a_free_port(move || {
        ui::UiState::new(
            ui::Phase::silent(ui::Lifecycle::Unconfigured).into_handle(),
            dir_for_state.clone(),
            Arc::new(smartme_bridge::core::SystemClock::new()),
            Arc::new(tokio::sync::Notify::new()),
        )
    })
    .await;

    let output = probe(port, &dir);
    assert_eq!(
        output.status.code(),
        Some(0),
        "a bridge that answers on /healthz is healthy — even one publishing nothing \
         on purpose: exit code {:?}, stderr: {:?}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **AC2's unhealthy half in its FIRST form: the endpoint answers, and says no.**
///
/// **Added by the review of this story, 2026-08-20, and it is the gap that
/// mattered.** The story shipped with the probe tested against a healthy endpoint
/// and against no endpoint at all — and never against a `/healthz` that returns
/// non-200, which is *the one state the whole feature exists for*. AR12's restart
/// fires on exactly that, and nothing exercised it.
///
/// **A hand-written server rather than a wedged bridge**, deliberately: the unit
/// under test is the probe's verdict, and `/healthz`'s own decision to return 503
/// has its own tests (`a_wedged_poll_loop_is_unhealthy_and_a_slow_one_is_not`, and
/// `chaos_poller_wedge` end to end). Building a wedged bridge here would test that
/// decision a third time and the probe not at all.
///
/// FALSIFIED 2026-08-20 — mutation RUN, output copied: treating any answer as
/// healthy (`Ok(_) => exit(0)`) goes red with
///
/// ```text
/// a 503 from /healthz is the ONE state a restart repairs, and the probe reported
/// healthy: exit code Some(0)
/// ```
#[test]
fn a_bridge_that_answers_503_is_unhealthy() {
    let dir = scratch("wedged");
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a port");
    let port = listener.local_addr().expect("addr").port();

    // One request, answered 503, then the thread ends. `Connection: close` so the
    // client does not wait on a keep-alive that nobody will service.
    let server = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            use std::io::{Read, Write};
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 2\r\n\
                  Connection: close\r\n\r\n{}",
            );
        }
    });

    let output = probe(port, &dir);
    let _ = server.join();

    assert_eq!(
        output.status.code(),
        Some(1),
        "a 503 from /healthz is the ONE state a restart repairs, and the probe \
         reported healthy: exit code {:?}",
        output.status.code()
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("503"),
        "and the health log must carry what was answered, or an operator reading \
         `docker inspect` learns only that something is wrong: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **AC2's unhealthy half, in its second form: nothing answers at all.**
///
/// A bridge whose own web server is not responding cannot be asked anything, and
/// it is the state a restart most plausibly clears. This is not an opinion the
/// probe adds — it is the absence of an answer.
///
/// FALSIFIED 2026-08-20 — mutation RUN: exiting 0 when the request errors (treating
/// "cannot ask" as "fine") goes red with `a bridge whose web server does not answer
/// cannot be called healthy`.
#[test]
fn a_bridge_that_does_not_answer_is_unhealthy() {
    let dir = scratch("silent");
    // A port nothing is listening on: the kernel hands one out and we do not bind.
    let port = common::an_unused_host_port();

    let output = probe(port, &dir);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a bridge whose web server does not answer cannot be called healthy: \
         nothing can be asked of it, and a restart is the plausible repair"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("could not be reached"),
        "and the reason belongs on stderr, which is where `docker inspect`'s health \
         log keeps it: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **AC3 — an unrecognised argument refuses; it does not start a bridge.**
///
/// This is the one that would be found in production rather than in a test: a typo
/// in the `HEALTHCHECK` line would have started a SECOND bridge inside the
/// container, competing for the state directory and opening a second Sparkplug
/// session under the same edge node — every thirty seconds, for as long as nobody
/// noticed.
///
/// **It waits with a deadline rather than calling `output()`**, and that is a
/// repair of this test's own first version: under the mutation, the process starts
/// a bridge and never exits, so `output()` blocked for ever and the falsification
/// was a HANG rather than a red test. A test that cannot fail promptly is the same
/// defect as one that cannot fail at all.
///
/// FALSIFIED 2026-08-20 — mutation RUN, output copied: falling through to `main`'s
/// normal start on an unknown argument goes red with
///
/// ```text
/// an unrecognised argument must refuse: the process was still running after 5 s,
/// which means it started a bridge — inside the container that is a SECOND bridge,
/// on every probe
/// ```
#[test]
fn an_unrecognised_argument_refuses_rather_than_starting_a_bridge() {
    let dir = scratch("typo");
    let mut child = Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .arg("--healtcheck") // the typo this test exists for
        .env("SMARTME_STATE_DIR", &dir)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("it runs");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let status = loop {
        match child.try_wait().expect("wait") {
            Some(status) => break Some(status),
            None if std::time::Instant::now() >= deadline => break None,
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    };
    let Some(status) = status else {
        let _ = child.kill();
        panic!(
            "an unrecognised argument must refuse: the process was still running after \
             5 s, which means it started a bridge — inside the container that is a \
             SECOND bridge, on every probe"
        );
    };
    let mut stderr = String::new();
    use std::io::Read;
    let _ = child
        .stderr
        .take()
        .expect("piped")
        .read_to_string(&mut stderr);
    let output = std::process::Output {
        status,
        stdout: Vec::new(),
        stderr: stderr.into_bytes(),
    };

    assert_eq!(
        output.status.code(),
        Some(2),
        "an unrecognised argument must refuse: falling through would start a second \
         bridge inside the container on every probe"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--healthcheck"),
        "and the refusal must name what it accepts: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
