//! Sparkplug B topic construction.
//!
//! The topic namespace is half of the Sparkplug contract: a correct payload on
//! the wrong topic is invisible to a consumer. Building topics from a validated
//! type rather than by string concatenation means an identifier containing a
//! wildcard — which would silently redirect or broaden a publication — cannot
//! reach the broker.

use std::fmt;

/// The Sparkplug B namespace element, fixed by the specification.
pub const NAMESPACE: &str = "spBv1.0";

/// Why a topic could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopicError {
    /// A node-level message type was addressed to a device topic, or the
    /// reverse. Enforced in every build profile: a DBIRTH on a node topic is
    /// invisible to every device subscriber, which is exactly the failure this
    /// module exists to prevent.
    WrongLevel {
        /// The message type that was passed.
        message: MessageType,
    },
    /// The identifier was empty.
    Empty {
        /// Which element (`group`, `node`, `device`).
        element: &'static str,
    },
    /// The identifier contained a character illegal in a topic level.
    IllegalCharacter {
        /// Which element (`group`, `node`, `device`).
        element: &'static str,
        /// The offending character.
        character: char,
    },
}

impl fmt::Display for TopicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TopicError::WrongLevel { message } => {
                write!(f, "{} addresses the wrong topic level", message.token())
            }
            TopicError::Empty { element } => write!(f, "{element} identifier is empty"),
            TopicError::IllegalCharacter { element, character } => {
                write!(f, "{element} identifier contains {character:?}")
            }
        }
    }
}

impl std::error::Error for TopicError {}

/// The message types this crate can address.
///
/// `DCMD` is deliberately absent. `tck-id-message-flow-device-dcmd-subscribe`
/// (`Sparkplug_5_Operational_Behavior.adoc:403-407`) is conditional — *"if the
/// Device supports writing to outputs"* — and an unused variant would invite a
/// subscription nothing needs.
///
/// # If you are adding `DCmd`, read this first ([#38])
///
/// **Two conformance verdicts stop being `n/a` the moment this variant exists**,
/// and they will not tell you themselves — `docs/sparkplug-conformance.md` is not
/// what anyone opens while writing a relay-command story, which is why this note
/// lives here instead. Adding the variant is the one edit that cannot be skipped,
/// so it is the one place the reminder cannot be missed.
///
/// - `tck-id-message-flow-device-dcmd-subscribe` — the bridge MUST then subscribe
///   to `spBv1.0/{group}/DCMD/{node}/{device}`. Recorded `n/a` **on the stated
///   condition** that no device supports writing to outputs; a meter relay is
///   exactly such an output.
/// - `topics-dcmd-topic` (chapter 4) — the matching topic-grammar clause, `n/a`
///   for the same reason.
///
/// Re-verdict both in the same change that adds the variant, or the matrix will
/// claim `n/a` for a condition that has stopped holding — the failure mode this
/// project has already paid for several times: a claim that stayed correct while
/// the world moved under it.
///
/// *Status on 2026-08-28: a meter relay is not planned "for the time being"
/// (Guy). The condition holds today; the expiry is deferred, not cancelled.*
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Node birth certificate.
    NBirth,
    /// Node data.
    NData,
    /// Node death certificate.
    NDeath,
    /// Node command. INBOUND only: an Edge Node subscribes to this topic and
    /// never publishes on it. Present so the subscription topic is built from
    /// the same validated grammar as everything else
    /// (`tck-id-message-flow-edge-node-ncmd-subscribe`).
    NCmd,
    /// Device birth certificate.
    DBirth,
    /// Device data.
    DData,
    /// Device death certificate.
    DDeath,
}

impl MessageType {
    /// The wire token for this message type.
    pub const fn token(self) -> &'static str {
        match self {
            MessageType::NBirth => "NBIRTH",
            MessageType::NData => "NDATA",
            MessageType::NDeath => "NDEATH",
            MessageType::NCmd => "NCMD",
            MessageType::DBirth => "DBIRTH",
            MessageType::DData => "DDATA",
            MessageType::DDeath => "DDEATH",
        }
    }

    /// Whether this message type addresses a device (and therefore needs a
    /// device identifier).
    pub const fn is_device_level(self) -> bool {
        matches!(
            self,
            MessageType::DBirth | MessageType::DData | MessageType::DDeath
        )
    }
}

/// A validated edge-node address: the group and node identifiers every topic of
/// one node shares.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EdgeNode {
    group: String,
    node: String,
}

