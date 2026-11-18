//! Fuzzes `Hostname::from_str` and `Hostname::try_from`.
//!
//! Properties checked for every input:
//! - parsing never panics, and both constructors agree;
//! - an accepted name is the input lowercased, with no other rewriting;
//! - an accepted name satisfies the RFC 1123 rules, re-checked here independently of the parser,
//!   including that its last label is not all digits (so it never looks like an IP address);
//! - the `Display` output of a parsed name parses back to the same name.

#![no_main]

use libfuzzer_sys::fuzz_target;
use racksmith_domain::Hostname;

/// Maximum length of a whole host name, in bytes.
const MAX_LEN: usize = 253;

/// Maximum length of a single dot-separated label, in bytes.
const MAX_LABEL_LEN: usize = 63;

fuzz_target!(|input: &str| {
    let parsed = input.parse::<Hostname>();
    assert_eq!(
        parsed,
        Hostname::try_from(input.to_owned()),
        "FromStr and TryFrom<String> disagree on {input:?}"
    );
    let Ok(hostname) = parsed else {
        return;
    };
    let name = hostname.as_str();
    assert_eq!(
        name,
        input.to_ascii_lowercase(),
        "accepted input {input:?} was rewritten beyond lowercasing"
    );
    assert_rfc1123(name);
    assert_eq!(
        hostname.to_string().parse(),
        Ok(hostname.clone()),
        "{hostname} does not round-trip"
    );
});

/// Asserts that `name` is a canonical lowercase RFC 1123 host name.
fn assert_rfc1123(name: &str) {
    assert!(
        (1..=MAX_LEN).contains(&name.len()),
        "hostname {name:?} has invalid length {}",
        name.len()
    );
    for label in name.split('.') {
        assert!(
            (1..=MAX_LABEL_LEN).contains(&label.len()),
            "label {label:?} of {name:?} has invalid length {}",
            label.len()
        );
        assert!(
            label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
            "label {label:?} of {name:?} contains a character outside [a-z0-9-]"
        );
        assert!(
            !label.starts_with('-') && !label.ends_with('-'),
            "label {label:?} of {name:?} starts or ends with `-`"
        );
    }
    let top_label = name.rsplit('.').next().unwrap_or(name);
    assert!(
        !top_label.bytes().all(|byte| byte.is_ascii_digit()),
        "hostname {name:?} has an all-digit last label"
    );
}
