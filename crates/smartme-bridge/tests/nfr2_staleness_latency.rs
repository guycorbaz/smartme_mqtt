//! Story 3.3 AC1 and AC2 — **NFR2, measured for the first time.**
//!
//! NFR2 says staleness is signalled no later than
//! `last_success + 2×poll_interval + publish_margin`, and until
//! [ADR 0028](../../../docs/adr/0028-publish-margin-is-the-fetch-timeout.md) on
//! 2026-08-08 `publish_margin` **had no value anywhere in the repository** — four
//! occurrences, always inside that formula, never with a number, a derivation or
//! a definition. A bound with a free term can be quoted but not met or missed,
//! and every use of NFR2 until now was an *argument* rather than a *threshold*.
//! This is the threshold.
//!
//! # Why this lives in `tests/` and not beside the code it measures
//!
//! It was written in `app::poll_publish`'s test module first, and `arch_purity`
//! rejected it: `Instant::now(` is confined to `core/clock.rs` **with the rule
//! applying inside test modules too** (`CONFINED_TOKENS`' third field is `true`
//! for it, deliberately, unlike `FakeClock`'s). Everything under `src/` reads
//! time through the `Clock` seam or not at all.
//!
//! A latency measurement needs a clock that advances, and `FakeClock` does not
//! follow tokio's virtual time. So the honest home is an integration test, where
//! the bridge is exercised from outside and `tokio::time` is the instrument
//! rather than a smuggled dependency. The guard was right and moving the test is
//! not a way around it: nothing in `src/` gained an `Instant::now(`.
//!
//! # What is measured, and where the wire begins
//!
//! The instant a judged verdict reaches the driver's channel. The wire is one
//! encode away (`try_publish`, QoS 0, non-blocking) — that is ADR 0028's `ε`, and
//! the margin has a whole period of slack above it. What this cannot see is a
//! driver that never delivers, which is `chaos_stale_on_cloud_timeout`'s job.

use std::sync::Arc;
use std::time::Duration;

use smartme_bridge::app::poll_publish::{Heartbeats, run};
use smartme_bridge::app::supervisor::{BridgeConfig, ConfigHandle};
use smartme_bridge::app::{PollConfig, config};
use smartme_bridge::core::clock::{Clock, FakeClock};
use smartme_bridge::core::source::{FakeSource, Reading};
use smartme_bridge::core::state_machine::Policy;
use smartme_bridge::domain::{Kw, Kwh, Measurement, MeterId, Quality, Serial, UtcMillis};
use tokio::sync::mpsc;

const SANE_NOW: i64 = 1_784_984_793_000;
const BASE: i64 = 1_784_984_700_000;

fn reading(meter: &MeterId, quality: Quality) -> Reading {
    Reading {
        value: Measurement {
            meter: meter.clone(),
            serial: Serial::new("30000001"),
            power: Kw(0.018),
            energy: Kwh(4_843.822),
            value_date: UtcMillis(BASE),
            quality,
        },
        // 950 ms of age: comfortably fresh under the shipped allowance, so the
        // verdicts below are about the OUTAGE and not about staleness of value.
        http_date: Some(UtcMillis(BASE + 950)),
    }
}

/// The pacing NFR2 binds at.
///
/// **`PERIOD_MIN`, not the shipped 30 s.** At 30 s this test would pass with a
/// `publish_margin` of zero and would assert nothing about ADR 0028's decision:
/// the requirement is `margin >= T - P`, which is satisfied by anything at all
/// once `P >= T`. Running this at the default period is the way to make it green
/// and pointless.
fn bridge_config() -> BridgeConfig {
    BridgeConfig {
        api_base: "https://192.0.2.1".to_string(),
        credentials: smart_me_client::Credentials::ClientCredentials {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
        },
        http_timeout: Duration::from_secs(10),
        meters: Vec::new(),
        group_id: "Plant".to_string(),
        node_id: "Bridge01".to_string(),
        broker_host: "192.0.2.1".to_string(),
        broker_port: 1883,
        bd_seq_path: std::path::PathBuf::from("/tmp/nfr2-bdseq.toml"),
        poll: PollConfig {
            interval: config::PERIOD_MIN,
            fetch_timeout: Duration::from_secs(10),
        },
        policy: Policy::DEFAULT,
        log_dir: None,
        log_keep: None,
        ui_port: None,
    }
}

