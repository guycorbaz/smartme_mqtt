//! The mqtt driver task (Story 1.12).
//!
//! It owns the session lifecycle and `bdSeq` persistence, and decides NO truth:
//! it transports the verdict the poll task already reached.
//!
//! The boot order is not negotiable, and it is the reason the will is built
//! before anything connects:
//!
//! 1. restore `bdSeq` from disk and open the next session,
//! 2. serialise the DEATH certificate for that session,
//! 3. register it as the connection's last will IN the CONNECT packet,
//! 4. connect,
//! 5. subscribe to the node's NCMD topic,
//! 6. publish the BIRTH.
//!
//! Get this wrong and the broker holds no will while the node is alive — the
//! node dies and nobody is told, which is the silent lie this project exists to
//! prevent.
//!
//! # The seventh path: a BIRTH that follows no CONNACK (Story 4.7)
//!
//! The six steps above are the only way a session STARTS, and reading them as
//! the only way a BIRTH is published would be wrong. A Host Application can send
//! a `Node Control/Rebirth` request at any time, and the answer is step 6 alone:
//! a complete NBIRTH + DBIRTH sequence, published without steps 1–5 having run
//! again.
//!
//! Everything the first six steps establish stays as it was. In particular the
//! session number does NOT advance —
//! `tck-id-operational-behavior-data-commands-rebirth-action-3` requires the
//! rebirth's NBIRTH to carry the same `bdSeq` as the will registered at CONNECT,
//! because no new MQTT session is being established. The will the broker holds
//! is still the right certificate for the session that is running; a rebirth
//! re-announces it rather than replacing it. See [`announce`].
//!
//! # Why step 5 precedes step 6 (Story 4.6)
//!
//! `tck-id-message-flow-edge-node-ncmd-subscribe` and the section preamble that
//! introduces it (`Sparkplug_5_Operational_Behavior.adoc:155-163`) are explicit:
//! *"**Prior to sending an NBIRTH message**, the MQTT client associated with the
//! Edge Node must subscribe to receive NCMD messages"*, and *"It MUST subscribe
//! on this topic with a QoS of 1."* Same sequence is not enough — a host that
//! receives an NBIRTH may answer it immediately with a rebirth request, and a
//! node that has not yet subscribed never sees it.
//!
//! Steps 5 and 6 both live in the `Transport::Connected` arm, which fires on
//! EVERY CONNACK, so the ordering holds on a reconnect as much as on the first
//! connect.
//!
//! Re-subscribing on every connect is correct whether or not the broker kept
//! the old subscription, and it is written that way ON PURPOSE. The tempting
//! justification — *"`rumqttc` connects with a clean session, so the broker
//! discards the subscription"* — is true of the dependency's current default and
//! is exactly what this project's own conformance matrix records as a **gap
//! (unproven)** under `tck-id-principles-persistence-clean-session-311`:
//! `set_clean_session` is never called and no test asserts the flag
//! ([#35](https://github.com/guycorbaz/smartme_mqtt/issues/35), Story 4.10).
//! Making the re-subscribe *depend* on that premise would convert an open gap
//! into a settled fact. It does not depend on it: an unnecessary SUBSCRIBE costs
//! one packet, and a missing one costs every command for the life of the
//! session.
//!
//! The driver does NOT wait for the SubAck before birthing. MQTT delivers one
//! connection's packets in order, so a SUBSCRIBE queued before the PUBLISH
//! satisfies the clause; awaiting the acknowledgement would delay every birth on
//! an unknown latency and could hang a session that is otherwise healthy. The
//! SubAck is checked when it arrives, not waited on.
//!
//! # Why the EventLoop gets its own task
//!
//! `EventLoop::poll` is NOT cancellation-safe: dropping it mid-poll can abandon
//! a half-finished CONNECT (after the broker has already registered the will,
//! which then fires against a node that never birthed) or a half-written
//! packet. So it is never a `select!` branch — it runs alone in its own task and
//! reports what it sees through a channel.
//!
//! # Session identity
//!
//! **One CONNECT is one `bdSeq`, and the will registered in that CONNECT carries
//! the same number** — what the specification requires. This module owns its
//! reconnect loop (see THE SESSION LOOP in [`run`]) precisely so that it can:
//! `rumqttc` rebuilds the CONNECT packet from the `MqttOptions` captured at
//! construction, so a client that reconnects internally can never be given a new
//! will, and rebuilding the client is the only way to hand the broker a
//! certificate that matches the session it is about to cover.
//!
//! The number is persisted before each CONNECT rather than once per boot, so a
//! crash between the CONNECT and the next boot cannot replay a session the
//! broker has already seen. That write is bounded by `RECONNECT_FLOOR`, which is
//! why the backoff floor is a durability property and not merely politeness.
//!
//! **What must never be done instead**, because it is strictly worse than the
//! deviation this replaced: advancing `bdSeq` while leaving the old will in
//! place. The broker would then hold a death certificate for a session that no
//! longer exists, a consumer pairing death to birth by `bdSeq` would discard it,
//! and a frozen value would stay on screen presented as live.
//!
//! *Until Story 4.10 (2026-08-01) the session number was FIXED for a client's
//! lifetime and this section recorded that as a deviation. The deviation is
//! gone; `-will-message-payload-bdSeq` and `payloads-nbirth-bdseq-repeat` moved
//! to conformant in `docs/sparkplug-conformance.md`. Note that a reconnect now
//! mints a NEW number — anything that used to tell a reconnect from a rebirth by
//! observing an unchanged `bdSeq` has had its meaning inverted, which is what
//! Story 4.9 rearmed `chaos_sigterm_no_lie` for.*

use std::path::PathBuf;
use std::time::Duration;

use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS, SubscribeReasonCode};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::adapters::sparkplug_publisher::{Outbound, Published, Sink, SparkplugPublisher};
use crate::app::poll_publish::DropReason;
use crate::core::channel::MeterUpdate;
use crate::core::clock::Clock;
use crate::domain::{DeviceIdentity, Serial};
use sparkplug_b::{BdSeq, MessageType};

/// How to reach the broker and how to behave on it.
#[derive(Debug, Clone)]
pub struct MqttConfig {
    /// MQTT client identifier.
    pub client_id: String,
    /// Broker host.
    pub host: String,
    /// Broker port.
    pub port: u16,
    /// Keep-alive interval.
    pub keep_alive: Duration,
    /// Where the session number is persisted across restarts.
    pub bd_seq_path: PathBuf,
    /// Bound of the outbound queue handed to the client.
    pub capacity: usize,
    /// How long to keep pumping the transport after the DEATH is queued, so it
    /// actually reaches the wire before the socket closes.
    pub death_flush: Duration,
    /// The shortest wait between two CONNECT attempts, **raised only**.
    ///
    /// [`RECONNECT_FLOOR`] is a durability bound — it caps how often `bdSeq`
    /// reaches the disk — so this field is clamped UP to it and can never lower
    /// it. A caller that asks for less gets the floor, and the property holds by
    /// construction rather than by a comment.
    ///
    /// It exists because [#43] cannot be measured without it. That issue observes
    /// no NDEATH reaching a subscriber across a reconnect, and its stated
    /// hypothesis is session takeover: the new CONNECT arrives inside the floor
    /// and supersedes the half-open session before the broker publishes its will.
    /// Its own second bullet — *"vary the backoff floor; if a longer wait makes
    /// the will appear, takeover is confirmed"* — is the cheapest of the three
    /// things that would settle it, and the only one needing nothing outside this
    /// repository.
    ///
    /// [#43]: https://github.com/guycorbaz/smartme_mqtt/issues/43
    pub reconnect_floor: Duration,
}

/// The persisted session number.
#[derive(Debug, Serialize, Deserialize)]
struct PersistedBdSeq {
    bd_seq: u8,
}

/// Reads the last session number, or the before-first sentinel.
/// The session number the last run used, or `None` when this node has never
/// connected.
///
/// **Absent and corrupt are not the same answer, and until 2026-08-22 this
/// function gave them the same one.** Both produced a `before_first()` sentinel
/// of 0 that the session then advanced past, so a brand-new bridge published
/// `bdSeq = 1` — while `tck-id-topics-nbirth-bdseq-increment` requires the
/// number to *start at zero* ([#100], ADR 0042).
///
/// - **No file** — this node has never connected. `None`, and the first session
///   is numbered 0. Nothing can be replayed, because nothing was ever published.
/// - **Unreadable file** — this node HAS connected and we cannot tell under
///   which number. Answering 0 here would claim a first session that was not
///   one. The behaviour is deliberately left exactly as it was: continue from 0,
///   so the session opens at 1, and warn. It is a poor answer, it replays
///   numbers a long-lived consumer may have seen, and **refusing to start would
///   be safer** — that decision belongs to configuration validation and is not
///   this change's to take.
pub fn load_bd_seq(path: &std::path::Path) -> Option<BdSeq> {
    match crate::persist::load::<PersistedBdSeq>(path) {
        Ok(persisted) => Some(BdSeq::new(persisted.bd_seq)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!("no bdSeq state yet; this node's first session is numbered 0");
            None
        }
        Err(error) => {
            tracing::warn!(%error, "unreadable bdSeq state; continuing from 0 as before");
            Some(BdSeq::new(0))
        }
    }
}

/// Persists the session number BEFORE connecting, so a crash between connect
/// and the next boot cannot replay it.
pub fn store_bd_seq(path: &std::path::Path, bd_seq: BdSeq) -> std::io::Result<()> {
    crate::persist::persist_atomic(
        path,
        &PersistedBdSeq {
            bd_seq: bd_seq.value(),
        },
    )
}

/// A sink that queues outbound messages for the driver to publish.
#[derive(Debug, Default)]
struct Queue {
    pending: Vec<Outbound>,
}

impl Sink for Queue {
    fn emit(&mut self, message: Outbound) {
        self.pending.push(message);
    }
}

/// Delivery semantics per message type, read from the norm clause by clause.
///
/// **The specification does not say one thing for every edge-node message**, and
/// this function used to claim it did — *"requires QoS 0 and retain false for
/// every edge-node message"*. That sentence was false in one direction and
/// unsupported in the other, and it is the same over-generalisation ADR 0010
/// made about FR20, which `CLAUDE.md` records as having cost this project a
/// requirement. What the norm actually mandates for an Edge Node:
///
/// | message | QoS | retain | clause |
/// |---|---|---|---|
/// | the **will** (the NDEATH registered at CONNECT) | **1** | **false** | `tck-id-message-flow-edge-node-birth-publish-will-message-qos` (`Sparkplug_5:183`) and `-will-message-will-retained` (`:185`); **also** `tck-id-payloads-ndeath-will-message-qos` (`Sparkplug_6:1513`) and `-will-message-retain` (`:1515`) |
/// | NBIRTH | **0** | **false** | `tck-id-payloads-nbirth-qos` (`Sparkplug_6:1087`) and `-nbirth-retain` (`:1089`); also `…-publish-nbirth-qos` (`Sparkplug_5:228`) |
/// | DBIRTH | **0** | **false** | `tck-id-payloads-dbirth-qos` (`Sparkplug_6:1188`) and `-dbirth-retain` (`:1190`); also `tck-id-message-flow-device-birth-publish-dbirth-qos` (`Sparkplug_5:425`) |
/// | NDATA | **0** | **false** | `tck-id-payloads-ndata-qos` (`Sparkplug_6:1314`) and `-ndata-retain` (`:1316`) |
/// | DDATA | **0** | **false** | `tck-id-payloads-ddata-qos` (`Sparkplug_6:1371`) and `-ddata-retain` (`:1373`) |
/// | DDEATH, the **explicit** NDEATH | — | — | no QoS or retain clause exists |
///
/// **CORRECTED 2026-08-11 by the review of this story.** The table above said
/// *"NDATA, DDATA, DDEATH, the explicit NDEATH — the norm is silent"* and
/// counted three mandated rows. That was false: `tck-id-payloads-ndata-qos`
/// (*"NDATA messages MUST be published with the MQTT QoS set to 0"*) and
/// `tck-id-payloads-ddata-qos` say the opposite, and their `-retain` siblings
/// mandate `false` as well. **Five of the seven are mandated, not three.** The
/// sentence this story DELETED was accurate. It is the third time in this
/// project that a claim about the norm was written without reading the norm,
/// which is why `CLAUDE.md` opens with that rule — and the irony is that the
/// claim landed in the very test written to stop a MUST from being moved while
/// looking like a preference. The published behaviour was never wrong: QoS 0
/// was already what these messages carried.
///
/// What genuinely remains ours to choose is DDEATH and the explicitly published
/// NDEATH — the chapter-6 NDEATH clauses all govern the **Will Message**, not a
/// death published on the way out:
///
/// - **QoS 0 for DDEATH**, where nothing constrains us. At a 30 s period a lost
///   DDATA would be superseded within one cycle anyway, so a delivery guarantee
///   buys little; that reasoning is moot for NDATA/DDATA now that the norm is
///   read correctly, and survives only for DDEATH.
/// - **The explicit NDEATH rides at QoS 1 with the will. This is a CHOICE, not a
///   requirement.** It was justified here by `tck-id-…-death-payload`
///   (`Sparkplug_5:808-812`), which the review found to be
///   `tck-id-operational-behavior-host-application-death-payload` — the **Host
///   Application's** STATE will, JSON UTF-8 with `online`/`timestamp`. It says
///   nothing about an Edge Node, and nothing in chapters 5 or 6 mandates
///   byte-identity between the explicit NDEATH and the registered will. The
///   choice stands on its own merits: both are built from `MessageType::NDeath`,
///   and a lost death is the precise lie this bridge exists to prevent — the
///   host keeps a frozen value on screen and calls it current.
/// - **`retain = false` throughout.** Mandated for five of the seven (the will,
///   both births, NDATA and DDATA); chosen for DDEATH and the explicit NDEATH
///   for the reason the mandate exists — a retained payload is a value the
///   broker replays to a new subscriber with no way to judge its age, a stored
///   lie. Freshness is the BIRTH's job, not the broker's.
///
/// `MessageType::NCmd` is inbound: the bridge subscribes and never publishes it,
/// so the value returned here governs nothing. It is matched rather than caught
/// by a wildcard so that adding a message type stops the build until somebody
/// reads the norm for it.
///
/// **Left open deliberately:** DDEATH is unconstrained and stays at QoS 0, while
/// NDEATH now rides at 1. A lost DDEATH strands one device on the host exactly
/// as a lost NDEATH strands the node, so the asymmetry is real — but harmonising
/// them is a decision of its own and not the violation this story was written to
/// fix ([#26]).
/// Registers the death certificate as the connection's last will, inside the
/// CONNECT packet.
///
/// **Extracted 2026-08-11 so that a test can observe what is actually
/// registered.** Until then the only evidence for `tck-id-message-flow-edge-node-
/// birth-publish-will-message-qos` was an assertion on [`qos_for`], a pure
/// function three call sites away from the will — while the sibling clause
/// `-will-message-will-retained` was graded `gap (unproven)` in
/// `docs/sparkplug-conformance.md` for exactly that reason. Two halves of one
/// `set_last_will` call, identically derived and graded differently. A
/// `set_last_will` that dropped the `retain` argument, or that was handed the
/// wrong `MessageType`, passed every test in the tree.
///
/// The mistake this guards against is not hypothetical: the will's QoS was wrong
/// here from 2026-07-26 to 2026-08-10 with a green suite throughout, because the
/// test asserted the value the bug produced ([#26]).
fn register_will(options: &mut MqttOptions, topic: &str, payload: &[u8]) {
    let (qos, retain) = qos_for(MessageType::NDeath);
    options.set_last_will(rumqttc::LastWill::new(topic, payload, qos, retain));
}

fn qos_for(message: MessageType) -> (QoS, bool) {
    match message {
        // Mandated 0 — `-nbirth-qos`, `-dbirth-qos`.
        MessageType::NBirth | MessageType::DBirth => (QoS::AtMostOnce, false),
        // Mandated 1 for the will; the explicit death is the same payload.
        MessageType::NDeath => (QoS::AtLeastOnce, false),
        // Unconstrained by the norm: our choice, stated above.
        MessageType::NData | MessageType::DData | MessageType::DDeath => (QoS::AtMostOnce, false),
        // Inbound only; publishes nothing.
        MessageType::NCmd => (QoS::AtMostOnce, false),
    }
}

/// A change to the served device set, applied to a session that is already live
/// (Story 5.2 AC4).
///
/// **Devices, not metrics.** The norm allows a DBIRTH mid-session provided the
/// NBIRTH went out in the same session
/// (`tck-id-message-flow-device-birth-publish-nbirth-wait`), which is what makes
/// enabling a meter cost one certificate instead of a whole rebirth. Adding a
/// *metric* to an existing device is a different thing and does need the full
/// sequence (`Sparkplug_5:695`) — see [`SparkplugPublisher::device_birth`].
///
/// **It carries both names since contract v13** ([ADR 0049]): the published one
/// builds the topic, the serial keys the declaration and is declared as a
/// property of the certificate. A command carrying only the serial would leave
/// the driver to look the other up, which is a second place for a device to
/// acquire a name.
///
/// [ADR 0049]: ../../../docs/adr/0049-the-device-is-named-by-its-measuring-point-and-vouched-for-by-its-serial.md
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceCommand {
    /// A meter was enabled, or its serial changed: announce it.
    Birth(DeviceIdentity),
    /// A meter was disabled, deleted, or re-serialised: bury it, so its last
    /// value stops reading as current on the host's screen.
    Death(DeviceIdentity),
}

/// What the transport task saw.
///
/// Deliberately no longer `Copy`: a SubAck carries the broker's answer, which is
/// a `Vec`. Losing `Copy` is the price of not throwing that answer away.
#[derive(Debug, PartialEq, Eq)]
enum Transport {
    /// The broker accepted the connection: it is time to subscribe, then birth.
    Connected,
    /// The connection dropped; the will covers us until we reconnect.
    Lost,
    /// The broker answered our SUBSCRIBE. Carried through rather than discarded:
    /// a refusal is return code `0x80` — not an error, not a disconnect — so a
    /// discarded answer makes a refused subscription indistinguishable from a
    /// topic nobody publishes on.
    Subscribed(Vec<SubscribeReasonCode>),
}

/// One inbound node command, exactly as it arrived.
///
/// Bytes, not a decoded payload: the decode can fail, and a malformed payload
/// from the network is an expected input whose failure belongs in a trace, not
/// in the transport task.
#[derive(Debug, PartialEq, Eq)]
struct Command {
    topic: String,
    payload: Vec<u8>,
    /// The MQTT retain flag as it arrived.
    ///
    /// Carried rather than discarded because it changes what the payload MEANS.
    /// `tck-id-payloads-ncmd-retain` (`Sparkplug_6_Payloads.adoc:1421`): *"NCMD
    /// messages MUST be published with the MQTT retain flag set to false"* — so a
    /// retained NCMD is not something a conformant Host Application can have
    /// sent, and acting on one is acting on a message nobody sent now. See
    /// [`classify`].
    retained: bool,
}

/// The shortest wait between two CONNECT attempts, and therefore the ceiling on
/// how often `bdSeq` is persisted — one fsync-ing write per session (Story 4.10).
/// A broker refusing every connection can cost at most one write per second.
///
/// **This floor is a durability bound, not politeness**, which is why
/// [`jittered`] only ever adds to a wait and never subtracts from one.
const RECONNECT_FLOOR: Duration = Duration::from_secs(1);

/// The longest wait *before jitter*. Doubling from the floor, capped here, so an
/// outage that lasts hours does not leave the bridge waiting hours to notice it
/// is over. With jitter the longest actual wait is half again as long — 45 s —
/// which still serves that purpose.
const RECONNECT_CEILING: Duration = Duration::from_secs(30);

/// Bound of the inbound-command queue.
///
/// Small on purpose, and paired with a traced drop rather than a block — see
/// [`pump_transport`]. Unbounded would trade a stall for unbounded memory.
///
/// *(This doc comment was attached to [`RECONNECT_FLOOR`] until 2026-08-03,
/// leaving that constant described as a queue bound and this one undocumented.)*
const COMMAND_QUEUE: usize = 8;

/// Spreads reconnect attempts, **upwards only**: the wait becomes
/// `backoff + [0, backoff/2)`.
///
/// # Why not the usual recipe
///
/// The textbook form is *full jitter*, `sleep(rand(0, backoff))`. It is wrong
/// here. [`RECONNECT_FLOOR`] bounds how often `bdSeq` reaches the disk — one
/// fsync-ing write per session — so a wait shorter than the floor would quietly
/// break the durability property Story 4.10 rests on, and nothing in the type
/// system or the tests would have said so. Additive jitter cannot violate it:
/// the result is never smaller than its input, and the input is never below the
/// floor.
///
/// # Where the entropy comes from
///
/// The monotonic clock's low bits, through the existing [`Clock`] seam, rather
/// than a new dependency. Two consequences worth stating: it is *predictable*,
/// not random — fine, because the purpose is to keep independent clients from
/// re-synchronising after a shared outage, and independent clients have
/// independent clock offsets — and it stays injectable, so a test can pin it.
///
/// A `span` of zero (a sub-2 ms backoff, which the floor makes unreachable in
/// production) returns the backoff unchanged rather than dividing by zero.
/// One step of the reconnect ladder: returns `(the base wait to use now, the
/// base for the step after this one)`.
///
/// `established` means the session that just ended had reached CONNACK.
///
/// # Why a session that connected resets the ladder
///
/// Until 2026-08-03 there was no reset at all: `backoff` was declared outside
/// the session loop and only ever doubled, so it was monotonic for the lifetime
/// of the process. Five transport losses put it at the ceiling and it stayed
/// there — not for the outage, but until someone restarted the bridge. A
/// one-second blip on a Tuesday, after a rough Monday, cost thirty seconds; and
/// since Story 4.10 a reconnect is a full NDEATH → NBIRTH, so for those thirty
/// seconds the node was **offline to every consumer** and the readings taken in
/// the window were lost. Nothing announced it: the WARN printed the number, but
/// reading it meant noticing that a value which should be 1000 was 30000. [#46]
///
/// The ladder exists for a broker that will not accept us. A broker that accepts
/// us and later drops the connection has answered that question — the evidence
/// the ladder was accumulating is stale, so it is discarded.
///
/// # What this does NOT do
///
/// A broker that accepts and immediately drops, forever, now reconnects at the
/// floor rather than climbing away from it. That is deliberate and already
/// bounded: one attempt and one fsync-ing `bdSeq` write per second, which is the
/// rate [`RECONNECT_FLOOR`] was chosen to cap and which its own doc comment
/// already calls acceptable for a broker refusing every connection. A flapping
/// broker costs exactly the same as a refusing one.
fn ladder_step(previous: Duration, established: bool, floor: Duration) -> (Duration, Duration) {
    let now = if established { floor } else { previous };
    (now, (now * 2).min(RECONNECT_CEILING.max(floor)))
}

