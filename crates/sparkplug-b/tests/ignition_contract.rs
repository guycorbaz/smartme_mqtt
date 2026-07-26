//! Tier 3 — the deciding oracle for the hand-rolled protobuf.
//!
//! Every other test in this crate checks the encoder against itself: round-trip
//! through our own decoder, property tests over our own invariants. That proves
//! self-consistency and nothing about conformance. A codec can be perfectly
//! self-consistent and still be rejected — or worse, silently misread — by a
//! real Sparkplug host. This test is the only thing that closes that gap, and
//! it closes it by putting real bytes in front of a real consumer and asking a
//! human what it saw.
//!
//! It is therefore **manual and interactive**. It publishes a scripted session,
//! stops at each step, and tells you exactly what to look for. There is no
//! automated assertion here worth the name: the assertion is your eyes on the
//! tag browser.
//!
//! # Running it
//!
//! ```text
//! SPARKPLUG_CONTRACT_BROKER=host:1883 \
//! SPARKPLUG_CONTRACT_GROUP=ContractTest \
//!   cargo test -p sparkplug-b --test ignition_contract -- --ignored --nocapture
//! ```
//!
//! `--nocapture` is not optional: without it you see none of the prompts and the
//! test appears to hang.
//!
//! # Why it refuses to guess its target
//!
//! There is no default broker and no default group, and it will not publish into
//! a group called `Site`. A Sparkplug host persists what it discovers: whatever
//! group you name becomes a folder in its tag tree that outlives this test and
//! has to be deleted by hand. Publishing that into a production namespace by
//! accident is not recoverable by re-running anything.
//!
//! Clean-up is part of the procedure, not an afterthought — see
//! `docs/ignition-contract-runbook.md`.

use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use sparkplug_b::{
    BdSeq, EdgeNode, MessageType, Metric, MetricValue, NodeSession, Quality, encode,
};

const NODE_ID: &str = "ContractNode";
const DEVICE: &str = "30000001";

const METRIC_POWER: &str = "Power";
const METRIC_ENERGY: &str = "Energy";
const UNIT_POWER: &str = "kW";
const UNIT_ENERGY: &str = "kWh";

/// Values chosen to be unmistakable by eye. Nothing round, nothing that could be
/// confused with a default, a placeholder or a real reading.
const POWER_FIRST: f64 = 1.234;
const POWER_SECOND: f64 = 2.345;
const ENERGY_FIRST: f64 = 5678.9;
const ENERGY_SECOND: f64 = 5679.1;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_millis() as u64
}

