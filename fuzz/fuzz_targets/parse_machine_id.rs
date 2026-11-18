//! Fuzzes `MachineId::from_str`.
//!
//! Properties checked for every input:
//! - parsing never panics;
//! - only the canonical form is accepted: a parsed id displays as exactly the input string, which
//!   also makes `Display` and `FromStr` round-trip.

#![no_main]

use libfuzzer_sys::fuzz_target;
use racksmith_domain::MachineId;

fuzz_target!(|input: &str| {
    let Ok(id) = input.parse::<MachineId>() else {
        return;
    };
    assert_eq!(
        id.to_string(),
        input,
        "accepted machine id {input:?} is not in canonical form"
    );
});
