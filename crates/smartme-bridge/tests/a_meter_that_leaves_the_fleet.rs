//! Story 3.5 — the fleet topology says absence, driven through `run` itself.
//!
//! Two leavings, and the asymmetry between them is the design (ADR 0034):
//!
//! - **the operator disables a meter** — the API stops being asked, the log
//!   stops filling, and the alarm the operator aimed at is retired with the
//!   meter ([#65]'s three never-decided consequences); re-enabling judges
//!   afresh, Stale-until-proven;
//! - **the account refuses the device** — one DDEATH ends the device on the
//!   wire, then silence (the certificate IS the publication, ADR 0027 §3), and
//!   the alarm STAYS: the account saying "gone" is a fault someone must fix.
//!
//! Driven at the loop level, not against `step_once`: every decision under test
//! here lives in `run` (the enabled read, the retirement, the certification,
//! the post-certificate silence), and Epic 2's recurring review finding was a
//! property tested one layer above — or below — where it lives.
//!
//! What this file deliberately does NOT re-test: the disable-DDEATH itself is
//! `reconfigure`'s (`classify_meters`, exercised against a real broker by
//! `chaos_device_certificates`), and the bridge-death ending is the will's
//! (`register_will`'s QoS-1 tests, story 4.17). Together with the certificate
//! asserted here, those are the three endings AC4 requires to be
//! distinguishable — each pinned where it is produced.

use std::sync::Arc;
use std::time::Duration;

use smartme_bridge::app::config::MeterConfig;
use smartme_bridge::app::mqtt_driver::DeviceCommand;
use smartme_bridge::app::poll_publish::{Heartbeats, PolledMeter, run};
use smartme_bridge::app::supervisor::{BridgeConfig, ConfigHandle};
use smartme_bridge::app::{PollConfig, config};
use smartme_bridge::core::clock::{Clock, FakeClock};
use smartme_bridge::core::oracle::Cause;
use smartme_bridge::core::source::{FakeSource, Reading, Refusal, SourceError};
use smartme_bridge::core::state_machine::Policy;
use smartme_bridge::domain::{Kw, Kwh, Measurement, MeterId, Quality, Serial, UtcMillis};
use tokio::sync::mpsc;

const SANE_NOW: i64 = 1_784_984_793_000;
const BASE: i64 = 1_784_984_700_000;
const PERIOD_MS: i64 = 5_000;

fn reading(meter: &MeterId, tick: i64) -> Reading {
    let shift = tick * PERIOD_MS;
    Reading {
        value: Measurement {
            meter: meter.clone(),
            serial: Serial::new("9202685"),
            power: Some(Kw(0.7)),
            energy: Some(Kwh(4_843.822)),
            value_date: UtcMillis(BASE + shift),
            quality: Quality::Good,
        },
        http_date: Some(UtcMillis(BASE + shift + 950)),
        faults: smartme_bridge::core::source::SourceFaults::NONE,
    }
}

fn fleet_config(enabled: bool) -> BridgeConfig {
    fleet_config_at(enabled, config::PERIOD_MIN)
}

fn fleet_config_at(enabled: bool, interval: Duration) -> BridgeConfig {
    BridgeConfig {
        api_base: "https://192.0.2.1".to_string(),
        credentials: smart_me_client::Credentials::ClientCredentials {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
        },
        http_timeout: Duration::from_secs(10),
        meters: vec![MeterConfig {
            meter: MeterId::new("garage"),
            device_id: "dev-garage".to_string(),
            serial: Serial::new("9202685"),
            enabled,
        }],
        group_id: "Plant".to_string(),
        node_id: "Bridge01".to_string(),
        broker_host: "192.0.2.1".to_string(),
        broker_port: 1883,
        bd_seq_path: std::path::PathBuf::from("/tmp/leaves-bdseq.toml"),
        poll: PollConfig {
            interval,
            fetch_timeout: Duration::from_secs(2),
        },
        policy: Policy::DEFAULT,
        log_dir: None,
        log_keep: None,
        ui_port: None,
    }
}

