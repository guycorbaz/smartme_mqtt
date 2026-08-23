//! Story 6.1 AC5, the half that was open for three days — a panicking handler
//! must cost the page and nothing else ([#51]).
//!
//! # Why this file is behind a feature, and what that costs the claim
//!
//! Asserting this needs a route that panics, and shipping one was refused when
//! the story was written: *"a test-only route is not the code under test"*. The
//! objection was right about the route and wrong about everything around it.
//! What the `panic-probe` feature adds is **four lines** — one `.route()` and a
//! handler whose whole body is `panic!`. The router, the middleware that catches
//! the unwind, the trace it emits, `serve`, the spawn, the poll loop, and the
//! binary this test launches are all exactly what an operator runs. No released
//! image carries the probe: `docker-publish.yml` builds with default features.
//!
//! So the claim this file supports is precise: *given* a handler that panics,
//! production turns it into a traced `500` and the meters keep being polled. It
//! does not claim that any shipped handler ever panics — none is known to.
//!
//! # The vacuity this test had to be built against
//!
//! Every absence assertion here would hold over a bridge that never started, and
//! two of them would hold over one that died at the panic. So the order is:
//! prove the loop is running BEFORE the panic (a `/healthz` with a live
//! heartbeat), then panic, then prove the same loop is still running after —
//! against the SAME endpoint, so a bridge that died cannot answer at all.
#![cfg(feature = "panic-probe")]

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A port nothing holds at this instant, asked of the kernel rather than chosen.
///
/// **This was `const PORT: u16 = 18090` until 2026-08-19** ([#98]). A constant was
/// defensible while nothing else could take it — and then an orphaned bridge did,
/// for an hour, and every run of this test was answered by it. The orphan is fixed
/// by `Bridge`'s `Drop`; the constant is fixed here, because [ADR 0037] already
/// established what a fixed port costs on a machine where *"d'autres
/// développements sont en cours dans d'autres fenêtres"* is the normal state.
///
/// Nothing forced the constant: this test WRITES `ui_port` into the configuration
/// it hands the binary, so it can name any port it likes. The window between the
/// release here and the bridge's bind is the same one `common::an_unused_host_port`
/// documents, and losing it is loud rather than silent.
fn an_unused_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("the kernel has a port")
        .local_addr()
        .expect("a bound listener has an address")
        .port()
}

fn state_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("smartme_panic_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch dir");
    path
}

/// The whole response, status line included — the status is half of what this
/// file asserts, so a helper returning only a body would hide it.
fn get(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).ok()?;
    Some(raw)
}

/// A spawned bridge that dies with the test, **whatever path the test leaves by**.
///
/// **Added 2026-08-19, [#98], after this test refused a push and then kept
/// refusing it.** `std::process::Child` does not kill on drop, and this test tore
/// its bridge down on exactly two paths: the UI never answering, and the nominal
/// end. Any assertion failing between them left the binary running — on a port that was a CONSTANT,
/// which is a CONSTANT — and every later run was then answered by that orphan,
/// whose state directory the new run had just deleted. `loop_age_ms: null`, a
/// failure in 0.10 s where a real bridge takes 11 s, and a fresh orphan each time:
/// one transient failure became a permanent inability to push.
///
/// `Drop` covers every exit path, panics included. It is the same shape as
/// `ScratchDir` in the chaos tests, which has always removed its directory this
/// way; the process needed the same treatment and did not have it.
///
/// FALSIFIED 2026-08-19, and BOTH HALVES were run, because "no orphan" is only
/// evidence if the same experiment can produce one:
/// - an assertion made to fail between the spawn and the teardown — test RED,
///   `pgrep -f target/debug/smartme-bridge` finds NOTHING;
/// - the same, with this `Drop` body emptied — test RED, and `pgrep` finds
///   `1932524 /home/.../target/debug/smartme-bridge`, still holding 18090.
struct Bridge(Option<Child>);

impl Bridge {
    fn child(&mut self) -> &mut Child {
        self.0.as_mut().expect("the bridge has not been taken")
    }