impl EdgeNode {
    /// Validates and stores the pair. Identifiers may not be empty and may not
    /// contain `/`, `+` or `#`.
    pub fn new(group: impl Into<String>, node: impl Into<String>) -> Result<Self, TopicError> {
        let group = group.into();
        let node = node.into();
        check_identifier(&group, "group")?;
        check_identifier(&node, "node")?;
        Ok(Self { group, node })
    }

    /// The group identifier.
    pub fn group(&self) -> &str {
        &self.group
    }

    /// The node identifier.
    pub fn node(&self) -> &str {
        &self.node
    }

    /// The topic for a node-level message. A device-level message type is
    /// refused — in release builds too.
    pub fn node_topic(&self, message: MessageType) -> Result<String, TopicError> {
        if message.is_device_level() {
            return Err(TopicError::WrongLevel { message });
        }
        Ok(format!(
            "{NAMESPACE}/{}/{}/{}",
            self.group,
            message.token(),
            self.node
        ))
    }

    /// The topic for a device-level message. The device identifier is validated
    /// here, at the last moment before it becomes a topic level.
    pub fn device_topic(&self, message: MessageType, device: &str) -> Result<String, TopicError> {
        if !message.is_device_level() {
            return Err(TopicError::WrongLevel { message });
        }
        check_identifier(device, "device")?;
        Ok(format!(
            "{NAMESPACE}/{}/{}/{}/{}",
            self.group,
            message.token(),
            self.node,
            device
        ))
    }
}

