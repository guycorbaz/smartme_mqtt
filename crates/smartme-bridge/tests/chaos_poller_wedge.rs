//! Story 4.14 AC2–AC3 — a silence that is honest, and a loop that has stopped,
//! told apart from outside the process.
//!
//! # The wedge is not caused by the source, and that decided the shape of this file
//!
//! `epics.md` asked for *"a source that hangs beyond every deadline"*. There is
//! none: every fetch is wrapped in `tokio::time::timeout(config.fetch_timeout,
//! …)` and the heartbeat is written before it — *"Heartbeat FIRST: before
//! anything that can block"*. A hanging source costs the loop at most
//! `fetch_timeout`, and with the shipped numbers that is **10 s against a 15 s
//! allowance at the shortest legal period**. `ui::tests::
//! the_wedge_allowance_outlives_a_blocked_fetch` (AC1) is what keeps that margin,
//! and it is the reason the wedge below has to be BUILT rather than provoked.
//!
//! **So the two tests here differ in exactly one number.** Same tarpit, same
//! 300 ms period; a `fetch_timeout` of 500 ms leaves the loop healthy, and one of
//! 5 s wedges it. That is AC1's arithmetic, observed end to end instead of
//! asserted: the margin is the whole difference between "a slow server" and "a
//! restart that kills every meter's Sparkplug session".
//!
//! # What this drives, and what it does not
//!
//! `app::run` does NOT serve the UI — `main.rs::lifecycle` spawns `ui::serve`
//! beside it. This file assembles the same two pieces the binary does, because
//! the binary reads its configuration through `config.rs`, which fixes
//! `fetch_timeout` at 10 s and refuses a period under 5 s: **the wedge in AC3 is
//! unreachable through a configuration file, by design**. The poll loop, the
//! driver, `ui::serve`, `healthz` and `loop_age` are all the production ones;
//! what the test supplies is the wiring `lifecycle` would.
//!
//! # The trap this file could fall into
//!
//! `chaos_stale_on_cloud_timeout` (story 1.14) already proves `Stale` on the wire
//! when the cloud is silent, using an unroutable address — a **connect** timeout.
//! Here the tarpit accepts the connection and then says nothing, which is the
//! shape of a server that is up and stuck, and the point is not the staleness at
//! all: it is that the same run answers **200** on `/healthz` while doing it.
//! Do not cite 1.14 as evidence for anything here, and do not re-prove 1.14.
//!
//! # Falsification — 2026-08-19, three mutations RUN, output copied
//!
//! **AC3, the allowance.** `WEDGED_AFTER_PERIODS` 3 → 100 goes red with `THE
//! BLOCKED LOOP NEVER READ AS WEDGED … loop_age_ms: 4957,
//! loop_age_allowed_ms: 30000` — the loop held inside one 5 s fetch and the
//! endpoint calling that healthy, which is the reading Epic 7 would act on.
//!
//! **AC3, the verdict.** Forcing `wedged = false` in `healthz` goes red with the
//! same sentence and a body that contradicts itself: `loop_age_ms: 4947,
//! loop_age_allowed_ms: 900`. The two mutations are told apart by the numbers
//! rather than by the message, which is worth knowing before reading a failure.
//!
//! **AC2, the heartbeat, AND THE PREDICTION DID NOT SURVIVE ITS RUN.** This note
//! said the deletion of `heartbeat.touch(…)` would go red on the frozen age
//! (`2093 then 2093`). It does not: with no touch at all a meter has no
//! `last_tick`, `loop_age` returns `None`, and the endpoint reports
//! `"loop_age_ms":null` **beside `"wedged":false`** — a dead loop reading as
//! healthy. The real output is `THE LOOP HAS NO AGE AT ALL. loop_age_ms is null,
//! which is what a bridge whose poll task never ticked reports`. The assertion
//! was written to survive that shape only because the run was done first; a
//! `number()` helper returning `0` for `null` would have made this mutation
//! GREEN. Fifth prediction in four stories that did not survive its own run.

mod common;

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use smartme_bridge::adapters::sparkplug_publisher::{
    METRIC_ENERGY, METRIC_POWER, ignition_quality_code,
};
use smartme_bridge::app::{BridgeConfig, PollConfig};
use smartme_bridge::core::state_machine::Policy;
use smartme_bridge::domain::{MeterId, Serial};
use smartme_bridge::ui;

const SERIAL: &str = "30000003";
const METER: &str = "garage";

/// The poll period both tests run at. Short, because the property is about the
/// RATIO between this and the fetch deadline, not about wall time.
const PERIOD: Duration = Duration::from_millis(300);

/// `WEDGED_AFTER_PERIODS × PERIOD` — 900 ms. Written out so the two deadlines
/// below can be read against it.
const ALLOWANCE: Duration = Duration::from_millis(900);