/// Prints what to check and waits. The pause is the point: an automated run
/// would prove only that the broker accepted the bytes.
fn checkpoint(step: &str, look_for: &[&str]) {
    println!("\n──────────────────────────────────────────────────────────────");
    println!("  {step}");
    println!("──────────────────────────────────────────────────────────────");
    for item in look_for {
        println!("  [ ] {item}");
    }
    print!("\n  Press Enter when you have checked the above (or Ctrl-C to abort)… ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

fn power(value: f64, quality: Quality, ts: u64) -> Metric {
    Metric::new(METRIC_POWER, MetricValue::Double(value), ts)
        .with_quality(quality)
        .with_engineering_unit(UNIT_POWER)
}

fn energy(value: f64, quality: Quality, ts: u64) -> Metric {
    Metric::new(METRIC_ENERGY, MetricValue::Double(value), ts)
        .with_quality(quality)
        .with_engineering_unit(UNIT_ENERGY)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "manual Tier-3 gate: publishes to a real broker for a human to inspect"]
async fn ignition_contract() {
    let target = std::env::var("SPARKPLUG_CONTRACT_BROKER")
        .expect("set SPARKPLUG_CONTRACT_BROKER=host:port — there is deliberately no default");
    let group = std::env::var("SPARKPLUG_CONTRACT_GROUP").expect(
        "set SPARKPLUG_CONTRACT_GROUP to a disposable group — there is deliberately no default",
    );
    assert_ne!(
        group, "Site",
        "refusing to publish a contract-test node into the default production group"
    );
    let (host, port) = target
        .rsplit_once(':')
        .expect("SPARKPLUG_CONTRACT_BROKER must be host:port");
    let port: u16 = port.parse().expect("the port must be a number");

    let node = EdgeNode::new(group.clone(), NODE_ID.to_string()).expect("valid identifiers");
    let n_birth = node.node_topic(MessageType::NBirth).expect("node topic");
    let n_death = node.node_topic(MessageType::NDeath).expect("node topic");
    let d_birth = node
        .device_topic(MessageType::DBirth, DEVICE)
        .expect("device topic");
    let d_data = node
        .device_topic(MessageType::DData, DEVICE)
        .expect("device topic");

    println!("\n=== Tier-3 Ignition contract test ===");
    println!("broker : {host}:{port}");
    println!("topics : {n_birth}");
    println!("         {d_birth}");
    println!("\nThis publishes a disposable node. Clean-up instructions are at the end.");

    // The session, and its death certificate built BEFORE connecting — the same
    // boot order the bridge uses, because that ordering is part of what is
    // being validated.
    let session = NodeSession::start(BdSeq::new(7));
    let will = session.will(now_ms());

    let mut options = MqttOptions::new("sparkplug-contract-test", host, port);
    options.set_keep_alive(Duration::from_secs(30));
    options.set_last_will(rumqttc::LastWill::new(
        n_death.clone(),
        encode(&will),
        QoS::AtMostOnce,
        false,
    ));
    let (client, mut eventloop) = AsyncClient::new(options, 32);

    // The event loop must keep turning for anything to reach the wire.
    let pump = tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => println!("  → connected"),
                Ok(_) => {}
                Err(error) => {
                    println!("  → transport: {error}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    });
    tokio::time::sleep(Duration::from_secs(2)).await;

    let publish = |topic: String, payload: Vec<u8>| {
        let client = client.clone();
        async move {
            client
                .publish(topic, QoS::AtMostOnce, false, payload)
                .await
                .expect("publish");
        }
    };

    // ---- 1. Node and device birth, cold start -----------------------------
    let ts = now_ms();
    let (mut live, birth) = session.birth(ts, vec![]);
    publish(n_birth.clone(), encode(&birth)).await;

    // A cold-start device BIRTH declares the tag set with NO value and quality
    // Stale — the bridge's honest first message. Ignition must accept a null
    // metric that still declares its datatype.
    let cold = vec![
        Metric::new(
            METRIC_POWER,
            MetricValue::Null(sparkplug_b::DataType::Double),
            ts,
        )
        .with_quality(Quality::Stale)
        .with_engineering_unit(UNIT_POWER),
        Metric::new(
            METRIC_ENERGY,
            MetricValue::Null(sparkplug_b::DataType::Double),
            ts,
        )
        .with_quality(Quality::Stale)
        .with_engineering_unit(UNIT_ENERGY),
    ];
    let payload = live.device_birth(ts, cold);
    publish(d_birth.clone(), encode(&payload)).await;

    checkpoint(
        "STEP 1 — birth, cold start",
        &[
            "The node appears ONLINE in the tag tree",
            "A device folder appears, named exactly: 30000001",
            "It holds two tags: Power and Energy",
            "Both have NO value (null), not zero",
            "Both show quality STALE / uncertain — not good",
            "Both carry an engineering unit property: kW and kWh",
        ],
    );

    // ---- 2. First real reading --------------------------------------------
    let ts = now_ms();
    let payload = live.device_data(
        ts,
        vec![
            power(POWER_FIRST, Quality::Good, ts),
            energy(ENERGY_FIRST, Quality::Good, ts),
        ],
    );
    publish(d_data.clone(), encode(&payload)).await;

    checkpoint(
        "STEP 2 — first reading",
        &[
            "Power reads exactly 1.234 (kW) — not rounded, not scaled",
            "Energy reads exactly 5678.9 (kWh)",
            "Both qualities are now GOOD",
            "The tag timestamps match the values' own time, not 'now' at Ignition",
        ],
    );

    // ---- 3. An update ------------------------------------------------------
    let ts = now_ms();
    let payload = live.device_data(
        ts,
        vec![
            power(POWER_SECOND, Quality::Good, ts),
            energy(ENERGY_SECOND, Quality::Good, ts),
        ],
    );
    publish(d_data.clone(), encode(&payload)).await;

    checkpoint(
        "STEP 3 — the values update",
        &[
            "Power now reads 2.345",
            "Energy now reads 5679.1 — it went UP, never backwards",
            "The change arrived without a rebirth or a reconnect",
        ],
    );

    // ---- 4. App-level staleness, node still alive --------------------------
    // The failure this project exists to prevent: the cloud stops answering
    // while the bridge stays connected. The node is healthy; the DATA is not.
    let ts = now_ms();
    let payload = live.device_data(
        ts,
        vec![
            power(POWER_SECOND, Quality::Stale, ts),
            energy(ENERGY_SECOND, Quality::Stale, ts),
        ],
    );
    publish(d_data.clone(), encode(&payload)).await;

    checkpoint(
        "STEP 4 — STALE while the node stays online (the critical case)",
        &[
            "The node is STILL shown as online — it has not died",
            "Both tags now show quality STALE / uncertain",
            "The values are unchanged, but they are NOT presented as trustworthy",
            "=> If Ignition still shows these as good, the whole guarantee fails here",
        ],
    );

    // ---- 5. Graceful death: both certificates ------------------------------
    // The bridge publishes its own NDEATH and then DROPS the socket, so the
    // broker's will fires as well (ADR 0011). A consumer therefore sees TWO
    // NDEATH messages carrying the same bdSeq. Whether Ignition tolerates that
    // is exactly what no broker-level test can answer.
    let death = live.death(now_ms());
    publish(n_death.clone(), encode(&death)).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("\n  → explicit NDEATH published; dropping the socket so the will fires too");
    pump.abort();
    tokio::time::sleep(Duration::from_secs(3)).await;

    checkpoint(
        "STEP 5 — death (two certificates, one session)",
        &[
            "The node is marked OFFLINE / dead",
            "Both tags are marked STALE — the value is not left looking live",
            "Ignition did NOT log an error, warning or duplicate-session complaint",
            "=> The second NDEATH (the broker's will) must be a no-op, not a fault",
        ],
    );

    println!("\n=== Result ===");
    println!("If every box above is ticked, the hand-rolled protobuf is accepted by a real");
    println!("Sparkplug host and the contract holds. Record the Ignition version you used.");
    println!("\n=== Clean-up (required) ===");
    println!("In the Designer tag browser, under the MQTT Engine provider, delete:");
    println!("    Edge Nodes/{group}/{NODE_ID}");
    println!("Delete ONLY that folder — removing MQTT Engine tags also discards their");
    println!("alarm and history configuration, and the real edge nodes share the parent.");
}