/// The floor actually in force: the configured value, never below
/// [`RECONNECT_FLOOR`].
///
/// The clamp is here rather than at the call site so there is ONE place the
/// durability bound is enforced, and so a configuration cannot lower it by
/// arriving from somewhere new.
fn floor_in_force(configured: Duration) -> Duration {
    configured.max(RECONNECT_FLOOR)
}

fn jittered(backoff: Duration, entropy: i64) -> Duration {
    let span = (backoff.as_millis() as u64) / 2;
    if span == 0 {
        return backoff;
    }
    backoff + Duration::from_millis(entropy.unsigned_abs() % span)
}

/// Bound on an inbound MQTT packet, in bytes.
///
/// The same value `rumqttc` defaults to, set EXPLICITLY. A bound that
/// `AC-LEAK-01` relies on for bounded memory must not be able to change under a
/// dependency bump with nothing failing, and a reader must be able to find it in
/// this repository rather than in the library's `Default` impl. See where it is
/// applied in [`run`] for the exposure it does and does not close.
const MAX_INCOMING_PACKET: usize = 10 * 1024;

/// Bound on an outbound MQTT packet, in bytes.
///
/// Generous by a wide margin: the largest thing this bridge publishes is a DBIRTH
/// carrying two metrics. Stated for the same reason as [`MAX_INCOMING_PACKET`] —
/// so that the day a payload grows toward it, the limit is somewhere a grep can
/// find.
const MAX_OUTGOING_PACKET: usize = 10 * 1024;

/// What the broker granted, distilled from the SubAck return codes.
///
/// Split out from the trace so it can be tested: the three failing shapes are
/// exactly the ones a live broker produces least often and a review notices
/// least easily.
#[derive(Debug, PartialEq, Eq)]
enum Granted {
    /// QoS 1, as `tck-id-message-flow-edge-node-ncmd-subscribe` requires.
    AsRequired,
    /// Accepted, but not at the mandated QoS 1. A downgrade to 0 is silent on
    /// every other channel: only this byte says so.
    NotAsRequired(QoS),
    /// Refused outright — return code `0x80`.
    Refused,
    /// A SubAck with no return code at all: a malformed answer, and not the
    /// same thing as a grant.
    Empty,
    /// More return codes than this bridge issued topic filters. The answer
    /// cannot be attributed, and guessing which entry is ours would put a
    /// confident, possibly wrong line in the log about the exact byte this
    /// story exists to stop discarding.
    Unattributable(usize),
}

/// Reads the broker's answer.
///
/// # Why the whole slice is examined and not just the first entry
///
/// A SubAck carries one return code per topic filter in the SUBSCRIBE it
/// answers, in order. This driver issues exactly one filter per connect, so a
/// well-formed answer holds exactly one code — and that invariant is CHECKED
/// here rather than assumed, because the code that assumes it goes wrong
/// silently.
///
/// It is not a hypothetical: Story 4.5 must subscribe to a STATE topic
/// (`tck-id-…-state-subs`), and `pump_transport` forwards every SubAck without
/// correlating it to the SUBSCRIBE it answers — `try_subscribe` returns no
/// packet id to correlate with. On the day a second subscription exists, taking
/// `codes[0]` would report a refused STATE subscription as a refused *NCMD*
/// one, sending the operator to the wrong ACL. `Unattributable` says "I do not
/// know" instead, which is the only honest answer available without packet-id
/// correlation.
fn granted(codes: &[SubscribeReasonCode]) -> Granted {
    match codes {
        [] => Granted::Empty,
        [SubscribeReasonCode::Failure] => Granted::Refused,
        [SubscribeReasonCode::Success(QoS::AtLeastOnce)] => Granted::AsRequired,
        [SubscribeReasonCode::Success(qos)] => Granted::NotAsRequired(*qos),
        many => Granted::Unattributable(many.len()),
    }
}

/// The name of the one command this bridge implements.
///
/// Re-exported from the publisher rather than spelled again: the string a host
/// addresses is the same string the NBIRTH declares, and two copies can drift
/// apart with nothing failing — the handler would simply stop matching what the
/// birth advertises, silently.
use crate::adapters::sparkplug_publisher::METRIC_NODE_CONTROL_REBIRTH;

/// How an inbound command payload was understood.
///
/// This classifies BYTES for a log line and for one decision. It is deliberately
/// not the action: `classify` recognises, [`trace_command_outcome`] says what
/// arrived, and the driver's command arm acts. Keeping the three apart is what
/// lets the next command — a meter relay, which switches physical hardware — add
/// an authorisation step between the second and the third without restructuring
/// anything. Do not fuse recognition and action into one match arm because there
/// is currently only one command.
///
/// Nothing here is a quality or staleness verdict: the driver still decides no
/// truth.
#[derive(Debug, PartialEq, Eq)]
enum Inbound {
    /// The bytes are not a Sparkplug payload. Expected input, not a bug.
    Undecodable(String),
    /// It decoded and carried nothing to act on.
    NoMetrics,
    /// A conformant Rebirth Request: a metric named `Node Control/Rebirth`
    /// carrying the boolean value `true`
    /// (`tck-id-operational-behavior-data-commands-ncmd-rebirth-name` and
    /// `-rebirth-value`).
    Rebirth {
        /// Any OTHER metrics the same payload carried, which are ignored.
        ///
        /// Ignored is not the same as unseen. This arm used to return before the
        /// name list was built, so a payload of
        /// `["Node Control/Next Server", "Node Control/Rebirth"=true]` was
        /// answered and the host was told nothing about the command that was
        /// discarded — while the module advertises that nothing is ignored
        /// silently. Found by the Story 4.7 code review. Capped at
        /// [`MAX_TRACED_METRICS`].
        ignored_alongside: Vec<String>,
    },
    /// Something addressed the rebirth endpoint and is NOT a Rebirth Request —
    /// the near miss, and the reason this variant exists at all.
    ///
    /// `-ncmd-rebirth-value` defines a request as carrying `true`, and this
    /// bridge implements the norm's reading. The failure mode of a strict
    /// matcher is that it never fires, SILENTLY, if a live host encodes the
    /// request some other way: the bridge would then report FR19 as implemented
    /// with nothing observably wrong anywhere. That is this project's signature
    /// failure shape — the contract-v1 quality codes, the four Epic 1 tests, the
    /// `bdSeq` tautology.
    ///
    /// So a near miss is recorded with the exact bytes that missed. It is not a
    /// courtesy: it is the whole mitigation, and it is what makes the
    /// pre-production Ignition run (Story 4.8) diagnose itself in one log line
    /// instead of presenting as silence.
    ///
    /// # The detection net is WIDER than the action, deliberately
    ///
    /// [`NearMiss`] carries which way it missed. The action requires an exact
    /// name and boolean `true`; detection also catches a name that only nearly
    /// matches, and a retained message. A detector no wider than the matcher it
    /// guards cannot report the matcher's own blind spot — which was the whole
    /// point of having one. Added by the Story 4.7 code review.
    RebirthNearMiss {
        /// Which clause it missed.
        reason: NearMiss,
        /// The offending metrics, capped at [`MAX_TRACED_METRICS`].
        received: Vec<RebirthAsReceived>,
        /// How many matched in total, before the cap.
        total: usize,
    },
    /// It decoded and named metrics, none of which this bridge implements.
    Unrecognised {
        /// The names, capped at [`MAX_TRACED_METRICS`].
        names: Vec<String>,
        /// How many metrics the payload carried, before the cap.
        total: usize,
    },
}

/// Which way a near miss missed.
///
/// One variant per clause, because the diagnosis differs: a `false` value is a
/// host doing something deliberate, a nearly-right name is a host built against
/// a different spelling, and a retained message is not a live request at all.
/// Three distinct traces, and `an_answered_a_missed_and_an_unknown_command_do_not_read_alike`
/// holds them apart.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum NearMiss {
    /// The name matched exactly and the value was not boolean `true`.
    /// `tck-id-operational-behavior-data-commands-ncmd-rebirth-value`.
    ValueNotTrue,
    /// The name only NEARLY matched — case, surrounding whitespace, or the
    /// specification's own `Node Control/Refresh` slip. See [`nearly_rebirth`].
    NameOnlyNearly,
    /// It arrived with the MQTT retain flag set, so it is not a request a
    /// conformant host can have sent. `tck-id-payloads-ncmd-retain`.
    Retained,
}

/// A metric that addressed the rebirth endpoint, exactly as it came off the wire.
///
/// Every field is what ARRIVED, never what was expected — a trace that renders
/// the expectation instead of the observation cannot diagnose a mismatch, which
/// is the only thing it is here to do.
#[derive(Debug, PartialEq, Eq)]
struct RebirthAsReceived {
    /// The name as received. Load-bearing for [`NearMiss::NameOnlyNearly`],
    /// where the spelling IS the diagnosis, and worth having on the other arms
    /// so one trace shape serves all three.
    name: String,
    /// The declared datatype code, or `None` if the metric declared none.
    /// `Boolean` is 11.
    datatype: Option<u32>,
    /// The value, rendered from the decoded wire variant.
    value: String,
}

/// Bound on how many metrics any one command trace renders.
///
/// The per-field cap ([`MAX_TRACED_CHARS`]) bounds one metric; this bounds the
/// LINE. Without it a ~10 KB NCMD carrying thousands of minimal metrics renders
/// ~13× its own size into the log — synchronously, on the task that also
/// publishes DATA — so the count achieves exactly the disk-fill the per-field cap
/// was written to prevent. Found by the Story 4.7 code review; the cap that
/// shipped bounded only the value.
const MAX_TRACED_METRICS: usize = 8;

/// Bound on how many characters any one rendered field carries.
const MAX_TRACED_CHARS: usize = 200;

/// Caps one rendered field, and SAYS when it capped: a line that has been
/// shortened must not read like a complete one.
fn cap_chars(text: &str) -> String {
    let count = text.chars().count();
    if count <= MAX_TRACED_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(MAX_TRACED_CHARS).collect();
    format!("{head}… <truncated from {count} chars>")
}

/// Renders a metric value for the near-miss trace.
///
/// The protobuf variant name is kept (`BooleanValue`, `IntValue`, …) because it
/// IS the diagnosis: a host that encodes the request as `IntValue(1)` rather
/// than `BooleanValue(true)` is the case this trace exists to make visible, and
/// a renderer that printed only `1` would hide exactly that.
///
/// # The two places it does not reproduce the bytes
///
/// The broker is unauthenticated, so anyone on the LAN can publish an NCMD, and
/// a string or bytes value has no bound. Those two variants are shortened BEFORE
/// they are formatted: `format!("{value:?}")` on a hostile 10 KB string would
/// materialise the whole thing — with Debug escaping able to multiply it — and a
/// cap applied afterwards has already paid the cost it exists to avoid. Every
/// other variant is small by construction, so it is rendered and then capped.
fn describe_value(value: Option<&sparkplug_b::protobuf::payload::metric::Value>) -> String {
    use sparkplug_b::protobuf::payload::metric::Value;

    let Some(value) = value else {
        // NO value field at all. Note what this does NOT cover: `is_null` is a
        // SEPARATE field on the metric, and a payload may legally set
        // `is_null: true` while also carrying a value. This function never sees
        // `is_null`, so `classify` is where that combination is judged — a
        // metric carrying `BooleanValue(true)` is a request whatever `is_null`
        // says, because the value is what -ncmd-rebirth-value constrains.
        // (The comment here previously claimed both cases landed in this branch,
        // which was false; corrected by the Story 4.7 code review.)
        return "<no value>".to_string();
    };
    match value {
        Value::StringValue(text) if text.chars().count() > MAX_TRACED_CHARS => {
            let head: String = text.chars().take(MAX_TRACED_CHARS).collect();
            format!(
                "StringValue({head:?}… <truncated from {} chars>)",
                text.chars().count()
            )
        }
        Value::BytesValue(bytes) if bytes.len() > MAX_TRACED_CHARS => {
            format!(
                "BytesValue(<{} bytes, first {MAX_TRACED_CHARS}: {:?}>)",
                bytes.len(),
                &bytes[..MAX_TRACED_CHARS]
            )
        }
        other => cap_chars(&format!("{other:?}")),
    }
}

/// Whether a name is the exact metric the norm names.
///
/// The ACTION requires this and nothing looser: three chapters spell
/// `Node Control/Rebirth`, and answering a different spelling would be inventing
/// a contract no host is bound by.
fn is_rebirth(name: &str) -> bool {
    name == METRIC_NODE_CONTROL_REBIRTH
}

/// Whether a name only NEARLY matches — for DETECTION, never for the action.
///
/// Three shapes, each a real host mistake rather than a hypothetical:
///
/// - **Case and surrounding whitespace.** A host that lower-cases its tag names,
///   or emits a trailing space, produces a name this bridge must not answer and
///   an operator must be able to see.
/// - **`Node Control/Refresh`.** The specification contradicts itself:
///   `Sparkplug_5_Operational_Behavior.adoc:950` says a host *"can send a
///   'Rebirth Request' using the 'Node Control/Refresh' metric"*, while every
///   `tck-id` in that same section — `-rebirth-name` at `:956`, `-ncmd-rebirth-name`
///   at `:973` — says `Node Control/Rebirth`. The tck-ids govern, so `Refresh` is
///   NOT answered; but a host built by someone reading the prose would send it,
///   and that is precisely the silent never-fires this detector exists to catch.
///
/// Returns false for the exact name: [`classify`] tests exactness first, and a
/// predicate that matched both would make the two arms ambiguous.
fn nearly_rebirth(name: &str) -> bool {
    /// The name the specification's own prose uses once, contradicting its
    /// tck-ids. Detected, never answered.
    const SPEC_PROSE_SLIP: &str = "Node Control/Refresh";

    if is_rebirth(name) {
        return false;
    }
    let trimmed = name.trim();
    trimmed.eq_ignore_ascii_case(METRIC_NODE_CONTROL_REBIRTH)
        || trimmed.eq_ignore_ascii_case(SPEC_PROSE_SLIP)
}

/// How a metric identified itself, for the trace.
///
/// Sparkplug lets a host address a metric by `alias` INSTEAD of by name — the
/// alias is established in the BIRTH and used thereafter. This bridge publishes
/// `alias: None` on everything it sends, but that governs what it emits, not
/// what a host may send it. An alias-addressed `Node Control/Rebirth` is legal
/// and would arrive with no name at all.
///
/// Collapsing that to a bare `<unnamed>` would reproduce the confusion the
/// `NoMetrics` arm exists to prevent: a name list whose names were, literally,
/// lost. Naming the alias keeps the trace diagnostic, and tells whoever
/// implements Story 4.7 that matching on the name alone is not sufficient.
fn metric_label(metric: &sparkplug_b::protobuf::payload::Metric) -> String {
    match (&metric.name, metric.alias) {
        // Capped: a name is attacker-supplied and has no bound on the wire.
        (Some(name), _) => cap_chars(name),
        (None, Some(alias)) => format!("<alias {alias}>"),
        (None, None) => "<neither name nor alias>".to_string(),
    }
}

/// Renders the rebirth-addressed metrics for the trace, capped, with the true
/// total alongside so a capped line cannot be mistaken for a complete one.
fn as_received(
    metrics: &[&sparkplug_b::protobuf::payload::Metric],
) -> (Vec<RebirthAsReceived>, usize) {
    let rendered = metrics
        .iter()
        .take(MAX_TRACED_METRICS)
        .map(|m| RebirthAsReceived {
            name: m.name.as_deref().map(cap_chars).unwrap_or_else(|| {
                m.alias
                    .map(|a| format!("<alias {a}>"))
                    .unwrap_or_else(|| "<neither name nor alias>".to_string())
            }),
            datatype: m.datatype,
            value: describe_value(m.value.as_ref()),
        })
        .collect();
    (rendered, metrics.len())
}

/// Classifies an inbound command payload for the trace.
///
/// Never panics: `decode` returns a `Result` and it is matched, because a
/// malformed payload arriving from the network is an ordinary event that must
/// not take the bridge down.
/// # What makes a Rebirth Request, and what only looks like one
///
/// `tck-id-operational-behavior-data-commands-ncmd-rebirth-name` and
/// `-ncmd-rebirth-value` (`Sparkplug_5_Operational_Behavior.adoc:970-975`)
/// define it as a metric named `Node Control/Rebirth` carrying the value
/// `true`. Both halves are required here. The name alone is NOT enough, and the
/// difference is not pedantic: `false` is the value this bridge's own NBIRTH
/// declares, so a host echoing our declaration back would otherwise trigger a
/// birth on every round trip.
///
/// The match is on the NAME, never on an alias. `-rebirth-name-aliases` exists
/// so that a host can request a rebirth *"without knowledge of any potential
/// alias"*; this bridge publishes `alias: None` on everything, so no host can
/// legitimately hold one for our metrics, and honouring an alias would add both
/// a path no conformant host can exercise and a way for an unrelated number to
/// trigger a birth. `metric_label` renders such a metric as `<alias N>` for the
/// trace, but the decision above never consults it — a display function is not
/// a place to keep a semantic rule.
///
/// A payload carrying several metrics of which one is the request is still a
/// request: the clause says the request MUST include the metric, not that it
/// must be alone.
///
/// # A retained NCMD is never a request
///
/// `tck-id-payloads-ncmd-retain` (`Sparkplug_6_Payloads.adoc:1421`): *"NCMD
/// messages MUST be published with the MQTT retain flag set to false."* So a
/// retained NCMD cannot have come from a conformant Host Application, and acting
/// on one means acting on a message nobody is sending now: the broker replays it
/// on every SUBSCRIBE, so one publish by any client on this unauthenticated
/// broker would make every future session answer a request that no longer exists
/// — indefinitely, with nobody present, and looking in the log exactly like a
/// real host asking. It is rejected here and reported as a near miss, so the
/// replay is visible rather than silent. See ADR 0017.
fn classify(payload: &[u8], retained: bool) -> Inbound {
    use sparkplug_b::protobuf::payload::metric::Value;

    let decoded = match sparkplug_b::decode(payload) {
        Err(error) => return Inbound::Undecodable(error.to_string()),
        Ok(decoded) => decoded,
    };
    if decoded.metrics.is_empty() {
        return Inbound::NoMetrics;
    }

    let exact: Vec<_> = decoded
        .metrics
        .iter()
        .filter(|m| m.name.as_deref().is_some_and(is_rebirth))
        .collect();

    if exact
        .iter()
        .any(|m| m.value == Some(Value::BooleanValue(true)))
    {
        // Conformant in every respect but one: the retain flag. Reported rather
        // than answered, and reported with the bytes, because a rejection nobody
        // can see is the same failure as a match that never fires.
        let (received, total) = as_received(&exact);
        return if retained {
            Inbound::RebirthNearMiss {
                reason: NearMiss::Retained,
                received,
                total,
            }
        } else {
            Inbound::Rebirth {
                ignored_alongside: decoded
                    .metrics
                    .iter()
                    .filter(|m| !m.name.as_deref().is_some_and(is_rebirth))
                    .take(MAX_TRACED_METRICS)
                    .map(metric_label)
                    .collect(),
            }
        };
    }
    if !exact.is_empty() {
        let (received, total) = as_received(&exact);
        return Inbound::RebirthNearMiss {
            reason: NearMiss::ValueNotTrue,
            received,
            total,
        };
    }

    // DETECTION ONLY, and wider than the action on purpose — see `nearly_rebirth`.
    // A name that misses by case, by whitespace, or by the specification's own
    // `Node Control/Refresh` slip is the most likely way a real host's request
    // fails to match, so it must not fall into the low-signal unrecognised path
    // with no datatype and no value.
    let nearly: Vec<_> = decoded
        .metrics
        .iter()
        .filter(|m| m.name.as_deref().is_some_and(nearly_rebirth))
        .collect();
    if !nearly.is_empty() {
        let (received, total) = as_received(&nearly);
        return Inbound::RebirthNearMiss {
            reason: NearMiss::NameOnlyNearly,
            received,
            total,
        };
    }

    Inbound::Unrecognised {
        names: decoded
            .metrics
            .iter()
            .take(MAX_TRACED_METRICS)
            .map(metric_label)
            .collect(),
        total: decoded.metrics.len(),
    }
}

