# MQTT — the clauses this repository depends on, pinned

**This is a PINNED CITATION, not a vendored copy**, and the difference is deliberate.
`docs/spec/sparkplug-b-3.0.0/` holds the Sparkplug specification in full because `CLAUDE.md`
requires every Sparkplug claim to be settled by reading it and cited by `tck-id-…`. MQTT is not
vendored here: what follows is the small set of statements this repository actually relies on,
quoted verbatim with their normative identifiers, so a reader can verify them against the source
instead of trusting a summary — which is what [#34] asked for in the words *"so the fix is written
against a norm rather than against memory"*.

## Why it exists, and what its absence cost

Until 2026-08-29 there was nothing here. Chapter 1 of Sparkplug **defers the identifier character
set to MQTT** — *"Because the Group ID is used in MQTT topic strings the Group ID MUST only contain
characters allowed for MQTT topics per the MQTT Specification"* (`tck-id-intro-group-id-chars`, and
the same for the edge node and the device) — so a repository that pins Sparkplug and not MQTT can
follow the reference exactly one step and then has to guess.

**It cost two blockings in one day**, which is what turned it from a footnote into work:

- **[#34] could not be closed.** Its repair shipped on 2026-08-28 — a `U+0000` no longer reaches a
  topic level — but its body required this file first, and the body was right: the fix's
  justification was written from memory.
- **[#43] could not be interpreted.** Its hypothesis was that a broker suppresses a will on session
  takeover. What a Server owes a Will Message was not citable here, so the argument could only be
  settled by measurement — which it was, and the measurement said something else entirely.

## The document

| | |
|---|---|
| **Title** | MQTT Version 3.1.1 |
| **Stage** | OASIS Standard |
| **Date** | 29 October 2014 |
| **Source** | `http://docs.oasis-open.org/mqtt/mqtt/v3.1.1/os/mqtt-v3.1.1-os.html` |
| **Retrieved** | 2026-08-29 |

**Version 3.1.1 is what this bridge speaks, and that is read off the code rather than assumed.**
`mqtt_driver.rs` imports `rumqttc::{AsyncClient, MqttOptions, …}` — the crate's v4 re-export
(`rumqttc/src/lib.rs:141`, `pub use mqttbytes::v4::*`) and not `rumqttc::v5` — and that encoder
writes protocol level `0x04` (`mqttbytes/v4/connect.rs:108`). Which the specification pins:

> *"The value of the Protocol Level field for the version 3.1.1 of the protocol is 4 (0x04)"*
> — **[MQTT-3.1.2-2]**, §3.1.2.2 Protocol Level

Sparkplug B 3.0.0 admits both 3.1.1 and 5.0; the clauses below are 3.1.1's. **If the driver ever
moves to `rumqttc::v5`, this file is wrong and must be re-fetched** — the section numbering differs
(UTF-8 encoded strings move to §1.5.4).

## §1.5.3 — UTF-8 encoded strings

The clauses `check_identifier` implements, and the ones Sparkplug's `-chars` trio defers to.

> *"The character data in a UTF-8 encoded string MUST be well-formed UTF-8"* — **[MQTT-1.5.3-1]**
>
> *"If a Server or Client receives a Control Packet containing ill-formed UTF-8 it MUST close the
> Network Connection"* — **[MQTT-1.5.3-1]**

> *"A UTF-8 encoded string MUST NOT include an encoding of the null character U+0000"* —
> **[MQTT-1.5.3-2]**
>
> *"If a receiver receives a Control Packet containing U+0000 it MUST close the Network Connection"*
> — **[MQTT-1.5.3-2]**

> *"A UTF-8 encoded sequence 0xEF 0xBB 0xBF is always to be interpreted to mean U+FEFF"* —
> **[MQTT-1.5.3-3]**

And, **SHOULD NOT** rather than MUST NOT, which is the distinction the repair turns on:

> *"The data SHOULD NOT include encodings of the Unicode code points listed below… U+0001..U+001F
> control characters, U+007F..U+009F control characters, Code points defined in the Unicode
> specification to be non-characters"* — §1.5.3

**Two cautions on the quotes above, stated rather than smoothed over.** The two *"MUST close the
Network Connection"* sentences were returned under the same identifiers as the statements they
follow; a reader checking against the source should confirm how the specification splits them. And
the SHOULD NOT paragraph is elided at *"listed below"*, where the source prints a list.

### What the bridge does with them, and why it stops where it does

`check_identifier` (`crates/sparkplug-b/src/topic.rs`) refuses `U+0000` and nothing else beyond
Sparkplug's own wildcard-and-separator rule. **The MUST is honoured and the SHOULD NOT is
deliberately not**: refusing `U+0001..U+001F` and `U+007F..U+009F` would be stricter than either
specification, so an identifier both of them allow would fail to start a bridge. The unpaired
surrogates MQTT also forbids cannot exist in a Rust `char`, and `String` discharges
[MQTT-1.5.3-1] by construction.

`a_null_character_is_refused_and_a_control_character_is_not` asserts **both directions**, so a
repair reaching too far fails it as surely as one that does not reach far enough.

## §3.1.2.5 — the Will Message

The clauses ADR 0011's two-mechanism design rests on, and the ones [#43] needed.

> *"If the Will Flag is set to 1, the Will Message MUST be published when the Network Connection is
> subsequently closed unless the Will Message has been deleted by the Server on receipt of a
> DISCONNECT Packet"* — **[MQTT-3.1.2-8]**

> *"The Will Message MUST be removed from the stored Session state in the Server once it has been
> published or the Server has received a DISCONNECT packet from the Client"* — **[MQTT-3.1.2-10]**

**This is why `mqtt_driver::run` never sends a DISCONNECT**, on either the shutdown path or the
transport-lost path: both end in `pump.abort()`. A graceful DISCONNECT would delete the will, so if
the explicit death did not reach the wire, nothing would ever be delivered. The code said so before
this file existed; now it can cite the clause it was written against.

**And it is why [#43]'s hypothesis was implausible before it was refuted.** [MQTT-3.1.2-8] makes
publication the Server's obligation whenever the connection closes without a DISCONNECT, with one
stated exception that is not takeover. The measurement agreed: the will does fire, and the missing
one was the observer's own socket.

## §3.1.3.1 — Client Identifier

Relevant because the bridge reuses one `client_id` across reconnects, which is what [#43] suspected.

> *"The ClientId MUST be a UTF-8 encoded string as defined in Section 1.5.3"* — **[MQTT-3.1.3-4]**

> *"The Server MUST allow ClientIds which are between 1 and 23 UTF-8 encoded bytes in length, and
> that contain only the characters
> '0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ'"* — **[MQTT-3.1.3-5]**

Note what [MQTT-3.1.3-5] does and does not say: it is a floor on what a Server must **allow**, not a
ceiling on what a Client may **send**. Nothing here is asserted about the bridge's `client_id`, which
is operator-supplied.

## §4.7.3 — Topic semantic and usage: NOT OBTAINED

**Wanted and not retrieved on 2026-08-29**, and recorded as a hole rather than filled from memory.
The statements sought are `[MQTT-4.7.3-1]`, `-2` and `-3` — the minimum length of a Topic Name, the
prohibition on `U+0000`, and the maximum encoded length in bytes. The fetched rendering of the OASIS
HTML is truncated before that section, and two attempts returned nothing.

**It does not weaken the repair of [#34]**: a Topic Name is a UTF-8 encoded string, so [MQTT-1.5.3-2]
governs it, and that is the clause the null-character refusal is written against. What is missing is
the topic-specific restatement and the length bound — **the bridge asserts nothing about topic
length**, and that absence is now visible rather than merely unexamined.

Whoever fetches it should add it here, keeping the shape of the sections above.

[#34]: https://github.com/guycorbaz/smartme_mqtt/issues/34
[#43]: https://github.com/guycorbaz/smartme_mqtt/issues/43
