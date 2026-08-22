# sparkplug-b

A small, pure Sparkplug B library: protobuf encode/decode, the EON node and device model, and
the `seq` / `bdSeq` / rebirth lifecycle.

It was written for [smartme_mqtt](https://github.com/guycorbaz/smartme_mqtt), a bridge that
republishes smart-me energy meters to a SCADA host, and it is kept separate for one reason: the
lifecycle rules are the specification's, not that bridge's.

```rust
use sparkplug_b::{
    BdSeq, EdgeNode, MessageType, Metric, MetricValue, NodeSession, Quality, encode,
};

fn main() -> Result<(), sparkplug_b::TopicError> {
    let node = EdgeNode::new("Plant", "Bridge01")?;
    let topic = node.device_topic(MessageType::DBirth, "9202685")?;
    assert_eq!(topic, "spBv1.0/Plant/DBIRTH/Bridge01/9202685");

    // A node that has never connected has NO previous session, and its first one is
    // numbered 0 — `tck-id-topics-nbirth-bdseq-increment` requires the number to
    // start at zero. After a restart it passes back the bdSeq it persisted, so the
    // sequence continues instead of replaying a number the host has already seen.
    let session = NodeSession::start(None);
    assert_eq!(session.bd_seq(), BdSeq::new(0), "a first session starts at zero");

    // The will is built FIRST: it is registered with the broker before connecting,
    // and it carries no seq — the broker publishes it at a moment this node cannot
    // number.
    let will = session.will(1_700_000_000_000);
    assert_eq!(will.seq, None);

    let energy = Metric::new("Energy", MetricValue::Double(4_843.822), 1_700_000_000_000)
        .with_quality(Quality::Good)
        .with_engineering_unit("kWh");
    let (mut live, birth) = session.birth(1_700_000_000_000, vec![energy]);
    assert_eq!(birth.seq, Some(0));

    let bytes = encode(&birth);
    assert!(!bytes.is_empty());

    // Later readings go out on the open session, which carries the seq forward.
    let _reading = live.data(1_700_000_060_000, vec![]);
    Ok(())
}
```

*This example is compiled and run by the crate's test suite — see `src/lib.rs`. It was not,
until 2026-08-21, and it did not compile.*

## What it does

- **Topics**, built and validated rather than formatted: an identifier the grammar forbids is
  refused when the topic is built, not when a host rejects the message.
- **Payloads**: encode and decode, with `Metric` carrying its own datatype and quality.
- **The session lifecycle**: `seq` wrapping at 255, `bdSeq` across restarts, the will payload,
  and the rebirth answer.
- **Quality as a first-class value**, because the point of Sparkplug for a measurement bridge
  is that a value can be published *marked* rather than withheld.

## What it does not do

- **No transport.** It produces bytes and topics; connecting, publishing and reconnecting are
  the caller's. There is no MQTT client in its dependency graph.
- **No host application side.** It models an edge node publishing, not a primary host consuming
  — there is no `STATE` subscriber and no host-application birth here.
- **No DCMD handling, and no metric aliasing.** Both are legal Sparkplug and neither is
  implemented; a caller that needs them needs more than this crate.
- **No `unsafe`**, enforced by `#![forbid(unsafe_code)]`.

## Conformance scope

The library is written against the committed Sparkplug B **v3.0.0** specification, and the
consuming bridge maintains a clause-by-clause conformance matrix citing `tck-id-…` identifiers.
**What is implemented here is the edge-node publishing path**: namespace and topic grammar,
payload and metric encoding, datatypes, `seq`/`bdSeq` sequencing, the BIRTH/DATA/DEATH lifecycle
and the will.

**What is deliberately not implemented**: the primary-host `STATE` topic, DCMD, metric aliases,
and payload compression. Those clauses are recorded as out of scope rather than as passing.

## A public dependency, stated rather than discovered

`protobuf::Payload` and its neighbours are generated **in this crate** by `build.rs`, from the
specification's own `.proto`, using [`prost`]. That makes `prost` a **public dependency**: the
generated types implement `prost::Message`, so a major `prost` release is a breaking change for
this crate too, and a consumer holding a `Payload` is holding a type built by it.

Everything this crate writes by hand keeps its own types — including `DecodeError`, so matching
on a failure does not make you a `prost` consumer. That boundary is enforced by a test rather
than by intention.

[`prost`]: https://crates.io/crates/prost

## Versioning

The crate's version currently tracks the workspace it lives in, which means it moves when the
bridge releases. See `CHANGELOG.md` for what that implies if you depend on it.

## Licence

**MIT**, like the workspace it lives in.

The Sparkplug B specification it is written against is published under EPL-2.0; a copy of that
specification is committed in the parent repository for citation, and it is the *document* that
carries that licence, not this code.
