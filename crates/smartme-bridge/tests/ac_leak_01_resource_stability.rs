//! Story 4.15 — `AC-LEAK-01`: the bridge runs for weeks without growing (NFR3, NFR9).
//!
//! # What a hundred thousand iterations actually are
//!
//! At the default 30 s publish period a bridge performs **2 880 iterations a day**
//! for one meter, so this run is **34.7 days of production**, compressed. It is
//! not a poor substitute for a 24-hour observation — it is thirty-five times
//! longer than one. What it cannot see is wall-clock effects (log rotation, a
//! broker's daily churn); what it sees better than any soak is the per-iteration
//! path.
//!
//! # The criteria were amended before this was written, and [ADR 0038] is why
//!
//! NFR3 asked for RSS sampled **every 60 s** and a slope over **24 h**. The
//! measuring spike `CLAUDE.md` requires — rather than a decision deferred to a
//! test that did not exist — showed the run takes **≈ 100 s**: 1 000 iterations a
//! second, which is the 1 ms pacing floor and not the cost of the work. Sixty-
//! second sampling therefore yields **two points**, a regression through two
//! points is an exact line with no residual, and the 24-hour figure was a ×864
//! multiplication of it.
//!
//! The thresholds did not move. RSS_max ≤ 100 MB and FD ≤ 64 are NFR3's, intact.
//! The slope is now **≤ 80 kB per 1 000 iterations** — 0.23 % of RSS_max per day
//! at the default period, an order of magnitude stricter than the clause it
//! replaces, and stated for the window it is measured on.
//!
//! [ADR 0038]: ../../../docs/adr/0038-the-leak-gate-measures-per-iteration-growth-not-a-24-hour-slope.md
//!
//! # What is exercised, and what is NOT
//!
//! **Exercised:** the poll loop and its ticker, the oracle and its per-meter
//! memory, the monotonicity reference on disk, the publisher, protobuf encoding,
//! the outbox, the MQTT driver, `rumqttc` and a real mosquitto container. That is
//! the whole permanent path a reading takes.
//!
//! **Not exercised: the real HTTP client on its NOMINAL path.** It refuses any
//! endpoint that is not `https` and validates certificates, so 100 000 successful
//! fetches against a local server would need a trust root installed in the test
//! environment. The client is the likeliest place for a descriptor leak, which is
//! exactly why the omission is named here instead of being inferred from a green
//! run — and why the second test below drives the real client 10 000 times on its
//! FAILURE path, which is the half that can be reached.
//!
//! # Falsification — 2026-08-19, two mutations RUN, output copied
//!
//! The mutations belong in the test rather than in production: what is measured
//! is the process, so a leak injected into the process is the faithful break.
//! Both are reachable by a reader — `AC_LEAK_INJECT_RSS=1` and
//! `AC_LEAK_INJECT_FD=1` — because a falsification nobody can re-run is a claim.
//!
//! **The slope.** `AC_LEAK_INJECT_RSS=1`, leaking 1 kB per iteration into a `Vec`
//! that is never dropped, goes red with `RSS IS GROWING WITH THE ITERATION COUNT:
//! 1040.4 kB per 1000 iterations against a bound of 80` — thirteen times the
//! bound, on 5 000 iterations.
//!
//! **The descriptors.** `AC_LEAK_INJECT_FD=1`, opening `/proc/self/status` once
//! per iteration and holding the handle, goes red with `THE PROCESS IS
//! ACCUMULATING FILE DESCRIPTORS: 5496 open against a bound of 64`.
//!
//! **BOTH PREDICTIONS IN THE FIRST DRAFT OF THIS NOTE WERE WRONG** — 1123.7 and
//! 1034, against 1040.4 and 5496 — and they are the sixth and seventh in five
//! stories. The descriptor one was wrong by a factor of five because it assumed
//! `ulimit -n` was 1024 and would cap the leak; it did not. Numbers written
//! before the run are guesses wearing evidence's clothes.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use smartme_bridge::app::poll_publish::Heartbeats;
use smartme_bridge::app::{BridgeConfig, PollConfig};
use smartme_bridge::core::clock::{Clock, SystemClock};
use smartme_bridge::core::source::{Reading, Source, SourceError, SourceFaults};
use smartme_bridge::core::state_machine::Policy;
use smartme_bridge::domain::{
    DeviceIdentity, Kw, Kwh, Measurement, MeterId, Quality, Serial, UtcMillis,
};