/// Traces what the broker answered to the command subscription.
///
/// # Why this is a function and not four arms inside the `select!`
///
/// It was four arms, and the Story 4.6 review showed why that is not enough.
/// The falsification for AC2 collapsed [`granted`], which the unit test calls
/// directly — so it went red. But swapping the *bodies* of two arms, so that a
/// refusal logs *"granted at QoS 1"*, left every test green: nothing asserted
/// the log line, only the classification behind it. AC2 is written entirely in
/// terms of what an operator can see, so the line IS the behaviour, and a
/// behaviour inside a `select!` arm cannot be tested without running the whole
/// driver against a broker.
///
/// None of these outcomes aborts the session. A bridge that can publish but not
/// receive is strictly better than one that does neither, and the operator
/// learns which it is from the log alone — no broker access needed.
fn trace_subscription_outcome(topic: &str, outcome: Granted) {
    match outcome {
        Granted::AsRequired => {
            tracing::info!(%topic, "command subscription granted at QoS 1");
        }
        Granted::NotAsRequired(qos) => {
            tracing::warn!(
                %topic,
                ?qos,
                "the broker granted the command subscription at a QoS the \
                 specification does not permit (it requires 1); a command may be \
                 lost without either side noticing"
            );
        }
        Granted::Refused => {
            tracing::error!(
                %topic,
                "the broker REFUSED the command subscription (return code 0x80); \
                 the bridge keeps publishing but can receive no command — check \
                 the broker's ACL for this topic"
            );
        }
        Granted::Empty => {
            tracing::error!(
                %topic,
                "the broker answered the subscription with no return code; whether \
                 it is in force is unknown"
            );
        }
        Granted::Unattributable(count) => {
            tracing::error!(
                %topic,
                count,
                "a SubAck carried more return codes than this bridge subscribed to \
                 topics; the answer cannot be attributed to any one subscription \
                 and is NOT being read as a grant"
            );
        }
    }
}

/// Traces what arrived on the command topic, and drops it.
///
/// Extracted for the same reason as [`trace_subscription_outcome`]: AC3 says an
/// unrecognised command is ignored *loudly*, so the loudness is the property and
/// it needs a test that is not a broker.
fn trace_command_outcome(topic: &str, inbound: &Inbound) {
    match inbound {
        Inbound::Undecodable(error) => {
            tracing::warn!(
                %topic,
                %error,
                "an NCMD that is not a Sparkplug payload was ignored"
            );
        }
        Inbound::NoMetrics => {
            tracing::info!(
                %topic,
                "an NCMD carrying no metric was ignored (never silently)"
            );
        }
        Inbound::Rebirth { ignored_alongside } => {
            // INFO, and it must stay at INFO: `main.rs` sets INFO as the default
            // directive, so this is the highest level that is guaranteed visible
            // with no `RUST_LOG` set. AC2 is written in terms of what an
            // operator sees, and a criterion nobody can observe is not met.
            //
            // It does NOT contain the word "ignored". The Story 4.6 chaos test
            // greps for the ignore phrase, and a handler that acted on a command
            // while logging that it had thrown it away would keep that assertion
            // green while doing the opposite of what it asserts.
            //
            // This line reports RECOGNITION, not the answer. `announce` traces
            // the answer itself, and the two must not be conflated: a test that
            // greps for this line proves the bytes were understood, and stays
            // green if the action is deleted or fails. The Story 4.7 code review
            // found `chaos_ncmd_subscription` doing exactly that.
            tracing::info!(
                %topic,
                metric = METRIC_NODE_CONTROL_REBIRTH,
                "Rebirth Request accepted; re-announcing the node and its devices"
            );
            if !ignored_alongside.is_empty() {
                // Rare, and silent until the Story 4.7 review: a request may ride
                // alongside commands this bridge does not implement, and those
                // were discarded with nothing said.
                tracing::info!(
                    %topic,
                    ?ignored_alongside,
                    "the same NCMD carried other metrics, which are ignored; this \
                     bridge implements Node Control/Rebirth and no other command"
                );
            }
        }
        Inbound::RebirthNearMiss {
            reason,
            received,
            total,
        } => {
            // WARN, not INFO. This is the near-miss detector: it fires when
            // something addressed the rebirth endpoint and did not match the
            // norm's definition of a request. Nothing in normal operation sends
            // one, so it is rare by construction and worth finding — and if a
            // live host encodes the request in a way this bridge does not
            // accept, THIS is the line that says so, with the bytes.
            //
            // `?received` is not decoration. Without the name, datatype and value
            // exactly as they arrived, a strict matcher that never fires is
            // indistinguishable from a host that never asked, which is the one
            // failure mode of implementing the norm's reading literally.
            //
            // One message per clause missed. A shared message would make the
            // three indistinguishable in a log, and the whole value of the
            // detector is telling an operator WHICH way the request missed.
            let clause = match reason {
                NearMiss::ValueNotTrue => {
                    "tck-id-operational-behavior-data-commands-ncmd-rebirth-value \
                     requires the boolean value true, and this metric does not carry \
                     it. If a Host Application meant to request a rebirth, the \
                     datatype and value above are what it actually sent"
                }
                NearMiss::NameOnlyNearly => {
                    "the name only NEARLY matches Node Control/Rebirth — it differs \
                     by case, by surrounding whitespace, or it is the \
                     'Node Control/Refresh' spelling that the specification's own \
                     prose uses at Sparkplug_5_Operational_Behavior.adoc:950 while \
                     every tck-id in that section says 'Rebirth'. The tck-ids \
                     govern, so this was NOT answered; the name above is what \
                     arrived, and it is what a host would have to change"
                }
                NearMiss::Retained => {
                    "it arrived with the MQTT RETAIN flag set, and \
                     tck-id-payloads-ncmd-retain requires NCMD to be published with \
                     retain false — so this is a message the broker replayed, not a \
                     request a host is making now. Answering it would re-announce \
                     the node on every reconnect for as long as the retained \
                     message exists. Clear it by publishing an empty retained \
                     payload to this topic. See ADR 0017"
                }
            };
            tracing::warn!(
                %topic,
                metric = METRIC_NODE_CONTROL_REBIRTH,
                ?reason,
                ?received,
                total,
                shown = received.len(),
                "an NCMD addressed the rebirth endpoint but is not a Rebirth \
                 Request and was ignored: {clause}"
            );
        }
        Inbound::Unrecognised { names, total } => {
            // The NAMES, not the payload: a name list is diagnostic, a full dump
            // is noise and may carry values. Capped in count and per name, with
            // `total` alongside so a capped line cannot read as a complete one.
            tracing::info!(
                %topic,
                ?names,
                total,
                shown = names.len(),
                "unrecognised NCMD ignored; this bridge implements Node \
                 Control/Rebirth and no other command"
            );
        }
    }
}

/// Runs the driver until the inbox closes or shutdown is signalled.
///
/// `pulse` is the SAME fleet state the poll tasks write, not a second one: FOUR
/// of the six ways a reading can be lost happen on this side of the channel
/// (story 4.11), and a count kept separately here would give `/healthz` two
/// Whether the broker is reachable, and since when (story 6.5, [#53]).
///
/// # Why this is observed rather than inferred
///
/// `/healthz` reported `publishing: true` for any bridge that had reached the
/// publishing arm, including one whose broker was unreachable — corrected to
/// `intends_to_publish` on 2026-08-04 because that is all the bridge could
/// honestly assert. This is the fact that field could not carry: the driver sees
/// every CONNACK and every drop, and until now told nobody.
///
/// **`None` is not `Disconnected`.** A bridge that has never connected has not
/// lost anything, and an operator reading "disconnected" about a bridge that never
/// tried would go looking for an outage that did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkState {
    /// Whether the last transport event was a CONNACK.
    pub connected: bool,
    /// When that event happened.
    pub since: crate::domain::UtcMillis,
}

/// The shared handle: the driver writes, every surface reads.
///
/// Same shape as [`Heartbeats`](crate::app::poll_publish::Heartbeats) — a `watch`
/// the writer owns and readers clone from — and for the same reason: a reader
/// holding a snapshot holds a state that existed.
#[derive(Clone)]
pub struct SinkHealth(std::sync::Arc<tokio::sync::watch::Sender<Option<SinkState>>>);

impl Default for SinkHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl SinkHealth {
    /// A sink nothing has reported on yet.
    pub fn new() -> Self {
        Self(std::sync::Arc::new(tokio::sync::watch::Sender::new(None)))
    }

    /// Records a transport event. The driver calls this and nobody else.
    pub fn observed(&self, connected: bool, at: crate::domain::UtcMillis) {
        self.0.send_replace(Some(SinkState {
            connected,
            since: at,
        }));
    }

    /// The sink as it stands, or `None` if it has never connected.
    pub fn state(&self) -> Option<SinkState> {
        *self.0.borrow()
    }
}

/// The two independent healths FR29 asks for, travelling together.
///
/// **This type exists because the `#[allow]` below named its own revisit trigger**
/// — *"eight parameters, and the revisit trigger is the ninth"* — and story 6.5
/// reached it. The revision it asked for is this one: the meters' health and the
/// broker's are not two unrelated parameters, they are the pair FR29 requires to be
/// reported independently. Bundling them removes a decision rather than adding
/// ceremony, which is exactly the test the old comment set.
#[derive(Clone)]
pub struct Health {
    /// Per-meter liveness and verdicts — the SOURCE side.
    pub meters: crate::app::poll_publish::Heartbeats,
    /// Broker reachability — the SINK side.
    pub sink: SinkHealth,
}

/// numbers to reconcile for one question.
#[allow(clippy::too_many_arguments)]
// **Eight parameters, and the ninth arrived on 2026-08-19** — story 6.5 needed the
// sink's health beside the meters'. The trigger this comment set was honoured
// rather than raised: the two healths went into `Health`, because they are the pair
// FR29 requires to be reported independently and they DO travel together. The count
// is unchanged at eight, and the next revisit trigger is the ninth again.
pub async fn run(
    config: MqttConfig,
    node: sparkplug_b::EdgeNode,
    mut meters: Vec<DeviceIdentity>,
    clock: std::sync::Arc<dyn Clock + Send + Sync>,
    health: Health,
    mut inbox: mpsc::Receiver<MeterUpdate>,
    mut devices: mpsc::Receiver<DeviceCommand>,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    // The meters' half of `health`, bound once so the body below reads exactly as it
    // did before story 6.5 grouped the two.
    let pulse = health.meters.clone();
    // The command topic, built from the same validated grammar as everything we
    // publish, and built BEFORE `node` is handed to the publisher.
    //
    // The identifiers were already validated when the `EdgeNode` was
    // constructed and NCMD is node-level, so this cannot fail today. It is
    // still handled rather than unwrapped: publishing without a command path is
    // strictly better than not publishing.
    let ncmd_topic = match node.node_topic(MessageType::NCmd) {
        Ok(topic) => Some(topic),
        Err(error) => {
            tracing::error!(
                %error,
                "no NCMD topic could be built; the bridge will publish but can \
                 receive no command"
            );
            None
        }
    };

    // 1. Restore the session number. `SparkplugPublisher::new` advances past the
    //    value on disk, so the first session of this process is already fresh.
    let previous = load_bd_seq(&config.bd_seq_path);
    let mut publisher = SparkplugPublisher::new(node, previous);

    // THE SESSION LOOP (Story 4.10). One iteration = one CONNECT = one `bdSeq`.
    //
    // The loop exists because `rumqttc` cannot be told about a new will: it
    // rebuilds the CONNECT packet from the `MqttOptions` captured when the client
    // was constructed. Reconnecting internally therefore re-registers the OLD
    // death certificate, which is why the session number used to be frozen for a
    // client's lifetime. Rebuilding the client is the only way to give the broker
    // a will that matches the session it is about to cover.
    //
    // What must NOT be done instead: advance `bdSeq` while leaving the will
    // alone. That is strictly worse than the old deviation — the broker would
    // hold a certificate for a session that no longer exists, a consumer pairing
    // death to birth would discard it, and a frozen value would stay on screen
    // presented as live.
    let floor = floor_in_force(config.reconnect_floor);
    let mut backoff = floor;
    let mut first_session = true;
    // Whether the reconfiguration channel is still open. It belongs OUTSIDE the
    // session loop: a supervisor that dropped its end stays dropped, and
    // re-arming the branch on every reconnect would spin the new session
    // instead.
    let mut devices_open = true;
    loop {
        // 2. Advance for every session after the first, then persist BEFORE
        //    connecting: a crash between the CONNECT and the next boot must not
        //    be able to replay a number the broker has already seen.
        //
        //    This write now happens once per CONNECT rather than once per boot,
        //    and `persist_atomic` is write + fsync + rename + fsync(dir). The
        //    rate is bounded by `RECONNECT_FLOOR` below — which makes the backoff
        //    floor a DURABILITY property, not merely a politeness to the broker.
        if !first_session {
            publisher.new_session();
        }
        first_session = false;
        if let Err(error) = store_bd_seq(&config.bd_seq_path, publisher.bd_seq()) {
            tracing::error!(%error, "could not persist bdSeq; a restart may replay a session");
        }

        // 3. Serialise the DEATH for THIS session...
        let will = publisher.will(clock.wall());

        // 4. ...and register it IN the CONNECT packet, before connecting.
        let mut options = MqttOptions::new(&config.client_id, &config.host, config.port);
        options.set_keep_alive(config.keep_alive);
        // The incoming packet bound is OURS, stated, not inherited.
        //
        // `rumqttc` defaults `max_incoming_packet_size` to 10 KiB and rejects a larger
        // frame inside `poll()`, which returns `Err` — so the socket drops
        // UNGRACEFULLY and the broker fires our will. The host is told the node died
        // while it was alive. The bridge cannot drop the frame and keep the session;
        // that is rumqttc's deliberate behaviour, and the correct control is
        // broker-side (`message_size_limit` in Mosquitto, plus ACLs — Epics 5/7).
        //
        // The value is set here anyway, at the same size, for two reasons the Story 4.7
        // review made concrete. It is a limit `AC-LEAK-01` depends on for bounded
        // memory, so it must not change silently under a dependency bump. And no
        // legitimate NCMD for this bridge approaches it: the one command it implements
        // is a single boolean metric. Raising it would only move the cliff, at the cost
        // of the bound.
        //
        // The residual exposure is recorded in `deferred-work.md` (deferred by
        // decision, 2026-07-29) and re-examined by the Story 4.7 review: `retain`
        // removes that deferral's assumption of a SUSTAINED attacker, because a
        // retained oversized frame is redelivered on every reconnect. `classify`
        // rejects a retained NCMD, but that runs after decode and this frame never
        // decodes — so the two are separate problems and only one of them is closed.
        options.set_max_packet_size(MAX_INCOMING_PACKET, MAX_OUTGOING_PACKET);
        register_will(&mut options, &will.topic, &will.payload);

        // 5. Connect — the EventLoop pumps in its own task (see the module docs).
        let (client, eventloop) = AsyncClient::new(options, config.capacity);
        let (transport_tx, mut transport_rx) = mpsc::channel(8);
        // Inbound commands get their OWN channel. Sharing the transport channel
        // would put a live, externally-driven path behind an 8-slot bound whose
        // sender blocks — see `pump_transport`.
        let (command_tx, mut command_rx) = mpsc::channel(COMMAND_QUEUE);
        let pump = tokio::spawn(pump_transport(
            eventloop,
            transport_tx,
            command_tx,
            ncmd_topic.clone(),
        ));

        // Did this session ever reach CONNACK? It is what tells a broker that
        // will not have us from one that had us and let go — see [`ladder_step`].
        let mut established = false;

        let ended = loop {
            tokio::select! {
                event = transport_rx.recv() => {
                    match event {
                        Some(Transport::Connected) => {
                            established = true;
                            // **THE FACT [#53] ASKED FOR**, recorded where it is
                            // known and nowhere else: this arm fires on every
                            // CONNACK, so it covers the first connect and every
                            // reconnect alike.
                            health.sink.observed(true, clock.wall());
                            // 5. Connected: subscribe to NCMD BEFORE birthing. The
                            // order of these two statements IS the requirement; see
                            // the module docs.
                            if let Some(topic) = &ncmd_topic {
                                subscribe_to_commands(&client, topic);
                            }
                            // 6. Then publish the BIRTH.
                            announce(
                                &client,
                                &mut publisher,
                                clock.wall(),
                                &meters,
                                BirthReason::Connected,
                            );
                        }
                        Some(Transport::Subscribed(codes)) => {
                            let topic = ncmd_topic.as_deref().unwrap_or("<none>");
                            trace_subscription_outcome(topic, granted(&codes));
                        }
                        Some(Transport::Lost) => {
                            health.sink.observed(false, clock.wall());
                            tracing::warn!("transport lost; the will covers us until we reconnect");
                        }
                        None => {
                            // The pump returns on a transport error, so this is the
                            // normal disconnect path, not an anomaly.
                            break SessionEnd::TransportLost;
                        }
                    }
                }
                command = command_rx.recv() => {
                    let Some(command) = command else {
                        // The pump owns both senders, so this means the same thing
                        // as the transport channel closing. Breaking here rather
                        // than continuing matters: `recv` on a closed channel
                        // returns `None` immediately and forever, so a `continue`
                        // would spin.
                        break SessionEnd::TransportLost;
                    };
                    // Recognise, then say what arrived, then act — three steps, in
                    // that order, and deliberately not fused. The next command this
                    // bridge implements is a meter relay, which switches physical
                    // hardware; it will need an authorisation step between the
                    // second and the third, and this shape is where it goes.
                    let inbound = classify(&command.payload, command.retained);
                    trace_command_outcome(&command.topic, &inbound);
                    if matches!(inbound, Inbound::Rebirth { .. }) {
                        // INLINE AND SYNCHRONOUS, and that is the whole of AC3.
                        //
                        // `tck-id-operational-behavior-data-commands-rebirth-action-1`
                        // requires the node to IMMEDIATELY stop sending DATA on
                        // receipt. `select!` runs one branch to completion before
                        // polling the others, so between this line and the last
                        // DBIRTH the `inbox` branch cannot run and no DATA can
                        // interleave. Nothing here enforces that — the SHAPE does.
                        //
                        // So: do not `tokio::spawn` this, do not set a flag consumed
                        // on a later iteration, do not push it through a channel.
                        // Any of the three would satisfy `-rebirth-action-2` (a
                        // birth does go out) while breaking `-rebirth-action-1`,
                        // with no test, no log line and no wire symptom to notice
                        // it by. `chaos_ncmd_rebirth_answered` asserts the absence
                        // of a DDATA across that window precisely because the
                        // property lives in the shape rather than in a check.
                        //
                        // NO RATE LIMIT AND NO COALESCING, decided rather than
                        // deferred. A host may burst — Ignition resends — and the
                        // command channel is 8 slots with a traced drop
                        // (`COMMAND_QUEUE`), so a burst already degrades to some
                        // answers plus visible drops and never to a stall. A birth
                        // here is 1 NBIRTH + N DBIRTHs for a single configured
                        // meter, so the answer is cheap; suppressing a request is
                        // the exact failure this handler exists to fix; and a
                        // suppressed answer is INVISIBLE to the host, which cannot
                        // tell it from a node that never heard. If a burst ever
                        // proves costly, Story 4.13 is where it is measured, not
                        // guessed.
                        announce(
                            &client,
                            &mut publisher,
                            clock.wall(),
                            &meters,
                            BirthReason::RebirthRequested,
                        );
                    }
                }
                // The device set changed while the session is alive (AC4).
                //
                // INLINE AND SYNCHRONOUS, for the same reason the rebirth answer
                // above is: `select!` runs one branch to completion, so between
                // the DBIRTH emitted here and the next `inbox` reading no DDATA
                // can interleave. That IS the ordering AC4 asks for — a DDATA
                // carrying a metric the host has not been told about is what
                // `Sparkplug_5_Operational_Behavior.adoc:863` warns against, and
                // publishing the certificate first is what avoids it. The shape
                // enforces it; no check does.
                command = devices.recv(), if devices_open => {
                    let Some(command) = command else {
                        // The supervisor dropped its end. `recv` on a closed
                        // channel returns `None` immediately and forever, so an
                        // unguarded branch would spin this loop at full speed.
                        // Disarm it rather than break: losing the ability to
                        // reconfigure is not a reason to end a healthy session.
                        devices_open = false;
                        continue;
                    };
                    let mut queue = Queue::default();
                    let outcome = match &command {
                        DeviceCommand::Birth(device) => {
                            if !meters.contains(device) {
                                meters.push(device.clone());
                            }
                            publisher.device_birth(clock.wall(), device, &mut queue)
                        }
                        DeviceCommand::Death(device) => {
                            // Drop it from the set BEFORE the certificate goes
                            // out, so a reconnect racing this cannot re-birth a
                            // device we are in the middle of burying.
                            //
                            // **By SERIAL, not by the pair.** A meter that is
                            // being buried because its serial changed is the same
                            // measuring point under a new box: the entry to drop
                            // is the one for the device that is going away, and
                            // matching the whole pair would leave it in the set.
                            meters.retain(|m| m.serial != device.serial);
                            publisher.device_death(clock.wall(), device, &mut queue)
                        }
                    };
                    match outcome {
                        Ok(true) => {
                            let refused = publish_all(&client, &mut queue);
                            if !refused.is_empty() {
                                // Same defect as [#88] one level down: a DBIRTH
                                // this branch emitted for a newly enabled meter
                                // leaves the device declared here while the host
                                // never heard of it. A DDEATH is not withdrawn —
                                // `device_death` has already removed the device,
                                // and a burial the transport refused leaves the
                                // host holding a device we no longer publish for,
                                // which no local state can repair.
                                let devices: Vec<Serial> = refused
                                    .iter()
                                    .filter(|m| m.message == MessageType::DBirth)
                                    .filter_map(|m| m.device.clone())
                                    .collect();
                                publisher.withdraw(&devices);
                                tracing::error!(
                                    ?command,
                                    ?devices,
                                    "the device certificate was NOT published: the \
                                     outbound queue rejected it. The host still holds \
                                     the previous view of this device"
                                );
                            } else {
                                tracing::info!(?command, "device certificate published");
                            }
                        }
                        Ok(false) => {
                            // No live session, so there is nothing to amend. The
                            // next connect births `meters`, which this branch has
                            // already updated — so the change is not lost, it is
                            // simply carried by the birth instead of a certificate.
                            tracing::info!(
                                ?command,
                                "no live session; the change will be carried by the next BIRTH"
                            );
                        }
                        Err(error) => {
                            tracing::error!(
                                %error,
                                ?command,
                                "the device identifier is not a usable topic level; \
                                 the device set is unchanged on the wire"
                            );
                        }
                    }
                }
                update = inbox.recv() => {
                    let Some(update) = update else {
                        tracing::info!("poll task closed the channel; stopping");
                        break SessionEnd::InboxClosed;
                    };
                    // EVERY OUTCOME EITHER PUBLISHES OR COUNTS, and story 4.11
                    // is the requirement that there is no third possibility. The
                    // reason slug and the source timestamp travel with each, so a
                    // reading lost here is identified by WHEN IT WAS TRUE — the
                    // only handle an operator has for lining a gap up against the
                    // source.
                    //
                    // ONE LOSS, ONE RECORD. This emitted an `error!` here AND the
                    // `warn!` below for the same unpublishable reading until the
                    // 2026-08-18 review — so a log-based tally double-counted, and
                    // the `error!` fired BEFORE the counter moved, leaving a window
                    // where a scrape saw a logged loss with no count. The error is
                    // now a field on the one line that carries `meter`, `reason`
                    // and `value_date`.
                    if let Some(loss) = deliver(&client, &mut publisher, &update) {
                        let Loss { reason, fault, .. } = &loss;
                        let (reason, fault) = (*reason, fault.clone());
                        count_loss(&pulse, &update.meter, &loss);
                        tracing::warn!(
                            meter = %update.meter,
                            serial = %update.measurement.serial,
                            reason = crate::app::poll_publish::DropReason::as_str(reason),
                            value_date = update.measurement.value_date.0,
                            fault = fault.as_deref().unwrap_or("-"),
                            "the judged reading never reached the wire; dropped, never buffered"
                        );
                    }
                }
                _ = &mut shutdown => {
                    tracing::info!("shutdown requested");
                    break SessionEnd::Shutdown;
                }
            }
        };

        if matches!(ended, SessionEnd::TransportLost) {
            // Reconnect under a NEW session number. The will the broker still
            // holds belongs to the session that just ended and covers it
            // correctly; the next CONNECT registers a different one.
            pump.abort();
            // Computed BEFORE the log, so the number traced is the wait that
            // actually happens. Logging the un-jittered base would have made the
            // log describe something other than the behaviour — the defect this
            // project keeps finding in its own documents.
            let (base, next) = ladder_step(backoff, established, floor);
            let wait = jittered(base, clock.monotonic().0);
            tracing::warn!(
                bd_seq = publisher.bd_seq().value(),
                established,
                backoff_ms = base.as_millis() as u64,
                wait_ms = wait.as_millis() as u64,
                "transport lost; the will covers the ended session, reconnecting under a new one"
            );
            tokio::time::sleep(wait).await;
            backoff = next;
            continue;
        }

        // A polite death: queue it, then keep the transport pumping long enough for
        // it to actually reach the wire. We never send a graceful DISCONNECT — that
        // instructs the broker to DISCARD the will, so if our explicit death did not
        // make it, nothing would ever be delivered. Dropping the connection instead
        // keeps the will as the second mechanism.
        publish(&client, publisher.will(clock.wall()));
        let flushed = tokio::time::timeout(config.death_flush, async {
            // The pump is still running; give it time to drain the request channel.
            tokio::time::sleep(config.death_flush).await;
        })
        .await;
        if flushed.is_err() {
            tracing::warn!("death flush timed out; falling back to the will");
        }
        pump.abort();
        tracing::info!("death published; transport dropped");
        return;
    }
}