/// FALSIFIED 2026-08-08, and the attempt that FAILED is the one worth reading.
///
/// **The ceiling itself** — a 6-second "let us retry before crying" backoff in
/// front of the bad news, in `step_once`'s publish arm. A plausible regression,
/// not an absurd one:
///
/// ```text
/// test a_silent_meters_verdict_arrives_inside_nfr2s_bound ... FAILED
///
/// NFR2: a silent meter must be signalled within 20000 ms of its last success
/// (2 x 5000 ms period + 10000 ms margin, ADR 0028); measured 21000 ms
/// ```
///
/// **The first attempt at that mutation changed nothing observable**: a
/// `continue` placed after `step_once` in `run`, which cannot delay anything —
/// `step_once` has already sent the update by the time it returns. The test
/// stayed green. *A mutation that changes nothing observable is not a
/// falsification*; story 3.1 paid for this exact lesson on its cadence test and
/// it cost a second round here.
///
/// **Freezing the measured instants** — `let at = 0` in the collection loop, the
/// shape of the Epic 1 defect where a fake clock never advanced:
///
/// ```text
/// the silent meter must be SIGNALLED, not merely stop being published:
/// a withheld verdict is the failure ADR 0027 exists to forbid
/// ```
///
/// It dies on the PRESENCE assertion rather than on the elapsed-time guard, and
/// the difference is worth naming: with every instant equal, no non-good update
/// can be `> last_success`, so the search finds nothing. The `elapsed` guard
/// covers a different failure — a runtime whose virtual clock does not advance at
/// all — and neither subsumes the other.
#[tokio::test(start_paused = true)]
async fn a_silent_meters_verdict_arrives_inside_nfr2s_bound() {
    let config = bridge_config();
    let period_ms = config.poll.interval.as_millis() as i64;
    // ADR 0028: the margin IS the fetch timeout. Read from the configuration
    // rather than written as a number, so the test cannot drift from the
    // decision it exists to check.
    let margin_ms = config.poll.fetch_timeout.as_millis() as i64;
    assert_eq!(
        period_ms,
        config::PERIOD_MIN.as_millis() as i64,
        "this test only measures anything at the MINIMUM period"
    );
    let ceiling = 2 * period_ms + margin_ms;

    let clock = Arc::new(FakeClock::new(UtcMillis(SANE_NOW)));
    let (tx, mut rx) = mpsc::channel(64);
    let healthy = [
        MeterId::new("garage"),
        MeterId::new("cellar"),
        MeterId::new("attic"),
    ];
    let silent = MeterId::new("unplugged");
    let all: Vec<MeterId> = healthy.iter().cloned().chain([silent.clone()]).collect();
    let beats = Heartbeats::for_meters(all.clone());
    let handle: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(config));

    let started = tokio::time::Instant::now();
    let mut tasks = Vec::new();
    for meter in &all {
        // The reading must carry ITS OWN meter id: story 3.1's first cadence test
        // read [9, 0, 0] because a shared fixture hard-coded one, so every task's
        // update arrived labelled "garage".
        //
        // The silent meter answers ONCE and then stops. That first Good is what
        // makes this a latency measurement rather than a cold start — NFR2 counts
        // from `last_success`, and a meter that never answered has none (Story
        // 3.2 AC4 gives it nothing, deliberately).
        let source = if *meter == silent {
            FakeSource::new()
                .then(Ok(reading(meter, Quality::Good)))
                .then_hang()
                .then_hang()
                .then_hang()
        } else {
            let mut s = FakeSource::new();
            for _ in 0..6 {
                s = s.then(Ok(reading(meter, Quality::Good)));
            }
            s
        };
        tasks.push(tokio::spawn(run(
            meter.clone(),
            source,
            Arc::clone(&clock) as Arc<dyn Clock + Send + Sync>,
            Arc::clone(&handle),
            beats.clone(),
            tx.clone(),
        )));
    }
    drop(tx);

    let mut seen: Vec<(MeterId, Quality, i64)> = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(25_000);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            break;
        }
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(update)) => {
                let at = (tokio::time::Instant::now() - started).as_millis() as i64;
                seen.push((update.meter.clone(), update.published(), at));
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    for task in &tasks {
        task.abort();
    }

    // THE PREMISE. Four Epic 1 tests passed for the wrong reason, one of them
    // over a fake clock that never advanced: every duration measured against a
    // frozen clock is zero and every upper bound holds.
    let elapsed = (tokio::time::Instant::now() - started).as_millis() as i64;
    assert!(
        elapsed >= 15_000,
        "the clock must ADVANCE before any latency measured against it means \
         anything: {elapsed} ms elapsed over a run that should span at least 15000 ms"
    );

    // AC1 — the silent meter's own timeline.
    let last_success = seen
        .iter()
        .filter(|(m, q, _)| *m == silent && *q == Quality::Good)
        .map(|(_, _, at)| *at)
        .next_back()
        .expect(
            "the silent meter must answer ONCE, or there is no `last_success` to \
             measure from and this test is about a cold start instead",
        );
    let first_non_good = seen
        .iter()
        .filter(|(m, q, at)| *m == silent && *q != Quality::Good && *at > last_success)
        .map(|(_, _, at)| *at)
        .next()
        .expect(
            "the silent meter must be SIGNALLED, not merely stop being published: \
             a withheld verdict is the failure ADR 0027 exists to forbid",
        );
    let latency = first_non_good - last_success;
    assert!(
        latency <= ceiling,
        "NFR2: a silent meter must be signalled within {ceiling} ms of its last \
         success (2 x {period_ms} ms period + {margin_ms} ms margin, ADR 0028); \
         measured {latency} ms"
    );
    // The observed figure is RECORDED, not asserted as the bound.
    //
    // ADR 0028 keeps NFR2's ceiling deliberately looser than what the bridge
    // achieves — one period, not two, since ADR 0027 made a single missed tick
    // enough. Tightening the requirement to today's behaviour would leave nothing
    // able to catch a regression. So this prints rather than fails, and a change
    // that doubles the real latency is visible without being an error.
    println!(
        "NFR2 observed: {latency} ms against a {ceiling} ms ceiling \
         (period {period_ms} ms, margin {margin_ms} ms)"
    );

    // AC2 — and the three others were fine the whole time.
    //
    // Asserted PER METER and by name. "Exactly one meter went stale" is satisfied
    // by the WRONG meter being the stale one.
    for meter in &healthy {
        let theirs: Vec<Quality> = seen
            .iter()
            .filter(|(m, _, at)| m == meter && *at <= first_non_good)
            .map(|(_, q, _)| *q)
            .collect();
        assert!(
            !theirs.is_empty(),
            "{meter} published nothing at all, so 'it stayed fresh' would be a \
             claim about silence"
        );
        assert!(
            theirs.iter().all(|q| *q == Quality::Good),
            "{meter} was dragged out of Good by ANOTHER meter's outage, which is \
             FR12 failing: {theirs:?}"
        );
    }
}
