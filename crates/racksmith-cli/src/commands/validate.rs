//! `racksmith validate`: check a MAC address or hostname.
//!
//! A valid value is printed in canonical form on stdout; an invalid one is explained on stderr
//! and the command exits with a failure status.

use std::fmt::Display;
use std::io::{self, Write};
use std::process::ExitCode;
use std::str::FromStr;

use racksmith_domain::{Hostname, MacAddress};

use crate::cli::ValidateTarget;

/// Validates `target`, writing the canonical form to `out` or the reason it is invalid to `err`.
///
/// # Errors
///
/// Returns an error only if writing to `out` or `err` fails; an invalid value is reported
/// through the returned [`ExitCode`].
pub(crate) fn run(
    target: &ValidateTarget,
    out: &mut impl Write,
    err: &mut impl Write,
) -> io::Result<ExitCode> {
    match target {
        ValidateTarget::Mac { value } => check::<MacAddress>(value, "MAC address", out, err),
        ValidateTarget::Hostname { value } => check::<Hostname>(value, "hostname", out, err),
    }
}

/// Parses `value` as `T` and reports the outcome.
fn check<T>(
    value: &str,
    kind: &str,
    out: &mut impl Write,
    err: &mut impl Write,
) -> io::Result<ExitCode>
where
    T: FromStr + Display,
    T::Err: Display,
{
    match value.parse::<T>() {
        Ok(parsed) => {
            writeln!(out, "{parsed}")?;
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            writeln!(err, "invalid {kind} {value:?}: {error}")?;
            Ok(ExitCode::FAILURE)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `target` and returns the exit code, stdout and stderr.
    fn validate(target: &ValidateTarget) -> (ExitCode, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(target, &mut out, &mut err).unwrap();
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn mac(value: &str) -> ValidateTarget {
        ValidateTarget::Mac {
            value: value.to_owned(),
        }
    }

    fn hostname(value: &str) -> ValidateTarget {
        ValidateTarget::Hostname {
            value: value.to_owned(),
        }
    }

    #[test]
    fn valid_mac_prints_canonical_form() {
        let (code, out, err) = validate(&mac("52-54-00-AB-CD-EF"));
        assert_eq!(code, ExitCode::SUCCESS, "exit code");
        assert_eq!(out, "52:54:00:ab:cd:ef\n", "stdout");
        assert_eq!(err, "", "stderr");
    }

    #[test]
    fn invalid_mac_fails_with_reason_on_stderr() {
        let (code, out, err) = validate(&mac("52:54:00"));
        assert_eq!(code, ExitCode::FAILURE, "exit code");
        assert_eq!(out, "", "stdout");
        assert!(
            err.starts_with("invalid MAC address \"52:54:00\": "),
            "stderr: {err}"
        );
    }

    #[test]
    fn valid_hostname_prints_lowercase_form() {
        let (code, out, _) = validate(&hostname("Node-01.Rack-A"));
        assert_eq!(code, ExitCode::SUCCESS, "exit code");
        assert_eq!(out, "node-01.rack-a\n", "stdout");
    }

    #[test]
    fn invalid_hostname_fails_with_reason_on_stderr() {
        let (code, out, err) = validate(&hostname("node_01"));
        assert_eq!(code, ExitCode::FAILURE, "exit code");
        assert_eq!(out, "", "stdout");
        assert!(
            err.starts_with("invalid hostname \"node_01\": "),
            "stderr: {err}"
        );
    }
}
