//! Sparkplug B metric data types and their wire codes.
//!
//! The `datatype` field of a metric (and the `type` field of a property value)
//! is an integer code defined by the Sparkplug B specification. Encoding it as
//! an enum keeps the codes in one place: a metric can never be tagged with a
//! type that does not exist, and the value/datatype pairing is checked by
//! construction (see [`crate::MetricValue`]).

/// A Sparkplug B data type, with its wire code.
///
/// The variants mirror the specification's numbering exactly; the `u32`
/// discriminants ARE the wire values, so [`DataType::code`] is a cast, not a
/// lookup table that could drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
#[non_exhaustive]
pub enum DataType {
    /// 8-bit signed integer.
    Int8 = 1,
    /// 16-bit signed integer.
    Int16 = 2,
    /// 32-bit signed integer.
    Int32 = 3,
    /// 64-bit signed integer.
    Int64 = 4,
    /// 8-bit unsigned integer.
    UInt8 = 5,
    /// 16-bit unsigned integer.
    UInt16 = 6,
    /// 32-bit unsigned integer.
    UInt32 = 7,
    /// 64-bit unsigned integer.
    UInt64 = 8,
    /// 32-bit IEEE-754 float. Lossy for counters — see [`DataType::Double`].
    Float = 9,
    /// 64-bit IEEE-754 float: the only float type that preserves the full
    /// resolution of a long-running cumulative counter.
    Double = 10,
    /// Boolean.
    Boolean = 11,
    /// UTF-8 string.
    String = 12,
    /// Milliseconds since the UNIX epoch.
    DateTime = 13,
    /// Multi-line / large text.
    Text = 14,
    /// UUID as a string.
    Uuid = 15,
    /// Byte array.
    Bytes = 17,
}

impl DataType {
    /// The specification's integer code for this type.
    pub const fn code(self) -> u32 {
        self as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_the_specification_numbering() {
        assert_eq!(DataType::Int8.code(), 1);
        assert_eq!(DataType::Int64.code(), 4);
        assert_eq!(DataType::UInt64.code(), 8);
        assert_eq!(DataType::Float.code(), 9);
        assert_eq!(DataType::Double.code(), 10);
        assert_eq!(DataType::Boolean.code(), 11);
        assert_eq!(DataType::String.code(), 12);
        assert_eq!(DataType::DateTime.code(), 13);
        assert_eq!(DataType::Bytes.code(), 17);
    }
}