struct ScratchDir(std::path::PathBuf);
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A server that accepts the connection and then never says anything.
///
/// **Not an unroutable address.** TEST-NET-1 (what `chaos_stale_on_cloud_timeout`
/// uses) never answers the SYN, so the fetch dies in `connect`. A tarpit
/// completes the handshake and holds the request, which is what a server that is
/// up and stuck does — and it is the only way to hold the poll loop inside the
/// fetch for a known length of time.
///
/// The accepted sockets are kept in the task rather than dropped: dropping one
/// closes it, and a closed connection is a fast error, not a hang.
///
/// **It answers `https://` and speaks no TLS, deliberately.** The client refuses
/// any endpoint whose scheme is not `https` — *"refusing endpoint: scheme is
/// \"http\", require https"*, which is how the first draft of this file found
/// out — so the tarpit is addressed as HTTPS and simply never replies to the
/// ClientHello. The hang lands in the handshake instead of the request, which is
/// the same thing from the poll loop's side: one fetch, held to its deadline. No
/// certificate is needed to say nothing.
fn tarpit() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("the kernel has a port");
    let port = listener.local_addr().expect("bound").port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            match stream {
                Ok(socket) => held.push(socket),
                Err(_) => break,
            }
        }
    });
    port
}

/// One `GET`, returning the status code and the body.
///
/// Both, because this file asserts on both and a helper that returned only the
/// body would hide the half AR12 is about.
fn healthz(port: u16) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        stream,
        "GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).ok()?;
    let code = raw.split_whitespace().nth(1)?.parse().ok()?;
    let body = raw.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    Some((code, body))
}

