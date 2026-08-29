//! Tier 3 — the deciding oracle for the hand-rolled protobuf **of this crate**.
//!
//! # What this attests, and what it does NOT (re-scoped 2026-08-01, Story 4.8)
//!
//! **It attests to the crate.** These bytes are assembled from `sparkplug-b`
//! primitives, and the quality codes are the *specification's* — `Good = 192`,
//! `Stale = 500`, `Bad = 0` — because that is what this crate returns since
//! [ADR 0012](../../../docs/adr/0012-quality-codes-spec-versus-host.md).
//!
//! **It does NOT attest to the bridge.** `smartme_mqtt` deviates deliberately and
//! publishes Ignition's codes instead (`Bad_Stale = 0x8000_0000 | 516`). Between
//! `fce148f` and `d28bb02` this file and the product agreed; since `d28bb02` they
//! do not, and the run table's `v2 | Pass` row was obtained under the old state.
//! The drift is [#40]. **The gate for the product is
//! `crates/smartme-bridge/tests/ignition_contract.rs`**, added by Story 4.8, which
//! drives the real driver.
//!
//! **Step 4 is therefore no longer a pass/fail on staleness — it is a
//! demonstration.** It shows the specification's `Stale = 500` being displayed by
//! a real host as `Good(500)`, which is the measurement ADR 0012 rests on. Keeping
//! it is the only standing external evidence that the deviation was necessary;
//! retiring it would lose that. Decided by Guy, 2026-07-31, recorded on [#40].
//!
//! [#40]: https://github.com/guycorbaz/smartme_mqtt/issues/40
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
    BdSeq, DataType, EdgeNode, MessageType, Metric, MetricValue, NodeSession, Quality, encode,
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
    let session = NodeSession::start(Some(BdSeq::new(7)));
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
        "STEP 4 — the specification's STALE code, and what Ignition does with it",
        &[
            "The node is STILL shown as online — it has not died",
            "The values are unchanged",
            "=> EXPECT quality Good(500). This step now DEMONSTRATES a defect rather than",
            "   testing a guarantee: this crate publishes the specification's Stale = 500,",
            "   and Ignition reads the quality LEVEL from the TOP BITS of a 32-bit code, so",
            "   500 lands in the 'good' band with 500 as a subcode. A non-good quality that",
            "   displays as good is the exact silent lie the project exists to prevent.",
            "=> If you see a NON-good quality here, something has changed: either Ignition's",
            "   encoding, or this crate's codes. Both are findings. Record it and open an issue.",
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

// ===========================================================================
// Quality-code probe
// ===========================================================================

/// Publishes one tag per candidate quality code and lets the host tell us what
/// each one means to it.
///
/// This exists because the first contract run found that our `STALE` code (500)
/// renders as `Good(500)` in Ignition — the host reads the property, it just
/// classifies the value at the Good level. That points at the raw-integer
/// encoding: the quality LEVEL lives in the high bits, and the numbers in
/// Ignition's published table (192, 257, 512, 516 …) are SUBCODES.
///
/// The correct raw integers can be derived from that — Cirrus Link documents
/// `Bad_Disabled` as `-2147483133`, and `0x80000000 | 515` is exactly that — but
/// a derivation is not a measurement, and these numbers are about to become a
/// wire contract. So this asks the host directly.
///
/// It also settles the open design choice: seeing `Uncertain_LastKnownValue` and
/// `Bad_Stale` side by side in the tag browser shows which one an operator would
/// actually read as "do not trust this".
///
/// ```text
/// SPARKPLUG_CONTRACT_BROKER=host:1883 \
/// SPARKPLUG_CONTRACT_GROUP=QualityProbe \
///   cargo test -p sparkplug-b --test ignition_contract quality_code_probe -- --ignored --nocapture
/// ```
#[tokio::test(flavor = "multi_thread")]
#[ignore = "manual probe: publishes to a real broker for a human to inspect"]
async fn quality_code_probe() {
    let target = std::env::var("SPARKPLUG_CONTRACT_BROKER")
        .expect("set SPARKPLUG_CONTRACT_BROKER=host:port — there is deliberately no default");
    let group = std::env::var("SPARKPLUG_CONTRACT_GROUP").expect(
        "set SPARKPLUG_CONTRACT_GROUP to a disposable group — there is deliberately no default",
    );
    assert_ne!(
        group, "Site",
        "refusing to publish into the production group"
    );
    let (host, port) = target
        .rsplit_once(':')
        .expect("SPARKPLUG_CONTRACT_BROKER must be host:port");
    let port: u16 = port.parse().expect("the port must be a number");

    // (tag name, raw Int32 bit pattern). Every tag carries the SAME value, so the
    // only thing that can differ downstream is the quality.
    const BAD: u32 = 0x8000_0000;
    const UNCERTAIN: u32 = 0x4000_0000;
    let candidates: Vec<(String, u32)> = vec![
        ("q_192_good_today".to_string(), 192),
        ("q_500_stale_today".to_string(), 500),
        ("q_257_uncertain_lastknown".to_string(), UNCERTAIN | 257),
        ("q_256_uncertain".to_string(), UNCERTAIN | 256),
        ("q_512_bad".to_string(), BAD | 512),
        ("q_516_bad_stale".to_string(), BAD | 516),
    ];

    let node = EdgeNode::new(group.clone(), "QualityProbe".to_string()).expect("identifiers");
    let n_birth = node.node_topic(MessageType::NBirth).expect("topic");
    let n_death = node.node_topic(MessageType::NDeath).expect("topic");
    let d_birth = node
        .device_topic(MessageType::DBirth, DEVICE)
        .expect("topic");

    let session = NodeSession::start(Some(BdSeq::new(9)));
    let will = session.will(now_ms());
    let mut options = MqttOptions::new("sparkplug-quality-probe", host, port);
    options.set_keep_alive(Duration::from_secs(30));
    options.set_last_will(rumqttc::LastWill::new(
        n_death,
        encode(&will),
        QoS::AtMostOnce,
        false,
    ));
    let (client, mut eventloop) = AsyncClient::new(options, 32);
    let pump = tokio::spawn(async move {
        loop {
            if let Err(error) = eventloop.poll().await {
                println!("  → transport: {error}");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    });
    tokio::time::sleep(Duration::from_secs(2)).await;

    let ts = now_ms();
    let (mut live, birth) = session.birth(ts, vec![]);
    client
        .publish(n_birth, QoS::AtMostOnce, false, encode(&birth))
        .await
        .expect("publish");

    // Built against the protobuf types directly: `Metric::with_quality` can only
    // express our own three-valued enum, and the whole point here is to send
    // codes that enum cannot currently produce.
    let mut payload = live.device_birth(ts, vec![]);
    payload.metrics = candidates
        .iter()
        .map(|(name, code)| sparkplug_b::protobuf::payload::Metric {
            name: Some(name.clone()),
            alias: None,
            timestamp: Some(ts),
            datatype: Some(sparkplug_b::DataType::Double.code()),
            is_historical: None,
            is_transient: None,
            is_null: None,
            metadata: None,
            properties: Some(sparkplug_b::protobuf::payload::PropertySet {
                keys: vec![Quality::PROPERTY_KEY.to_string()],
                values: vec![sparkplug_b::protobuf::payload::PropertyValue {
                    r#type: Some(sparkplug_b::DataType::Int32.code()),
                    is_null: None,
                    value: Some(
                        sparkplug_b::protobuf::payload::property_value::Value::IntValue(*code),
                    ),
                }],
            }),
            value: Some(sparkplug_b::protobuf::payload::metric::Value::DoubleValue(
                42.0,
            )),
        })
        .collect();
    client
        .publish(d_birth, QoS::AtMostOnce, false, encode(&payload))
        .await
        .expect("publish");

    println!("\n=== Quality-code probe ===");
    println!("Every tag below carries the SAME value (42.0). Only the Quality property differs.\n");
    for (name, code) in &candidates {
        println!("  {name:<28} sent as Int32 {:>12}", *code as i32);
    }

    checkpoint(
        "Read the quality Ignition shows for each tag",
        &[
            "q_192_good_today            -> expected Good",
            "q_500_stale_today           -> what we ship now; showed Good(500) last run",
            "q_257_uncertain_lastknown   -> Uncertain / last known value?",
            "q_256_uncertain             -> Uncertain?",
            "q_512_bad                   -> Bad?",
            "q_516_bad_stale             -> Bad / stale?",
            "",
            "Write down EXACTLY what each one displays, including any number in",
            "parentheses. That string is the evidence the fix will be based on.",
        ],
    );

    pump.abort();
    println!("\n=== Clean-up (required) ===");
    println!("Delete Edge Nodes/{group}/QualityProbe in the Designer — only that folder.");
}

/// **The two questions a table cannot answer about a DDATA's shape** — what
/// Ignition does with a metric that declares no datatype, and which timestamp it
/// believes.
///
/// # Why one probe for two issues
///
/// They are the same look at the same screen. Both are read off one DDATA, in one
/// Designer session, and separating them would buy nothing but a second set-up.
///
/// **[#28] / [ADR 0053] — the datatype is gone from every DATA message.** The
/// specification says it SHOULD NOT be there and then prints a DDATA example that
/// carries it (`Sparkplug_6_Payloads.adoc:1391`), so a host built on the example
/// is not a strange thing to imagine. The valued case is attested by the bridge's
/// own gate, whose steps 2 and 3 update a tag from a DDATA. **The case no gate
/// reaches is a NULL metric**: it arrives with a name, `is_null`, its properties
/// and nothing else — no value to infer a type from, and no declared type. The
/// bridge publishes exactly that shape whenever an oracle returns `Bad`.
///
/// **[#29] / [ADR 0013] — the payload timestamp is the reading's `ValueDate`.**
/// That deviates from two MUSTs, deliberately: a stale reading must read as old
/// even to a consumer that ignores the quality flag. The conformant shape — `now`
/// at payload level, `ValueDate` at metric level — is refused because it assumes
/// the host reads METRIC timestamps, which is the assumption contract v1
/// disproved for the quality property. ADR 0013 names the condition for
/// revisiting it in one sentence: *"If a future host is shown to read metric
/// timestamps correctly, this ADR should be revisited rather than worked
/// around."* This probe is that showing, and nothing else decides it.
///
/// # The offset is 37 minutes, and that is not arbitrary
///
/// Ignition displays local time (measured 2026-08-28: `11:31:17 AM` for an event
/// at `09:31:17` UTC), so an absolute instant proves nothing about which
/// timestamp was read. An OFFSET does — provided no time zone can produce it. Zone
/// offsets come in whole hours and in the :30 and :45 quarters; **none is :37**.
/// A tag reading 37 minutes behind its neighbours is reading the metric
/// timestamp, and no clock setting can imitate it.
///
/// ```text
/// SPARKPLUG_CONTRACT_BROKER=host:1883 \
/// SPARKPLUG_CONTRACT_GROUP=ShapeProbe \
///   cargo test -p sparkplug-b --test ignition_contract ddata_shape_probe -- --ignored --nocapture
/// ```
///
/// [#28]: https://github.com/guycorbaz/smartme_mqtt/issues/28
/// [#29]: https://github.com/guycorbaz/smartme_mqtt/issues/29
/// [ADR 0053]: ../../../docs/adr/0053-the-datatype-leaves-the-data-messages.md
/// [ADR 0013]: ../../../docs/adr/0013-payload-timestamp-is-acquisition-time.md
#[tokio::test(flavor = "multi_thread")]
#[ignore = "manual probe: publishes to a real broker for a human to inspect"]
async fn ddata_shape_probe() {
    let target = std::env::var("SPARKPLUG_CONTRACT_BROKER")
        .expect("set SPARKPLUG_CONTRACT_BROKER=host:port — there is deliberately no default");
    let group = std::env::var("SPARKPLUG_CONTRACT_GROUP").expect(
        "set SPARKPLUG_CONTRACT_GROUP to a disposable group — there is deliberately no default",
    );
    assert_ne!(
        group, "Site",
        "refusing to publish into the production group"
    );
    let (host, port) = target
        .rsplit_once(':')
        .expect("SPARKPLUG_CONTRACT_BROKER must be host:port");
    let port: u16 = port.parse().expect("the port must be a number");

    /// The offset no time zone can imitate. See the doc comment.
    const SKEW_MS: u64 = 37 * 60 * 1_000;
    const TS_NOW: &str = "ts_stamped_now";
    const TS_BEHIND: &str = "ts_stamped_37_min_back";
    const GOES_NULL: &str = "goes_null_in_ddata";

    let node = EdgeNode::new(group.clone(), "ShapeProbe".to_string()).expect("identifiers");
    let n_birth = node.node_topic(MessageType::NBirth).expect("topic");
    let n_death = node.node_topic(MessageType::NDeath).expect("topic");
    let d_birth = node
        .device_topic(MessageType::DBirth, DEVICE)
        .expect("topic");
    let d_data = node
        .device_topic(MessageType::DData, DEVICE)
        .expect("topic");

    let session = NodeSession::start(Some(BdSeq::new(9)));
    let will = session.will(now_ms());
    let mut options = MqttOptions::new("sparkplug-shape-probe", host, port);
    options.set_keep_alive(Duration::from_secs(30));
    options.set_last_will(rumqttc::LastWill::new(
        n_death,
        encode(&will),
        QoS::AtMostOnce,
        false,
    ));
    let (client, mut eventloop) = AsyncClient::new(options, 32);
    let pump = tokio::spawn(async move {
        loop {
            if let Err(error) = eventloop.poll().await {
                println!("  → transport: {error}");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    });
    tokio::time::sleep(Duration::from_secs(2)).await;

    // ---- The declaration: three tags, each with a value and a datatype ------
    let ts = now_ms();
    let (mut live, birth) = session.birth(ts, vec![]);
    client
        .publish(n_birth, QoS::AtMostOnce, false, encode(&birth))
        .await
        .expect("publish");

    let declared = |name: &str| {
        Metric::new(name, MetricValue::Double(11.0), ts)
            .with_quality(Quality::Good)
            .with_engineering_unit(UNIT_POWER)
    };
    let d_birth_payload = live.device_birth(
        ts,
        vec![declared(TS_NOW), declared(TS_BEHIND), declared(GOES_NULL)],
    );
    client
        .publish(
            d_birth.clone(),
            QoS::AtMostOnce,
            false,
            encode(&d_birth_payload),
        )
        .await
        .expect("publish");

    checkpoint(
        "SET-UP — three tags are declared, all reading 11.0 kW and GOOD",
        &[
            "All three tags exist under contract-meter and read 11.0",
            "If any is missing, STOP: the DBIRTH still carries its datatypes, so a",
            "  tag absent here is a set-up fault and nothing measured below is worth",
            "  recording",
        ],
    );

    // ---- The measurement: ONE DDATA, three shapes ---------------------------
    //
    // Every metric below travels WITHOUT a datatype: `device_data` encodes with
    // `Datatype::Omitted` since ADR 0053, so this is the bridge's real wire shape
    // and not a hand-built imitation of it.
    let publish_ts = now_ms();
    let data = live.device_data(
        publish_ts,
        vec![
            // The control: it moves, so a still screen is a still screen and not
            // a rejected message.
            Metric::new(TS_NOW, MetricValue::Double(22.0), publish_ts)
                .with_quality(Quality::Good)
                .with_engineering_unit(UNIT_POWER),
            // The question: the PAYLOAD says now, the METRIC says 37 minutes ago.
            Metric::new(TS_BEHIND, MetricValue::Double(22.0), publish_ts - SKEW_MS)
                .with_quality(Quality::Good)
                .with_engineering_unit(UNIT_POWER),
            // The sharp edge: a name, `is_null`, properties, and nothing else.
            Metric::new(GOES_NULL, MetricValue::Null(DataType::Double), publish_ts)
                .with_quality(Quality::Bad)
                .with_engineering_unit(UNIT_POWER),
        ],
    );
    client
        .publish(d_data, QoS::AtMostOnce, false, encode(&data))
        .await
        .expect("publish");
    tokio::time::sleep(Duration::from_secs(2)).await;

    println!("\n=== DDATA shape probe ===");
    println!("  payload timestamp : {publish_ts}");
    println!("  {TS_NOW:<24} metric timestamp = {publish_ts} (same)");
    println!(
        "  {TS_BEHIND:<24} metric timestamp = {} (37 min earlier)",
        publish_ts - SKEW_MS
    );
    println!("  {GOES_NULL:<24} metric timestamp = {publish_ts}, is_null, NO value");
    println!("\n  None of the three carries a datatype: that is what ADR 0053 changed.\n");

    checkpoint(
        "READ THE SCREEN — and record all three answers, including the boring ones",
        &[
            "[#28, valued]  ts_stamped_now reads 22.0 — a DDATA with NO datatype",
            "               updated a tag. If it still reads 11.0, ADR 0053 must be",
            "               reverted before anything ships.",
            "",
            "[#28, null]    goes_null_in_ddata — WHAT DOES IT SHOW? Look in the tag's",
            "               `value` row, not the browser's Value column (the column",
            "               renders the quality string for any non-good tag, so",
            "               blanked and frozen look identical there). Record whether",
            "               the tag went null, kept 11.0, or vanished.",
            "",
            "[#29]          ts_stamped_37_min_back — compare its timestamp with",
            "               ts_stamped_now's. THREE outcomes, and all three are answers:",
            "                 · 37 MINUTES APART -> Ignition reads the METRIC timestamp.",
            "                   ADR 0013 gets revisited: the conformant shape costs",
            "                   nothing.",
            "                 · IDENTICAL -> it reads the PAYLOAD timestamp. ADR 0013",
            "                   stands, and the deviation is load-bearing rather than",
            "                   merely deliberate. AND the conformant shape would move",
            "                   every future historised point from its ValueDate to its",
            "                   publish instant — a discontinuity in a running series.",
            "                 · STILL 11.0 while ts_stamped_now reads 22.0 -> Engine READ",
            "                   the metric timestamp and REFUSED the value as out of",
            "                   order. That is the strongest form of the first answer,",
            "                   and it says something the other two do not: a host that",
            "                   rejects a backdated metric would also reject a DDATA the",
            "                   bridge publishes after any outage. Record it as its own",
            "                   finding, not as a failed step.",
            "",
            "⚠ THIS STEP PASSES WRONGLY IF:",
            "  · you compare ts_stamped_37_min_back against the WALL CLOCK instead of",
            "    against ts_stamped_now. Ignition shows local time; the wall clock",
            "    tells you about your time zone, not about which field was read.",
            "  · the tag group has not polled since the DDATA, so all three still show",
            "    the DBIRTH's instant — check ts_stamped_now moved to 22.0 FIRST.",
            "  · you read the tag's `LastChange` or the browser's own receipt time",
            "    rather than the tag timestamp: both are Ignition's clock and neither",
            "    is the question.",
            "  · a residual folder from an earlier run is on screen. That produced a",
            "    false Good on 2026-08-28; delete the folder BEFORE the pass, not only",
            "    after.",
        ],
    );

    pump.abort();
    println!("\n=== Clean-up (required) ===");
    println!("Delete Edge Nodes/{group}/ShapeProbe in the Designer — only that folder.");
}

/// **Does this host apply a metric whose timestamp EQUALS the one it already
/// holds?** Opened by `ddata_shape_probe` on 2026-08-29, and it is a bigger
/// question than the one that produced it.
///
/// # Why it was asked
///
/// `ddata_shape_probe` showed Engine **refusing** a metric stamped 37 minutes
/// behind the value it held: the tag kept its old number while its neighbour
/// moved. So Engine reads metric timestamps and acts on them. Which raises the
/// case nobody had measured — **equality**, not lateness.
///
/// It matters because equality is not a corner case for the bridge that owns
/// this crate; it is its ordinary staleness path. When a source goes quiet, that
/// bridge republishes **the last known value with a degraded verdict**, stamped
/// with that value's own acquisition time — the same instant it already sent.
/// It has a name for that shape in its own code (`is_republication`). If Engine
/// applies a metric only when its timestamp advances, then **the degradation
/// never reaches the screen and a stale reading goes on displaying as good** —
/// which is the exact failure the whole design exists to prevent, arriving
/// through the one door nobody had checked.
///
/// # Why no existing gate answers it
///
/// The bridge gate's step 4 shows an honest STALE and has passed repeatedly. It
/// does not answer this: it sends its stale reading with a FRESH acquisition
/// time, so its timestamp advances like any other. The equal-timestamp case has
/// never been on the wire in front of a host. That is a false pass the gate has
/// carried since it was written, and it is recorded on the step rather than
/// here.
///
/// # What it publishes
///
/// Two tags, born at 11.0. One DDATA moves both to 22.0 at instant `T` — the
/// control, which must be seen to land before anything else counts. Then a
/// SECOND DDATA stamped with **the same `T`**:
///
/// - `same_ts_new_value` carries 33.0. A value change is unambiguous on screen,
///   so this one says whether an equal timestamp is accepted AT ALL.
/// - `same_ts_new_quality` keeps 22.0 and changes only its quality, which is the
///   bridge's real shape: same measurement, new verdict.
///
/// ```text
/// SPARKPLUG_CONTRACT_BROKER=host:1883 \
/// SPARKPLUG_CONTRACT_GROUP=StaleProbe \
///   cargo test -p sparkplug-b --test ignition_contract staleness_republication_probe -- --ignored --nocapture
/// ```
#[tokio::test(flavor = "multi_thread")]
#[ignore = "manual probe: publishes to a real broker for a human to inspect"]
async fn staleness_republication_probe() {
    let target = std::env::var("SPARKPLUG_CONTRACT_BROKER")
        .expect("set SPARKPLUG_CONTRACT_BROKER=host:port — there is deliberately no default");
    let group = std::env::var("SPARKPLUG_CONTRACT_GROUP").expect(
        "set SPARKPLUG_CONTRACT_GROUP to a disposable group — there is deliberately no default",
    );
    assert_ne!(
        group, "Site",
        "refusing to publish into the production group"
    );
    let (host, port) = target
        .rsplit_once(':')
        .expect("SPARKPLUG_CONTRACT_BROKER must be host:port");
    let port: u16 = port.parse().expect("the port must be a number");

    const NEW_VALUE: &str = "same_ts_new_value";
    const NEW_QUALITY: &str = "same_ts_new_quality";

    let node = EdgeNode::new(group.clone(), "StaleProbe".to_string()).expect("identifiers");
    let n_birth = node.node_topic(MessageType::NBirth).expect("topic");
    let n_death = node.node_topic(MessageType::NDeath).expect("topic");
    let d_birth = node
        .device_topic(MessageType::DBirth, DEVICE)
        .expect("topic");
    let d_data = node
        .device_topic(MessageType::DData, DEVICE)
        .expect("topic");

    let session = NodeSession::start(Some(BdSeq::new(9)));
    let will = session.will(now_ms());
    let mut options = MqttOptions::new("sparkplug-stale-probe", host, port);
    options.set_keep_alive(Duration::from_secs(30));
    options.set_last_will(rumqttc::LastWill::new(
        n_death,
        encode(&will),
        QoS::AtMostOnce,
        false,
    ));
    let (client, mut eventloop) = AsyncClient::new(options, 32);
    let pump = tokio::spawn(async move {
        loop {
            if let Err(error) = eventloop.poll().await {
                println!("  → transport: {error}");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    });
    tokio::time::sleep(Duration::from_secs(2)).await;

    let born = now_ms();
    let (mut live, birth) = session.birth(born, vec![]);
    client
        .publish(n_birth, QoS::AtMostOnce, false, encode(&birth))
        .await
        .expect("publish");

    let declared = |name: &str| {
        Metric::new(name, MetricValue::Double(11.0), born)
            .with_quality(Quality::Good)
            .with_engineering_unit(UNIT_POWER)
    };
    let d_birth_payload = live.device_birth(born, vec![declared(NEW_VALUE), declared(NEW_QUALITY)]);
    client
        .publish(d_birth, QoS::AtMostOnce, false, encode(&d_birth_payload))
        .await
        .expect("publish");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // ---- The control: an ordinary update, at instant T ----------------------
    let t = now_ms();
    let at_t = |name: &str, value: f64, quality: Quality| {
        Metric::new(name, MetricValue::Double(value), t)
            .with_quality(quality)
            .with_engineering_unit(UNIT_POWER)
    };
    let first = live.device_data(
        t,
        vec![
            at_t(NEW_VALUE, 22.0, Quality::Good),
            at_t(NEW_QUALITY, 22.0, Quality::Good),
        ],
    );
    client
        .publish(d_data.clone(), QoS::AtMostOnce, false, encode(&first))
        .await
        .expect("publish");
    tokio::time::sleep(Duration::from_secs(2)).await;

    println!("\n=== Staleness-republication probe ===");
    println!("  metric timestamp of BOTH messages: {t}");

    checkpoint(
        "CONTROL — both tags must read 22.0 before the measurement means anything",
        &[
            "same_ts_new_value    -> 22.0",
            "same_ts_new_quality  -> 22.0",
            "",
            "If either still reads 11.0, STOP. The second message is stamped with",
            "the SAME instant as this one, so a tag that did not take this update",
            "cannot tell you anything about the next.",
        ],
    );

    // ---- The measurement: the SAME instant, published again -----------------
    let second = live.device_data(
        t,
        vec![
            // Unambiguous on screen: does an equal timestamp land at all?
            at_t(NEW_VALUE, 33.0, Quality::Good),
            // The bridge's real shape: same measurement, degraded verdict.
            at_t(NEW_QUALITY, 22.0, Quality::Stale),
        ],
    );
    client
        .publish(d_data, QoS::AtMostOnce, false, encode(&second))
        .await
        .expect("publish");
    tokio::time::sleep(Duration::from_secs(2)).await;

    checkpoint(
        "THE MEASUREMENT — a second DDATA at the SAME metric timestamp",
        &[
            "same_ts_new_value    -> 33.0, or still 22.0?",
            "same_ts_new_quality  -> its quality changed, or not?",
            "",
            "★ THE QUALITY IS THE SPECIFICATION'S 500, NOT THE BRIDGE'S CODE. This",
            "  crate publishes the specified codes, and Ignition renders 500 as",
            "  `Good(500)` — NOT as Bad_Stale (ADR 0012, and the reason it exists).",
            "  So the change you are looking for is `Good` becoming `Good(500)`:",
            "  READ THE NUMBER IN PARENTHESES, not the word. Looking for the word",
            "  `Bad` here reports a message that landed as one that did not.",
            "",
            "WHAT EACH ANSWER MEANS:",
            "  · 33.0 AND the quality moved -> an equal timestamp is applied. The",
            "    bridge's staleness republication reaches a host, and the question",
            "    this probe was written for is closed in the safe direction.",
            "  · NEITHER moved -> Engine applies a metric only when its timestamp",
            "    ADVANCES. The bridge republishes its last known value under its own",
            "    acquisition time, so a degradation would never reach the screen and",
            "    a stale reading would go on displaying as good. That is the failure",
            "    the design exists to prevent, and it would be a defect of the FIRST",
            "    order — not a conformance point.",
            "  · 33.0 but the quality did NOT move -> the value is applied and the",
            "    property is not. That is the 2026-08-22 finding again in a new place",
            "    (a property is written by a BIRTH and by nothing else, ADR 0044) and",
            "    it would say the quality property cannot be refreshed at all, which",
            "    contradicts what that same session measured. Record it verbatim and",
            "    do not reconcile it here.",
            "",
            "⚠ THIS STEP PASSES WRONGLY IF:",
            "  · you are reading the browser's Value column for the quality — it",
            "    renders a quality string for non-good tags, and `Good(500)` is not",
            "    non-good. Open the tag's properties.",
            "  · the tag group has not polled since the second message: confirm by",
            "    same_ts_new_value, which is the unambiguous one.",
            "  · a residual folder from an earlier run is on screen.",
        ],
    );

    pump.abort();
    println!("\n=== Clean-up (required) ===");
    println!("Delete Edge Nodes/{group}/StaleProbe in the Designer — only that folder.");
}
