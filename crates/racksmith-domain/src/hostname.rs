//! DNS host names.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::serde_str;

/// Maximum length of a whole host name, in bytes.
const MAX_LEN: usize = 253;

/// Maximum length of a single dot-separated label, in bytes.
const MAX_LABEL_LEN: usize = 63;

/// A host name following RFC 1123: dot-separated labels of ASCII letters, digits and hyphens.
///
/// Input is lowercased, so a `Hostname` is always in canonical lowercase form. The whole name is
/// 1 to 253 bytes long; each label is 1 to 63 bytes long and neither starts nor ends with `-`.
/// The last label is not all digits (RFC 1123 section 2.1, RFC 3696 section 2), so a host name
/// can never be mistaken for an IPv4 address such as `192.168.1.1`. A trailing dot (an absolute
/// name) is rejected. Serde uses the textual form.
///
/// ```
/// use racksmith_domain::Hostname;
///
/// let name: Hostname = "Node-01.Rack-A".parse()?;
/// assert_eq!(name.as_str(), "node-01.rack-a");
/// assert!("-node".parse::<Hostname>().is_err());
/// assert!("10.0.0.1".parse::<Hostname>().is_err());
/// # Ok::<(), racksmith_domain::ParseHostnameError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hostname(String);

impl Hostname {
    /// Returns the host name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reason a string is not a valid [`Hostname`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseHostnameError {
    /// The string is empty.
    #[error("hostname must not be empty")]
    Empty,
    /// The string is longer than 253 bytes.
    #[error("hostname must be at most {MAX_LEN} bytes, found {len}")]
    TooLong {
        /// Length of the input in bytes.
        len: usize,
    },
    /// A label is empty, as in `a..b`, `.a` or `a.`.
    #[error("hostname labels must not be empty")]
    EmptyLabel,
    /// A label is longer than 63 bytes.
    #[error("hostname labels must be at most {MAX_LABEL_LEN} bytes, found {len}")]
    LabelTooLong {
        /// Length of the offending label in bytes.
        len: usize,
    },
    /// A character other than an ASCII letter, digit, `-` or `.` was found.
    #[error("hostname must contain only a-z, 0-9, `-` and `.`, found {character:?}")]
    InvalidCharacter {
        /// The first offending character.
        character: char,
    },
    /// A label starts or ends with `-`.
    #[error("hostname labels must not start or end with `-`")]
    HyphenAtLabelEdge,
    /// The last label is all digits, so the name could be read as an IP address.
    #[error(
        "the last hostname label must not be all digits, so it cannot be read as an IP address"
    )]
    NumericTopLabel,
}

impl TryFrom<String> for Hostname {
    type Error = ParseHostnameError;

    /// Validates `value`, lowercasing it in place so no new allocation is made.
    fn try_from(mut value: String) -> Result<Self, Self::Error> {
        value.make_ascii_lowercase();
        validate(&value)?;
        Ok(Self(value))
    }
}

impl FromStr for Hostname {
    type Err = ParseHostnameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s.to_owned())
    }
}

/// Checks a lowercased candidate against the RFC 1123 rules described on [`Hostname`].
fn validate(name: &str) -> Result<(), ParseHostnameError> {
    if name.is_empty() {
        return Err(ParseHostnameError::Empty);
    }
    if name.len() > MAX_LEN {
        return Err(ParseHostnameError::TooLong { len: name.len() });
    }
    name.split('.').try_for_each(validate_label)?;
    let top_label = name.rsplit('.').next().unwrap_or(name);
    if top_label.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ParseHostnameError::NumericTopLabel);
    }
    Ok(())
}