/// Why one session ended, and therefore whether another should begin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionEnd {
    /// The transport dropped. Reconnect under a new `bdSeq`.
    TransportLost,
    /// SIGTERM (or the supervisor). Announce the death and stop.
    Shutdown,
    /// The poll task closed the channel; there is nothing left to publish.
    InboxClosed,
}

/// Owns the `EventLoop` alone, because `poll()` is not cancellation-safe.
///
/// `ncmd_topic` is what an inbound publish is matched against — exact equality,
/// not a prefix: the bridge subscribes to one topic and anything else arriving
/// here is not a command addressed to this node.
async fn pump_transport(
    mut eventloop: EventLoop,
    events: mpsc::Sender<Transport>,
    commands: mpsc::Sender<Command>,
    ncmd_topic: Option<String>,
) {
    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                if events.send(Transport::Connected).await.is_err() {
                    return;
                }
            }
            Ok(Event::Incoming(Packet::SubAck(ack))) => {
                if events
                    .send(Transport::Subscribed(ack.return_codes))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Ok(Event::Incoming(Packet::Publish(publish)))
                if ncmd_topic.as_deref() == Some(publish.topic.as_str()) =>
            {
                // `try_send`, never `send().await`. THIS task is what answers
                // PINGREQ: blocking it on a full queue would cost the session
                // its keep-alive and the broker would disconnect a bridge that
                // was otherwise healthy. Same rule as `publish()` — a full
                // queue is a traced drop, never a block.
                let command = Command {
                    topic: publish.topic.clone(),
                    payload: publish.payload.to_vec(),
                    // Carried, not discarded: `tck-id-payloads-ncmd-retain`
                    // forbids a host from retaining an NCMD, so this flag decides
                    // whether the bytes are a request or a replay. It was dropped
                    // here until the Story 4.7 code review — see `classify`.
                    retained: publish.retain,
                };
                match commands.try_send(command) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(dropped)) => {
                        tracing::warn!(
                            topic = %dropped.topic,
                            "command queue full; NCMD dropped (never silently)"
                        );
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => return,
                }
            }
            Ok(_) => {}
            Err(error) => {
                // The command topic is in the line ON PURPOSE, and it is the only
                // route from this symptom to one of its causes. A PUBLISH larger
                // than `MAX_INCOMING_PACKET` on that topic is rejected by
                // `poll()` before any of this bridge's code sees it, so it
                // presents here — as a bare transport error — and nowhere else.
                // Without the topic an operator cannot tell an oversized NCMD from
                // an ordinary broker drop. Added by the Story 4.7 code review.
                tracing::warn!(
                    %error,
                    command_topic = ncmd_topic.as_deref().unwrap_or("<none>"),
                    max_incoming_bytes = MAX_INCOMING_PACKET,
                    "transport error; if this repeats, check whether something is \
                     publishing an oversized payload to the command topic — such a \
                     packet is rejected before it can be classified, and it drops \
                     the session ungracefully, which fires the will"
                );
                let _ = events.send(Transport::Lost).await;
                // RETURN, do not retry. Until Story 4.10 this arm slept a second
                // and polled again, which let `rumqttc` reconnect internally —
                // and an internal reconnect rebuilds the CONNECT packet from the
                // `MqttOptions` captured at construction, so it re-registers the
                // OLD will under the OLD `bdSeq`. Owning the session number means
                // owning the reconnect: the driver rebuilds the client, and the
                // backoff lives there with it.
                return;
            }
        }
    }
}

/// Why the node is announcing itself.
///
/// The two paths publish the SAME bytes; only the trace differs. That is not an
/// oversight to be tidied away — an operator reading a log needs to know whether
/// a node re-announced because its transport reconnected or because a host asked
/// it to, and those have entirely different causes to go looking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BirthReason {
    /// The broker accepted a connection (first connect or reconnect).
    Connected,
    /// A Host Application sent a conformant Rebirth Request.
    RebirthRequested,
}

/// Publishes the complete BIRTH sequence: one NBIRTH, then one DBIRTH per meter.
///
/// # Why the rebirth answer and the connect birth are the same three lines
///
/// `tck-id-operational-behavior-data-commands-rebirth-action-2` requires *"a
/// complete BIRTH sequence including the NBIRTH and DBIRTH(s)"* — which is
/// exactly what a connect already publishes. Two copies of that would be two
/// things to keep conformant, and the copy exercised once a day would be the one
/// that rots.
///
/// # The function this must NOT call
///
/// [`SparkplugPublisher::new_session`] advances `bdSeq`, and
/// `-rebirth-action-3` forbids that here: *"The NBIRTH MUST include the same
/// bdSeq metric with the same value it had included in the Will Message of the
/// previous MQTT CONNECT packet … Because a new MQTT Session is not being
/// established, there is no reason to update the bdSeq number."*
///
/// A rebirth RE-ANNOUNCES a session; it does not open one. Advancing the number
/// would leave the broker holding a will for a session number the live node no
/// longer claims, so the death that eventually fires would be discarded by any
/// consumer that pairs death to birth by `bdSeq` — the node would die and its
/// tags would stay green. Nothing calls `new_session` today: it is reachable,
/// wrong on this path, and the compiler will not stop anyone.
///
/// # A birth is never REPORTED complete unless it was
///
/// Two ways a birth can come out half-emitted. The first is closed by
/// construction: `birth()` validates every topic before emitting anything, so a
/// `TopicError` means the queue is empty and the session is untouched.
///
/// The second was open, and silent, until the Story 4.7 code review found it.
/// This drains the queue message by message and [`publish`] turns a full request
/// channel into a WARN and continues, so the NBIRTH could be queued and the DBIRTH
/// dropped — during the pump's one-second error-arm backoff, under TCP
/// back-pressure, or under the request burst this handler deliberately permits.
/// The host then resets its view of the node on an NBIRTH and never receives the
/// DBIRTH re-declaring the device, while the publisher has already committed
/// `Session::Live` and goes on emitting DDATA for a device the host regards as
/// undeclared.
///
/// **What was worse than the gap: the trace called that sequence *complete*.** So
/// the count of failures is now carried out of the drain and the success line is
/// emitted only if it is zero; otherwise this is an ERROR naming what the host now
/// believes.
///
/// # Why this is not an all-or-nothing drain, which is what the review asked for
///
/// It cannot be, against `rumqttc` 0.25. Refusing to publish anything unless the
/// whole sequence fits requires knowing how many slots are free, and there is no
/// public way to ask: `AsyncClient` wraps a private `flume::Sender` and exposes no
/// `capacity()`, `EventLoop::requests_tx` is `pub(crate)`, and there is no
/// constructor that takes a receiver we made ourselves. Nor can the drain simply
/// `await` its way to completeness: blocking here would hold the driver's `select!`
/// across an arbitrary broker outage, and the shutdown branch with it.
///
/// The residual gap is bounded and now self-healing. For the sequence not to fit,
/// 63 of 64 slots must be backed up, which means the broker is not draining and
/// the will is about to fire regardless — and a host that receives an NBIRTH
/// without its DBIRTH is exactly the condition that makes it send a Rebirth
/// Request, which this bridge now answers. Recorded rather than hidden: a
/// misreported birth was the defect, and that is fixed; an unlikely partial birth
/// is a known, logged, recoverable state.
fn announce(
    client: &AsyncClient,
    publisher: &mut SparkplugPublisher,
    now: crate::domain::UtcMillis,
    meters: &[DeviceIdentity],
    reason: BirthReason,
) {
    let mut queue = Queue::default();
    match publisher.birth(now, meters, &mut queue) {
        Ok(()) => {
            let queued = queue.pending.len();
            let refused = publish_all(client, &mut queue);
            if !refused.is_empty() {
                // Undo the half of the declaration the host never received,
                // BEFORE the log line — so a panic in formatting could not leave
                // the publisher counting readings it is not delivering ([#88]).
                let withdrawal = withdrawal_for(&refused);
                match &withdrawal {
                    Withdrawal::EveryDevice => publisher.withdraw_all(),
                    Withdrawal::These(devices) => publisher.withdraw(devices),
                }
                tracing::error!(
                    dropped = refused.len(),
                    queued,
                    ?withdrawal,
                    ?reason,
                    "the BIRTH sequence was only PARTLY published: the outbound \
                     queue rejected part of it. The host has NOT been told about \
                     these devices, so their readings are now counted as dropped \
                     rather than published, and stay so until a rebirth declares \
                     them"
                );
                return;
            }
            let bd_seq = publisher.bd_seq().value();
            match reason {
                BirthReason::Connected => {
                    tracing::info!(bd_seq, "session born");
                }
                BirthReason::RebirthRequested => {
                    // The `bdSeq` is in the line ON PURPOSE: it is the field
                    // -rebirth-action-3 constrains, and printing it next to the
                    // connect birth's identical value is how an operator sees
                    // that a rebirth did not open a session.
                    tracing::info!(
                        bd_seq,
                        "node re-announced on a Rebirth Request: complete BIRTH \
                         sequence republished under the SAME bdSeq, because a \
                         rebirth re-announces a session rather than opening one"
                    );
                }
            }
        }
        Err(error) => {
            tracing::error!(%error, ?reason, "refusing to birth: nothing was published");
        }
    }
}

/// Queues the node's command subscription.
///
/// Never blocks and never aborts the session: a failure here costs the bridge
/// its command path, not its ability to publish, and the birth still goes out.
///
/// # The QoS here does not contradict `qos_for`
///
/// `qos_for` governs what the edge node PUBLISHES, per message type, and
/// `the_delivery_table_matches_the_specification_clause_by_clause` pins it.
/// This is a *subscribe* QoS — a different field, in a different packet,
/// travelling the other way — and `tck-id-message-flow-edge-node-ncmd-subscribe`
/// mandates 1 for it. No conflict.
///
/// (Corrected by story 4.17: this block used to say `qos_for` "returns QoS 0 for
/// every message". It never should have, and since 4.17 it is plainly false —
/// the will rides at QoS 1.)
fn subscribe_to_commands(client: &AsyncClient, topic: &str) {
    // `try_subscribe` and `try_publish` feed the SAME request channel, so the
    // SUBSCRIBE queued here leaves the socket before the BIRTH queued after it.
    // That FIFO is what makes "prior to sending an NBIRTH" true without waiting
    // for the SubAck.
    if let Err(error) = client.try_subscribe(topic.to_string(), QoS::AtLeastOnce) {
        tracing::error!(
            %topic,
            %error,
            "could not queue the command subscription; the bridge births anyway \
             but can receive no command this session"
        );
    }
}

/// How the publisher's answer maps onto a [`DropReason`] (story 4.11 AC5).
///
/// # Pure, and extracted for that reason
///
/// The mapping is the only place a lost reading can be filed under the wrong
/// name, and inside the `select!` arm it would be reachable only through a live
/// broker — the shape the Epic 2 retrospective's action C1 exists to stop (*"a
/// property behind an `async fn` is extracted pure before the story closes"*).
/// Here it is a total function over the publisher's result, and
/// `every_publisher_outcome_is_classified` enumerates it.
///
/// `None` means the publisher emitted. It does **not** mean the reading reached
/// the wire, and there are TWO reasons for that, one of which nothing here can
/// close:
///
/// - the transport can refuse the queued message, which is the other axis and is
///   decided by [`publish_all`]'s count at the call site;
/// - and `AsyncClient::try_publish` answers `Ok` when the message enters
///   `rumqttc`'s request channel, **not** when it leaves the socket. Whatever is
///   still in that channel when the connection drops is discarded with the event
///   loop, uncounted. See [#85] — the undercount is largest in the first seconds
///   of an outage, which is exactly when FR22 is being asked its question.
///
/// [#85]: https://github.com/guycorbaz/smartme_mqtt/issues/85
///
/// **Kept for the tests that pin the whole mapping, error arm included.** Since
/// [#92] the production path answers a [`Pending`], so it reads the outcome half
/// through [`reason_for_outcome`] and handles the error at the call site — but
/// the two arms are one rule and a test that could only reach half of it would
/// stop being the guard it was written as.
#[cfg(test)]
fn reason_for(outcome: &Result<Published, sparkplug_b::TopicError>) -> Option<DropReason> {
    match outcome {
        Ok(published) => reason_for_outcome(published),
        Err(_) => Some(DropReason::Unpublishable),
    }
}

/// The mapping itself, over what the publisher decided.
///
/// Split out when `publish` began answering with a [`Pending`] ([#92], ADR 0046):
/// the error arm belongs to the call, the rest belongs to the outcome, and
/// `deliver` needs the second half alone.
const fn reason_for_outcome(published: &Published) -> Option<DropReason> {
    match published {
        Published::Emitted => None,
        Published::DroppedBeforeBirth => Some(DropReason::BeforeBirth),
        Published::DroppedUndeclaredDevice { .. } => Some(DropReason::UndeclaredDevice),
    }
}

/// Queues one message. A full queue is a traced drop, never a block: a blocked
/// driver stops draining the inbox, and then NOTHING is published.
///
/// Returns whether the message was queued, so a caller that emits a SEQUENCE can
/// tell whether the sequence is intact. `announce` is the one that needs it: a
/// birth whose DBIRTH was dropped must not be reported as complete.
fn publish(client: &AsyncClient, message: Outbound) -> bool {
    let (qos, retain) = qos_for(message.message);
    match client.try_publish(message.topic.clone(), qos, retain, message.payload) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(
                topic = %message.topic,
                %error,
                "outbound queue full; message dropped (never silently)"
            );
            false
        }
    }
}

/// Publishes one judged reading, and says what became of it — `None` when the
/// host has it, the drop reason and any fault when it does not.
///
/// # Why this is a function, and it is [#95]
///
/// It has two ways of failing and they were pinned unequally. The publisher's
/// refusals — a DATA before the BIRTH, an undeclared device, an unencodable
/// payload — are reached from `chaos_broker_recovery`. The TRANSPORT's refusal
/// was reached by nothing: deleting the `transport-queue-full` count left the
/// whole suite green, measured 2026-08-19.
///
/// The issue judged that hole hard to close, because making a real outage fill
/// `rumqttc`'s request channel is timing this harness does not control, and
/// pinning the timing would pin the harness. **That is true of a chaos test and
/// false of this one**: a client built with a request channel of capacity 1 whose
/// `EventLoop` is never polled refuses the second message every time, with no
/// timing involved at all. The dispatch was inside a `select!` arm, where nothing
/// could call it; as a function it is called directly.
///
/// Everything it does, the branch did in this order, and the order is the point:
/// the queue is drained ONLY when the publisher emitted, because a reading the
/// publisher refused has nothing to drain and must not be reported against the
/// transport.
///
/// COUNTED ONCE PER READING, not per message: one reading is one DDATA today,
/// and should that ever become two, a reading half-published is still ONE reading
/// the host did not receive.
fn deliver(
    client: &AsyncClient,
    publisher: &mut SparkplugPublisher,
    update: &MeterUpdate,
) -> Option<Loss> {
    // REPORT BY EXCEPTION ([#32], ADR 0047), and it is the FIRST thing asked. A
    // reading identical in every published respect to the last one the host
    // CONFIRMED carries nothing it does not already hold: the session is stateful,
    // and a consumer that needs to relearn issues a Rebirth Request, which this
    // driver answers. Any change publishes — quality and cause included.
    //
    // Answering `None` says "nothing to count", which is the truth: a message the
    // bridge chose not to send is a decision, not a loss. It lives here rather
    // than in the `select!` arm so a test can reach it — the arm's answer would be
    // `continue`, which is control flow and not a value, and the whole suite
    // stayed green with the suppression deleted (measured 2026-08-24).
    if publisher.is_unchanged(update) {
        return None;
    }
    let mut queue = Queue::default();
    let pending = match publisher.publish(update, &mut queue) {
        Ok(pending) => pending,
        Err(error) => {
            return Some(Loss {
                reason: DropReason::Unpublishable,
                fault: Some(error.to_string()),
                republication: false,
            });
        }
    };
    // The publisher's own refusals never queued anything: answer them without
    // touching the transport, and `confirmed` records nothing for them.
    // ASKED BEFORE THE ANSWER, because confirming would make this reading the
    // last one and every reading its own repeat ([#92]).
    let republication = publisher.is_republication(update);
    if let Some(reason) = reason_for_outcome(pending.outcome()) {
        publisher.confirmed(pending);
        return Some(Loss {
            reason,
            fault: None,
            republication,
        });
    }
    // Queued. THE TRANSPORT NOW DECIDES WHETHER THE PUBLISHER'S STATE MOVES
    // ([#92], ADR 0046): a reading it refused reached no host, so the sequence
    // number goes back and `declared` keeps what it had.
    if publish_all(client, &mut queue).is_empty() {
        publisher.confirmed(pending);
        None
    } else {
        publisher.refused(pending);
        Some(Loss {
            reason: DropReason::TransportQueueFull,
            fault: None,
            republication,
        })
    }
}

/// Counts one loss against the meter, on the axis the loss says it belongs to.
///
/// # Why this is a function and not two lines in the `select!` arm
///
/// It is the whole of [#92]'s third half at the point where it can go wrong, and
/// inside the arm nothing could reach it — the same gap [#95] was filed for. A
/// `deliver` that classifies perfectly and a caller that always calls `dropped`
/// produce exactly the defect being repaired, with every other test green.
fn count_loss(
    pulse: &crate::app::poll_publish::Heartbeats,
    meter: &crate::domain::MeterId,
    loss: &Loss,
) {
    if loss.republication {
        pulse.dropped_republication(meter, loss.reason);
    } else {
        pulse.dropped(meter, loss.reason);
    }
}

/// One reading that never reached the host, and what is known about it.
///
/// `republication` is the axis [#92] added: the reason says WHY the bridge could
/// not hand the message over, and this says WHAT was lost — a distinct
/// measurement, or another copy of one already counted. Under a source failure
/// and a broker outage at once, the same stale value is refused once per period,
/// and a total that could not tell them apart said N where the historian was
/// missing one.
#[derive(Debug, PartialEq, Eq)]
struct Loss {
    reason: DropReason,
    fault: Option<String>,
    republication: bool,
}

/// Which device declarations a partly-published BIRTH sequence invalidates.
#[derive(Debug, PartialEq, Eq)]
enum Withdrawal {
    /// The node certificate itself did not go out, so nothing under it stands.
    EveryDevice,
    /// These devices' certificates did not go out; the rest were declared.
    These(Vec<Serial>),
}

