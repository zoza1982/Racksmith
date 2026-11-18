//! Machine identifiers.

use std::fmt;
use std::num::NonZeroU64;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{hex, serde_str};

/// Prefix of the textual form of a [`MachineId`].
const PREFIX: &str = "m-";

/// Number of hexadecimal digits in the textual form of a [`MachineId`].
const DIGITS: usize = 16;

/// Opaque, non-zero identifier of a machine.
///
/// The textual form is `m-` followed by exactly 16 lowercase hexadecimal digits, for example
/// `m-000000000000002a`. Parsing accepts only that canonical form, so `Display` and `FromStr`
/// round-trip exactly. Serde uses the same textual form.
///
/// ```
/// use std::num::NonZeroU64;
///
/// use racksmith_domain::MachineId;
///
/// let id: MachineId = "m-000000000000002a".parse()?;
/// assert_eq!(id.get(), NonZeroU64::new(42).ok_or("zero")?);
/// assert_eq!(id.to_string(), "m-000000000000002a");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MachineId(NonZeroU64);

impl MachineId {
    /// Creates an identifier from its numeric value.
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// Returns the numeric value of the identifier.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

impl fmt::Display for MachineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{PREFIX}{:0width$x}", self.0.get(), width = DIGITS)
    }
}

/// Reason a string is not a valid [`MachineId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseMachineIdError {
    /// The string does not start with `m-`.
    #[error("machine id must start with `{PREFIX}`")]
    MissingPrefix,
    /// The part after the prefix is not exactly 16 bytes long.
    #[error("machine id must have {DIGITS} hex digits after `{PREFIX}`, found {len} bytes")]
    InvalidLength {
        /// Length in bytes of the part after the prefix.
        len: usize,
    },
    /// The part after the prefix contains a byte that is not a lowercase hexadecimal digit.
    #[error("machine id digits must be lowercase hexadecimal (0-9, a-f)")]
    InvalidDigit,
    /// The identifier is all zeroes, which is reserved.
    #[error("machine id must not be zero")]
    Zero,
}

impl FromStr for MachineId {
    type Err = ParseMachineIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let digits = s
            .strip_prefix(PREFIX)
            .ok_or(ParseMachineIdError::MissingPrefix)?;
        if digits.len() != DIGITS {
            return Err(ParseMachineIdError::InvalidLength { len: digits.len() });
        }
        let value = digits.bytes().try_fold(0_u64, |acc, byte| {
            hex::lower_digit(byte)
                .map(|digit| acc << 4 | u64::from(digit))
                .ok_or(ParseMachineIdError::InvalidDigit)
        })?;
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(ParseMachineIdError::Zero)
    }
}

impl Serialize for MachineId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde_str::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for MachineId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde_str::deserialize(deserializer, "a machine id like m-000000000000002a")
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::{MachineId, ParseMachineIdError};

    fn id(value: u64) -> MachineId {
        MachineId::new(NonZeroU64::new(value).expect("test ids are non-zero"))
    }

    #[test]
    fn displays_as_prefixed_zero_padded_lowercase_hex() {
        assert_eq!(id(1).to_string(), "m-0000000000000001");
        assert_eq!(id(0xdead_beef).to_string(), "m-00000000deadbeef");
        assert_eq!(id(u64::MAX).to_string(), "m-ffffffffffffffff");
    }

    #[test]
    fn parses_canonical_form() {
        assert_eq!("m-00000000deadbeef".parse(), Ok(id(0xdead_beef)));
        assert_eq!("m-ffffffffffffffff".parse(), Ok(id(u64::MAX)));
    }

    #[test]
    fn rejects_non_canonical_forms() {
        let cases = [
            ("00000000deadbeef", ParseMachineIdError::MissingPrefix),
            ("M-00000000deadbeef", ParseMachineIdError::MissingPrefix),
            ("m-deadbeef", ParseMachineIdError::InvalidLength { len: 8 }),
            ("m-", ParseMachineIdError::InvalidLength { len: 0 }),
            ("m-00000000DEADBEEF", ParseMachineIdError::InvalidDigit),
            ("m-+0000000deadbeef", ParseMachineIdError::InvalidDigit),
            ("m-0000000000000000", ParseMachineIdError::Zero),
        ];
        for (input, expected) in cases {
            assert_eq!(input.parse::<MachineId>(), Err(expected), "input {input:?}");
        }
    }

    #[test]
    fn serializes_as_display_string() {
        let json = serde_json::to_string(&id(42)).expect("serialization is infallible");
        assert_eq!(json, r#""m-000000000000002a""#);
        let back: MachineId = serde_json::from_str(&json).expect("round-trip");
        assert_eq!(back, id(42));
    }

    #[test]
    fn deserialization_reports_parse_errors() {
        let error = serde_json::from_str::<MachineId>(r#""m-0000000000000000""#)
            .expect_err("zero id must be rejected");
        assert!(error.to_string().contains("must not be zero"), "{error}");
    }
}