const SERIAL: &str = "30000004";
const METER: &str = "garage";

/// NFR3's ceiling, in the unit `/proc/self/status` reports.
const RSS_MAX_KB: u64 = 100 * 1024;

/// NFR3's descriptor ceiling.
const FD_MAX: usize = 64;

/// [ADR 0038]'s per-iteration bound, replacing the 1 %/24 h that could not be
/// measured on a 100-second run.
///
/// [ADR 0038]: ../../../docs/adr/0038-the-leak-gate-measures-per-iteration-growth-not-a-24-hour-slope.md
const SLOPE_KB_PER_1000: f64 = 80.0;

/// How many iterations AC-LEAK-01 asks for. Overridable so the gate can be
/// rehearsed quickly; **the default is the criterion**, and the report says which
/// number it ran.
fn target_iterations() -> u64 {
    std::env::var("AC_LEAK_ITERATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000)
}

/// A source with no script and no growth: every call computes its answer.
///
/// `FakeSource` holds a `Vec` of scripted steps, so 100 000 of them would grow
/// the very number this file measures — the harness would be the leak. A
/// treadmill answers for ever out of two counters.
struct Treadmill {
    calls: Arc<AtomicU64>,
    base: i64,
    /// The deliberate leaks, off unless a falsification run turns them on
    /// (`AC_LEAK_INJECT_RSS`, `AC_LEAK_INJECT_FD`). Fields rather than `#[cfg]`
    /// blocks: the mutation is then one environment variable at one call site,
    /// which is what makes it re-runnable by a reader rather than a claim.
    leak: Vec<Vec<u8>>,
    leaking: bool,
    /// The descriptor leak. `File::open` failing is kept rather than unwrapped:
    /// past `ulimit -n` every further open fails, and the point is the count
    /// already reached, not a panic from the harness.
    handles: Vec<std::fs::File>,
    leaking_fds: bool,
}

impl Source for Treadmill {
    async fn fetch(&mut self, meter: &MeterId) -> Result<Reading, SourceError> {
        let n = self.calls.fetch_add(1, Ordering::Relaxed);
        if self.leaking {
            self.leak.push(vec![0u8; 1024]);
        }
        if self.leaking_fds {
            if let Ok(handle) = std::fs::File::open("/proc/self/status") {
                self.handles.push(handle);
            }
        }
        let at = self.base + (n as i64) * 1_000;
        Ok(Reading {
            value: Measurement {
                meter: meter.clone(),
                serial: Serial::new(SERIAL),
                power: Some(Kw(0.018)),
                // Strictly increasing, so the monotonicity oracle stays on its
                // nominal path instead of latching a fault after two ticks and
                // measuring a loop that has stopped doing the work.
                energy: Some(Kwh(4_843.0 + n as f64 * 0.001)),
                value_date: UtcMillis(at),
                quality: Quality::Good,
            },
            http_date: Some(UtcMillis(at)),
            faults: SourceFaults::default(),
            energy_unit: Some("kWh".to_string()),
        })
    }
}

fn rss_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("procfs");
    status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .expect("VmRSS is reported on Linux")
}

fn open_fds() -> usize {
    std::fs::read_dir("/proc/self/fd").expect("procfs").count()
}