/// Reads a refused BIRTH sequence and says what the host was NOT told ([#88]).
///
/// # Why this is a function and not two lines inside `announce`
///
/// `announce` needs a broker socket to reach, so a test of it needs a client
/// whose request channel is full — which pins how MANY messages are refused but
/// not WHICH, because a bounded queue fills in order and the node certificate is
/// always first. The rule that matters most here is the one that case cannot
/// reach: *a lost NBIRTH takes every device with it, including the ones whose own
/// certificate went out fine*. As a function it is decided on a list, so a test
/// hands it the list a transport could not be made to produce.
fn withdrawal_for(refused: &[Outbound]) -> Withdrawal {
    // A DBIRTH means nothing under a session the host never opened: whatever got
    // through is discarded on arrival, so the devices that succeeded are in
    // exactly the same position as the ones that did not.
    if refused
        .iter()
        .any(|message| message.message == MessageType::NBirth)
    {
        return Withdrawal::EveryDevice;
    }
    Withdrawal::These(refused.iter().filter_map(|m| m.device.clone()).collect())
}

/// Drains a queue to the client and returns the messages that were DROPPED.
///
/// Every message is attempted: stopping at the first failure would leave the rest
/// in the sink with nothing said about them, and what comes back is what lets the
/// caller distinguish a complete sequence from a partial one.
///
/// **It returns the messages and not a count** since [#88]. A caller that only
/// needs to know *whether* the sequence was complete asks `is_empty()`; a caller
/// that has to undo a refused declaration needs to know WHICH device it was, and
/// deriving that from a count is not possible at all.
fn publish_all(client: &AsyncClient, queue: &mut Queue) -> Vec<Outbound> {
    let mut dropped = Vec::new();
    for message in queue.pending.drain(..) {
        // `publish` consumes the message, so the copy for the caller is taken
        // first. It costs one clone per REFUSED message, on a path that is
        // already the abnormal one.
        let refused = message.clone();
        if !publish(client, message) {
            dropped.push(refused);
        }
    }
    dropped
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Story 4.17 — the delivery table, per clause, replacing a test that pinned
    /// a violation.
    ///
    /// **The test this replaces asserted the bug.**
    /// `every_edge_node_message_is_qos_zero_and_never_retained` required QoS 0
    /// for `MessageType::NDeath`, which is what registers the will — and
    /// `tck-id-message-flow-edge-node-birth-publish-will-message-qos`
    /// (`Sparkplug_5:183`) says *"The Edge Node's MQTT Will Message's MQTT QoS
    /// MUST be 1"*. It could not be repaired in place: any fix turned it red, so
    /// it would have had to be edited by whoever fixed the violation, which is
    /// the shape where a test stops being evidence ([#26]).
    ///
    /// Split in two on purpose. The mandated rows cite their clause and may not
    /// be changed without one; the chosen rows are ours and say so. A single
    /// table would let a future edit move a MUST while looking like a preference.
    ///
    /// **AND THAT IS EXACTLY WHAT THIS TEST DID, on the day it was written.**
    /// NDATA and DDATA were filed under CHOSEN on the claim that the norm was
    /// silent about them. It is not: `tck-id-payloads-ndata-qos`
    /// (`Sparkplug_6:1314`) and `tck-id-payloads-ddata-qos` (`:1371`) both read
    /// *"MUST be published with the MQTT QoS set to 0"*, and their `-retain`
    /// siblings mandate `false`. Corrected 2026-08-11 by this story's review;
    /// five of the seven rows are mandated, and only DDEATH is genuinely ours.
    /// The split was the right idea and it still failed, because the split only
    /// helps if the reading behind it was done — which is the whole content of
    /// `CLAUDE.md`'s first rule.
    ///
    /// `MessageType::NCmd` is ABSENT ON PURPOSE — do not "complete" this list.
    /// NCMD is inbound: the bridge subscribes to it and never publishes one.
    /// Asserting a publish rule about a message we do not send would PASS for a
    /// reason unrelated to the property it names — the exact shape of the four
    /// Epic 1 tests this project had to throw away. The QoS that does govern
    /// NCMD is the SUBSCRIBE QoS, mandated as 1 by
    /// `tck-id-message-flow-edge-node-ncmd-subscribe`, and it lives in
    /// `subscribe_to_commands`.
    ///
    /// FALSIFIED 2026-08-10, three mutations, each red with its own message:
    /// `NDeath -> AtMostOnce` (the violation restored) fails the mandated table;
    /// `NBirth -> AtLeastOnce` fails it too; `retain = true` anywhere fails the
    /// retain assertion. Run before the fix was trusted, not after.
    #[test]
    fn the_delivery_table_matches_the_specification_clause_by_clause() {
        // MANDATED. Each row names the clause that fixes it; changing a row here
        // means the norm changed, and the norm is pinned at
        // `docs/spec/sparkplug-b-3.0.0/`.
        let mandated = [
            // `tck-id-payloads-nbirth-qos` (`Sparkplug_6:1087`), and
            // `tck-id-message-flow-edge-node-birth-publish-nbirth-qos` (5:228)
            (MessageType::NBirth, QoS::AtMostOnce),
            // `tck-id-payloads-dbirth-qos` (`Sparkplug_6:1188`), and
            // `tck-id-message-flow-device-birth-publish-dbirth-qos` (5:425)
            (MessageType::DBirth, QoS::AtMostOnce),
            // `tck-id-payloads-ndata-qos` (`Sparkplug_6:1314`) — *"NDATA messages
            // MUST be published with the MQTT QoS set to 0"*. ADDED 2026-08-11:
            // this row sat under CHOSEN, on the claim that the norm was silent.
            (MessageType::NData, QoS::AtMostOnce),
            // `tck-id-payloads-ddata-qos` (`Sparkplug_6:1371`) — same wording,
            // same correction.
            (MessageType::DData, QoS::AtMostOnce),
            // `tck-id-message-flow-edge-node-birth-publish-will-message-qos`
            // (5:183) and `tck-id-payloads-ndeath-will-message-qos` (6:1513) —
            // this is the will, registered from `MessageType::NDeath`.
            (MessageType::NDeath, QoS::AtLeastOnce),
        ];
        for (message, qos) in mandated {
            assert_eq!(
                qos_for(message).0,
                qos,
                "{message:?} is MANDATED at {qos:?} by the specification, not by us"
            );
        }

        // CHOSEN — and after 2026-08-11 this list holds exactly ONE message.
        // DDEATH has no QoS clause anywhere in the norm; every chapter-6 NDEATH
        // clause governs the WILL, not a death published on the way out, and the
        // explicit NDEATH is covered above by our own choice to send it at 1.
        assert_eq!(
            qos_for(MessageType::DDeath).0,
            QoS::AtMostOnce,
            "DDeath is unconstrained by the norm and we chose QoS 0"
        );

        // `retain = false`. MANDATED for five: the will
        // (`tck-id-message-flow-edge-node-birth-publish-will-message-will-retained`
        // 5:185, `tck-id-payloads-ndeath-will-message-retain` 6:1515), both
        // births (`-nbirth-retain` 6:1089, `-dbirth-retain` 6:1190), and both
        // data messages (`-ndata-retain` 6:1316, `-ddata-retain` 6:1373).
        for message in [
            MessageType::NBirth,
            MessageType::DBirth,
            MessageType::NData,
            MessageType::DData,
            MessageType::NDeath,
        ] {
            assert!(
                !qos_for(message).1,
                "{message:?} is MANDATED retain=false by the specification, not by us"
            );
        }
        // CHOSEN for DDEATH, for the reason the mandate exists: a retained
        // payload is a value the broker replays with no way to judge its age.
        assert!(
            !qos_for(MessageType::DDeath).1,
            "DDeath is unconstrained and we chose retain=false"
        );
    }

    /// AC1, observed on the WILL ITSELF rather than on the function behind it.
    ///
    /// **ADDED 2026-08-11 by the story's review.** `the_delivery_table_…` asserts
    /// on `qos_for`, and nothing anywhere looked at the `LastWill` the driver
    /// builds from it — which is why the conformance matrix grades the QoS clause
    /// `conformant` and its retain sibling `gap (unproven)` on evidence of
    /// identical strength. This reads the will back out of the `MqttOptions` the
    /// broker would receive, so the clause and its sibling are proven by the same
    /// artefact and at the same standard.
    ///
    /// FALSIFIED 2026-08-11, three mutations, each red on its own message:
    /// `qos_for(MessageType::NDeath)` → `NData` in `register_will` (the QoS drops
    /// to 0 — the [#26] violation restored, and this time a test says so);
    /// `retain` hard-coded to `true`; and the topic and payload swapped, which no
    /// QoS assertion would ever notice.
    #[test]
    fn the_registered_will_carries_the_qos_and_retain_the_norm_mandates() {
        let mut options = MqttOptions::new("client", "localhost", 1883);
        assert!(
            options.last_will().is_none(),
            "the fixture must start with no will, or this test could pass on a \
             default rather than on what `register_will` did"
        );

        register_will(&mut options, "spBv1.0/g/NDEATH/node", b"death-certificate");

        let will = options.last_will().expect("a will must be registered");
        assert_eq!(
            will.qos,
            QoS::AtLeastOnce,
            "tck-id-message-flow-edge-node-birth-publish-will-message-qos \
             (Sparkplug_5:183) and tck-id-payloads-ndeath-will-message-qos \
             (Sparkplug_6:1513): the Will QoS MUST be 1. At QoS 0 the broker owes \
             no acknowledgement, no retry and no queueing, so the one message \
             whose loss shows a frozen value as live may simply be dropped."
        );
        assert!(
            !will.retain,
            "tck-id-message-flow-edge-node-birth-publish-will-message-will-retained \
             (Sparkplug_5:185) and tck-id-payloads-ndeath-will-message-retain \
             (Sparkplug_6:1515): the Will Retained flag MUST be false. A retained \
             death is replayed to every future subscriber, which marks a live node \
             dead on a payload the broker stored."
        );
        assert_eq!(will.topic, "spBv1.0/g/NDEATH/node");
        assert_eq!(&*will.message, b"death-certificate");
    }

    /// Story 4.6 / AC2 — the broker's answer is read, and the three shapes that
    /// are NOT a clean grant are each distinguishable.
    ///
    /// This is the byte Story 4.4's observer threw away: a refusal is return
    /// code `0x80`, which is neither an error nor a disconnect, so a discarded
    /// answer reads exactly like a topic nobody publishes on. A downgrade to
    /// QoS 0 is just as silent, and it matters because the clause is a MUST on
    /// QoS 1.
    ///
    /// Falsified 2026-07-29: collapsing `granted` to `Granted::AsRequired` for
    /// every input turns three of these four assertions red.
    #[test]
    fn the_brokers_answer_to_the_subscription_is_read_not_assumed() {
        assert_eq!(
            granted(&[SubscribeReasonCode::Success(QoS::AtLeastOnce)]),
            Granted::AsRequired,
            "QoS 1 is what tck-id-message-flow-edge-node-ncmd-subscribe requires"
        );
        assert_eq!(
            granted(&[SubscribeReasonCode::Failure]),
            Granted::Refused,
            "return code 0x80 is a REFUSAL; reporting it as a grant is the \
             false-negative this assertion exists for"
        );
        assert_eq!(
            granted(&[SubscribeReasonCode::Success(QoS::AtMostOnce)]),
            Granted::NotAsRequired(QoS::AtMostOnce),
            "a silent downgrade to QoS 0 leaves the bridge non-conformant and \
             only this byte says so"
        );
        assert_eq!(
            granted(&[]),
            Granted::Empty,
            "a SubAck with no return code is not a grant"
        );
        assert_eq!(
            granted(&[
                SubscribeReasonCode::Success(QoS::AtLeastOnce),
                SubscribeReasonCode::Failure,
            ]),
            Granted::Unattributable(2),
            "this driver issues ONE topic filter per connect, so two return codes \
             mean the answer is not ours alone; reading the first one would report \
             a refused STATE subscription (Story 4.5) as a refused NCMD one"
        );
    }

    /// Runs `body` with every trace captured, and returns what was written.
    ///
    /// THROUGH THE SHARED HARNESS since [#94]: a capture built by hand here is one
    /// a thread with no subscriber can switch off for the whole process.
    fn captured(body: impl FnOnce()) -> String {
        crate::test_capture::capture(body)
    }

    /// Story 4.6 / AC2 — the SEVERITY and the CONTENT of the answer, not just
    /// its classification.
    ///
    /// Added by the Story 4.6 review, which found that
    /// `the_brokers_answer_to_the_subscription_is_read_not_assumed` proves only
    /// that [`granted`] classifies correctly. Swapping the bodies of two arms of
    /// [`trace_subscription_outcome`] — so a refusal announces itself as a grant
    /// — left the whole suite green. AC2 is about what an operator can see, so
    /// the level and the words are the property under test.
    ///
    /// Falsified 2026-07-29: swapping the `Refused` and `AsRequired` arm bodies
    /// turns the first two assertions red; downgrading the `Refused` arm from
    /// `error!` to `info!` turns its level assertion red on its own.
    #[test]
    fn a_refused_subscription_announces_itself_as_a_refusal_and_at_error() {
        let log = captured(|| {
            trace_subscription_outcome("spBv1.0/G/NCMD/N", Granted::Refused);
        });
        assert!(
            log.contains("ERROR"),
            "a refusal the operator must act on cannot be below ERROR; got: {log}"
        );
        assert!(
            log.contains("REFUSED"),
            "the line must say the subscription was refused; got: {log}"
        );
        assert!(
            log.contains("spBv1.0/G/NCMD/N"),
            "AC2 requires the topic to be named; got: {log}"
        );
    }

    /// Story 4.6 / AC2 — a silent downgrade is reported, at WARN, with the value.
    ///
    /// Falsified 2026-07-29: swapping this arm's body with the `AsRequired` one
    /// turns all three assertions red.
    #[test]
    fn a_downgraded_subscription_names_the_granted_qos_and_at_warn() {
        let log = captured(|| {
            trace_subscription_outcome("spBv1.0/G/NCMD/N", Granted::NotAsRequired(QoS::AtMostOnce));
        });
        assert!(
            log.contains("WARN"),
            "AC2 puts a downgrade at WARN; got: {log}"
        );
        assert!(
            log.contains("AtMostOnce"),
            "AC2 requires the GRANTED value to be named, not just that it was \
             wrong; got: {log}"
        );
        assert!(
            !log.contains("granted the command subscription at QoS 1"),
            "a downgrade must not read as a clean grant; got: {log}"
        );
    }

    /// Story 4.6 / AC2 — a clean grant is INFO and says so.
    ///
    /// Present so that the three bad shapes cannot be made to pass by making
    /// EVERY outcome shout: if the happy path were also ERROR, an operator
    /// grepping for trouble would find it every session.
    ///
    /// Falsified 2026-07-29: raising this arm to `error!` turns the level
    /// assertion red.
    #[test]
    fn a_clean_grant_is_reported_at_info_and_not_as_a_problem() {
        let log = captured(|| {
            trace_subscription_outcome("spBv1.0/G/NCMD/N", Granted::AsRequired);
        });
        assert!(log.contains("INFO"), "a grant is not a problem; got: {log}");
        assert!(
            !log.contains("ERROR") && !log.contains("WARN"),
            "the happy path must not be indistinguishable from a failure; got: {log}"
        );
    }

    /// Story 4.6 / AC3 — the three ignore shapes are DISTINGUISHABLE in the log.
    ///
    /// Added by the Story 4.6 review. The chaos test asserted the metric NAME
    /// (`Node Control/Rebirth`) rather than the ignore trace, so a future handler
    /// that ACTED on the command while logging its name would have kept that
    /// assertion green.
    ///
    /// Falsified 2026-07-29: giving all three arms of `trace_command_outcome` the
    /// same message turns the distinctness assertions red; dropping the `?names`
    /// field turns the name assertion red.
    #[test]
    fn the_three_ignore_shapes_do_not_read_alike() {
        let unrecognised = captured(|| {
            trace_command_outcome(
                "t",
                &Inbound::Unrecognised {
                    names: vec!["Node Control/Rebirth".to_string()],
                    total: 1,
                },
            );
        });
        let undecodable = captured(|| {
            trace_command_outcome("t", &Inbound::Undecodable("bad varint".to_string()));
        });
        let empty = captured(|| trace_command_outcome("t", &Inbound::NoMetrics));

        for log in [&unrecognised, &undecodable, &empty] {
            assert!(
                log.contains("ignored"),
                "every one of these paths must say the command was ignored; got: {log}"
            );
        }
        assert!(
            unrecognised.contains("Node Control/Rebirth"),
            "AC3 requires the metric names to be traced; got: {unrecognised}"
        );
        assert!(
            undecodable.contains("WARN") && undecodable.contains("not a Sparkplug payload"),
            "an undecodable payload is a WARN and says why; got: {undecodable}"
        );
        assert!(
            empty.contains("no metric"),
            "a decoded-but-empty payload is not silently dropped; got: {empty}"
        );
        // The three must not collapse into one another.
        assert_ne!(
            unrecognised.split_whitespace().collect::<Vec<_>>(),
            undecodable.split_whitespace().collect::<Vec<_>>()
        );
        assert_ne!(
            unrecognised.split_whitespace().collect::<Vec<_>>(),
            empty.split_whitespace().collect::<Vec<_>>()
        );
    }

    fn command_payload(names: &[&str]) -> Vec<u8> {
        let metrics = names
            .iter()
            .map(|name| sparkplug_b::protobuf::payload::Metric {
                name: Some((*name).to_string()),
                ..Default::default()
            })
            .collect();
        sparkplug_b::encode(&sparkplug_b::protobuf::Payload {
            timestamp: Some(1_700_000_000_000),
            metrics,
            seq: None,
            uuid: None,
            body: None,
        })
    }

    /// Story 4.6 / AC3 — a payload that does not decode is an ORDINARY input.
    ///
    /// It arrives from the network, so anyone can send one; `.expect()` here
    /// would let a stranger stop the bridge by publishing eleven bytes.
    ///
    /// Falsified 2026-07-29: replacing the `Err` arm of `classify` with
    /// `.expect("a Sparkplug payload")` turns this red — the test panics instead
    /// of returning a verdict, which is precisely the production behaviour it
    /// forbids.
    #[test]
    fn a_command_that_does_not_decode_is_classified_never_unwrapped() {
        // A varint whose continuation bit never clears: not a Sparkplug payload
        // by any reading.
        let garbage = [0xffu8; 11];
        assert!(
            matches!(classify(&garbage, false), Inbound::Undecodable(_)),
            "malformed bytes must produce a verdict, not a panic"
        );
    }

    /// An NCMD payload built from `(name, datatype, value)` triples, exactly as
    /// a Host Application would encode it.
    ///
    /// The value is a parameter and NOT defaulted, which is the whole point.
    /// `command_payload` above builds metrics with `..Default::default()`, so
    /// `value: None` — and under `-ncmd-rebirth-value` a valueless
    /// `Node Control/Rebirth` is not a request. A test that reached for the
    /// convenient helper would assert the strict matcher's behaviour on a
    /// payload no conformant host ever sends, and would stay green against an
    /// implementation that answers nothing at all.
    fn command_payload_valued(
        metrics: &[(
            &str,
            Option<u32>,
            Option<sparkplug_b::protobuf::payload::metric::Value>,
        )],
    ) -> Vec<u8> {
        let metrics = metrics
            .iter()
            .map(
                |(name, datatype, value)| sparkplug_b::protobuf::payload::Metric {
                    name: Some((*name).to_string()),
                    datatype: *datatype,
                    is_null: value.is_none().then_some(true),
                    value: value.clone(),
                    ..Default::default()
                },
            )
            .collect();
        sparkplug_b::encode(&sparkplug_b::protobuf::Payload {
            timestamp: Some(1_700_000_000_000),
            metrics,
            seq: None,
            uuid: None,
            body: None,
        })
    }

    /// The conformant Rebirth Request, as `-ncmd-rebirth-name` and
    /// `-ncmd-rebirth-value` define it.
    fn a_real_rebirth_request() -> Vec<u8> {
        command_payload_valued(&[(
            METRIC_NODE_CONTROL_REBIRTH,
            Some(sparkplug_b::DataType::Boolean.code()),
            Some(sparkplug_b::protobuf::payload::metric::Value::BooleanValue(
                true,
            )),
        )])
    }

    /// Story 4.7 / AC6 — a Rebirth Request is the name AND the value, and the
    /// four near misses are each recognised as near misses rather than as
    /// requests or as ordinary unknown commands.
    ///
    /// `tck-id-operational-behavior-data-commands-ncmd-rebirth-value`
    /// (`Sparkplug_5_Operational_Behavior.adoc:974-975`) is explicit: *"A
    /// Rebirth Request MUST include a metric value of true."* This bridge
    /// implements the norm's reading rather than a liberal one; the residual
    /// risk of a host that encodes it differently is carried by the near-miss
    /// trace, which `the_near_miss_records_the_datatype_and_value_as_received`
    /// asserts separately, and by the Story 4.8 pre-production run.
    ///
    /// Falsified 2026-07-30: dropping the value check from `classify` — matching
    /// on the name alone — turns the `false`, valueless and `IntValue` cases red
    /// while leaving the `true` case green. That asymmetry is the point: a test
    /// that only checked the `true` case would pass against a matcher with no
    /// value check at all.
    #[test]
    fn a_rebirth_request_is_the_name_and_the_value_never_the_name_alone() {
        use sparkplug_b::protobuf::payload::metric::Value;
        let boolean = Some(sparkplug_b::DataType::Boolean.code());

        assert_eq!(
            classify(&a_real_rebirth_request(), false),
            Inbound::Rebirth {
                ignored_alongside: vec![]
            },
            "name + boolean true is exactly what -ncmd-rebirth-value defines"
        );

        // `false` is the value our OWN NBIRTH declares. A host that echoes the
        // birth back at us is not asking for anything, and answering it would
        // make every declaration a self-inflicted rebirth.
        assert_eq!(
            classify(
                &command_payload_valued(&[(
                    METRIC_NODE_CONTROL_REBIRTH,
                    boolean,
                    Some(Value::BooleanValue(false)),
                )]),
                false
            ),
            Inbound::RebirthNearMiss {
                reason: NearMiss::ValueNotTrue,
                received: vec![RebirthAsReceived {
                    name: METRIC_NODE_CONTROL_REBIRTH.to_string(),
                    datatype: boolean,
                    value: "BooleanValue(false)".to_string(),
                }],
                total: 1,
            },
            "boolean false is not a request"
        );

        // No value at all — the shape `..Default::default()` produces, and the
        // shape that made the Story 4.6 chaos assertion undiscriminating.
        assert_eq!(
            classify(
                &command_payload_valued(&[(METRIC_NODE_CONTROL_REBIRTH, boolean, None,)]),
                false
            ),
            Inbound::RebirthNearMiss {
                reason: NearMiss::ValueNotTrue,
                received: vec![RebirthAsReceived {
                    name: METRIC_NODE_CONTROL_REBIRTH.to_string(),
                    datatype: boolean,
                    value: "<no value>".to_string(),
                }],
                total: 1,
            },
            "a metric with no value declares nothing to act on"
        );

        // Non-boolean: the encoding a host might plausibly use, and therefore
        // the near miss most worth having in the log.
        assert_eq!(
            classify(
                &command_payload_valued(&[(
                    METRIC_NODE_CONTROL_REBIRTH,
                    Some(sparkplug_b::DataType::Int32.code()),
                    Some(Value::IntValue(1)),
                )]),
                false
            ),
            Inbound::RebirthNearMiss {
                reason: NearMiss::ValueNotTrue,
                received: vec![RebirthAsReceived {
                    name: METRIC_NODE_CONTROL_REBIRTH.to_string(),
                    datatype: Some(sparkplug_b::DataType::Int32.code()),
                    value: "IntValue(1)".to_string(),
                }],
                total: 1,
            },
            "IntValue(1) is not BooleanValue(true), and the trace must say so \
             rather than the matcher quietly declining"
        );
    }

    /// Story 4.7 code review / ADR 0017 — a RETAINED NCMD is never answered.
    ///
    /// `tck-id-payloads-ncmd-retain` (`Sparkplug_6_Payloads.adoc:1421`): *"NCMD
    /// messages MUST be published with the MQTT retain flag set to false."* So a
    /// retained NCMD is not something a conformant Host Application sent, and it
    /// is not a request anyone is making NOW — the broker replays it to every
    /// subscriber, so honouring one would make the bridge re-announce itself on
    /// every connect for as long as the retained message exists, with nobody
    /// present and nothing in the log to distinguish it from a real host asking.
    ///
    /// The payload is byte-identical to `a_real_rebirth_request` on purpose: the
    /// ONLY difference between the answered case above and this one is the
    /// transport flag, which is exactly the property under test.
    ///
    /// Falsified 2026-07-30: dropping the `retained` argument from `classify`'s
    /// `true`-value branch — going back to what shipped — turns this red and
    /// leaves every other classification test green, because no other test
    /// varies the flag.
    #[test]
    fn a_retained_ncmd_is_a_replay_and_never_a_rebirth_request() {
        let conformant_but_retained = classify(&a_real_rebirth_request(), true);
        assert_eq!(
            conformant_but_retained,
            Inbound::RebirthNearMiss {
                reason: NearMiss::Retained,
                received: vec![RebirthAsReceived {
                    name: METRIC_NODE_CONTROL_REBIRTH.to_string(),
                    datatype: Some(sparkplug_b::DataType::Boolean.code()),
                    value: "BooleanValue(true)".to_string(),
                }],
                total: 1,
            },
            "a retained NCMD must be reported as a replay, not answered"
        );
        // And the same bytes, unretained, ARE a request — otherwise this test
        // would pass against a matcher that answers nothing at all.
        assert!(matches!(
            classify(&a_real_rebirth_request(), false),
            Inbound::Rebirth { .. }
        ));
    }

    /// Story 4.7 code review / AC6 — the detection net is WIDER than the action.
    ///
    /// The action requires the exact name; detection must also catch the ways a
    /// real host misses it, or the strict matcher's one failure mode — never
    /// firing, silently — is exactly what the detector cannot see. Three shapes:
    ///
    /// - **Case**, for a host that normalises tag names.
    /// - **Trailing whitespace**, for a host that concatenates.
    /// - **`Node Control/Refresh`**, because the specification contradicts
    ///   itself: `Sparkplug_5_Operational_Behavior.adoc:950` says a host *"can
    ///   send a 'Rebirth Request' using the 'Node Control/Refresh' metric"* while
    ///   `-rebirth-name` (`:956`) and `-ncmd-rebirth-name` (`:973`) both say
    ///   `Rebirth`. A host built from the prose sends `Refresh`.
    ///
    /// None of them is ANSWERED — the tck-ids govern — and every one of them is
    /// now in the log with its name, datatype and value.
    ///
    /// Falsified 2026-07-30: removing the `nearly` branch from `classify` turns
    /// all three red, and each lands in `Unrecognised` — which is precisely the
    /// low-signal path this test exists to keep them out of.
    #[test]
    fn a_name_that_only_nearly_matches_is_a_near_miss_not_an_unknown_command() {
        use sparkplug_b::protobuf::payload::metric::Value;
        let boolean = Some(sparkplug_b::DataType::Boolean.code());

        for name in [
            "node control/rebirth",
            "NODE CONTROL/REBIRTH",
            "Node Control/Rebirth ",
            "Node Control/Refresh",
        ] {
            let classified = classify(
                &command_payload_valued(&[(name, boolean, Some(Value::BooleanValue(true)))]),
                false,
            );
            assert_eq!(
                classified,
                Inbound::RebirthNearMiss {
                    reason: NearMiss::NameOnlyNearly,
                    received: vec![RebirthAsReceived {
                        name: name.to_string(),
                        datatype: boolean,
                        value: "BooleanValue(true)".to_string(),
                    }],
                    total: 1,
                },
                "{name:?} must be DETECTED as a near miss — it is not answered, \
                 but a request that missed by a spelling is the one a strict \
                 matcher hides"
            );
        }

        // And the exact name is still not a near miss, or the two arms would be
        // ambiguous and the answered case would stop being reachable.
        assert!(!nearly_rebirth(METRIC_NODE_CONTROL_REBIRTH));
    }

    /// Story 4.7 code review — one NCMD cannot render an unbounded log line.
    ///
    /// The cap that shipped bounded one VALUE at 200 characters, under a comment
    /// claiming *"a hostile payload cannot fill a disk one command at a time"*.
    /// It bounded the wrong axis: the number of metrics was unbounded, so a ~10 KB
    /// NCMD carrying thousands of them rendered ~13× its own size into the log —
    /// at INFO, which `main.rs` makes visible by default, written synchronously
    /// on the task that also publishes DATA.
    ///
    /// Falsified 2026-07-30: removing `.take(MAX_TRACED_METRICS)` from either
    /// `as_received` or the `Unrecognised` arm turns the matching assertion red.
    #[test]
    fn a_hostile_metric_count_is_capped_and_the_line_says_it_capped() {
        use sparkplug_b::protobuf::payload::metric::Value;
        let boolean = Some(sparkplug_b::DataType::Boolean.code());
        let many = MAX_TRACED_METRICS * 5;

        // Many near misses: capped, with the true total still reported.
        let near: Vec<_> = (0..many)
            .map(|_| {
                (
                    METRIC_NODE_CONTROL_REBIRTH,
                    boolean,
                    Some(Value::BooleanValue(false)),
                )
            })
            .collect();
        match classify(&command_payload_valued(&near), false) {
            Inbound::RebirthNearMiss {
                received, total, ..
            } => {
                assert_eq!(received.len(), MAX_TRACED_METRICS);
                assert_eq!(total, many, "the TRUE count must survive the cap");
            }
            other => panic!("expected a near miss; got {other:?}"),
        }

        // Many unknown names: same bound, same honesty.
        let unknown: Vec<_> = (0..many).map(|_| "Node Control/Whatever").collect();
        match classify(&command_payload(&unknown), false) {
            Inbound::Unrecognised { names, total } => {
                assert_eq!(names.len(), MAX_TRACED_METRICS);
                assert_eq!(total, many);
            }
            other => panic!("expected an unknown command; got {other:?}"),
        }

        // And one hostile VALUE is still capped, and still says so.
        let huge = "x".repeat(MAX_TRACED_CHARS * 10);
        let rendered = describe_value(Some(&Value::StringValue(huge)));
        assert!(
            rendered.contains("truncated from"),
            "a shortened rendering must not read like a complete one; got: {rendered}"
        );
        assert!(
            rendered.chars().count() < MAX_TRACED_CHARS * 2,
            "the cap must bound the OUTPUT, not merely annotate it; got {} chars",
            rendered.chars().count()
        );
    }

    /// Story 4.7 / AC6 — an ALIAS-addressed metric is never a Rebirth Request.
    ///
    /// `-rebirth-name-aliases` forbids an NBIRTH from aliasing this metric
    /// precisely so that a host can request a rebirth *"without knowledge of
    /// any potential alias"*. This bridge publishes `alias: None` on
    /// everything, so no host can legitimately hold an alias for our metrics —
    /// and matching one would add a path no conformant host can exercise plus a
    /// way for an unrelated number to trigger a birth.
    ///
    /// Falsified 2026-07-30 (mutation 9): widening `classify`'s filter to
    /// `name == METRIC_NODE_CONTROL_REBIRTH || m.alias.is_some()` turns this
    /// red — and also turns
    /// `a_command_addressed_by_alias_is_not_reported_as_nameless` red, which is
    /// the Story 4.6 assertion that says an alias keeps its identity in the
    /// trace. The two guard the same boundary from opposite sides.
    #[test]
    fn an_alias_addressed_metric_is_never_a_rebirth_request() {
        let by_alias = sparkplug_b::encode(&sparkplug_b::protobuf::Payload {
            timestamp: Some(1_700_000_000_000),
            metrics: vec![sparkplug_b::protobuf::payload::Metric {
                name: None,
                alias: Some(7),
                datatype: Some(sparkplug_b::DataType::Boolean.code()),
                value: Some(sparkplug_b::protobuf::payload::metric::Value::BooleanValue(
                    true,
                )),
                ..Default::default()
            }],
            seq: None,
            uuid: None,
            body: None,
        });
        assert_eq!(
            classify(&by_alias, false),
            Inbound::Unrecognised {
                names: vec!["<alias 7>".to_string()],
                total: 1,
            },
            "a numeric alias this bridge never published must not be able to \
             trigger a birth; it is an unknown command, and not even a near miss"
        );
    }

    /// Story 4.7 / AC6 — a request is still a request in company.
    ///
    /// `-ncmd-rebirth-name` requires the payload to INCLUDE the metric; it does
    /// not require it to be alone. A matcher that demanded a single-metric
    /// payload would decline a legal request, and would decline it silently.
    ///
    /// Falsified 2026-07-30: changing `classify` to require
    /// `decoded.metrics.len() == 1` before returning `Rebirth` turns this red
    /// and leaves every other classification test green.
    ///
    /// Extended by the Story 4.7 code review: the co-travelling metrics used to be
    /// discarded with NOTHING said, in a module whose other three arms all
    /// advertise that nothing is ignored silently. Answering the request is
    /// correct; saying nothing about `Node Control/Next Server` was not.
    /// Falsified 2026-07-30: dropping `ignored_alongside` from the `Rebirth` arm
    /// turns the second assertion red while the first stays green — which is why
    /// it is asserted separately.
    #[test]
    fn a_rebirth_request_carried_alongside_other_metrics_is_still_a_request() {
        use sparkplug_b::protobuf::payload::metric::Value;
        let bytes = command_payload_valued(&[
            ("Node Control/Next Server", None, None),
            (
                METRIC_NODE_CONTROL_REBIRTH,
                Some(sparkplug_b::DataType::Boolean.code()),
                Some(Value::BooleanValue(true)),
            ),
        ]);
        let classified = classify(&bytes, false);
        assert_eq!(
            classified,
            Inbound::Rebirth {
                ignored_alongside: vec!["Node Control/Next Server".to_string()],
            },
            "the request is answered, AND the command that rode with it is named"
        );

        let log = captured(|| trace_command_outcome("t", &classified));
        assert!(
            log.contains("Node Control/Next Server"),
            "a discarded command must appear in the log even when a recognised \
             one travelled with it; got: {log}"
        );
    }

    /// Story 4.7 / AC6 — the near-miss trace carries the datatype and the value
    /// AS RECEIVED, and this is asserted on its own.
    ///
    /// It needs its own test because the classification tests above stay green
    /// when the two fields are dropped from the log line: `classify` would still
    /// return the right variant, and the operator would still be told nothing.
    /// The whole mitigation for the strict matcher is that a request which
    /// missed leaves the exact bytes that missed behind it — a trace saying only
    /// *"a rebirth-named metric was not answered"* discards precisely what makes
    /// the Story 4.8 run self-diagnosing.
    ///
    /// Falsified 2026-07-30: removing the `?received` field from the near-miss
    /// arm of `trace_command_outcome` turns all three field assertions red while
    /// `a_rebirth_request_is_the_name_and_the_value_never_the_name_alone` stays
    /// green.
    ///
    /// # The datatype needle, and why it was worthless
    ///
    /// This asserted `log.contains('3')`. `captured` builds
    /// `tracing_subscriber::fmt()` without `.without_time()`, so every captured
    /// line carries a full RFC-3339 timestamp — and a one-digit needle is
    /// satisfied by the clock. The datatype half of AC6 was therefore never
    /// falsified: the recorded RED came from the `IntValue(1)` assertion alone,
    /// which is why the mutation that removed BOTH fields looked decisive.
    /// Found by the Story 4.7 code review. The needle is now the rendered field,
    /// which nothing else in the line can produce.
    ///
    /// Falsified 2026-07-30, independently of the value: rendering the near-miss
    /// field as values only — `received = ?received.iter().map(|r| &r.value)` —
    /// turns the datatype and name assertions red while the value one stays green.
    /// The failure output is its own evidence for why the old needle was
    /// worthless; the captured line began
    /// `2026-07-30T12:00:33.211428Z WARN …`, which contains three `3`s before the
    /// message even starts.
    ///
    /// (A first attempt at this mutation removed `datatype` inside `as_received`
    /// and left the test GREEN — correctly, because this test builds its `Inbound`
    /// directly and never calls `as_received`. Recorded because a mutation that
    /// misses its target proves nothing about the assertion, and reporting it as a
    /// falsification would have been the very defect this comment is about.)
    #[test]
    fn the_near_miss_records_the_datatype_and_value_as_received() {
        let log = captured(|| {
            trace_command_outcome(
                "spBv1.0/G/NCMD/N",
                &Inbound::RebirthNearMiss {
                    reason: NearMiss::ValueNotTrue,
                    received: vec![RebirthAsReceived {
                        name: METRIC_NODE_CONTROL_REBIRTH.to_string(),
                        datatype: Some(sparkplug_b::DataType::Int32.code()),
                        value: "IntValue(1)".to_string(),
                    }],
                    total: 1,
                },
            );
        });
        assert!(
            log.contains("IntValue(1)"),
            "the VALUE that arrived must be in the log, or a host encoding the \
             request differently is indistinguishable from a host that sent \
             nothing; got: {log}"
        );
        assert!(
            log.contains("datatype: Some(3)"),
            "the DATATYPE that arrived must be in the log as a datatype (Int32 is \
             3) — not merely as a digit the timestamp also supplies; got: {log}"
        );
        assert!(
            log.contains(METRIC_NODE_CONTROL_REBIRTH),
            "and the NAME as received, which is the whole diagnosis when a host \
             misses by a spelling; got: {log}"
        );
        assert!(
            log.contains("spBv1.0/G/NCMD/N"),
            "and the topic, so the operator knows which node; got: {log}"
        );
    }

    /// Story 4.7 / AC6 — the near miss reads as neither an answered rebirth nor
    /// an ordinary unknown command.
    ///
    /// The Story 4.6 review's sharpest finding was that swapping two arms'
    /// BODIES left every test green: the log line is the observable the
    /// criterion is written in. Three outcomes that collapse into one another
    /// in the log are one outcome as far as an operator is concerned.
    ///
    /// Falsified 2026-07-30: giving the `Rebirth`, `RebirthNearMiss` and
    /// `Unrecognised` arms the same message turns the distinctness assertions
    /// red.
    ///
    /// Extended by the Story 4.7 code review: there are now THREE ways to miss,
    /// and they must not read alike either. An operator who cannot tell a `false`
    /// value from a misspelt name from a retained replay has three different
    /// repairs and one log line. Falsified 2026-07-30: collapsing the `clause`
    /// match in `trace_command_outcome` to a single string turns the near-miss
    /// distinctness assertions red while every classification test stays green.
    #[test]
    fn an_answered_a_missed_and_an_unknown_command_do_not_read_alike() {
        let near_miss = |reason: NearMiss, name: &str| {
            let name = name.to_string();
            captured(move || {
                trace_command_outcome(
                    "t",
                    &Inbound::RebirthNearMiss {
                        reason,
                        received: vec![RebirthAsReceived {
                            name: name.clone(),
                            datatype: None,
                            value: "<no value>".to_string(),
                        }],
                        total: 1,
                    },
                )
            })
        };

        let answered = captured(|| {
            trace_command_outcome(
                "t",
                &Inbound::Rebirth {
                    ignored_alongside: vec![],
                },
            )
        });
        let missed = near_miss(NearMiss::ValueNotTrue, METRIC_NODE_CONTROL_REBIRTH);
        let misspelt = near_miss(NearMiss::NameOnlyNearly, "Node Control/Refresh");
        let replayed = near_miss(NearMiss::Retained, METRIC_NODE_CONTROL_REBIRTH);
        let unknown = captured(|| {
            trace_command_outcome(
                "t",
                &Inbound::Unrecognised {
                    names: vec!["Whatever".to_string()],
                    total: 1,
                },
            )
        });

        assert!(
            answered.contains("INFO"),
            "AC2 requires the answer to be visible under the default filter, \
             which starts at INFO; got: {answered}"
        );
        assert!(
            !answered.contains("ignored"),
            "an ANSWERED command must not say it was ignored — that is the \
             sentence the Story 4.6 chaos test greps for; got: {answered}"
        );
        for log in [&missed, &misspelt, &replayed, &unknown] {
            assert!(
                log.contains("ignored"),
                "a command that was not acted on must say so; got: {log}"
            );
        }
        assert!(
            missed.contains("Node Control/Rebirth"),
            "a near miss must name the metric it nearly matched; got: {missed}"
        );
        assert!(
            misspelt.contains("Node Control/Refresh"),
            "a name-only near miss must show the SPELLING that arrived, because \
             that is the whole repair; got: {misspelt}"
        );
        assert!(
            replayed.contains("RETAIN") || replayed.contains("retain"),
            "a replayed command must say it was retained, or the operator looks \
             for a host that is not there; got: {replayed}"
        );

        // Every pair must be distinguishable, including the three near misses
        // from one another.
        let all = [
            ("answered", &answered),
            ("value-not-true", &missed),
            ("name-only", &misspelt),
            ("retained", &replayed),
            ("unknown", &unknown),
        ];
        for (i, (name_a, a)) in all.iter().enumerate() {
            for (name_b, b) in all.iter().skip(i + 1) {
                assert_ne!(
                    a.split_whitespace().collect::<Vec<_>>(),
                    b.split_whitespace().collect::<Vec<_>>(),
                    "{name_a} and {name_b} read alike, and two outcomes that read \
                     alike are one outcome to an operator"
                );
            }
        }
    }

    /// Story 4.6 / AC3 — a metric addressed by ALIAS still identifies itself.
    ///
    /// Added by the Story 4.6 review. Sparkplug lets a host address a metric by
    /// alias instead of by name; this bridge publishes `alias: None` on
    /// everything it sends, but that governs what it emits, not what a host may
    /// send it. Every existing `command_payload` sets a name, so the fallback
    /// branch had no test at all.
    ///
    /// It matters beyond tidiness: an alias-addressed `Node Control/Rebirth`
    /// would have traced `names=["<unnamed>"]` — a name list whose names were
    /// literally lost, which is the confusion the `NoMetrics` arm exists to
    /// prevent — and Story 4.7's name-matching handler would silently never
    /// fire, with a log line that looks like ordinary operation.
    ///
    /// Falsified 2026-07-29: restoring the `unwrap_or_else(|| "<unnamed>")`
    /// fallback turns the first assertion red.
    #[test]
    fn a_command_addressed_by_alias_is_not_reported_as_nameless() {
        let by_alias = sparkplug_b::encode(&sparkplug_b::protobuf::Payload {
            timestamp: Some(1_700_000_000_000),
            metrics: vec![
                sparkplug_b::protobuf::payload::Metric {
                    name: None,
                    alias: Some(7),
                    ..Default::default()
                },
                sparkplug_b::protobuf::payload::Metric {
                    name: None,
                    alias: None,
                    ..Default::default()
                },
            ],
            seq: None,
            uuid: None,
            body: None,
        });
        assert_eq!(
            classify(&by_alias, false),
            Inbound::Unrecognised {
                names: vec![
                    "<alias 7>".to_string(),
                    "<neither name nor alias>".to_string(),
                ],
                total: 2,
            },
            "an alias is an identity; collapsing it to <unnamed> throws away the \
             only thing that says which command arrived"
        );
    }

    /// Story 4.6 / AC3 — a well-formed payload carrying nothing is traced too.
    ///
    /// Without its own arm it would fall into the "unrecognised" branch with an
    /// empty name list, which reads in a log as a command whose names were lost
    /// rather than a command that carried none.
    ///
    /// Falsified 2026-07-29: deleting the `metrics.is_empty()` arm makes this
    /// return `Unrecognised([])` and the test goes red.
    #[test]
    fn a_command_carrying_no_metric_is_not_silently_dropped() {
        assert_eq!(classify(&command_payload(&[]), false), Inbound::NoMetrics);
    }

    /// Story 4.7 code review — a birth that was only PARTLY published is counted,
    /// so it cannot be reported as complete.
    ///
    /// `publish` turns a full request channel into a WARN and continues, so the
    /// NBIRTH could be queued and the DBIRTH dropped. That gap is bounded (see
    /// `announce`) and now self-healing, but the trace called such a sequence
    /// *"complete BIRTH sequence republished"* — a log that says the opposite of
    /// what happened is worse than the gap it hides. `announce` gates that line on
    /// this count being zero, so the count is the property.
    ///
    /// No runtime and no broker: the `EventLoop` is created and never polled, so
    /// nothing drains the request channel and it fills after one message.
    ///
    /// Falsified 2026-07-30: replacing the body of the loop with
    /// `let _ = publish(client, message);` — dropping the count, which is what
    /// shipped — turns this red. Before this test the same mutation left the whole
    /// suite green, which is why `announce`'s misreport survived review-by-reading.
    /// [#32], ADR 0047 — an identical reading is suppressed, and ANY change
    /// publishes.
    ///
    /// # The half that matters is the second one
    ///
    /// Suppressing on the value alone would let a quality transition go
    /// unpublished, and staleness would stop being observable — a frozen value
    /// staying on an operator's screen labelled good is the failure this project
    /// is named for. So the test asserts four transitions, and three of them
    /// change nothing an eye would call "the reading": the quality, the cause, and
    /// the acquisition instant with identical numbers.
    ///
    /// # Why against the CONFIRMED reading
    ///
    /// A reading the transport refused never reached the host, so an identical one
    /// afterwards is the first time the host would hear it. Comparing against the
    /// last *attempted* publication would suppress it and leave the host holding
    /// nothing — which is the state [#92] repaired one issue ago.
    ///
    /// **Falsification, 2026-08-24:**
    ///
    /// 1. `is_unchanged` comparing `measurement.power` only: RED on the quality
    ///    transition — the shape [#32] warned must not be lost;
    /// 2. `is_unchanged` answering `true` whenever a last reading exists: RED on
    ///    every transition;
    /// 3. the suppression deleted from `deliver` — the state before ADR 0047:
    ///    RED, `Some(TransportQueueFull)` where `None` is owed.
    ///
    /// # The third mutation took three attempts to catch, and that is the lesson
    ///
    /// It first lived in the `select!` arm as a `continue`, where no test could
    /// reach it — the same gap [#95] was filed for, one issue earlier. Deleting it
    /// left the WHOLE suite green, chaos tier included, because nothing in this
    /// repository observes a frozen meter. **Measured, not feared.** Moving it
    /// into `deliver` made it a value rather than control flow.
    ///
    /// The second attempt asserted on a queue this test owned, and the mutation
    /// walked straight through: `deliver` builds its own queue and hands it to the
    /// client, so a local one observes nothing whatever happens.
    ///
    /// What works is a request channel with no slot left: a suppressed reading
    /// touches nothing and answers `None`, a published one is refused and comes
    /// back `TransportQueueFull`. The distinction is visible from outside, which
    /// is the only place a test stands.
    #[test]
    fn an_identical_reading_is_suppressed_and_any_change_publishes() {
        use crate::core::oracle::{Cause, Verdict};
        use crate::domain::{Kw, Kwh, Measurement, MeterId, Quality, UtcMillis};

        let serial = Serial::new("30000001");
        let device = DeviceIdentity::new(crate::domain::MeterId::new("cpt01"), serial.clone());
        let meter = MeterId::new("meter");
        let reading = |value_date: i64, power: f64, verdict: Verdict| {
            MeterUpdate::uniform(
                meter.clone(),
                Measurement {
                    meter: meter.clone(),
                    serial: serial.clone(),
                    power: Some(Kw(power)),
                    energy: Some(Kwh(4_843.822)),
                    value_date: UtcMillis(value_date),
                    quality: Quality::Good,
                },
                verdict,
            )
        };

        let node = sparkplug_b::EdgeNode::new("Site", "Bridge").expect("valid ids");
        let options = MqttOptions::new("roomy", "127.0.0.1", 1883);
        let (client, _never_polled) = AsyncClient::new(options, 64);
        let mut publisher = SparkplugPublisher::new(node, None);
        announce(
            &client,
            &mut publisher,
            UtcMillis(1_000),
            std::slice::from_ref(&device),
            BirthReason::Connected,
        );

        // A SECOND publisher whose channel holds its birth and one reading, and
        // nothing more: the suppression is then the only thing standing between a
        // repeat and a refusal.
        let tight_options = MqttOptions::new("tight", "127.0.0.1", 1883);
        let (tight_client, _also_never_polled) = AsyncClient::new(tight_options, 3);
        let mut tight = SparkplugPublisher::new(
            sparkplug_b::EdgeNode::new("Site", "Bridge").expect("valid ids"),
            None,
        );
        announce(
            &tight_client,
            &mut tight,
            UtcMillis(1_000),
            std::slice::from_ref(&device),
            BirthReason::Connected,
        );

        let first = reading(1_784_984_792_050, 0.018, Verdict::good());
        assert_eq!(
            deliver(&tight_client, &mut tight, &first),
            None,
            "the premise: the third slot takes this one"
        );

        assert!(
            !publisher.is_unchanged(&first),
            "nothing has been confirmed for this device, so the host holds nothing \
             and no reading can be redundant"
        );
        assert_eq!(deliver(&client, &mut publisher, &first), None);

        assert!(
            publisher.is_unchanged(&first),
            "the same reading again tells the host what it is already displaying: \
             this is the unplugged meter's 17 000 messages a day"
        );

        // AND `deliver` ACTS ON IT — the half an `is_unchanged` assertion cannot
        // show, and the half that was missing. Measured on 2026-08-24: with the
        // suppression deleted the WHOLE suite stayed green, chaos tier included,
        // because nothing observed a frozen meter.
        //
        // Observed through a FULL request channel, because `deliver` builds its
        // own queue and hands it straight to the client: a suppressed reading
        // touches nothing and answers `None`; a published one meets a channel with
        // no slot left and comes back as `TransportQueueFull`. The two are
        // therefore distinguishable from outside, which a queue this test owns
        // could never be — the first draft asserted on such a queue and the
        // mutation walked straight through it.
        assert_eq!(
            deliver(&tight_client, &mut tight, &first),
            None,
            "the channel has been full since the birth, so a reading that reached \
             it would come back refused. `None` means nothing was sent — and a \
             suppressed reading is not a loss either: counting it would report a \
             fault where the bridge made a decision"
        );

        // THE FOUR TRANSITIONS, and three of them leave the numbers alone.
        assert!(
            !publisher.is_unchanged(&reading(1_784_984_792_050, 0.019, Verdict::good())),
            "a new value publishes"
        );
        assert!(
            !publisher.is_unchanged(&reading(
                1_784_984_792_050,
                0.018,
                Verdict::stale(Cause::ReadingTooOld)
            )),
            "A QUALITY TRANSITION PUBLISHES, with the value untouched. Suppressing \
             on the value alone would leave a frozen reading on screen labelled \
             good, which is the failure this bridge exists to prevent"
        );
        assert!(
            !publisher.is_unchanged(&reading(
                1_784_984_792_050,
                0.018,
                Verdict::stale(Cause::TimestampsDisagree)
            )) && !publisher.is_unchanged(&reading(
                1_784_984_792_050,
                0.018,
                Verdict::stale(Cause::ReadingTooOld)
            )),
            "and so does a CAUSE transition at equal quality: contract v12 \
             publishes the cause as its own metric (ADR 0044), so an operator \
             reads it and it must not go stale in silence"
        );
        assert!(
            !publisher.is_unchanged(&reading(1_784_984_852_050, 0.018, Verdict::good())),
            "a new acquisition instant publishes even with identical numbers: it \
             is what tells the historian the measurement was confirmed again"
        );
    }

    /// [#92] — the classification reaches the COUNTERS, which `deliver` alone
    /// cannot show.
    ///
    /// A `deliver` that classifies perfectly and a caller that always calls
    /// `dropped` produce exactly the defect being repaired, with every other test
    /// green. That is the gap [#95] was filed for, one issue later.
    ///
    /// **Falsification, 2026-08-24:** `count_loss` calling `dropped` for every
    /// loss — the state before this change: RED, `republications` stays 0 where
    /// 1 is owed, while the total is right in both cases and cannot tell them
    /// apart.
    #[test]
    fn a_republication_and_a_reading_move_different_counters() {
        use crate::app::poll_publish::Heartbeats;
        use crate::domain::MeterId;

        let meter = MeterId::new("garage");
        let beats = Heartbeats::for_meters([meter.clone()]);
        let loss = |republication| Loss {
            reason: DropReason::TransportQueueFull,
            fault: None,
            republication,
        };

        count_loss(&beats, &meter, &loss(false));
        count_loss(&beats, &meter, &loss(true));

        let fleet = beats.snapshot();
        let lost = fleet.dropped();
        assert_eq!(lost.len(), 1, "one reason, one row: {lost:?}");
        assert_eq!(
            lost[0].count, 2,
            "BOTH were messages the bridge could not hand over, and the manual \
             defines the total as exactly that: it must not shrink because one \
             was a copy"
        );
        assert_eq!(
            lost[0].republications, 1,
            "and one of them was a copy, so the historian is missing ONE distinct \
             measurement rather than two"
        );
    }

    /// [#92], third half — the same stale value refused twice is ONE distinct
    /// measurement lost, and the counters say both things.
    ///
    /// # The failure this prevents is an operator reading forty for one
    ///
    /// ADR 0027 makes every cycle publish something, so a meter whose source has
    /// gone quiet re-sends its last known value with a degraded verdict, once per
    /// period. Add a broker outage and the SAME value is refused again and again:
    /// at a 30 s period, twenty minutes of double failure counted forty losses
    /// where the historian was missing exactly one measurement.
    ///
    /// Both numbers are now published and neither replaces the other: `count` is
    /// what the manual defines — messages the bridge could not hand over, which
    /// must not shrink — and `count` minus `republications` is what the historian
    /// is missing.
    ///
    /// # The question is asked BEFORE the answer, and that ordering is the test
    ///
    /// `is_republication` compares against the last CONFIRMED reading. Asking it
    /// after confirming would make every reading its own repeat, because
    /// confirming is what makes it the last one.
    ///
    /// **Falsification, 2026-08-24:**
    ///
    /// 1. `deliver` calling `dropped` for every loss — the state before this
    ///    change: RED, `republications` stays 0 where 1 is owed;
    /// 2. `is_republication` answering `true` unconditionally: RED on the control
    ///    below, where a genuinely new reading is counted as a copy;
    /// 3. `is_republication` comparing the VALUE instead of the `ValueDate` —
    ///    `last.measurement.power == update.measurement.power`, which is what one
    ///    writes when thinking "same reading means same numbers": RED on the
    ///    control, where a genuinely new measurement carrying an unchanged power
    ///    is filed as a copy. A steady load makes that the ordinary case, not the
    ///    exotic one.
    #[test]
    fn one_stale_value_refused_twice_is_one_distinct_measurement_lost() {
        use crate::core::oracle::Verdict;
        use crate::domain::{Kw, Kwh, Measurement, MeterId, Quality, UtcMillis};

        let serial = Serial::new("30000001");
        let device = DeviceIdentity::new(crate::domain::MeterId::new("cpt01"), serial.clone());
        let meter = MeterId::new("meter");
        // The verdict travels because a republication is the last known value
        // re-sent with a DEGRADED one — that is what makes it a republication
        // rather than a repeat, and since ADR 0047 a strictly identical reading is
        // suppressed before the transport ever sees it. The first draft of this
        // test re-sent an identical reading and went red the day report-by-
        // exception landed, which is the composition working.
        let at = |value_date: i64, verdict: Verdict| {
            MeterUpdate::uniform(
                meter.clone(),
                Measurement {
                    meter: meter.clone(),
                    serial: serial.clone(),
                    power: Some(Kw(0.018)),
                    energy: Some(Kwh(4_843.822)),
                    value_date: UtcMillis(value_date),
                    quality: Quality::Good,
                },
                verdict,
            )
        };

        let node = sparkplug_b::EdgeNode::new("Site", "Bridge").expect("valid ids");
        // Three slots: NBIRTH, DBIRTH, and ONE accepted DDATA. Everything after
        // is refused by the transport, which is the outage half of the scenario.
        let options = MqttOptions::new("tight", "127.0.0.1", 1883);
        let (client, _never_polled) = AsyncClient::new(options, 3);
        let mut publisher = SparkplugPublisher::new(node, None);
        announce(
            &client,
            &mut publisher,
            UtcMillis(1_000),
            std::slice::from_ref(&device),
            BirthReason::Connected,
        );

        // The reading the host DOES receive, and which every re-send below
        // repeats.
        assert_eq!(
            deliver(
                &client,
                &mut publisher,
                &at(1_784_984_792_050, Verdict::good())
            ),
            None,
            "the premise: this one goes out, or nothing below is a repeat of \
             anything"
        );

        // The source has gone quiet: the same measurement, re-sent, refused.
        let first = deliver(
            &client,
            &mut publisher,
            &at(
                1_784_984_792_050,
                Verdict::stale(crate::core::oracle::Cause::ReadingTooOld),
            ),
        );
        assert_eq!(
            first,
            Some(Loss {
                reason: DropReason::TransportQueueFull,
                fault: None,
                republication: true,
            }),
            "the same acquisition instant is the same measurement, whatever the \
             clock said when it was re-sent"
        );

        // THE CONTROL: a genuinely NEW measurement, refused by the same full
        // queue, is not a copy of anything. Without it, `republication: true`
        // everywhere would pass the assertion above.
        assert_eq!(
            deliver(
                &client,
                &mut publisher,
                &at(1_784_984_852_050, Verdict::good())
            ),
            Some(Loss {
                reason: DropReason::TransportQueueFull,
                fault: None,
                republication: false,
            }),
            "a new acquisition instant is a distinct measurement the historian \
             will never have: counting it as a copy would UNDERSTATE the loss, \
             which is the opposite error and the worse one"
        );
    }

    /// [#92] — a refused reading leaves NO trace: no hole in the sequence, and
    /// nothing recorded as last published.
    ///
    /// # Why the hole matters more than the missing message
    ///
    /// A `seq` jump is not "one message short" to a Sparkplug host. The
    /// specification makes it a lost-message condition: the host issues a Rebirth
    /// Request or marks the node stale. So one counted drop escalated into a
    /// session-level event that `dropped_readings` never mentioned — and it is
    /// the arm that fires most during an outage, because a full outbound queue is
    /// exactly what an outage looks like from here.
    ///
    /// The second half is the disagreement [#90] made visible: `/healthz` said
    /// the reading was lost while the publisher's memory had it as delivered, so
    /// a later rebirth re-declared it.
    ///
    /// # The control is what stops this passing for the wrong reason
    ///
    /// A publisher that never advanced its sequence at all would satisfy the
    /// first assertion, so the accepted reading is checked to have consumed a
    /// number. And a publisher that recorded nothing ever would satisfy the
    /// second, so the accepted reading is checked to be re-declared by a rebirth.
    ///
    /// **Falsification, 2026-08-24:**
    ///
    /// 1. `refused` not calling `give_back_seq` — the state before ADR 0046:
    ///    RED, the DDATA after the refusal carries 3 where 2 is owed;
    /// 2. `refused` behaving as `confirmed` — the other half of the same defect:
    ///    RED, and it is worth saying WHERE, because it is not where it was
    ///    expected. It fails on the SEQUENCE assertion (`3` where `2` is owed),
    ///    which comes first, and never reaches the memory assertion it was
    ///    written for. The mutation is caught; the reason recorded here is the
    ///    measured one and not the intended one;
    /// 3. `confirmed` recording nothing: RED on the memory assertion
    ///    (`None` where `0.019` is owed) — which is what gives the control a
    ///    subject, and the only mutation that reaches that half.
    #[test]
    fn a_reading_the_transport_refused_leaves_neither_a_hole_nor_a_memory() {
        use crate::core::oracle::Verdict;
        use crate::domain::{Kw, Kwh, Measurement, MeterId, Quality, UtcMillis};

        let serial = Serial::new("30000001");
        let device = DeviceIdentity::new(crate::domain::MeterId::new("cpt01"), serial.clone());
        let reading = |power: f64| {
            MeterUpdate::uniform(
                MeterId::new("meter"),
                Measurement {
                    meter: MeterId::new("meter"),
                    serial: serial.clone(),
                    power: Some(Kw(power)),
                    energy: Some(Kwh(4_843.822)),
                    value_date: UtcMillis(1_784_984_792_050),
                    quality: Quality::Good,
                },
                Verdict::good(),
            )
        };

        let node = sparkplug_b::EdgeNode::new("Site", "Bridge").expect("valid ids");
        // Two slots: the NBIRTH and the DBIRTH take them, so the device is
        // declared and everything after is refused by the transport alone.
        let options = MqttOptions::new("tight", "127.0.0.1", 1883);
        let (client, _never_polled) = AsyncClient::new(options, 2);
        let mut publisher = SparkplugPublisher::new(node, None);
        announce(
            &client,
            &mut publisher,
            UtcMillis(1_000),
            std::slice::from_ref(&device),
            BirthReason::Connected,
        );

        // The birth consumed seq 0 (NBIRTH) and 1 (DBIRTH), so the next message
        // owes 2. Read through what a publication actually emits rather than
        // from a counter nobody sees.
        let seq_of = |sink: &Queue| {
            sparkplug_b::decode(&sink.pending.last().expect("a message").payload)
                .expect("valid protobuf")
                .seq
        };

        let mut refused_sink = Queue::default();
        let pending = publisher
            .publish(&reading(0.018), &mut refused_sink)
            .expect("valid topics");
        assert_eq!(seq_of(&refused_sink), Some(2), "the refused DDATA took 2");
        publisher.refused(pending);

        let mut next = Queue::default();
        let pending = publisher
            .publish(&reading(0.019), &mut next)
            .expect("valid topics");
        assert_eq!(
            seq_of(&next),
            Some(2),
            "2 never reached the wire, so 2 is what the next message owes: a host \
             that sees a jump does not report one lost reading, it issues a \
             Rebirth Request or marks the node stale"
        );
        publisher.confirmed(pending);

        // THE CONTROL: the accepted reading DID consume a number, so the first
        // assertion is not satisfied by a publisher that never advances.
        let mut third = Queue::default();
        let pending = publisher
            .publish(&reading(0.020), &mut third)
            .expect("valid topics");
        assert_eq!(
            seq_of(&third),
            Some(3),
            "an accepted reading advances the sequence; without this, a publisher \
             stuck at one number would pass the assertion above"
        );
        publisher.refused(pending);

        // And the memory: a rebirth re-declares the ACCEPTED reading, never the
        // refused ones.
        let mut rebirth = Queue::default();
        publisher
            .birth(
                UtcMillis(2_000),
                std::slice::from_ref(&device),
                &mut rebirth,
            )
            .expect("valid topics");
        let dbirth = sparkplug_b::decode(&rebirth.pending[1].payload).expect("valid protobuf");
        let power = dbirth
            .metrics
            .iter()
            .find(|m| m.name.as_deref() == Some(crate::adapters::sparkplug_publisher::METRIC_POWER))
            .expect("the power metric");
        assert_eq!(
            power.value,
            Some(sparkplug_b::protobuf::payload::metric::Value::DoubleValue(
                0.019
            )),
            "the rebirth must re-declare the reading the host RECEIVED (0.019), \
             not one the transport refused: `dropped_readings` naming a reading \
             the rebirth treats as delivered is the contradiction [#92] reports"
        );
    }

    /// [#95] — the transport's refusal is counted, and it is the call site
    /// nothing reached.
    ///
    /// Measured on 2026-08-19: deleting the `transport-queue-full` count left the
    /// whole suite GREEN. `chaos_broker_recovery` reaches the publisher's
    /// refusals through `before-birth` and has never once produced this one,
    /// which its own comment predicted — so the count could be removed and
    /// nothing would say so.
    ///
    /// **A capacity-1 request channel that nothing drains makes it
    /// deterministic**, with no timing at all: the first message takes the slot,
    /// the second is refused, every run. That is what [#95] judged out of reach —
    /// correctly for a chaos test against a real broker, and not for this one.
    ///
    /// The control is the same publisher and the same reading on a client with
    /// room: the reading goes out and NOTHING is counted. Without it this test
    /// would pass just as well against a `deliver` that reported a loss every
    /// time, which is the failure that costs an operator the most — a bridge
    /// crying loss over readings the host has.
    ///
    /// **Falsification, 2026-08-23:**
    ///
    /// 1. the `transport-queue-full` arm deleted — the state [#95] measured: RED;
    /// 2. `deliver` reporting the loss without asking `publish_all`: RED on the
    ///    control.
    ///
    /// **And one mutation that is NOT caught, recorded because it was run and
    /// survived:** hoisting the drain out of the `None` arm so it runs for every
    /// outcome. It was written expecting red and stayed green through two
    /// attempts, including the third case below which was added for it.
    ///
    /// It is green because it is EQUIVALENT, not because this test is blind. A
    /// publisher that refuses a reading emits nothing, so the queue is empty on
    /// every `Some(reason)` path and a drain there can refuse nothing. The
    /// ordering is therefore load-bearing for the READER and not for the
    /// behaviour — which is worth knowing before someone "simplifies" it and
    /// believes the suite blessed the change. It would stop being equivalent the
    /// day a refused reading left something queued, and nothing here would say
    /// so.
    ///
    /// The third case below stands on its own merits regardless: it pins that the
    /// publisher's reason wins when both causes are live, which is the answer an
    /// operator acts on.
    #[test]
    fn a_reading_the_transport_refuses_is_counted_against_the_transport() {
        use crate::core::oracle::Verdict;
        use crate::domain::{Kw, Kwh, Measurement, MeterId, Quality, UtcMillis};

        let serial = Serial::new("30000001");
        let device = DeviceIdentity::new(crate::domain::MeterId::new("cpt01"), serial.clone());
        let reading = MeterUpdate::uniform(
            MeterId::new("meter"),
            Measurement {
                meter: MeterId::new("meter"),
                serial: serial.clone(),
                power: Some(Kw(0.018)),
                energy: Some(Kwh(4_843.822)),
                value_date: UtcMillis(1_784_984_792_050),
                quality: Quality::Good,
            },
            Verdict::good(),
        );

        let node = sparkplug_b::EdgeNode::new("Site", "Bridge").expect("valid ids");

        // THE CONTROL FIRST: room in the channel, and the reading goes out.
        let roomy = MqttOptions::new("roomy", "127.0.0.1", 1883);
        let (roomy_client, _roomy_loop) = AsyncClient::new(roomy, 16);
        let mut publisher = SparkplugPublisher::new(node.clone(), None);
        announce(
            &roomy_client,
            &mut publisher,
            UtcMillis(1_000),
            std::slice::from_ref(&device),
            BirthReason::Connected,
        );
        assert_eq!(
            deliver(&roomy_client, &mut publisher, &reading),
            None,
            "a reading the transport accepted must be counted against nothing; \
             a bridge that reported losses for readings the host HAS is worse \
             than one that reports none"
        );

        // AND THE SUBJECT: exactly two slots, taken by the NBIRTH and the DBIRTH,
        // so the device IS declared and the reading is refused by the transport
        // alone. The `EventLoop` is dropped on the floor, so nothing ever drains
        // it and the refusal needs no timing.
        //
        // **Not one slot**, and the first draft of this test used one: the DBIRTH
        // was then refused too, [#88] withdrew the declaration, and `deliver`
        // answered `undeclared-device` — the right answer to a different
        // question, and a test that would have passed for the wrong reason had it
        // asserted "some loss" instead of naming which.
        let tight = MqttOptions::new("tight", "127.0.0.1", 1883);
        let (tight_client, _never_polled) = AsyncClient::new(tight, 2);
        let mut publisher = SparkplugPublisher::new(node, None);
        announce(
            &tight_client,
            &mut publisher,
            UtcMillis(1_000),
            std::slice::from_ref(&device),
            BirthReason::Connected,
        );

        assert_eq!(
            deliver(&tight_client, &mut publisher, &reading),
            Some(Loss {
                reason: DropReason::TransportQueueFull,
                fault: None,
                republication: false,
            }),
            "the publisher emitted and the TRANSPORT refused: that is a reading \
             the host does not have, and until now deleting the line that counts \
             it left the suite green"
        );

        // AND THE DISCRIMINATION, which the first version of this test did not
        // have: the publisher refuses AND the channel is full at the same time.
        // The answer is the publisher's reason, because there was nothing to
        // hand the transport — a reading it never saw cannot be one it refused.
        //
        // One slot: the DBIRTH is refused, [#88] withdraws the declaration, and
        // the queue is full for the DDATA that follows. Both causes are live at
        // once, which is the only state that can tell them apart.
        let both = MqttOptions::new("both", "127.0.0.1", 1883);
        let (both_client, _also_never_polled) = AsyncClient::new(both, 1);
        let mut publisher =
            SparkplugPublisher::new(sparkplug_b::EdgeNode::new("Site", "Bridge").unwrap(), None);
        announce(
            &both_client,
            &mut publisher,
            UtcMillis(1_000),
            std::slice::from_ref(&device),
            BirthReason::Connected,
        );
        assert_eq!(
            deliver(&both_client, &mut publisher, &reading),
            Some(Loss {
                reason: DropReason::UndeclaredDevice,
                fault: None,
                republication: false,
            }),
            "the publisher's refusal wins: hoisting the drain out of its arm \
             would report a transport refusal for a reading the transport was \
             never offered, and send an operator after a broker that is fine"
        );
    }

    /// [#88], end to end — the wiring `withdrawal_for` alone cannot prove.
    ///
    /// The two decision tests above pin what SHOULD be withdrawn; nothing yet
    /// pins that `announce` acts on the answer. Deleting the `match withdrawal`
    /// arm leaves both of them green, and it is the whole repair.
    ///
    /// A capacity-2 request channel that nothing drains is what splits the
    /// sequence where it has to be split: the NBIRTH and the FIRST device
    /// certificate take the two slots, and the second device's is refused. So one
    /// device is genuinely declared and one genuinely is not, in a single birth.
    ///
    /// **Falsification, 2026-08-23:**
    ///
    /// 1. Deleting the `match withdrawal` arm in `announce` — the state before
    ///    this repair: RED (`30000002` publishes as `Emitted`).
    /// 2. `Withdrawal::EveryDevice` for every refusal: RED on `30000001`, which
    ///    the host DID receive.
    #[test]
    fn announce_undeclares_the_device_whose_certificate_the_transport_refused() {
        use crate::core::oracle::Verdict;
        use crate::domain::{Kw, Kwh, Measurement, MeterId, Quality, UtcMillis};

        let declared = DeviceIdentity::new(
            crate::domain::MeterId::new("cpt01"),
            Serial::new("30000001"),
        );
        let refused = DeviceIdentity::new(
            crate::domain::MeterId::new("cpt02"),
            Serial::new("30000002"),
        );

        let options = MqttOptions::new("falsify", "127.0.0.1", 1883);
        // Two slots, and the EventLoop is dropped on the floor: they are the only
        // two there will ever be. NBIRTH and the first DBIRTH take them.
        let (client, _never_polled) = AsyncClient::new(options, 2);

        let node = sparkplug_b::EdgeNode::new("Site", "Bridge").expect("valid ids");
        let mut publisher = SparkplugPublisher::new(node, None);
        announce(
            &client,
            &mut publisher,
            UtcMillis(1_000),
            &[declared.clone(), refused.clone()],
            BirthReason::Connected,
        );

        let reading = |serial: &Serial| {
            MeterUpdate::uniform(
                MeterId::new("meter"),
                Measurement {
                    meter: MeterId::new("meter"),
                    serial: serial.clone(),
                    power: Some(Kw(0.018)),
                    energy: Some(Kwh(4_843.822)),
                    value_date: UtcMillis(1_784_984_792_050),
                    quality: Quality::Good,
                },
                Verdict::good(),
            )
        };

        let mut sink = Queue::default();
        // Through `publish` + `confirmed`, as the driver does: since [#92] the
        // outcome is read from a `Pending` the caller must answer.
        let mut answered = |p: &mut SparkplugPublisher, update: &MeterUpdate| {
            let pending = p.publish(update, &mut sink).expect("valid topics");
            let outcome = pending.outcome().clone();
            p.confirmed(pending);
            outcome
        };
        assert_eq!(
            answered(&mut publisher, &reading(&declared.serial)),
            Published::Emitted,
            "this device's certificate took the second slot: the host has it, and \
             reporting its readings as lost would be the opposite error"
        );
        assert_eq!(
            answered(&mut publisher, &reading(&refused.serial)),
            Published::DroppedUndeclaredDevice {
                serial: refused.serial.clone()
            },
            "this device's certificate never left: every reading for it is thrown \
             away by the host, and [#88] is the bridge counting them as published"
        );
    }

    /// [#88] — a device whose BIRTH the transport refused is UNDECLARED, so its
    /// readings are counted as lost instead of as published.
    ///
    /// This is the discrimination half: `s2`'s certificate was refused and `s1`'s
    /// was not, and a repair that withdrew both would be as wrong as one that
    /// withdrew neither — it would turn readings the host does receive into
    /// reported losses.
    ///
    /// **Falsification, 2026-08-23** (mutations run against this test):
    ///
    /// 1. `Withdrawal::These(vec![])` regardless of input — the shape the defect
    ///    shipped in, where a refusal changes nothing: RED on `s2`.
    /// 2. `These` built from EVERY refused message's device without filtering,
    ///    i.e. returning `s1` too: RED on `s1` — the control has a subject.
    /// 3. Returning `EveryDevice` here: RED on `s1`.
    #[test]
    fn a_refused_device_birth_withdraws_that_device_and_not_the_others() {
        let s1 = Serial::new("30000001");
        let s2 = Serial::new("30000002");
        let refused = vec![Outbound {
            topic: "spBv1.0/Site/DBIRTH/Bridge/30000002".to_string(),
            payload: vec![],
            message: MessageType::DBirth,
            device: Some(s2.clone()),
        }];

        let Withdrawal::These(devices) = withdrawal_for(&refused) else {
            panic!("no node certificate was refused, so the session still stands");
        };
        assert!(
            devices.contains(&s2),
            "the host was never told about this device; counting its readings as \
             published is the defect"
        );
        assert!(
            !devices.contains(&s1),
            "this device's certificate DID go out — withdrawing it would report \
             losses the host is not suffering"
        );
    }

    /// [#88] — a BIRTH whose NODE certificate was refused takes every device with
    /// it, including the ones whose own certificate went out.
    ///
    /// **This is the case a transport-level test cannot reach**: a bounded request
    /// channel fills in order and the NBIRTH is always first, so no queue capacity
    /// produces "node refused, device accepted". It is nonetheless the ordinary
    /// case rather than an exotic one — the host discards a DBIRTH for a node it
    /// holds no session for, so a bridge that withdrew only the refused DBIRTHs
    /// would go on counting readings as published for devices the host is throwing
    /// away.
    ///
    /// **Falsification, 2026-08-23:** deleting the `NBirth` arm — the ordinary way
    /// to write this, one `filter_map` over the refused list — leaves the DBIRTH
    /// that DID go out declared, and turns this RED.
    #[test]
    fn a_refused_node_birth_withdraws_every_device_including_the_ones_that_went_out() {
        let refused = vec![Outbound {
            topic: "spBv1.0/Site/NBIRTH/Bridge".to_string(),
            payload: vec![],
            message: MessageType::NBirth,
            // A node message speaks for no device: the list of refused SERIALS is
            // empty here, which is exactly why a `filter_map` over it is not
            // enough.
            device: None,
        }];

        assert_eq!(
            withdrawal_for(&refused),
            Withdrawal::EveryDevice,
            "a DBIRTH means nothing under a session the host never opened"
        );
    }

    #[test]
    fn a_partly_published_birth_is_counted_and_never_reported_complete() {
        let options = MqttOptions::new("falsify", "127.0.0.1", 1883);
        // Capacity 1, and the EventLoop is dropped on the floor: the single slot
        // is the only one there will ever be.
        let (client, _never_polled) = AsyncClient::new(options, 1);

        let message = |topic: &str| Outbound {
            topic: topic.to_string(),
            payload: vec![1, 2, 3],
            message: MessageType::NBirth,
            device: None,
        };
        let mut queue = Queue {
            pending: vec![message("a"), message("b"), message("c")],
        };

        let dropped = publish_all(&client, &mut queue);
        assert_eq!(
            dropped.len(),
            2,
            "one slot means one message queued and two dropped; a caller that \
             cannot see the two has no way to know the sequence was broken"
        );
        // WHICH ones, not just how many: `announce` has to undo the declaration
        // of the devices whose BIRTH was refused, and it can only do that if the
        // refusal names them ([#88]). Pinned on the topics because this queue
        // carries no devices — `a_refused_device_birth_undeclares_that_device`
        // pins the serial itself.
        assert_eq!(
            dropped.iter().map(|m| m.topic.as_str()).collect::<Vec<_>>(),
            vec!["b", "c"],
            "the single slot takes the FIRST message; the caller must learn which \
             of the three did not go out, in order"
        );
        assert!(
            queue.pending.is_empty(),
            "every message must be ATTEMPTED — stopping at the first failure \
             would leave the rest in the sink with nothing said about them"
        );
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("smartme_bdseq_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join(name)
    }

    /// The defect [#46] was: no reset at all, so the ladder was monotonic for the
    /// life of the process. This drives the ladder to its ceiling the only way a
    /// caller can — repeated sessions that never connect — and then asserts that
    /// ONE session which reached CONNACK brings it back to the floor.
    ///
    /// The sequence is driven through `ladder_step` itself rather than
    /// re-implemented here; a test that restates production's own expression
    /// proves only that the expression equals itself, which this project has
    /// already been caught by once.
    ///
    /// FALSIFIED 2026-08-03 against the ORIGINAL defect, copied from the run.
    /// Mutation: `let now = previous;` — the pre-fix behaviour, no reset:
    ///
    /// ```text
    /// test app::mqtt_driver::tests::the_ladder_is_capped ... ok
    /// test app::mqtt_driver::tests::a_session_that_connected_forgives_the_ladder ... FAILED
    /// assertion `left == right` failed: a session that reached CONNACK must
    /// reset the ladder, not inherit 30s
    /// test result: FAILED. 1 passed; 1 failed
    /// ```
    ///
    /// `30s` in that message is the bug itself, reported by the test. The cap
    /// test stayed green, so this one carries the proof alone.
    #[test]
    fn a_session_that_connected_forgives_the_ladder() {
        let mut base = RECONNECT_FLOOR;
        let mut climbed = Vec::new();
        for _ in 0..8 {
            let (now, next) = ladder_step(base, false, RECONNECT_FLOOR);
            climbed.push(now);
            base = next;
        }
        assert_eq!(
            base, RECONNECT_CEILING,
            "eight failures should have reached the ceiling; got {climbed:?}"
        );

        let (after_a_healthy_session, _) = ladder_step(base, true, RECONNECT_FLOOR);
        assert_eq!(
            after_a_healthy_session, RECONNECT_FLOOR,
            "a session that reached CONNACK must reset the ladder, not inherit {base:?}"
        );
    }

    /// The ladder must not climb past the ceiling however long the outage runs.
    #[test]
    fn the_ladder_is_capped() {
        let mut base = RECONNECT_FLOOR;
        for _ in 0..64 {
            base = ladder_step(base, false, RECONNECT_FLOOR).1;
        }
        assert_eq!(base, RECONNECT_CEILING);
    }

    /// The property the durability bound depends on. If jitter can ever shorten
    /// a wait, `RECONNECT_FLOOR` stops bounding how often `bdSeq` is fsynced —
    /// and nothing else in the tree would notice.
    ///
    /// FALSIFIED 2026-08-03. Copied from the run, not written from memory —
    /// the first draft of this record stated numbers no run had produced.
    ///
    /// Mutation: `backoff + …` → `backoff - …`, the full-jitter shape this
    /// function exists to avoid. Only this test moved:
    ///
    /// ```text
    /// test app::mqtt_driver::tests::jitter_never_shortens_a_wait ... FAILED
    /// panicked at crates/smartme-bridge/src/app/mqtt_driver.rs:2507:17:
    /// jitter must never shorten a wait: 999ms < 1s (entropy 1)
    /// test result: FAILED. 2 passed; 1 failed
    /// ```
    ///
    /// Restored, re-run green: `test result: ok. 3 passed; 0 failed`.
    #[test]
    fn jitter_never_shortens_a_wait() {
        for base_ms in [1_000u64, 2_000, 8_000, 30_000] {
            let base = Duration::from_millis(base_ms);
            for entropy in [0i64, 1, -1, 7, i64::MAX, i64::MIN, 123_456_789] {
                let waited = jittered(base, entropy);
                assert!(
                    waited >= base,
                    "jitter must never shorten a wait: {waited:?} < {base:?} (entropy {entropy})"
                );
            }
        }
    }

    /// `i64::MIN` is the case a naive `entropy.abs()` panics on in debug and
    /// wraps negative on in release. `unsigned_abs` is the reason it is here.
    #[test]
    fn jitter_stays_within_half_again_of_the_backoff() {
        for base_ms in [1_000u64, 30_000] {
            let base = Duration::from_millis(base_ms);
            for entropy in [0i64, i64::MAX, i64::MIN, -999_999] {
                assert!(
                    jittered(base, entropy) < base + base / 2,
                    "jitter is bounded at half the backoff"
                );
            }
        }
    }

    /// Without this, a `jittered` that returned its input unchanged would satisfy
    /// both bounds above and spread nothing. An expected-red for the "it does
    /// something" direction, which the two bound tests cannot supply.
    ///
    /// FALSIFIED 2026-08-03, copied from the run. Mutation: the body replaced by
    /// `let _ = entropy; backoff`. The two bound tests above stayed GREEN — which
    /// is the point of this one existing:
    ///
    /// ```text
    /// test app::mqtt_driver::tests::jitter_never_shortens_a_wait ... ok
    /// test app::mqtt_driver::tests::jitter_stays_within_half_again_of_the_backoff ... ok
    /// test app::mqtt_driver::tests::jitter_actually_varies ... FAILED
    /// jitter that never varies is not jitter — got {8s}
    /// test result: FAILED. 2 passed; 1 failed
    /// ```
    #[test]
    fn jitter_actually_varies() {
        let base = Duration::from_secs(8);
        let spread: std::collections::BTreeSet<_> = (0..100i64)
            .map(|entropy| jittered(base, entropy * 37))
            .collect();
        assert!(
            spread.len() > 1,
            "jitter that never varies is not jitter — got {spread:?}"
        );
    }

    /// The floor makes this unreachable in production; it is here because the
    /// division would panic and a guard nobody exercises is a guard nobody has.
    #[test]
    fn a_backoff_too_short_to_halve_is_returned_unchanged() {
        let tiny = Duration::from_millis(1);
        assert_eq!(jittered(tiny, i64::MAX), tiny);
    }

    #[test]
    fn bd_seq_survives_a_round_trip_and_a_missing_file() {
        let path = temp("roundtrip.toml");
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            load_bd_seq(&path),
            None,
            "a missing file means this node has never connected — not a number"
        );
        store_bd_seq(&path, BdSeq::new(42)).expect("persist");
        assert_eq!(load_bd_seq(&path), Some(BdSeq::new(42)));
        let _ = std::fs::remove_file(&path);
    }

    /// A corrupt file is NOT a first session, and the distinction is the whole
    /// of [#100]'s repair: an absent file answers `None` and opens session 0, an
    /// unreadable one answers `Some(0)` and opens session 1 exactly as before.
    /// Collapsing the two — which is what the old sentinel did — either replays a
    /// number a consumer has seen, or misses the clause. Falsified by making the
    /// `NotFound` arm answer for both kinds of error.
    #[test]
    fn a_corrupt_bd_seq_file_is_not_read_as_a_node_that_never_connected() {
        let path = temp("corrupt.toml");
        std::fs::write(&path, "this is not toml {{{").expect("write");
        assert_eq!(
            load_bd_seq(&path),
            Some(BdSeq::new(0)),
            "unreadable means we cannot tell, not that there was nothing"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// **Story 4.11 AC5 — every publisher outcome is classified, and none twice.**
    ///
    /// The driver owns four of the six ways a reading can be lost, and this is the
    /// only place their names are decided. A wrong arm here does not fail
    /// loudly — it reports a real loss under the wrong reason, which sends an
    /// operator to the wrong fault with a number that looks authoritative.
    ///
    /// `Emitted` maps to `None` and that is NOT "it reached the wire": the
    /// transport can still refuse the queued message.
    ///
    /// **THAT AXIS IS NOT ASSERTED ANYWHERE, and this doc claimed it was** until
    /// the 2026-08-18 review — *"is asserted at the call site's own arm"*, of an
    /// assertion that did not exist. The call site is inside a `select!` in a
    /// function that needs a live broker, so deleting its `lost(...)` calls would
    /// leave the whole suite green. Recorded as [#86] rather than papered over;
    /// story 4.13's `chaos_broker_recovery` is where it becomes reachable.
    ///
    /// FALSIFIED 2026-08-18 — mutation RUN, output copied: filing
    /// `DroppedBeforeBirth` under `Unpublishable` goes red with `assertion `left
    /// == right` failed […] left: Some(Unpublishable), right: Some(BeforeBirth)`.
    /// The distinctness assertion is what makes that mutation reachable at all:
    /// without it, one variant for every arm would still satisfy each `assert_eq`
    /// that is not `None`.
    #[test]
    fn every_publisher_outcome_is_classified() {
        use crate::domain::Serial;

        assert_eq!(reason_for(&Ok(Published::Emitted)), None);
        assert_eq!(
            reason_for(&Ok(Published::DroppedBeforeBirth)),
            Some(DropReason::BeforeBirth)
        );
        assert_eq!(
            reason_for(&Ok(Published::DroppedUndeclaredDevice {
                serial: Serial::new("30000001")
            })),
            Some(DropReason::UndeclaredDevice)
        );
        assert_eq!(
            reason_for(&Err(sparkplug_b::EdgeNode::new("g", "n")
                .expect("a valid node")
                .device_topic(MessageType::DData, "a/b")
                .expect_err("a wildcard-bearing serial is not a topic level"))),
            Some(DropReason::Unpublishable)
        );

        // AND NO TWO OUTCOMES SHARE A REASON. Without this, mapping every arm to
        // one variant would satisfy every assertion above that is not `None`.
        let named = [
            reason_for(&Ok(Published::DroppedBeforeBirth)),
            reason_for(&Ok(Published::DroppedUndeclaredDevice {
                serial: Serial::new("30000001"),
            })),
            reason_for(&Err(sparkplug_b::EdgeNode::new("g", "n")
                .expect("a valid node")
                .device_topic(MessageType::DData, "a/b")
                .expect_err("a wildcard-bearing serial is not a topic level"))),
        ];
        let mut distinct = named.to_vec();
        distinct.sort_by_key(|r| r.map(DropReason::index));
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            named.len(),
            "two publisher outcomes filed under one reason: {named:?}"
        );
    }
}
