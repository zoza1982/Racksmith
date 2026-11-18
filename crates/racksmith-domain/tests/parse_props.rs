//! Property tests for the validated identifier types.
//!
//! For `MacAddress`, `Hostname` and `MachineId`: `Display` output parses back to an equal value,
//! serde JSON round-trips through the same text, and parsing arbitrary strings never panics.

use serde as _;
use thiserror as _;

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use proptest::prelude::*;
    use racksmith_domain::{Hostname, MacAddress, MachineId};

    /// One RFC 1123 label: 1 to 63 of `[a-z0-9-]`, not starting or ending with `-`.
    const LABEL: &str = "[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?";

    /// Valid hostnames of one to three labels (at most 191 bytes, under the 253-byte limit)
    /// whose last label is not all digits.
    fn hostnames() -> impl Strategy<Value = String> {
        proptest::collection::vec(LABEL, 1..=3)
            .prop_filter("the last label must not be all digits", |labels| {
                labels
                    .last()
                    .is_some_and(|label| !label.bytes().all(|byte| byte.is_ascii_digit()))
            })
            .prop_map(|labels| labels.join("."))
    }

    fn machine_ids() -> impl Strategy<Value = MachineId> {
        (1..=u64::MAX).prop_filter_map("non-zero", |n| NonZeroU64::new(n).map(MachineId::new))
    }

    /// Asserts that `value` survives a JSON round-trip and is encoded as its `Display` text.
    fn assert_json_round_trip<T>(value: &T) -> Result<(), TestCaseError>
    where
        T: serde::Serialize
            + serde::de::DeserializeOwned
            + std::fmt::Display
            + PartialEq
            + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialization is infallible");
        prop_assert_eq!(
            &json,
            &serde_json::to_string(&value.to_string()).expect("string")
        );
        let back: T = serde_json::from_str(&json).expect("valid JSON round-trips");
        prop_assert_eq!(&back, value);
        Ok(())
    }

    proptest! {
        #[test]
        fn mac_display_round_trips(octets in any::<[u8; 6]>()) {
            let mac = MacAddress::new(octets);
            let text = mac.to_string();
            prop_assert_eq!(text.parse::<MacAddress>(), Ok(mac));
            prop_assert_eq!(text.to_uppercase().replace(':', "-").parse::<MacAddress>(), Ok(mac));
            assert_json_round_trip(&mac)?;
        }

        #[test]
        fn hostname_display_round_trips(name in hostnames()) {
            let hostname: Hostname = name.parse().expect("generated names are valid");
            prop_assert_eq!(hostname.as_str(), name.as_str());
            prop_assert_eq!(hostname.to_string().parse::<Hostname>(), Ok(hostname.clone()));
            prop_assert_eq!(name.to_uppercase().parse::<Hostname>(), Ok(hostname.clone()));
            assert_json_round_trip(&hostname)?;
        }

        #[test]
        fn machine_id_display_round_trips(id in machine_ids()) {
            prop_assert_eq!(id.to_string().parse::<MachineId>(), Ok(id));
            assert_json_round_trip(&id)?;
        }

        #[test]
        fn arbitrary_strings_never_panic_and_accepted_ones_round_trip(input in any::<String>()) {
            if let Ok(mac) = input.parse::<MacAddress>() {
                prop_assert_eq!(mac.to_string().parse::<MacAddress>(), Ok(mac));
            }
            if let Ok(hostname) = input.parse::<Hostname>() {
                prop_assert_eq!(hostname.as_str(), input.to_ascii_lowercase());
            }
            if let Ok(id) = input.parse::<MachineId>() {
                prop_assert_eq!(id.to_string(), input.as_str());
            }
        }

        #[test]
        fn dotted_numbers_are_never_hostnames(octets in proptest::collection::vec(any::<u8>(), 1..=4)) {
            let text = octets.iter().map(u8::to_string).collect::<Vec<_>>().join(".");
            prop_assert!(text.parse::<Hostname>().is_err(), "{} parsed as a hostname", text);
        }

        #[test]
        fn near_miss_strings_never_panic(input in "[0-9a-fA-Fm:.-]{0,20}") {
            if let Ok(mac) = input.parse::<MacAddress>() {
                prop_assert_eq!(mac.to_string(), input.to_ascii_lowercase().replace('-', ":"));
            }
            if let Ok(id) = input.parse::<MachineId>() {
                prop_assert_eq!(id.to_string(), input.as_str());
            }
            if let Ok(hostname) = input.parse::<Hostname>() {
                prop_assert_eq!(hostname.as_str(), input.to_ascii_lowercase());
            }
        }
    }
}