    /// Hands the child over for `wait_with_output`, which consumes it. After
    /// this the guard has nothing left to kill, which is correct: the caller now
    /// owns the teardown and is about to do it.
    fn take(mut self) -> Child {
        self.0.take().expect("the bridge has not been taken")
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Waits for a `/healthz` **that shows a running poll loop**, not merely one that
/// answers.
///
/// **The distinction is the test's whole premise** ([#98]'s neighbour, found
/// 2026-08-19). The UI is served as soon as the lifecycle loop reaches its Running
/// arm, which happens BEFORE the first poll tick has touched a heartbeat — so a
/// probe that returned on the first answer could hand back
/// `"loop_age_ms":null`, and the assertion below would fail against a bridge that
/// was starting perfectly. That is a race, not a defect, and it went unseen while
/// an orphaned bridge on a fixed port produced a louder failure first.
///
/// FALSIFIED 2026-08-19 — with the deadline set to zero the test goes red with
/// `the poll loop never started within 20 s`, which is the shape a bridge whose
/// meters never spawn would produce.
fn wait_for_ui(port: u16, child: &mut Child) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(answer) = get(port, "/healthz") {
            if loop_age(&answer).is_some() {
                return Some(answer);
            }
        }
        if child.try_wait().expect("wait").is_some() {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// `loop_age_ms` out of a `/healthz` body, or `None` if the loop has never
/// ticked. Hand-parsed rather than through `serde_json`: the point is to read
/// what the endpoint really emitted, and a deserialiser that accepted a
/// reshaped body would defeat that.
fn loop_age(health: &str) -> Option<i64> {
    let after = health.split("\"loop_age_ms\":").nth(1)?;
    let digits: String = after
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    digits.parse().ok()
}

fn write_config(dir: &std::path::Path, port: u16) {
    std::fs::write(
        dir.join("config.toml"),
        format!(
            "schema_version = {}\n\
             group_id = \"Plant\"\n\
             node_id = \"Bridge01\"\n\
             # TEST-NET-1 (RFC 5737): unroutable. The poll loop still RUNS — it\n\
             # fails its fetch, records a verdict and touches its heartbeat,\n\
             # which is the liveness this test reads.\n\
             broker_host = \"192.0.2.1\"\n\
             broker_port = 1883\n\
             # Five seconds: the minimum the model allows, so the loop ticks\n\
             # several times inside this test rather than once.\n\
             publish_period_secs = 5\n\
             api_base = \"https://192.0.2.1\"\n\
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
}

/// AC5's second half.
///
/// FALSIFIED 2026-08-08 by removing `.layer(axum::middleware::from_fn(catch_panic))`
/// from `ui::router` — the state the code was in until today. Copied from the run:
///
/// ```text
/// test a_panicking_handler_costs_the_page_and_nothing_else ... FAILED
///
/// thread 'a_panicking_handler_costs_the_page_and_nothing_else' (605) panicked at
/// crates/smartme-bridge/tests/a_panicking_handler_does_not_cost_the_meters.rs:170:5:
/// a panicking handler must answer 500, not drop the connection: the operator
/// sees a reset in the browser and there is nothing to look up. Got: Some("")
/// ```
///
/// **`Some("")`, not a wrong status** — the TCP connection is accepted and then
/// closed with not one byte written, so there is no status line to be wrong. A
/// browser reports a reset. That is what an operator met before today, and it is
/// also why AC5's "traced, loudly" clause was false for this half: the default
/// panic hook writes to stderr, never through `tracing`, so a bridge logging to
/// a file recorded the outage nowhere.
#[test]
fn a_panicking_handler_costs_the_page_and_nothing_else() {
    let dir = state_dir("probe");
    let port = an_unused_port();
    write_config(&dir, port);

    let mut bridge = Bridge(Some(
        Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("SMARTME_STATE_DIR", &dir)
            // The UI port for a run with no `ui_port` in its file ([ADR 0037]).
            // NOT 8080: that is a deployment contract, and it is shared with other
            // projects on this machine. Nothing here tests the UI; the variable
            // exists so no test binary reaches for a port it does not own.
            .env("SMARTME_UI_PORT", an_unused_port().to_string())
            .env("SMARTME_CLIENT_ID", "x")
            .env("SMARTME_CLIENT_SECRET", "x")
            // The subscriber's fmt layer writes to stdout; the panic hook writes to
            // stderr. Both are captured, because the assertion below is precisely
            // that the failure reached the FORMER.
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the bridge binary runs"),
    ));

    let before = wait_for_ui(port, bridge.child());

    // THE PREMISE, and without it every assertion after the panic is vacuous.
    // The explicit `kill` that used to stand here is gone: `Bridge`'s `Drop` does
    // it on this path and on every other one, which is the whole of [#98].
    //
    // AND IT NAMES THE HARNESS WHEN THE HARNESS IS AT FAULT (R6, 2026-08-24). The
    // window this file documents — the kernel hands out a port, `an_unused_port`
    // releases it, something takes it before the bridge binds — produces exactly
    // this `None`, and the message used to blame the bridge for it: *"the poll
    // loop never started"*, about a bridge that started fine and found its port
    // occupied. Two causes, one symptom, and the one an operator would chase is
    // the wrong one.
    //
    // They are told apart AFTER the child is gone: the bridge is killed first, so
    // whatever still holds the port is not it. A port that binds means the bridge
    // really did fail to serve; a port that refuses means someone else has it and
    // this run proves nothing about the bridge.
    let before = match before {
        Some(health) => health,
        None => {
            bridge.take().kill().expect("the bridge can be killed");
            let stolen = std::net::TcpListener::bind(("127.0.0.1", port)).is_err();
            assert!(
                !stolen,
                "THE HARNESS LOST THE PORT, and this run says nothing about the \
                 bridge: {port} is still held by another process after the bridge \
                 was killed, so the bridge never got the port it was told to serve \
                 on. Re-run. This is the bind race `an_unused_port` documents \
                 above, and it is what R6 is made of"
            );
            panic!(
                "the poll loop never started within 20 s, so nothing below could mean anything. \
                 Either the UI never answered at all, or it answered with `loop_age_ms: null` \
                 throughout — a bridge serving its screen while polling nothing. The port was \
                 free once the bridge was killed, so this is the BRIDGE and not the harness"
            );
        }
    };
    assert!(
        loop_age(&before).is_some(),
        "the poll loop must be RUNNING before the panic, or 'the meters kept \
         going' is a claim about a loop that never started:\n{before}"
    );

    // The panic itself.
    let answer = get(port, "/debug/panic");
    assert!(
        answer
            .as_deref()
            .is_some_and(|a| a.starts_with("HTTP/1.1 500")),
        "a panicking handler must answer 500, not drop the connection: the \
         operator sees a reset in the browser and there is nothing to look up. \
         Got: {answer:?}"
    );

    // The loop must have ticked AGAIN after the panic — not merely be reachable.
    //
    // Asserting the age is small would not do it: a frozen loop keeps whatever
    // age it had at the instant it froze, and at a 5 s period that number stays
    // plausible for seconds. What proves a tick is the age FALLING — it counts
    // up from each tick and resets at the next one.
    //
    // Compared against the PREVIOUS sample rather than against the pre-panic
    // one, which is the mistake the first draft made: `before` was taken at an
    // arbitrary point in the cycle, so a perfectly live loop read as motionless
    // whenever that sample happened to be the younger of the two. The bug was in
    // the test and the run said so — `loop_age_ms: 145` on the last sample, a
    // loop that had ticked 145 ms earlier, reported as stopped.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut moved = false;
    let mut last = String::new();
    let mut previous = loop_age(&before);
    while Instant::now() < deadline && !moved {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(health) = get(port, "/healthz") {
            if let (Some(then), Some(now)) = (previous, loop_age(&health)) {
                moved = now < then;
            }
            previous = loop_age(&health);
            last = health;
        }
    }

    let alive = bridge.child().try_wait().expect("wait").is_none();
    let mut child = bridge.take();
    let _ = child.kill();
    let output = child.wait_with_output().expect("wait");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        alive,
        "the bridge exited on a panicking handler. A diagnostic aid that can stop \
         the meters has stopped being an aid:\n{said}"
    );
    assert!(
        moved,
        "the poll loop stopped ticking after the panic — the page was supposed to \
         be the only casualty. Last /healthz:\n{last}"
    );
    // AC5's second clause, and the one that was false before today: through
    // `tracing`, so it reaches the log FILE, not only a console nobody keeps.
    assert!(
        said.contains("a web UI handler PANICKED"),
        "the panic must be traced at error level, or an operator meets a 500 with \
         nothing to look up. The bridge said:\n{said}"
    );
    assert!(
        said.contains("the panic probe was called deliberately"),
        "the trace must carry the panic's own message; without it the line names \
         a page and not a cause:\n{said}"
    );
}