fn refs_dir(purpose: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("smartme_leaves_{purpose}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// **AC1 + AC2 — disabling stops the asking and retires the alarm; re-enabling
/// judges afresh.**
///
/// The proof that no fetch happens while disabled is the script itself: Good
/// readings are QUEUED behind the latching error, so a single fetch while
/// disabled would surface as an update — and none arrives. The proof that
/// re-enabling resets the latch is sharper still: the first update after
/// re-enable is published **Good**, which `State::Failed` makes impossible
/// (`prev == Failed` maps every tick to `Bad`), so a Good here can only mean
/// the state went back to `initial()`.
#[tokio::test(start_paused = true)]
async fn a_disabled_meters_alarm_retires_with_it_and_a_re_enable_judges_afresh() {
    let meter = MeterId::new("garage");
    let handle: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(fleet_config(true)));
    let clock = Arc::new(FakeClock::new(UtcMillis(SANE_NOW)));
    let beats = Heartbeats::for_meters([meter.clone()]);
    let (tx, mut rx) = mpsc::channel(16);
    let (device_tx, mut device_rx) = mpsc::channel(4);

    let source = FakeSource::new()
        // Tick 1: a credential latch — the fault the operator will aim at.
        .then(Err(SourceError::Fatal {
            refusal: Refusal::Credential,
            reason: "auth rejected".to_string(),
        }))
        // Queued behind it: readings that WOULD publish if any fetch happened
        // while the meter is disabled.
        .then(Ok(reading(&meter, 1)))
        .then(Ok(reading(&meter, 2)))
        .then(Ok(reading(&meter, 3)));

    let task = tokio::spawn(run(
        PolledMeter {
            meter: meter.clone(),
            serial: Serial::new("9202685"),
        },
        source,
        Arc::clone(&clock) as Arc<dyn Clock + Send + Sync>,
        Arc::clone(&handle),
        beats.clone(),
        tx,
        refs_dir("disable"),
        device_tx,
    ));

    // THE PREMISE: the latch lands and the surfaces carry it.
    let latched = tokio::time::timeout(Duration::from_secs(30), rx.recv())
        .await
        .expect("the first tick publishes")
        .expect("channel open");
    assert_eq!(latched.published(), Quality::Bad, "the premise: latched");
    assert_eq!(
        beats.snapshot().failed().len(),
        1,
        "the premise: the alarm is on the surfaces"
    );

    // THE OPERATOR'S GESTURE: disable.
    handle.store(Arc::new(fleet_config(false)));
    clock.advance_ms(60_000); // FakeClock does not follow tokio's virtual time
    tokio::time::sleep(Duration::from_secs(60)).await;

    assert!(
        rx.try_recv().is_err(),
        "a disabled meter publishes nothing — and the Good readings queued in \
         the script prove no fetch happened either, or one of them would have \
         surfaced here (as Bad under the latch, but surfaced)"
    );
    assert!(
        beats.snapshot().failed().is_empty(),
        "the alarm the operator aimed at is retired with the meter ([#65] \
         item 3) — a disabled meter must not go on being accused on /healthz"
    );
    let ticked_at = beats.snapshot().of(&meter).and_then(|m| m.last_tick);
    clock.advance_ms(20_000);
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_ne!(
        beats.snapshot().of(&meter).and_then(|m| m.last_tick),
        ticked_at,
        "idling on purpose is not wedging: the heartbeat keeps beating while \
         the meter is disabled, or Epic 7's healthcheck would restart a \
         container for a meter the operator removed"
    );
    assert!(
        device_rx.try_recv().is_err(),
        "the poll task sends NO certificate on disable — that DDEATH is \
         reconfigure's (`classify_meters`), and two senders for one ending \
         would race"
    );

    // THE RETURN: re-enable, and the first update is GOOD — impossible under a
    // carried latch, so this asserts the reset, not just the resumption.
    handle.store(Arc::new(fleet_config(true)));
    let resumed = tokio::time::timeout(Duration::from_secs(30), rx.recv())
        .await
        .expect("polling resumes on re-enable")
        .expect("channel open");
    assert_eq!(
        resumed.published(),
        Quality::Good,
        "re-enabling judges afresh from Stale-until-proven: a carried \
         `Failed` would have made this Bad by construction, and if the fault \
         is still real the next fetch re-latches loudly on its own"
    );

    task.abort();
}

/// **The review-of-the-repair's pin: an idle loop still re-paces.** The hoist
/// of the period-rebuild above the skips was the round's gravest repair and no
/// test held it — reverting it left the whole suite green. This is the pin: a
/// DISABLED meter under a slow period gets a hot interval shrink, and both
/// halves of the honest cadence are asserted — the recorded `period_ms` is the
/// new ask AND the ticks actually arrive at it. Under the reverted hoist the
/// ticker stays parked on the old period while recording the new one, which is
/// `loop_age`'s false-wedge denominator, fleet-wide.
///
/// FALSIFIED 2026-08-15, the revert RUN before this note: moving the rebuild
/// back below the skips goes RED here — no tick lands inside the whole window
/// while `period_ms` claims the short pace.
#[tokio::test(start_paused = true)]
async fn an_idle_meter_still_repaces_when_the_interval_changes() {
    let meter = MeterId::new("garage");
    let slow = Duration::from_secs(300);
    let handle: ConfigHandle =
        Arc::new(arc_swap::ArcSwap::from_pointee(fleet_config_at(true, slow)));
    let clock = Arc::new(FakeClock::new(UtcMillis(SANE_NOW)));
    let beats = Heartbeats::for_meters([meter.clone()]);
    let (tx, mut rx) = mpsc::channel(16);
    let (device_tx, _device_rx) = mpsc::channel(4);

    let source = FakeSource::new().then(Ok(reading(&meter, 0)));
    let task = tokio::spawn(run(
        PolledMeter {
            meter: meter.clone(),
            serial: Serial::new("9202685"),
        },
        source,
        Arc::clone(&clock) as Arc<dyn Clock + Send + Sync>,
        Arc::clone(&handle),
        beats.clone(),
        tx,
        refs_dir("repace"),
        device_tx,
    ));
    // One good tick at the slow pace, then the operator disables the meter and
    // hot-shortens the interval — the reviewed scenario.
    let _ = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("the first tick publishes");
    handle.store(Arc::new(fleet_config_at(false, Duration::from_secs(5))));

    // A period change takes effect at the NEXT tick of the OLD period — the
    // loop is parked in `ticker.tick()` until then, exactly as the enabled
    // path has always documented ("the next tick uses the new one"). The
    // reviewed false-wedge begins AFTER that tick: the old code then recorded
    // the 5 s ask while staying parked on 300 s. So the window under test
    // opens past the first slow tick.
    clock.advance_ms(301_000);
    tokio::time::sleep(Duration::from_secs(301)).await;
    let before = beats.snapshot().of(&meter).and_then(|m| m.last_tick);
    clock.advance_ms(30_000);
    tokio::time::sleep(Duration::from_secs(30)).await;
    let cell = beats.snapshot();
    let state = cell.of(&meter).expect("served");
    assert_ne!(
        state.last_tick, before,
        "thirty seconds passed under a five-second ask and the idle loop never \
         ticked: the ticker is parked on the OLD period while the cell records \
         the new one — the false-wedge denominator, fleet-wide, reintroduced"
    );
    assert_eq!(
        state.period_ms, 5_000,
        "and the recorded pace is the pace the ticks actually arrive at — the \
         `period_ms` doc's own invariant"
    );
    task.abort();
}

/// **AC3 — the account's refusal ends the device: one certificate, then
/// silence, and the alarm STAYS.**
///
/// The asymmetry with disable is asserted on both sides: here `failed()` keeps
/// naming the meter for as long as the latch holds (the account saying "gone"
/// is a fault someone must fix), while the wire's device is ended by exactly
/// one `DeviceCommand::Death` — and the Good readings queued behind the
/// refusal prove the loop stopped asking a question whose answer cannot
/// change an absorbing latch.
#[tokio::test(start_paused = true)]
async fn a_device_the_account_refuses_ends_with_one_certificate_and_the_alarm_stays() {
    let meter = MeterId::new("garage");
    let handle: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(fleet_config(true)));
    let clock = Arc::new(FakeClock::new(UtcMillis(SANE_NOW)));
    let beats = Heartbeats::for_meters([meter.clone()]);
    let (tx, mut rx) = mpsc::channel(16);
    let (device_tx, mut device_rx) = mpsc::channel(4);
    // THE DIVERGENCE IS STAGED (the review-of-the-repair's pin): the spawn-time
    // serial differs from the stored row's, as it does after a serial edit is
    // saved but not yet restarted into force. The certificate must name the
    // device the DBIRTH used — the spawn serial — or it buries a device the
    // wire never birthed while the born one stays alive for ever.
    let born_serial = Serial::new("1111111");

    let source = FakeSource::new()
        .then(Err(SourceError::Fatal {
            refusal: Refusal::DeviceNotInAccount,
            reason: "smart-me does not know device dev-garage".to_string(),
        }))
        // Queued behind the refusal: proof the loop stops fetching after the
        // certificate — any fetch would surface an update.
        .then(Ok(reading(&meter, 1)))
        .then(Ok(reading(&meter, 2)));

    let task = tokio::spawn(run(
        PolledMeter {
            meter: meter.clone(),
            serial: born_serial.clone(),
        },
        source,
        Arc::clone(&clock) as Arc<dyn Clock + Send + Sync>,
        Arc::clone(&handle),
        beats.clone(),
        tx,
        refs_dir("gone"),
        device_tx,
    ));

    // THE SEPARATION IS ASSERTED (the review-of-the-repair's pin): the latch
    // tick queues the verdict; the certificate must NOT share that tick, or
    // verdict and Death race through the driver's unbiased `select!` and the
    // final `device-not-in-account` can be dropped as undeclared. One short
    // sleep lets the latch tick run without reaching the next period.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        beats.snapshot().of(&meter).and_then(|m| m.verdict),
        Some(smartme_bridge::core::state_machine::State::Failed),
        "the premise: the latch tick has run"
    );
    assert!(
        device_rx.try_recv().is_err(),
        "the certificate must not share the latch verdict's tick — the one          period of separation IS the ordering"
    );

    // Then the ending: exactly one certificate, one period later. (The latch
    // itself reaches no outbox update here — a meter that has NEVER answered
    // has nothing to republish, story 3.2 AC4 — so the latch is attested on
    // the snapshot, which is where the operator surfaces read it anyway.)
    let ended = tokio::time::timeout(Duration::from_secs(30), device_rx.recv())
        .await
        .expect("the certificate follows the latch")
        .expect("channel open");
    assert_eq!(
        ended,
        DeviceCommand::Death(born_serial),
        "the certificate names the SPAWN-TIME serial — the device the DBIRTH          used — never the stored row's, which may hold an edit not yet          restarted into force"
    );
    let cell = beats.snapshot();
    let published = cell
        .of(&meter)
        .and_then(|m| m.published)
        .expect("the latch verdict is recorded for the surfaces");
    assert_eq!(published.quality(), Quality::Bad);
    assert_eq!(
        published.cause(),
        Some(Cause::DeviceNotInAccount),
        "the refusal names itself — the row or the account, not the file's \
         plumbing (the story 2.6 rule, applied by the 3.5 split)"
    );

    // And after the ending: silence on both channels, alarm still on.
    tokio::time::sleep(Duration::from_secs(90)).await;
    assert!(
        rx.try_recv().is_err(),
        "after the certificate, silence IS the publication (ADR 0027 §3): \
         republishing Bad would call a GONE device merely misbehaving, and \
         the queued Good readings prove no fetch asked the unanswerable \
         question again"
    );
    assert!(
        device_rx.try_recv().is_err(),
        "ONE certificate — a death per tick would hammer the session with \
         re-burials"
    );
    assert_eq!(
        beats.snapshot().failed().len(),
        1,
        "the alarm STAYS: the certificate retires the wire's device, never \
         the operator's alarm — the account saying 'gone' is a fault someone \
         must fix, and this is the deliberate asymmetry with disable"
    );

    task.abort();
}
