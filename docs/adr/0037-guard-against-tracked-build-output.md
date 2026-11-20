# 0037. Guard against tracked build output and large files

- Status: Proposed
- Date: 2026-11-20

## Context

Another project accidentally committed 3.26 GB of build output. Its `.gitignore` listed
`/target/`, which a leading slash anchors to the repository root, so a nested build directory
(a crate built on its own, or a tool that put its `target/` below the root) was not ignored, and
one `git add .` staged all of it. Once such a commit is pushed, every clone downloads it and only
a history rewrite removes it.

This repository had the same anchored pattern, plus a separate `/fuzz/target/` line for the one
nested build directory it knew about. Nothing checked what the git index actually contained: the
gate examines sources and manifests, not tracked files, so build output or a large binary added
by mistake, by a forced add or by a regressed ignore pattern would pass every check.

The options were:

1. **Fix the ignore pattern only.** This closes today's gap, but nothing stops a later edit from
   anchoring it again, and `git add -f` bypasses ignores entirely.
2. **Rely on server-side limits.** GitHub rejects files over 100 MB, far above what should ever
   be committed here, and only after the commit exists locally.
3. **Check the index in the gate and before each commit.** The gate fails on what is already
   tracked, and a hook stops a large file before it enters history at all.

## Decision

We will do all three of the cheap things: the ignore pattern, a rule in the gate and a hook.

- `.gitignore` ignores the unanchored `target/`, which matches a build directory at any depth,
  and drops the now-redundant `/fuzz/target/` line.
- `cargo xtask arch` rule 9 (`xtask/src/tracked_files.rs`) lists the index with
  `git ls-files -z` and fails on any tracked path with a `target` path component, reported once
  per build directory, and on any tracked file larger than `MAX_TRACKED_FILE_BYTES`, 5 MiB. Large
  binaries (disk images, firmware, archives) belong in release assets or Git LFS. The messages
  give the fix: `git rm -r --cached <dir>` and the `.gitignore` line. The rule counts the files it
  examined, prints the count in the summary line and fails if it is zero. Without `git` or a
  checkout it fails with an error rather than skipping, because CI always has a checkout and a
  rule that can skip there can never fail.
- `.githooks/pre-commit` also rejects staged files over the same limit, measured as the staged
  blob (`git cat-file -s :<path>`), so a large file is stopped before it reaches history.

## Consequences

- A nested `target/` can no longer be committed by accident, and one forced in fails the gate.
- A file that legitimately needs more than 5 MiB in the repository needs an ADR and a change to
  `MAX_TRACKED_FILE_BYTES` (and the matching limit in the hook), reviewed like any guardrail
  change. Today the largest tracked file is the system design, at about 270 KB.
- A source directory named `target` (as opposed to a file such as `target.rs`) is rejected too.
  None exists; a module that needs the name can use `targets` or a more specific one.
- The rule depends on `git` at gate time, which every development checkout and CI job already
  has. The hook is a convenience that `--no-verify` bypasses; the gate is the enforcement.