/// Least-squares slope of `rss_kb` against iteration count, scaled to kB per
/// 1 000 iterations.
///
/// Returns `None` when every sample carries the same iteration count — a run that
/// never advanced, where a slope would be a division by zero dressed as a
/// measurement.
fn slope_kb_per_1000(samples: &[(u64, u64)]) -> Option<f64> {
    let n = samples.len() as f64;
    if n < 2.0 {
        return None;
    }
    let mean_x = samples.iter().map(|(i, _)| *i as f64).sum::<f64>() / n;
    let mean_y = samples.iter().map(|(_, r)| *r as f64).sum::<f64>() / n;
    let mut covariance = 0.0;
    let mut variance = 0.0;
    for (iterations, rss) in samples {
        let dx = *iterations as f64 - mean_x;
        covariance += dx * (*rss as f64 - mean_y);
        variance += dx * dx;
    }
    if variance == 0.0 {
        return None;
    }
    Some(covariance / variance * 1_000.0)
}

/// **AC-LEAK-01, the sustained run.**
///
/// Reports before it asserts: *"a threshold met by a wide margin and one met
/// barely are different results"*, and only the figures say which this was.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "AC-LEAK-01: a 100 000-iteration resource-stability run (~100 s); run by hand before a release"]
async fn a_hundred_thousand_iterations_do_not_grow_the_process() {
    let target = target_iterations();
    let leaking = std::env::var("AC_LEAK_INJECT_RSS").is_ok();
    let leaking_fds = std::env::var("AC_LEAK_INJECT_FD").is_ok();

    let (_broker, broker_port) = common::start_broker().await;
    let dir = std::env::temp_dir().join(format!("ac_leak_01_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("state dir");

    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(SystemClock::new());
    let node = sparkplug_b::EdgeNode::new("Site", "AcLeak").expect("valid identifiers");
    let (tx, rx) = mpsc::channel(64);
    let (death_tx, death_rx) = oneshot::channel();
    let (device_tx, device_rx) = mpsc::channel(4);
    let meter = MeterId::new(METER);
    let pulse = Heartbeats::for_meters([meter.clone()]);

    let config = BridgeConfig {
        // Never contacted: this test drives the poll loop through `Treadmill`.
        // The address is the unroutable one so a mistake here fails loudly rather
        // than reaching anything real.
        api_base: "https://192.0.2.1".to_string(),
        credentials: smart_me_client::Credentials::Basic {
            user: "u".to_string(),
            password: "p".to_string(),
        },
        http_timeout: Duration::from_secs(10),
        meters: vec![smartme_bridge::app::config::MeterConfig {
            priority: false,
            meter: meter.clone(),
            device_id: "a1a1a1a1-b2b2-c3c3-d4d4-000000000004".to_string(),
            serial: Serial::new(SERIAL),
            enabled: true,
        }],
        group_id: "Site".to_string(),
        node_id: "AcLeak".to_string(),
        broker_host: "127.0.0.1".to_string(),
        broker_port,
        bd_seq_path: dir.join("bdseq.toml"),
        poll: PollConfig {
            // The pacing floor. 100 000 iterations at 1 ms is ≈ 100 s, and the
            // loop waits between them — the cost per iteration is lower, which is
            // why this measures accumulation rather than throughput.
            interval: Duration::from_millis(1),
            fetch_timeout: Duration::from_secs(5),
        },
        policy: Policy::DEFAULT,
        log_dir: None,
        log_keep: None,
        ui_port: None,
    };
    let handle: smartme_bridge::app::supervisor::ConfigHandle =
        Arc::new(arc_swap::ArcSwap::from_pointee(config));

    let driver = tokio::spawn(smartme_bridge::app::mqtt_driver::run(
        smartme_bridge::app::mqtt_driver::MqttConfig {
            client_id: "AcLeak".to_string(),
            host: "127.0.0.1".to_string(),
            port: broker_port,
            keep_alive: Duration::from_secs(30),
            bd_seq_path: dir.join("bdseq.toml"),
            capacity: 64,
            death_flush: Duration::from_secs(2),
            reconnect_floor: Duration::from_secs(1),
        },
        node,
        // Both names since contract v13 (ADR 0049): the topic carries the
        // published one, the declaration is filed under the serial.
        vec![DeviceIdentity::new(
            MeterId::new(METER),
            Serial::new(SERIAL),
        )],
        Arc::clone(&clock),
        smartme_bridge::app::mqtt_driver::Health {
            meters: pulse.clone(),
            sink: smartme_bridge::app::mqtt_driver::SinkHealth::new(),
        },
        rx,
        device_rx,
        death_rx,
    ));

    let calls = Arc::new(AtomicU64::new(0));
    let poller = tokio::spawn(smartme_bridge::app::poll_publish::run(
        smartme_bridge::app::poll_publish::PolledMeter {
            meter: meter.clone(),
            serial: Serial::new(SERIAL),
        },
        Treadmill {
            calls: Arc::clone(&calls),
            base: 1_786_968_000_000,
            leak: Vec::new(),
            leaking,
            handles: Vec::new(),
            leaking_fds,
        },
        Arc::clone(&clock),
        handle,
        pulse.clone(),
        tx,
        dir.clone(),
        device_tx,
    ));

    // The baseline is taken AFTER the first iterations rather than at zero: the
    // first connection, the first birth and the allocator's initial arena all land
    // in the first moments, and counting them as growth would make the slope a
    // measure of starting up.
    while calls.load(Ordering::Relaxed) < 500 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let started = std::time::Instant::now();
    let rss_at_start = rss_kb();
    let fds_at_start = open_fds();

    // At least 100 samples across the run ([ADR 0038] §2): 500 ms over ≈ 100 s is
    // roughly 200.
    let mut samples: Vec<(u64, u64)> = Vec::new();
    let mut fd_max = fds_at_start;
    let mut rss_max = rss_at_start;
    loop {
        let done = calls.load(Ordering::Relaxed);
        let rss = rss_kb();
        let fds = open_fds();
        rss_max = rss_max.max(rss);
        fd_max = fd_max.max(fds);
        samples.push((done, rss));
        if done >= target {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(900),
            "THE RUN DID NOT REACH ITS ITERATION COUNT: {done} of {target} after \
             15 minutes. The loop is not being paced by its 1 ms ticker any more, \
             and nothing below would be a measurement of accumulation"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let elapsed = started.elapsed();
    let done = calls.load(Ordering::Relaxed);
    let slope = slope_kb_per_1000(&samples).expect("the iteration count advanced across samples");

    // ---- REPORTED before asserted -----------------------------------------
    println!(
        "AC-LEAK-01 — {done} iterations in {:.1} s ({:.0}/s), {} samples\n\
         AC-LEAK-01 — RSS {rss_at_start} kB at baseline, {rss_max} kB max, bound {RSS_MAX_KB} kB \
         ({:.1} % of it)\n\
         AC-LEAK-01 — RSS slope {slope:.2} kB per 1 000 iterations, bound {SLOPE_KB_PER_1000} \
         ({:.1} % of it)\n\
         AC-LEAK-01 — FDs {fds_at_start} at baseline, {fd_max} max, bound {FD_MAX}\n\
         AC-LEAK-01 — at the default 30 s period this run is {:.1} days of production for one \
         meter",
        elapsed.as_secs_f64(),
        done as f64 / elapsed.as_secs_f64(),
        samples.len(),
        rss_max as f64 / RSS_MAX_KB as f64 * 100.0,
        slope.abs() / SLOPE_KB_PER_1000 * 100.0,
        done as f64 * 30.0 / 86_400.0,
    );

    assert!(
        rss_max <= RSS_MAX_KB,
        "RSS EXCEEDED NFR3's CEILING: {rss_max} kB against {RSS_MAX_KB} kB. The bridge is meant \
         to co-exist with everything else on a NAS or a Raspberry Pi (NFR9), and this is the \
         number that says whether it can"
    );
    assert!(
        fd_max <= FD_MAX,
        "THE PROCESS IS ACCUMULATING FILE DESCRIPTORS: {fd_max} open against a bound of \
         {FD_MAX}. A descriptor per iteration is the leak that ends in `too many open files` \
         weeks after a release, on a machine nobody is watching"
    );
    assert!(
        slope <= SLOPE_KB_PER_1000,
        "RSS IS GROWING WITH THE ITERATION COUNT: {slope:.1} kB per 1000 iterations against a \
         bound of {SLOPE_KB_PER_1000}. At the default period that is {:.1} MB a year, and NFR3 \
         exists because the fourth epic built on top of a leak is where it gets found otherwise",
        slope / 1_000.0 * 2_880.0 * 365.0 / 1_024.0
    );

    let _ = death_tx.send(());
    poller.abort();
    let _ = tokio::time::timeout(Duration::from_secs(10), driver).await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// **AC-LEAK-01's other half: the real HTTP client, on the path a test can
/// reach.**
///
/// The nominal path is unreachable locally — the client refuses non-`https`
/// endpoints and validates certificates — so this drives the failure path
/// instead: 10 000 refused connections through the shipped `SmartMeClient`,
/// counting descriptors. A socket that is not returned to the pool, or not
/// closed, shows here and nowhere else in this repository.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "AC-LEAK-01: 10 000 refused fetches through the real HTTP client; run by hand before a release"]
async fn refused_fetches_leak_no_descriptor() {
    // A port nothing is listening on: `connect` is refused immediately, which is
    // what makes ten thousand attempts affordable.
    let closed = common::an_unused_host_port();
    let client = smart_me_client::SmartMeClient::new(
        format!("https://127.0.0.1:{closed}"),
        smart_me_client::Credentials::Basic {
            user: "u".to_string(),
            password: "p".to_string(),
        },
        // One second, not less: the client refuses a shorter timeout outright —
        // `timeout under 1s would instant-fail every request` — which is a guard
        // worth knowing about before writing a test that tries to be quick. It
        // costs nothing here: a refused connection returns immediately and never
        // reaches the deadline.
        Duration::from_secs(1),
    )
    .expect("an https base URL and a >= 1 s timeout are accepted");

    // Warm-up first, for the same reason the run above takes its baseline late:
    // the first call builds the connection pool.
    for _ in 0..50 {
        let _ = client
            .get_device("a1a1a1a1-b2b2-c3c3-d4d4-000000000004", None)
            .await;
    }
    let fds_at_start = open_fds();
    let started = std::time::Instant::now();
    let mut fd_max = fds_at_start;

    const ATTEMPTS: usize = 10_000;
    for n in 0..ATTEMPTS {
        // `None`: Basic credentials need no token, and this call is about the
        // socket rather than the authentication.
        let outcome = client
            .get_device("a1a1a1a1-b2b2-c3c3-d4d4-000000000004", None)
            .await;
        assert!(
            outcome.is_err(),
            "nothing is listening on {closed}: attempt {n} must fail, or this test is measuring \
             a path it did not intend"
        );
        if n % 100 == 0 {
            fd_max = fd_max.max(open_fds());
        }
    }
    let fds_at_end = open_fds();
    fd_max = fd_max.max(fds_at_end);

    println!(
        "AC-LEAK-01 — {ATTEMPTS} refused fetches in {:.1} s. FDs {fds_at_start} at baseline, \
         {fd_max} max, {fds_at_end} at the end, bound {FD_MAX}",
        started.elapsed().as_secs_f64()
    );
    assert!(
        fd_max <= FD_MAX,
        "THE HTTP CLIENT IS LEAKING DESCRIPTORS ON ITS FAILURE PATH: {fd_max} open against a \
         bound of {FD_MAX}, over {ATTEMPTS} refused connections. A bridge whose cloud is down \
         for a day makes 2 880 of these per meter"
    );
}
