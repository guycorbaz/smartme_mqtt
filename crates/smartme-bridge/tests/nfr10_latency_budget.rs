//! Story 4.16 — **NFR10's latency budget, measured from outside the process.**
//!
//! NFR10, as amended by [ADR 0010]'s addendum on 2026-08-19 ([#99]): *a new reading
//! reaches MQTT within one poll cycle; **read→accepted-for-transmission** latency
//! p95 ≤ 3 s, p99 ≤ 5 s over a 24 h window under nominal load.*
//!
//! # What is measured, and why it is deliberately MORE than the requirement
//!
//! "Accepted for transmission" is not observable from outside: `try_publish`
//! answers `Ok` when the message enters `rumqttc`'s request channel, which is a
//! point inside the driver and — per [#85] — not the same as leaving the socket.
//! Instrumenting production to expose it would add a seam for a measurement.
//!
//! **So this measures a strictly LARGER interval: the reading's acquisition to its
//! arrival at an independent subscriber**, through the broker. Acceptance happens
//! somewhere inside that interval, so
//!
//! > p95(read → subscriber) ≤ 3 s  ⟹  p95(read → accepted) ≤ 3 s
//!
//! and the requirement is discharged by a bound it cannot fail while this one
//! holds. That also makes the measurement immune to [#85]: it does not depend on
//! where inside the driver acceptance is declared.
//!
//! # What is NOT covered, stated here rather than left to be assumed
//!
//! **The 24-hour window.** This run takes about half a minute. Per-reading latency
//! does not depend on how long the bridge has been up, but a 24 h window under
//! nominal load also contains reconnections, broker restarts and whatever the
//! machine does at 03:00 — none of which a compressed run sees. [#102] records
//! that, and production is the only 24 h window this project has, exactly as
//! [ADR 0038] concluded for the leak gate.
//!
//! [ADR 0010]: ../../../docs/adr/0010-fr20-delivery-claim-at-qos0.md
//! [ADR 0038]: ../../../docs/adr/0038-the-leak-gate-measures-per-iteration-growth-not-a-24-hour-slope.md

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};

use smartme_bridge::app::poll_publish::Heartbeats;
use smartme_bridge::app::{BridgeConfig, PollConfig};
use smartme_bridge::core::clock::{Clock, SystemClock};
use smartme_bridge::core::source::{Reading, Source, SourceError, SourceFaults};
use smartme_bridge::core::state_machine::Policy;
use smartme_bridge::domain::{Kw, Kwh, Measurement, MeterId, Quality, Serial, UtcMillis};

const SERIAL: &str = "30000005";
const METER: &str = "garage";

/// Readings to time. p99 over 300 samples means the third-worst reading decides
/// it, which is enough for a threshold stated to one significant figure and small
/// enough to run in half a minute.
const READINGS: usize = 300;