fn check_identifier(value: &str, element: &'static str) -> Result<(), TopicError> {
    if value.is_empty() {
        return Err(TopicError::Empty { element });
    }
    for character in value.chars() {
        // TWO RULES FROM TWO SPECIFICATIONS, and conflating them left a hole
        // ([#34]).
        //
        // `/`, `+` and `#` are chapter 4's wildcard-and-separator rule
        // (`tck-id-topic-structure-namespace-valid-*`): an identifier carrying one
        // would not address the level it names.
        //
        // U+0000 is the other rule, and it arrives from further away. Chapter 1
        // defers the character SET to MQTT — *"Because the Group ID is used in MQTT
        // topic strings the Group ID MUST only contain characters allowed for MQTT
        // topics per the MQTT Specification"* (`tck-id-intro-group-id-chars`, and
        // the same clause for the edge node and the device) — and MQTT's UTF-8
        // Encoded String **MUST NOT** carry U+0000. Implementing only chapter 4's
        // rule satisfied a narrower set and let a null through, measured during the
        // story 4.3 audit.
        //
        // **Stopping at the MUST is deliberate.** MQTT also says a string SHOULD
        // NOT carry U+0001..U+001F and U+007F..U+009F; refusing those would be
        // stricter than either specification and would turn a legal — if
        // eccentric — identifier into a bridge that will not start. The unpaired
        // surrogates MQTT also forbids cannot exist in a Rust `char`.
        if matches!(character, '/' | '+' | '#' | '\0') {
            return Err(TopicError::IllegalCharacter { element, character });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> EdgeNode {
        EdgeNode::new("Site", "Bridge").expect("valid")
    }

    #[test]
    fn node_topics_follow_the_namespace_grammar() {
        assert_eq!(
            node().node_topic(MessageType::NBirth).unwrap(),
            "spBv1.0/Site/NBIRTH/Bridge"
        );
        assert_eq!(
            node().node_topic(MessageType::NData).unwrap(),
            "spBv1.0/Site/NDATA/Bridge"
        );
        assert_eq!(
            node().node_topic(MessageType::NDeath).unwrap(),
            "spBv1.0/Site/NDEATH/Bridge"
        );
    }

    #[test]
    fn device_topics_append_the_device_identifier() {
        assert_eq!(
            node().device_topic(MessageType::DData, "30000001").unwrap(),
            "spBv1.0/Site/DDATA/Bridge/30000001"
        );
        assert_eq!(
            node()
                .device_topic(MessageType::DBirth, "30000001")
                .unwrap(),
            "spBv1.0/Site/DBIRTH/Bridge/30000001"
        );
    }

    #[test]
    fn wildcards_and_separators_are_refused_in_every_element() {
        for bad in ["a/b", "a+b", "a#b", ""] {
            assert!(EdgeNode::new(bad, "n").is_err(), "group {bad:?}");
            assert!(EdgeNode::new("g", bad).is_err(), "node {bad:?}");
            assert!(
                node().device_topic(MessageType::DData, bad).is_err(),
                "device {bad:?}"
            );
        }
    }

    /// [#34] — **a null character is refused at every level, because chapter 1
    /// defers the character set to MQTT and MQTT forbids it.**
    ///
    /// `check_identifier` implemented chapter 4's wildcard-and-separator rule and
    /// nothing else, so a U+0000 reached the topic — measured during the story 4.3
    /// audit with a throwaway probe, not deduced.
    ///
    /// The last case is the discriminating one and must NOT be refused: a control
    /// character MQTT merely says SHOULD NOT carry. A guard that refuses it too is
    /// stricter than both specifications and turns a legal identifier into a bridge
    /// that will not start — which is why this test would catch a fix that reached
    /// too far, as well as one that does not reach far enough.
    ///
    /// FALSIFIED 2026-08-28 — mutation RUN: dropping `'\0'` from the match in
    /// `check_identifier` goes red on the group case with *"a null must never reach
    /// a topic"*.
    #[test]
    fn a_null_character_is_refused_and_a_control_character_is_not() {
        assert!(
            EdgeNode::new("gro\u{0}up", "node").is_err(),
            "a null must never reach a topic: MQTT's UTF-8 Encoded String MUST NOT \
             carry U+0000, and chapter 1 defers to MQTT for the character set"
        );
        assert!(
            EdgeNode::new("group", "no\u{0}de").is_err(),
            "the edge node id is under the same clause as the group id"
        );
        assert!(
            node()
                .device_topic(MessageType::DData, "dev\u{0}ice")
                .is_err(),
            "and so is the device id"
        );
        assert!(
            EdgeNode::new("gro\u{1}up", "node").is_ok(),
            "a control character is a SHOULD NOT, not a MUST NOT — refusing it would \
             be stricter than MQTT and than Sparkplug, and would refuse to start on \
             an identifier both specifications allow"
        );
    }

    #[test]
    fn the_offending_element_and_character_are_reported() {
        let err = EdgeNode::new("g", "no+de").unwrap_err();
        assert_eq!(
            err,
            TopicError::IllegalCharacter {
                element: "node",
                character: '+'
            }
        );
        assert!(format!("{err}").contains("node"));
    }

    #[test]
    fn a_message_type_cannot_address_the_wrong_level() {
        assert_eq!(
            node().node_topic(MessageType::DData),
            Err(TopicError::WrongLevel {
                message: MessageType::DData
            })
        );
        assert_eq!(
            node().device_topic(MessageType::NData, "d"),
            Err(TopicError::WrongLevel {
                message: MessageType::NData
            })
        );
    }

    #[test]
    fn device_level_is_distinguishable() {
        assert!(MessageType::DData.is_device_level());
        assert!(!MessageType::NData.is_device_level());
    }

    /// Story 4.6 — the topic an Edge Node MUST subscribe to.
    ///
    /// `tck-id-message-flow-edge-node-ncmd-subscribe`
    /// (`Sparkplug_5_Operational_Behavior.adoc:158-163`) fixes the shape:
    /// *"a topic of the form 'spBv1.0/group_id/NCMD/edge_node_id'"*. The QoS it
    /// also mandates is a transport concern and belongs to the caller.
    ///
    /// Falsified 2026-07-29: spelling the token `"NCOMMAND"` turns this red.
    #[test]
    fn the_ncmd_topic_follows_the_namespace_grammar() {
        assert_eq!(
            node().node_topic(MessageType::NCmd).unwrap(),
            "spBv1.0/Site/NCMD/Bridge"
        );
        assert_eq!(MessageType::NCmd.token(), "NCMD");
    }

    /// `is_device_level` is a `matches!`, so a new variant falls through to
    /// `false` and COMPILES SILENTLY — unlike `token()`, which is exhaustive and
    /// fails the build. `false` happens to be the right answer for NCMD, which
    /// is why it needs asserting rather than assuming: the next variant added
    /// must not be decided by a fall-through nobody reviewed.
    ///
    /// Falsified 2026-07-29: adding `NCmd` to the `matches!` arm turns both
    /// assertions red.
    #[test]
    fn ncmd_is_node_level_deliberately_and_not_by_fall_through() {
        assert!(!MessageType::NCmd.is_device_level());
        assert!(
            node().device_topic(MessageType::NCmd, "30000001").is_err(),
            "NCMD addresses the node, never a device — DCMD is a separate, \
             conditional clause this bridge does not implement"
        );
    }
}
