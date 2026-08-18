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

const PORT: u16 = 18090;

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

fn wait_for_ui(port: u16, child: &mut Child) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(answer) = get(port, "/healthz") {
            return Some(answer);
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

fn write_config(dir: &std::path::Path) {
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
             ui_port = {PORT}\n\
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
    write_config(&dir);

    let mut bridge = Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("SMARTME_STATE_DIR", &dir)
        // The UI port for a run with no `ui_port` in its file ([ADR 0037]).
        // NOT 8080: that is a deployment contract, and it is shared with other
        // projects on this machine. Nothing here tests the UI; the variable
        // exists so no test binary reaches for a port it does not own.
        .env("SMARTME_UI_PORT", "18104")
        .env("SMARTME_CLIENT_ID", "x")
        .env("SMARTME_CLIENT_SECRET", "x")
        // The subscriber's fmt layer writes to stdout; the panic hook writes to
        // stderr. Both are captured, because the assertion below is precisely
        // that the failure reached the FORMER.
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the bridge binary runs");

    let before = wait_for_ui(PORT, &mut bridge);

    // THE PREMISE, and without it every assertion after the panic is vacuous.
    let before = match before {
        Some(health) => health,
        None => {
            let _ = bridge.kill();
            panic!("the UI never answered, so nothing below could mean anything");
        }
    };
    assert!(
        loop_age(&before).is_some(),
        "the poll loop must be RUNNING before the panic, or 'the meters kept \
         going' is a claim about a loop that never started:\n{before}"
    );

    // The panic itself.
    let answer = get(PORT, "/debug/panic");
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
        if let Some(health) = get(PORT, "/healthz") {
            if let (Some(then), Some(now)) = (previous, loop_age(&health)) {
                moved = now < then;
            }
            previous = loop_age(&health);
            last = health;
        }
    }

    let alive = bridge.try_wait().expect("wait").is_none();
    let _ = bridge.kill();
    let output = bridge.wait_with_output().expect("wait");
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
