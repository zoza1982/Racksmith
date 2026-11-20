//! The names of every test of the product crates, as `<package>::<libtest path>`, and which of
//! them are `#[ignore]`d.
//!
//! The listing needs no extra tool, so the check can never be skipped. `cargo test --no-run
//! --message-format=json` builds every test binary with the same arguments as the gate and
//! reports each one's package and executable; each binary then prints its tests with
//! `--list --format terse`. Asking cargo for the package of each binary, instead of reading the
//! `Running <binary>` lines of a test run, keeps integration tests of the same file name in
//! different packages (`tests/field_effects.rs`) apart. Doctests are not listed: a doctest
//! documents an item, it does not prove a requirement. Only product crates (`racksmith-*`) are
//! listed, so a guardrail's own unit test can never prove a product claim. Each binary also
//! lists its `#[ignore]`d tests (`--list --ignored`): the gate never runs them, so they cannot
//! prove anything either.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use cargo_metadata::Message;

use crate::spec::PRODUCT_PREFIX;
use crate::{process, workspace};

/// The tests of the product crates.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TestListing {
    /// Every test, ignored ones included.
    pub(crate) tests: BTreeSet<String>,
    /// The tests marked `#[ignore]`.
    pub(crate) ignored: BTreeSet<String>,
}

/// Lists every test of the product crates.
///
/// # Errors
///
/// Returns an error if the tests do not build, `cargo metadata` fails, or a test binary cannot
/// list its tests.
pub(crate) fn load(root: &Path) -> Result<TestListing> {
    let mut packages = workspace::load_package_names(root)?;
    packages.retain(|_, name| name.starts_with(PRODUCT_PREFIX));
    let messages = process::capture(process::cargo(root).args([
        "test",
        "--workspace",
        "--all-features",
        "--locked",
        "--no-run",
        "--message-format=json-render-diagnostics",
    ]))?;
    let mut listing = TestListing::default();
    for (package, executable) in test_binaries(&messages, &packages)? {
        let list = |extra: &[&str]| {
            process::capture(
                process::command(&executable)
                    .args(["--list", "--format", "terse"])
                    .args(extra)
                    .current_dir(root),
            )
        };
        listing.add(&package, &list(&[])?, &list(&["--ignored"])?);
    }
    Ok(listing)
}

impl TestListing {
    /// Adds the tests of one binary of `package`, from its `--list` output and its
    /// `--list --ignored` output.
    fn add(&mut self, package: &str, all: &str, ignored: &str) {
        self.tests.extend(listed_tests(package, all));
        self.ignored.extend(listed_tests(package, ignored));
    }
}

/// The package and executable of every test binary in cargo's JSON messages. `packages` maps
/// the IDs of the listed packages to names; artifacts of other packages are ignored.
///
/// # Errors
///
/// Returns an error if a message is not valid JSON.
fn test_binaries(
    messages: &str,
    packages: &BTreeMap<String, String>,
) -> Result<Vec<(String, PathBuf)>> {
    let mut binaries = Vec::new();
    for message in Message::parse_stream(messages.as_bytes()) {
        let Message::CompilerArtifact(artifact) =
            message.context("cargo printed a message that is not valid JSON")?
        else {
            continue;
        };
        let package = packages.get(&artifact.package_id.repr);
        if let (true, Some(executable), Some(package)) =
            (artifact.profile.test, artifact.executable, package)
        {
            binaries.push((package.clone(), executable.into_std_path_buf()));
        }
    }
    Ok(binaries)
}

/// The tests in the `--list --format terse` output of one binary of `package`.
fn listed_tests(package: &str, listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .filter(|name| !name.is_empty() && !name.contains(char::is_whitespace))
        .map(|name| format!("{package}::{name}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One cargo message per artifact, shortened from real `--message-format=json` output.
    fn artifact(package_id: &str, kind: &str, test: bool, executable: Option<&str>) -> String {
        let executable = executable.map_or_else(|| "null".to_owned(), |path| format!("\"{path}\""));
        format!(
            "{{\"reason\":\"compiler-artifact\",\"package_id\":\"{package_id}\",\
             \"manifest_path\":\"/repo/Cargo.toml\",\"target\":{{\"kind\":[\"{kind}\"],\
             \"crate_types\":[\"{kind}\"],\"name\":\"t\",\"src_path\":\"/repo/src/lib.rs\",\
             \"edition\":\"2024\",\"doc\":true,\"doctest\":true,\"test\":true}},\
             \"profile\":{{\"opt_level\":\"0\",\"debuginfo\":2,\"debug_assertions\":true,\
             \"overflow_checks\":true,\"test\":{test}}},\"features\":[],\"filenames\":[],\
             \"executable\":{executable},\"fresh\":true}}"
        )
    }

    #[test]
    fn finds_the_test_binaries_of_workspace_packages() {
        let api = "path+file:///repo/crates/racksmith-api#0.0.0";
        let cli = "path+file:///repo/crates/racksmith-cli#0.0.0";
        let messages = [
            artifact("registry+https://x#serde@1.0.0", "lib", false, None),
            artifact(api, "lib", false, None),
            artifact(api, "lib", true, Some("/t/deps/racksmith_api-1")),
            artifact(api, "test", true, Some("/t/deps/field_effects-2")),
            artifact(cli, "test", true, Some("/t/deps/field_effects-3")),
            artifact(cli, "bin", false, Some("/t/debug/racksmith")),
            "{\"reason\":\"build-finished\",\"success\":true}".to_owned(),
        ]
        .join("\n");
        let packages: BTreeMap<String, String> = [
            (api.to_owned(), "racksmith-api".to_owned()),
            (cli.to_owned(), "racksmith-cli".to_owned()),
        ]
        .into();
        let binaries = test_binaries(&messages, &packages).unwrap();
        assert_eq!(
            binaries,
            [
                (
                    "racksmith-api".to_owned(),
                    PathBuf::from("/t/deps/racksmith_api-1")
                ),
                (
                    "racksmith-api".to_owned(),
                    PathBuf::from("/t/deps/field_effects-2")
                ),
                (
                    "racksmith-cli".to_owned(),
                    PathBuf::from("/t/deps/field_effects-3")
                ),
            ],
            "the same file name in two packages stays apart"
        );
    }

    #[test]
    fn records_which_listed_tests_are_ignored() {
        let mut listing = TestListing::default();
        listing.add(
            "racksmith-api",
            "runs: test\nskipped: test\n2 tests, 0 benchmarks\n",
            "skipped: test\n1 test, 0 benchmarks\n",
        );
        assert_eq!(
            listing,
            TestListing {
                tests: ["racksmith-api::runs", "racksmith-api::skipped"]
                    .map(str::to_owned)
                    .into(),
                ignored: ["racksmith-api::skipped".to_owned()].into(),
            }
        );
    }

    #[test]
    fn lists_tests_but_not_benchmarks_or_summaries() {
        let listing = "machine::tests::new_machine: test\nconformance: test\nspeed: bench\n\
                       \n2 tests, 1 benchmark\n";
        assert_eq!(
            listed_tests("racksmith-domain", listing),
            [
                "racksmith-domain::machine::tests::new_machine",
                "racksmith-domain::conformance",
            ]
        );
    }
}