/// How many readings this run times, and how much delay to inject.
///
/// **Both exist for the falsification** (`NFR10_READINGS`, `NFR10_INJECT_DELAY_MS`),
/// because a gate that passes with three orders of magnitude to spare has to
/// demonstrate that it can fail at all — and demonstrating it at 300 readings × 4 s
/// would take twenty minutes. The defaults are the criterion.
fn readings() -> usize {
    std::env::var("NFR10_READINGS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(READINGS)
}

fn injected_delay() -> Duration {
    Duration::from_millis(
        std::env::var("NFR10_INJECT_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
    )
}

/// The pacing. Not "nominal" — nominal is 30 s and would make this run two and a
/// half hours — but the interval does not enter the measurement: each reading is
/// timed from its own acquisition, not from the tick before it.
const PERIOD: Duration = Duration::from_millis(100);

const P95_BUDGET: Duration = Duration::from_secs(3);
const P99_BUDGET: Duration = Duration::from_secs(5);

struct ScratchDir(std::path::PathBuf);
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A source that stamps the instant it answers, keyed by the `ValueDate` it
/// answers with.
///
/// The `ValueDate` is the join key and it is unique per reading — which works
/// because ADR 0013 puts it in the published payload timestamp, so the subscriber
/// can name the reading it received without the bridge carrying a correlation id
/// for the benefit of a test.
struct Stopwatch {
    at: Arc<Mutex<HashMap<i64, Instant>>>,
    base: i64,
    n: i64,
    /// Injected between the stamp and the answer, so it lands INSIDE the measured
    /// interval exactly as a slow bridge would.
    delay: Duration,
}

impl Source for Stopwatch {
    async fn fetch(&mut self, meter: &MeterId) -> Result<Reading, SourceError> {
        self.n += 1;
        let value_date = self.base + self.n * 1_000;
        // Recorded BEFORE the reading is handed back, so the interval includes
        // every step the bridge takes and none of the harness's own bookkeeping.
        self.at
            .lock()
            .expect("not poisoned")
            .insert(value_date, Instant::now());
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        Ok(Reading {
            value: Measurement {
                meter: meter.clone(),
                serial: Serial::new(SERIAL),
                power: Some(Kw(0.018)),
                energy: Some(Kwh(4_843.0 + self.n as f64 * 0.001)),
                value_date: UtcMillis(value_date),
                quality: Quality::Good,
            },
            http_date: Some(UtcMillis(value_date)),
            faults: SourceFaults::default(),
        })
    }
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    debug_assert!(!sorted.is_empty());
    // Nearest-rank: the smallest value at or above p% of the samples. Stated
    // because "p95" without a rule is three different numbers.
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// **NFR10 — every reading reaches the wire well inside its budget.**
///
/// Reports before it asserts, for the reason story 4.15 states: a threshold met by
/// a wide margin and one met barely are different results.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "NFR10: times 300 readings end to end (~30 s); run by hand before a release"]
async fn a_reading_reaches_the_wire_inside_the_latency_budget() {
    let (_broker, broker_port) = common::start_broker().await;
    let mut seen = common::independent_subscriber(broker_port).await;

    let dir = std::env::temp_dir().join(format!("nfr10_latency_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("state dir");
    let dir = ScratchDir(dir);

    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(SystemClock::new());
    let node = sparkplug_b::EdgeNode::new("Site", "Nfr10Latency").expect("valid identifiers");
    let (tx, rx) = mpsc::channel(64);
    let (death_tx, death_rx) = oneshot::channel();
    let (device_tx, device_rx) = mpsc::channel(4);
    let meter = MeterId::new(METER);
    let pulse = Heartbeats::for_meters([meter.clone()]);

    let config = BridgeConfig {
        api_base: "https://192.0.2.1".to_string(),
        credentials: smart_me_client::Credentials::Basic {
            user: "u".to_string(),
            password: "p".to_string(),
        },
        http_timeout: Duration::from_secs(10),
        meters: vec![smartme_bridge::app::config::MeterConfig {
            meter: meter.clone(),
            device_id: "a1a1a1a1-b2b2-c3c3-d4d4-000000000005".to_string(),
            serial: Serial::new(SERIAL),
            enabled: true,
        }],
        group_id: "Site".to_string(),
        node_id: "Nfr10Latency".to_string(),
        broker_host: "127.0.0.1".to_string(),
        broker_port,
        bd_seq_path: dir.0.join("bdseq.toml"),
        poll: PollConfig {
            interval: PERIOD,
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
            client_id: "Nfr10Latency".to_string(),
            host: "127.0.0.1".to_string(),
            port: broker_port,
            keep_alive: Duration::from_secs(30),
            bd_seq_path: dir.0.join("bdseq.toml"),
            capacity: 64,
            death_flush: Duration::from_secs(2),
        },
        node,
        vec![Serial::new(SERIAL)],
        Arc::clone(&clock),
        pulse.clone(),
        rx,
        device_rx,
        death_rx,
    ));

    let at: Arc<Mutex<HashMap<i64, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
    let poller = tokio::spawn(smartme_bridge::app::poll_publish::run(
        smartme_bridge::app::poll_publish::PolledMeter {
            meter: meter.clone(),
            serial: Serial::new(SERIAL),
        },
        Stopwatch {
            at: Arc::clone(&at),
            base: 1_786_968_000_000,
            n: 0,
            delay: injected_delay(),
        },
        Arc::clone(&clock),
        handle,
        pulse.clone(),
        tx,
        dir.0.clone(),
        device_tx,
    ));

    // Collect until READINGS DDATA have been timed, or the deadline passes. The
    // deadline is generous on purpose: this test measures latency, and a test that
    // failed on its own impatience would be measuring the harness.
    let target = readings();
    let mut samples: Vec<Duration> = Vec::with_capacity(target);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    while samples.len() < target && tokio::time::Instant::now() < deadline {
        let Ok(Some(message)) = tokio::time::timeout(Duration::from_secs(30), seen.recv()).await
        else {
            break;
        };
        if !message.topic.contains("/DDATA/") {
            continue;
        }
        let Some(stamp) = message.payload.timestamp else {
            continue;
        };
        let arrived = Instant::now();
        if let Some(sent) = at.lock().expect("not poisoned").get(&(stamp as i64)) {
            samples.push(arrived.duration_since(*sent));
        }
    }

    assert!(
        samples.len() >= target,
        "ONLY {} OF {target} READINGS WERE TIMED, so no percentile below means what it says. \
         Either the bridge stopped publishing or the join key stopped matching — the payload \
         timestamp IS the reading's ValueDate (ADR 0013), and this test would notice that \
         changing before any consumer did",
        samples.len()
    );

    samples.sort_unstable();
    let p50 = percentile(&samples, 50.0);
    let p95 = percentile(&samples, 95.0);
    let p99 = percentile(&samples, 99.0);
    let worst = *samples.last().expect("at least one sample");

    println!(
        "NFR10 — {} readings timed from acquisition to an independent subscriber\n\
         NFR10 — p50 {:?}, p95 {:?} (budget {:?}, {:.1} % of it), p99 {:?} (budget {:?}, \
         {:.1} % of it), worst {:?}",
        samples.len(),
        p50,
        p95,
        P95_BUDGET,
        p95.as_secs_f64() / P95_BUDGET.as_secs_f64() * 100.0,
        p99,
        P99_BUDGET,
        p99.as_secs_f64() / P99_BUDGET.as_secs_f64() * 100.0,
        worst,
    );

    assert!(
        p95 <= P95_BUDGET,
        "p95 IS {p95:?}, BUDGET {P95_BUDGET:?}. This is the interval an operator sees between a \
         meter being read and the value being on a screen, and it is measured on a bound LARGER \
         than the requirement — read to a subscriber, not read to acceptance — so a failure here \
         is a failure of the real thing too"
    );
    assert!(
        p99 <= P99_BUDGET,
        "p99 IS {p99:?}, BUDGET {P99_BUDGET:?}. The tail is what an operator remembers"
    );

    let _ = death_tx.send(());
    poller.abort();
    let _ = tokio::time::timeout(Duration::from_secs(10), driver).await;
}
