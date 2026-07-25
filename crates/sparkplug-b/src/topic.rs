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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Node birth certificate.
    NBirth,
    /// Node data.
    NData,
    /// Node death certificate.
    NDeath,
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
        if matches!(character, '/' | '+' | '#') {
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
}
