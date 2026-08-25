//! Story 5.2 AC4 — enabling and disabling a meter costs a device certificate,
//! not a session.
//!
//! # What an independent subscriber must see
//!
//! Disable a meter: a **DDEATH** for that device, and nothing else. Enable it
//! again: a **DBIRTH**, and nothing else. Across both, **exactly one NBIRTH** —
//! the one the connect produced — and one `bdSeq` throughout. A host watching
//! this sees one device come and go while the node it hangs off never blinks.
//!
//! # The norm, read rather than remembered
//!
//! > *"A Device can publish a DBIRTH as long as an NBIRTH has been sent
//! > previously and the MQTT session is active."*
//! > — `Sparkplug_5_Operational_Behavior.adoc:409`,
//! > `tck-id-message-flow-device-birth-publish-nbirth-wait`
//!
//! and for the other direction:
//!
//! > *"If at any time the Sparkplug Device cannot provide real time information,
//! > the Sparkplug Specification requires that an DDEATH be published."*
//! > — `Sparkplug_5_Operational_Behavior.adoc:470`
//!
//! A meter an operator has just switched off is exactly a device that cannot
//! provide real-time information. Stopping quietly would leave its last value on
//! the host's screen, current-looking and wrong, until the session ended.
//!
//! # Why the absence assertion here is not the vacuous kind
//!
//! *"No NBIRTH appeared"* would hold trivially over a broker nothing ever
//! connected to — the shape that has already produced a false pass in this
//! project. It cannot here: the test **counts** NBIRTHs and requires exactly
//! one, so the count proves the stream carried births at all before it proves it
//! carried no more of them.

mod common;

use std::time::Duration;

use smart_me_client::Credentials;
use smartme_bridge::app::{BridgeConfig, PollConfig};
use smartme_bridge::core::state_machine::Policy;
use smartme_bridge::domain::{MeterId, Serial};

const SERIAL: &str = "30000001";
/// What that meter is CALLED on the wire — the device level of every topic this
/// test waits for, since contract v13 (ADR 0049).
const METER: &str = "garage";
const NODE: &str = "ChaosDeviceCerts";

fn config(port: u16, state_dir: &std::path::Path) -> BridgeConfig {
    BridgeConfig {
        // TEST-NET-1 (RFC 5737): unroutable, so the cloud stays silent. The
        // Sparkplug session does not depend on having a reading.
        api_base: "https://192.0.2.1".to_string(),
        credentials: Credentials::Basic {
            user: "u".to_string(),
            password: "p".to_string(),
        },
        // Not under a second: the client refuses a timeout that would
        // instant-fail every request, and a swallowed StartupError made this
        // test say only "the control never arrived".
        http_timeout: Duration::from_secs(30),
        meters: vec![smartme_bridge::app::config::MeterConfig {
            priority: false,
            meter: MeterId::new(METER),
            device_id: "a1a1a1a1-b2b2-c3c3-d4d4-000000000001".to_string(),
            serial: Serial::new(SERIAL),
            enabled: true,
        }],
        group_id: "ChaosTest".to_string(),
        node_id: NODE.to_string(),
        broker_host: "127.0.0.1".to_string(),
        broker_port: port,
        bd_seq_path: state_dir.join("bdseq.toml"),
        poll: PollConfig {
            interval: Duration::from_millis(500),
            fetch_timeout: Duration::from_millis(500),
        },
        policy: Policy::DEFAULT,
        log_dir: None,
        log_keep: None,
        ui_port: None,
    }
}