/// Checks one lowercased label of a host name.
fn validate_label(label: &str) -> Result<(), ParseHostnameError> {
    if label.is_empty() {
        return Err(ParseHostnameError::EmptyLabel);
    }
    if label.len() > MAX_LABEL_LEN {
        return Err(ParseHostnameError::LabelTooLong { len: label.len() });
    }
    if let Some(character) = label
        .chars()
        .find(|&c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
    {
        return Err(ParseHostnameError::InvalidCharacter { character });
    }
    if label.starts_with('-') || label.ends_with('-') {
        return Err(ParseHostnameError::HyphenAtLabelEdge);
    }
    Ok(())
}

impl fmt::Display for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Hostname {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for Hostname {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde_str::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for Hostname {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde_str::deserialize(deserializer, "an RFC 1123 hostname like node-01.rack-a")
    }
}

#[cfg(test)]
mod tests {
    use super::{Hostname, MAX_LABEL_LEN, MAX_LEN, ParseHostnameError};

    #[test]
    fn accepts_valid_names_and_lowercases_them() {
        let cases = [
            ("a", "a"),
            ("node-01", "node-01"),
            ("Node-01.Rack-A.DC1", "node-01.rack-a.dc1"),
            ("0xdeadbeef", "0xdeadbeef"),
            ("10.rack-1", "10.rack-1"),
            ("node.1a", "node.1a"),
        ];
        for (input, canonical) in cases {
            let name: Hostname = input.parse().expect("valid hostname");
            assert_eq!(name.as_str(), canonical, "input {input:?}");
        }
    }

    #[test]
    fn accepts_names_at_the_length_limits() {
        let label = "a".repeat(MAX_LABEL_LEN);
        assert!(label.parse::<Hostname>().is_ok(), "63-byte label");

        // Four 62-byte labels joined by dots: 4 * 62 + 3 = 251 bytes; add ".b" for 253.
        let longest = format!("{0}.{0}.{0}.{0}.b", "a".repeat(62));
        assert_eq!(longest.len(), MAX_LEN, "test fixture length");
        assert!(longest.parse::<Hostname>().is_ok(), "253-byte name");
    }

    #[test]
    fn rejects_invalid_names() {
        let too_long = format!("{0}.{0}.{0}.{0}.bc", "a".repeat(62));
        let long_label = "a".repeat(MAX_LABEL_LEN + 1);
        let cases = [
            ("", ParseHostnameError::Empty),
            (too_long.as_str(), ParseHostnameError::TooLong { len: 254 }),
            ("a..b", ParseHostnameError::EmptyLabel),
            (".a", ParseHostnameError::EmptyLabel),
            ("a.", ParseHostnameError::EmptyLabel),
            (
                long_label.as_str(),
                ParseHostnameError::LabelTooLong { len: 64 },
            ),
            (
                "node_01",
                ParseHostnameError::InvalidCharacter { character: '_' },
            ),
            (
                "nöde",
                ParseHostnameError::InvalidCharacter { character: 'ö' },
            ),
            ("-node", ParseHostnameError::HyphenAtLabelEdge),
            ("rack.node-", ParseHostnameError::HyphenAtLabelEdge),
            ("1", ParseHostnameError::NumericTopLabel),
            ("192.168.1.1", ParseHostnameError::NumericTopLabel),
            ("node-01.42", ParseHostnameError::NumericTopLabel),
        ];
        for (input, expected) in cases {
            assert_eq!(input.parse::<Hostname>(), Err(expected), "input {input:?}");
        }
    }

    #[test]
    fn try_from_string_matches_from_str() {
        let owned = Hostname::try_from(String::from("Rack-A")).expect("valid hostname");
        assert_eq!(Ok(owned), "rack-a".parse::<Hostname>());
    }

    #[test]
    fn serializes_as_display_string() {
        let name: Hostname = "node-01".parse().expect("valid hostname");
        let json = serde_json::to_string(&name).expect("serialization is infallible");
        assert_eq!(json, r#""node-01""#);
        assert_eq!(
            serde_json::from_str::<Hostname>(&json).expect("round-trip"),
            name
        );
        assert!(
            serde_json::from_str::<Hostname>(r#""bad_name""#).is_err(),
            "deserialization validates"
        );
    }
}