/// The number behind a `"key":<digits>` in a `/healthz` body, or `None` when it
/// is `null` — which is what a bridge with no loop reports, and is not a zero.
fn number(body: &str, key: &str) -> Option<i64> {
    let start = body.find(&format!("\"{key}\":"))? + key.len() + 3;
    let rest = &body[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-')?;
    rest[..end].parse().ok()
}

fn config(
    api_port: u16,
    broker_port: u16,
    state_dir: &std::path::Path,
    node_id: &str,
    fetch_timeout: Duration,
) -> BridgeConfig {
    BridgeConfig {
        api_base: format!("https://127.0.0.1:{api_port}"),
        credentials: smart_me_client::Credentials::Basic {
            user: "u".to_string(),
            password: "p".to_string(),
        },
        http_timeout: Duration::from_secs(30),
        meters: vec![smartme_bridge::app::config::MeterConfig {
            meter: MeterId::new(METER),
            device_id: "a1a1a1a1-b2b2-c3c3-d4d4-000000000003".to_string(),
            serial: Serial::new(SERIAL),
            enabled: true,
        }],
        group_id: "Site".to_string(),
        node_id: node_id.to_string(),
        broker_host: "127.0.0.1".to_string(),
        broker_port,
        bd_seq_path: state_dir.join("bdseq.toml"),
        poll: PollConfig {
            interval: PERIOD,
            fetch_timeout,
        },
        policy: Policy::DEFAULT,
        log_dir: None,
        log_keep: None,
        ui_port: None,
    }
}

/// Starts the two halves `main.rs::lifecycle` starts, and hands back the UI port
/// and the shutdown switch.
async fn bridge_with_ui(
    config: BridgeConfig,
    state_dir: std::path::PathBuf,
) -> (u16, tokio::sync::oneshot::Sender<()>) {
    let ui_port = common::an_unused_host_port();
    let phase: ui::PhaseHandle = Arc::new(arc_swap::ArcSwap::from_pointee(ui::Phase::starting()));
    tokio::spawn(ui::serve(
        ui_port,
        ui::UiState::new(
            Arc::clone(&phase),
            state_dir,
            Arc::new(tokio::sync::Notify::new()),
        ),
    ));

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let handle = Arc::clone(&phase);
    tokio::spawn(async move {
        // The outcome is PRINTED rather than discarded: a `StartupError` here
        // makes every assertion below fail with "nothing arrived", which reads
        // as a bridge defect and is a harness one.
        if let Err(error) = smartme_bridge::app::supervisor::run_with_control(
            config,
            async {
                let _ = stop_rx.await;
            },
            move |control| handle.store(Arc::new(ui::Phase::running(control))),
        )
        .await
        {
            println!("HARNESS — the bridge refused to start: {error}");
        }
    });

    // The UI binds asynchronously; a probe that raced it would report "no
    // answer" about a server that was seconds from listening.
    //
    // **AND IT FAILS HERE IF IT NEVER ANSWERS, which it did not until 2026-08-19.**
    // This loop used to fall through silently, so a UI that never bound left every
    // assertion below to fail on its own terms — a pre-push gate reported
    // `THE BLOCKED LOOP NEVER READ AS WEDGED … Last body: ` with an EMPTY body,
    // accusing the bridge of not wedging when nothing had answered at all. A
    // harness that cannot start must say so, or it indicts the code under test.
    let mut answered = false;
    for _ in 0..100 {
        if healthz(ui_port).is_some() {
            answered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        answered,
        "THE UI NEVER ANSWERED on port {ui_port} within 10 s, so nothing below is about the \
         bridge. The likeliest cause is the bind race this harness documents: \
         `an_unused_host_port` asks the kernel for a free port and releases it, and something \
         else can take it before `ui::serve` binds — narrow, and real enough to have happened. \
         Re-run; if it repeats, the server is failing to start for a reason worth reading in \
         the log"
    );
    (ui_port, stop_tx)
}

/// **AC2 — a source that is up and stuck: STALE on the wire, HEALTHY on `/healthz`.**
///
/// The fetch deadline (500 ms) is inside the wedge allowance (900 ms), which is
/// the shipped relationship. Nothing here may report a fault: the bridge is doing
/// exactly what it should — publishing a silence it can account for.
///
/// This is also the answer [#62] is owed. That issue observes that a stale meter
/// reads healthy on `/healthz`; the meter IS named, in `degraded_meters`, and the
/// 200 is AR12's intent rather than an oversight. Both are asserted in one run
/// here, which is what lets the issue close on evidence.
#[tokio::test(flavor = "multi_thread")]
async fn a_source_that_is_up_and_stuck_is_stale_on_the_wire_and_healthy_on_healthz() {
    let (_broker, broker_port) = common::start_broker().await;
    let mut seen = common::independent_subscriber(broker_port).await;
    let api_port = tarpit();

    let dir = std::env::temp_dir().join(format!("chaos_wedge_honest_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("state dir");
    let dir = ScratchDir(dir);

    let (ui_port, stop) = bridge_with_ui(
        config(
            api_port,
            broker_port,
            &dir.0,
            "WedgeHonest",
            Duration::from_millis(500),
        ),
        dir.0.clone(),
    )
    .await;

    // ---- the wire: a silence that says it is one ---------------------------
    let birth = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/DBIRTH/") && s.topic.ends_with(SERIAL)
    })
    .await
    .expect("the device birth must reach an independent subscriber");
    for metric in [METRIC_POWER, METRIC_ENERGY] {
        assert_eq!(
            birth.quality_of(metric),
            Some(ignition_quality_code(sparkplug_b::Quality::Stale)),
            "{metric} must be STALE: the source has answered nothing"
        );
    }

    // ---- the endpoint: healthy at EVERY sample, and saying why it is quiet --
    //
    // Sampled across several periods rather than read once. A single reading
    // would pass over a bridge that flickered to 503 between two of them, and
    // under Epic 7 one 503 is one container restart — the healthcheck does not
    // average. The loop is also given time to record its first verdict: the
    // DBIRTH is published BEFORE any fetch has been attempted, so a `/healthz`
    // read at the birth finds `degraded_meters` legitimately empty.
    let mut ages = Vec::new();
    let mut named = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline {
        let (code, body) = healthz(ui_port).expect("/healthz must answer");
        assert_eq!(
            code, 200,
            "A SILENT SOURCE IS NOT A WEDGED LOOP. The poll loop is ticking, \
             timing out and publishing Stale — which is AR12's honest STALE, the \
             one case that must never restart the container. Body: {body}"
        );
        assert!(
            body.contains("\"wedged\":false"),
            "and it must say so in the body as well as the code: {body}"
        );
        ages.push(number(&body, "loop_age_ms").unwrap_or_else(|| {
            panic!(
                "THE LOOP HAS NO AGE AT ALL. `loop_age_ms` is null, which is what \
                 a bridge whose poll task never ticked reports — and it reads as \
                 `wedged:false`, so a health check trusting the flag alone would \
                 call a dead loop healthy. Body: {body}"
            )
        }));
        if body.contains(&format!("\"meter\":\"{METER}\"")) {
            named = Some(body);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let body = named.unwrap_or_else(|| {
        panic!(
            "THE SILENCE IS NOT REPORTED. Nothing could be read from the meter for \
             20 s and `degraded_meters` never named it, so the only surface an \
             operator has says the bridge is fine while a tag on their screen is \
             stale. That pairing — reported AND healthy — is what [#62] asks \
             about. Ages seen: {ages:?}"
        )
    });
    println!("AC2 MEASUREMENT — the degraded body: {body}");
    assert!(
        body.contains("\"cause\":\"source-unreachable\""),
        "and it names WHY, rather than leaving an operator to guess which of the \
         six causes it is: {body}"
    );

    // ---- and the heartbeat is ALIVE, which is the positive control ---------
    //
    // Without this the test would pass against a bridge whose loop had stopped
    // dead the instant after its first tick: an age that never moves is also an
    // age inside its allowance for the first 900 ms.
    let first = *ages.first().expect("at least one sample");
    let moved = ages.iter().any(|age| *age != first);
    println!("AC2 MEASUREMENT — loop_age_ms samples: {ages:?}");
    assert!(
        moved,
        "THE HEARTBEAT DID NOT ADVANCE ACROSS {} SAMPLES: every one read \
         {first} ms. A frozen age with `wedged:false` is the worst reading this \
         endpoint can give — it is what a stopped loop looks like to a health \
         check that trusts the flag alone",
        ages.len()
    );
    let last_age = *ages.last().expect("at least one sample");
    assert!(
        last_age < i64::try_from(ALLOWANCE.as_millis()).expect("the allowance fits an i64"),
        "the loop stays inside its allowance while the source hangs: {last_age} ms \
         against {} ms",
        ALLOWANCE.as_millis()
    );

    let _ = stop.send(());
}

/// **AC3 — a loop held past its allowance reads `wedged`, and stops reading it
/// when the block ends.**
///
/// The only difference from the test above is the fetch deadline: 5 s against a
/// 900 ms allowance. That configuration cannot be written in `config.toml` —
/// `PERIOD_MIN` is 5 s and `FETCH_TIMEOUT` is fixed at 10 s — so what this proves
/// is the WIRING from the heartbeat to the status code, against a cause AC1 keeps
/// unreachable in production.
///
/// **The recovery half is not a courtesy.** A `wedged` that latched would, under
/// Epic 7, restart the container every time a fetch ran long, and then again, and
/// again. The verdict has to be a measurement of now.
#[tokio::test(flavor = "multi_thread")]
async fn a_loop_blocked_past_its_allowance_reads_wedged_and_then_recovers() {
    let (_broker, broker_port) = common::start_broker().await;
    let api_port = tarpit();

    let dir = std::env::temp_dir().join(format!("chaos_wedge_blocked_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("state dir");
    let dir = ScratchDir(dir);

    let (ui_port, stop) = bridge_with_ui(
        config(
            api_port,
            broker_port,
            &dir.0,
            "WedgeBlocked",
            Duration::from_secs(5),
        ),
        dir.0.clone(),
    )
    .await;

    // ---- the wedge ----------------------------------------------------------
    //
    // Sized from the arithmetic rather than guessed: the loop is held for one
    // 5 s fetch, and the allowance is exhausted 900 ms into it. 30 s is ample for
    // the broker connection and the first tick to have happened as well.
    let mut wedged = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut last = String::new();
    while tokio::time::Instant::now() < deadline {
        if let Some((code, body)) = healthz(ui_port) {
            last = body.clone();
            if code == 503 {
                wedged = Some(body);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let body = wedged.unwrap_or_else(|| {
        panic!(
            "THE BLOCKED LOOP NEVER READ AS WEDGED. The poll task has been inside \
             one fetch for longer than {} ms, which is `WEDGED_AFTER_PERIODS` \
             periods, and `/healthz` still answers 200 — so the healthcheck Epic 7 \
             wires to this endpoint would never restart a bridge that had stopped \
             polling. Last body: {last}",
            ALLOWANCE.as_millis()
        )
    });
    println!("AC3 MEASUREMENT — the wedged body: {body}");
    assert!(
        body.contains("\"wedged\":true"),
        "a 503 whose body says `wedged:false` is the endpoint contradicting \
         itself: {body}"
    );
    let age = number(&body, "loop_age_ms").expect("a wedged loop has an age");
    let allowed = number(&body, "loop_age_allowed_ms").expect("and an allowance");
    assert!(
        age > allowed,
        "THE NUMBER AND THE VERDICT DISAGREE: age {age} ms, allowance {allowed} \
         ms, and the code was 503. They were read from separate clock samples \
         until story 6.1's review, and this is the assertion that keeps them one \
         reading: {body}"
    );

    // ---- and it is not a latch ---------------------------------------------
    //
    // The fetch times out after 5 s, the loop ticks, and the age falls back
    // inside the allowance for roughly 900 ms before the next fetch blocks
    // again. Polling at 50 ms samples that window many times over.
    let mut recovered = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while tokio::time::Instant::now() < deadline {
        if let Some((200, body)) = healthz(ui_port) {
            println!("AC3 MEASUREMENT — recovered: {body}");
            recovered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        recovered,
        "THE WEDGE VERDICT LATCHED. The fetch deadline expired and the loop ticked \
         again, so `/healthz` must answer 200 again — a verdict that never clears \
         is a container restarted every few seconds under Epic 7, which costs \
         every meter its Sparkplug session for a fault that has passed"
    );

    let _ = stop.send(());
}