/// FALSIFIED 2026-08-04, twice, and the second mutation is the one that matters.
///
/// `Control::apply` mutated to send no `Death` command — the test waited the full
/// 30 s and reported the wire, not a channel:
///
/// ```text
/// test chaos_enabling_and_disabling_a_meter_costs_certificates_not_a_session ... FAILED
/// a disabled meter owes a DDEATH: the host must be told to mark its metrics
/// STALE rather than left showing the last value as current
/// ... finished in 31.12s
/// ```
///
/// Then the driver's `Birth` arm mutated to answer with a FULL `announce` —
/// exactly the plausible implementation AC4 forbids, and the one that would look
/// right in every log:
///
/// ```text
/// assertion `left == right` failed: the node was re-born 2 times; enabling or
/// disabling a DEVICE must not cost a node certificate.
///   left: 2
///  right: 1
/// ```
///
/// The first green run finished in 0.59 s, which was suspicious enough to be
/// worth checking rather than trusting. The 31 s failure above is what shows the
/// speed was a warm container and not a test that measured nothing.
///
/// FALSIFIED AGAIN 2026-08-05, for the session-number assertion, which a review
/// had found to be `x == x`. The first mutation tried — `publisher.new_session()`
/// on a device birth — **failed at the wrong assertion**: it left the publisher
/// pending so no DBIRTH went out, and the test died on *"re-enabling the meter
/// announces it again"*. Right colour, wrong reason, and it would have proved
/// nothing about the line under repair.
///
/// The isolating mutation moves the persisted number and NOTHING else — the
/// DBIRTH still goes out, the NBIRTH count is still one:
///
/// ```text
/// the persisted session number is "bd_seq = 42\n", and the birth the host saw
/// carried 1. A device certificate must not open a new session
/// ```
#[tokio::test(flavor = "multi_thread")]
async fn chaos_enabling_and_disabling_a_meter_costs_certificates_not_a_session() {
    let (_broker, port) = common::start_broker().await;
    let mut seen = common::independent_subscriber(port).await;

    let state_dir = std::env::temp_dir().join(format!("chaos_device_certs_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state_dir);
    std::fs::create_dir_all(&state_dir).expect("state dir");

    let config = config(port, &state_dir);
    let (control_tx, control_rx) = tokio::sync::oneshot::channel();
    let bridge = tokio::spawn(async move {
        let outcome = smartme_bridge::app::supervisor::run_with_control(
            config,
            std::future::pending::<()>(),
            move |control| {
                let _ = control_tx.send(control);
            },
        )
        .await;
        // Surfaced rather than swallowed. A `let _ =` here cost a debugging
        // round: the supervisor refused to start, the control was never sent,
        // and all the test could say was that a channel had closed.
        if let Err(error) = outcome {
            eprintln!("the supervisor refused to start: {error}");
        }
    });
    let control = control_rx
        .await
        .expect("the supervisor hands out its control");

    // The session, and the premise for everything below.
    let birth = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await
    .expect("the node birth reaches an independent subscriber");
    let bd_seq = birth.bd_seq().expect("a birth carries its session number");
    // Counted through a cell: `wait_for` takes an `Fn`, and the count has to
    // survive two separate waits.
    let nbirths = std::cell::Cell::new(1_u32);

    common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/DBIRTH/") && s.topic.ends_with(METER)
    })
    .await
    .expect("the device is declared at connect");

    // ---- disable the meter -------------------------------------------------
    let mut off = control.current().as_ref().clone();
    off.meters[0].enabled = false;
    control.apply(off).await;

    let death = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        if s.topic.contains("/NBIRTH/") {
            nbirths.set(nbirths.get() + 1);
        }
        s.topic.contains("/DDEATH/")
    })
    .await
    .expect(
        "a disabled meter owes a DDEATH: the host must be told to mark its metrics \
         STALE rather than left showing the last value as current",
    );
    assert!(
        death.topic.ends_with(METER),
        "the certificate must name the device that went away, not another: {}",
        death.topic
    );

    // ---- enable it again ---------------------------------------------------
    let on = {
        let mut c = control.current().as_ref().clone();
        c.meters[0].enabled = true;
        c
    };
    control.apply(on).await;

    let reborn = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        if s.topic.contains("/NBIRTH/") {
            nbirths.set(nbirths.get() + 1);
        }
        s.topic.contains("/DBIRTH/")
    })
    .await
    .expect("re-enabling the meter announces it again");
    assert!(
        reborn.topic.ends_with(METER),
        "wrong device re-announced: {}",
        reborn.topic
    );

    // ---- and the node never blinked ---------------------------------------
    //
    // ONE NBIRTH, counted rather than assumed absent. A bridge that had
    // reconnected — or answered with a full rebirth — would have produced a
    // second one here, and the whole point of AC4 is that neither happens.
    let nbirths = nbirths.get();
    assert_eq!(
        nbirths, 1,
        "the node was re-born {nbirths} times; enabling or disabling a DEVICE must \
         not cost a node certificate. tck-id-message-flow-device-birth-publish-nbirth-wait \
         allows a mid-session DBIRTH precisely so that it does not"
    );
    // THE SESSION NUMBER, checked against something the test did not already
    // hold.
    //
    // This read `assert_eq!(birth.bd_seq(), bd_seq)` until a review on
    // 2026-08-04 — and `bd_seq` had been taken FROM `birth`, which is never
    // reassigned. It was `x == x`: unfalsifiable, and the exact defect this
    // repository's own rules cite from Epic 1 ("a bdSeq comparison of a constant
    // against itself"), reintroduced by someone who had read that line.
    //
    // `bdseq.toml` is an INDEPENDENT witness: the driver rewrites it once per
    // CONNECT, before registering the will. If it still holds the number the
    // birth carried, no second session was opened — which is the property, and
    // it cannot be satisfied by re-parsing the same buffer.
    let persisted = std::fs::read_to_string(state_dir.join("bdseq.toml"))
        .expect("the driver persists the session number once per connect");
    assert!(
        persisted.contains(&bd_seq.to_string()),
        "the persisted session number is {persisted:?}, and the birth the host \
         saw carried {bd_seq}. A device certificate must not open a new session"
    );

    bridge.abort();
    let _ = std::fs::remove_dir_all(&state_dir);
}
