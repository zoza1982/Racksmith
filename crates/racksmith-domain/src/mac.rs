//! IEEE 802 MAC-48 addresses.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{hex, serde_str};

/// Length in bytes of the textual form `aa:bb:cc:dd:ee:ff`.
const TEXT_LEN: usize = 17;

/// A 48-bit MAC address, typically of a machine's primary boot interface.
///
/// Parsing accepts six two-digit hexadecimal groups separated consistently by `:` or `-`, in any
/// letter case. `Display` always produces the lowercase colon form, so parsing the displayed
/// text yields an equal address. Serde uses the same textual form.
///
/// Every address is accepted; use [`MacAddress::is_multicast`] and
/// [`MacAddress::is_locally_administered`] to apply policy.
///
/// ```
/// use racksmith_domain::MacAddress;
///
/// let mac: MacAddress = "52-54-00-AB-CD-EF".parse()?;
/// assert_eq!(mac.to_string(), "52:54:00:ab:cd:ef");
/// assert!(mac.is_locally_administered());
/// # Ok::<(), racksmith_domain::ParseMacAddressError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MacAddress([u8; 6]);

impl MacAddress {
    /// Creates an address from its six octets in transmission order.
    #[must_use]
    pub const fn new(octets: [u8; 6]) -> Self {
        Self(octets)
    }

    /// Returns the six octets in transmission order.
    #[must_use]
    pub const fn octets(self) -> [u8; 6] {
        self.0
    }

    /// Returns `true` if the I/G bit is set, i.e. the address names a group of interfaces.
    #[must_use]
    pub const fn is_multicast(self) -> bool {
        self.0[0] & 0b01 != 0
    }

    /// Returns `true` if the U/L bit is set, i.e. the address was not assigned by the vendor.
    ///
    /// Virtual machines and some NICs in their default configuration use such addresses.
    #[must_use]
    pub const fn is_locally_administered(self) -> bool {
        self.0[0] & 0b10 != 0
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, octet) in self.0.iter().enumerate() {
            if position > 0 {
                f.write_str(":")?;
            }
            write!(f, "{octet:02x}")?;
        }
        Ok(())
    }
}

/// Reason a string is not a valid [`MacAddress`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseMacAddressError {
    /// The string is not exactly 17 bytes long.
    #[error("MAC address must be 17 characters like aa:bb:cc:dd:ee:ff, found {len} bytes")]
    Length {
        /// Length of the input in bytes.
        len: usize,
    },
    /// A separator is neither `:` nor `-`, or the separators are mixed.
    #[error("MAC address groups must be separated consistently by `:` or `-`")]
    Separator,
    /// A group is not two hexadecimal digits.
    #[error("MAC address group {position} is not two hexadecimal digits")]
    Octet {
        /// Zero-based index of the offending group.
        position: usize,
    },
}

impl FromStr for MacAddress {
    type Err = ParseMacAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != TEXT_LEN {
            return Err(ParseMacAddressError::Length { len: bytes.len() });
        }
        let Some(&separator @ (b':' | b'-')) = bytes.get(2) else {
            return Err(ParseMacAddressError::Separator);
        };
        let mut octets = [0_u8; 6];
        // 17 bytes split into chunks of 3 yields exactly six groups: "xx:" five times, then "xx".
        for (position, (octet, group)) in octets.iter_mut().zip(bytes.chunks(3)).enumerate() {
            *octet = parse_group(group, separator, position)?;
        }
        Ok(Self(octets))
    }
}

/// Parses one `xx` or `xx<separator>` group of the textual form.
fn parse_group(group: &[u8], separator: u8, position: usize) -> Result<u8, ParseMacAddressError> {
    let (high, low) = match *group {
        [high, low] => (high, low),
        [high, low, found] if found == separator => (high, low),
        [_, _, _] => return Err(ParseMacAddressError::Separator),
        _ => return Err(ParseMacAddressError::Octet { position }),
    };
    let digit = |byte: u8| hex::lower_digit(byte.to_ascii_lowercase());
    match (digit(high), digit(low)) {
        (Some(high), Some(low)) => Ok(high << 4 | low),
        _ => Err(ParseMacAddressError::Octet { position }),
    }
}

impl Serialize for MacAddress {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde_str::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for MacAddress {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde_str::deserialize(deserializer, "a MAC address like aa:bb:cc:dd:ee:ff")
    }
}

#[cfg(test)]
mod tests {
    use super::{MacAddress, ParseMacAddressError};

    const OCTETS: [u8; 6] = [0x52, 0x54, 0x00, 0xab, 0xcd, 0xef];

    #[test]
    fn parses_colon_and_dash_forms_in_any_case() {
        for input in [
            "52:54:00:ab:cd:ef",
            "52:54:00:AB:CD:EF",
            "52-54-00-ab-cd-ef",
            "52-54-00-Ab-cD-eF",
        ] {
            assert_eq!(
                input.parse(),
                Ok(MacAddress::new(OCTETS)),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn displays_lowercase_colon_form() {
        assert_eq!(MacAddress::new(OCTETS).to_string(), "52:54:00:ab:cd:ef");
    }

    #[test]
    fn rejects_malformed_input() {
        let cases = [
            ("", ParseMacAddressError::Length { len: 0 }),
            ("52:54:00:ab:cd", ParseMacAddressError::Length { len: 14 }),
            ("52:54-00:ab:cd:ef", ParseMacAddressError::Separator),
            ("52.54.00.ab.cd.ef", ParseMacAddressError::Separator),
            ("5:254:00:ab:cd:ef", ParseMacAddressError::Separator),
            (
                "52:54:0g:ab:cd:ef",
                ParseMacAddressError::Octet { position: 2 },
            ),
            (
                "52:54:00:ab:cd:+f",
                ParseMacAddressError::Octet { position: 5 },
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                input.parse::<MacAddress>(),
                Err(expected),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn classifies_address_bits() {
        let universal_unicast = MacAddress::new([0x00, 0x1b, 0x21, 0, 0, 1]);
        assert!(!universal_unicast.is_multicast(), "I/G bit clear");
        assert!(
            !universal_unicast.is_locally_administered(),
            "U/L bit clear"
        );

        let broadcast = MacAddress::new([0xff; 6]);
        assert!(broadcast.is_multicast(), "I/G bit set");
        assert!(broadcast.is_locally_administered(), "U/L bit set");
    }

    #[test]
    fn serializes_as_display_string() {
        let mac = MacAddress::new(OCTETS);
        let json = serde_json::to_string(&mac).expect("serialization is infallible");
        assert_eq!(json, r#""52:54:00:ab:cd:ef""#);
        assert_eq!(
            serde_json::from_str::<MacAddress>(&json).expect("round-trip"),
            mac
        );
    }
}
