//! Rule 9 of `cargo xtask arch`: no build output or large files in the git index.
//!
//! An ignore pattern anchored to the root (`/target/`) leaves a nested build directory such as
//! `crates/x/target/` unignored, and one `git add .` then commits gigabytes. `.gitignore` uses the
//! unanchored `target/`; this rule is the backstop for a pattern that regresses or a forced add.
//! See ADR 0037.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context as _, Result};

use crate::process;
use crate::violation::Violation;

/// The largest file the repository tracks, in bytes. Large binaries (images, disk images,
/// firmware, archives) belong in release assets or Git LFS, not the repository: every clone
/// downloads every version of them forever, and history cannot shed them without a rewrite. A
/// file that legitimately needs more needs an ADR that raises this limit.
pub(crate) const MAX_TRACKED_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// The path component that marks Cargo build output.
const BUILD_OUTPUT_DIR: &str = "target";

/// A file in the git index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrackedFile {
    /// The path relative to the workspace root, with `/` separators, as git prints it.
    pub(crate) path: String,
    /// The size in the working tree, or `None` if the file is deleted from it (its removal is
    /// pending, so there is nothing on disk to measure).
    pub(crate) bytes: Option<u64>,
}

/// The tracked files of the checkout at `root`, with their sizes in the working tree.
///
/// # Errors
///
/// Returns an error if `git` cannot run, `root` is not a git checkout, or a tracked file cannot
/// be inspected. A missing checkout is an error rather than a skip: CI always has one, so a rule
/// that skipped silently could never fail there.
pub(crate) fn load(root: &Path) -> Result<Vec<TrackedFile>> {
    let listing = process::capture(
        process::command("git")
            .args(["ls-files", "-z"])
            .current_dir(root),
    )
    .with_context(|| {
        format!(
            "architecture rule 9 lists tracked files with git; is git installed and {} a git \
             checkout?",
            root.display()
        )
    })?;
    listing
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(|path| {
            let bytes = match fs::symlink_metadata(root.join(path)) {
                Ok(metadata) => Some(metadata.len()),
                Err(error) if error.kind() == ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(error).with_context(|| format!("failed to inspect {path}"));
                }
            };
            Ok(TrackedFile {
                path: path.to_owned(),
                bytes,
            })
        })
        .collect()
}

/// The violations of rule 9 among `files`: one per tracked build-output directory, and one per
/// file larger than [`MAX_TRACKED_FILE_BYTES`].
pub(crate) fn check(files: &[TrackedFile]) -> Vec<Violation> {
    let mut build_output: BTreeMap<&str, usize> = BTreeMap::new();
    let mut violations = Vec::new();
    for file in files {
        if let Some(dir) = build_output_dir(&file.path) {
            *build_output.entry(dir).or_default() += 1;
        }
        if let Some(bytes) = file.bytes.filter(|&bytes| bytes > MAX_TRACKED_FILE_BYTES) {
            violations.push(Violation::TrackedFileTooLarge {
                path: file.path.clone(),
                bytes,
                limit: MAX_TRACKED_FILE_BYTES,
            });
        }
    }
    violations.extend(
        build_output
            .into_iter()
            .map(|(dir, count)| Violation::TrackedBuildOutput {
                dir: dir.to_owned(),
                files: count,
            }),
    );
    violations
}

/// The prefix of `path` up to and including its first `target` component, or `None` if no
/// component is exactly `target`.
fn build_output_dir(path: &str) -> Option<&str> {
    let mut end = 0;
    for component in path.split('/') {
        end += component.len();
        if component == BUILD_OUTPUT_DIR {
            return path.get(..end);
        }
        end += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, bytes: u64) -> TrackedFile {
        TrackedFile {
            path: path.to_owned(),
            bytes: Some(bytes),
        }
    }

    #[test]
    fn build_output_anywhere_in_the_tree_is_flagged_once_per_directory() {
        let files = [
            file("target/debug/racksmith", 10),
            file("crates/x/target/debug/foo", 10),
            file("crates/x/target/debug/bar", 10),
            file("fuzz/target/release/fuzz_parse", 10),
        ];
        assert_eq!(
            check(&files),
            [
                Violation::TrackedBuildOutput {
                    dir: "crates/x/target".into(),
                    files: 2
                },
                Violation::TrackedBuildOutput {
                    dir: "fuzz/target".into(),
                    files: 1
                },
                Violation::TrackedBuildOutput {
                    dir: "target".into(),
                    files: 1
                },
            ]
        );
    }

    #[test]
    fn only_a_whole_target_component_is_build_output() {
        let files = [
            file("crates/x/src/target.rs", 10),
            file("docs/targets/linux.md", 10),
            file("my-target/readme.md", 10),
            file("crates/x/src/target", 10),
        ];
        assert_eq!(
            check(&files),
            [Violation::TrackedBuildOutput {
                dir: "crates/x/src/target".into(),
                files: 1
            }],
            "a file named exactly `target` is flagged, `target.rs` and `targets/` are not"
        );
        assert_eq!(check(&files[..3]), Vec::<Violation>::new());
    }

    #[test]
    fn files_over_the_size_limit_are_flagged() {
        let six_mb = 6 * 1024 * 1024;
        let files = [
            file("images/disk.img", six_mb),
            file("docs/at-limit.pdf", MAX_TRACKED_FILE_BYTES),
            file("docs/just-over.pdf", MAX_TRACKED_FILE_BYTES + 1),
            file("README.md", 1_000),
        ];
        assert_eq!(
            check(&files),
            [
                Violation::TrackedFileTooLarge {
                    path: "images/disk.img".into(),
                    bytes: six_mb,
                    limit: MAX_TRACKED_FILE_BYTES
                },
                Violation::TrackedFileTooLarge {
                    path: "docs/just-over.pdf".into(),
                    bytes: MAX_TRACKED_FILE_BYTES + 1,
                    limit: MAX_TRACKED_FILE_BYTES
                },
            ],
            "exactly 5 MiB is allowed"
        );
    }

    #[test]
    fn the_limit_is_five_mebibytes() {
        assert_eq!(MAX_TRACKED_FILE_BYTES, 5_242_880);
    }

    #[test]
    fn a_file_deleted_from_the_working_tree_is_still_checked_for_build_output() {
        let deleted = TrackedFile {
            path: "crates/x/target/big".into(),
            bytes: None,
        };
        assert_eq!(
            check(&[deleted]),
            [Violation::TrackedBuildOutput {
                dir: "crates/x/target".into(),
                files: 1
            }]
        );
    }

    #[test]
    fn this_checkout_lists_tracked_files_with_their_sizes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let files = load(root).unwrap();
        assert!(
            files
                .iter()
                .any(|file| file.path == "xtask/src/arch.rs" && file.bytes > Some(0)),
            "git ls-files lists xtask's sources, sized from the working tree"
        );
    }

    #[test]
    fn a_directory_that_is_not_a_checkout_is_an_error() {
        let dir = std::env::temp_dir().join(format!("xtask-no-git-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let result = load(&dir);
        fs::remove_dir_all(&dir).unwrap();
        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("a git checkout"), "{error}");
    }
}
