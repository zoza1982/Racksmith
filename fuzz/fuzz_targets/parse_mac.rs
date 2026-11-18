//! Fuzzes `MacAddress::from_str`.
//!
//! Properties checked for every input:
//! - parsing never panics;
//! - an accepted input is exactly the canonical form up to letter case and a `-` separator, so the
//!   parser accepts nothing beyond the documented syntax;
//! - the `Display` output of a parsed address parses back to the same address.

#![no_main]

use libfuzzer_sys::fuzz_target;
use racksmith_domain::MacAddress;

fuzz_target!(|input: &str| {
    let Ok(mac) = input.parse::<MacAddress>() else {
        return;
    };
    let canonical = mac.to_string();
    assert_eq!(
        canonical,
        input.to_ascii_lowercase().replace('-', ":"),
        "accepted input {input:?} is not a separator/case variant of its canonical form"
    );
    assert_eq!(
        canonical.parse(),
        Ok(mac),
        "canonical form {canonical:?} does not round-trip"
    );
});
