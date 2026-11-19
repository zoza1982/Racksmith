# 0034. Harden the guardrails with lessons from a retrofit

- Status: Proposed
- Date: 2026-11-18

## Context

`xtask/` is a code-owned guardrail ([ADR 0003](0003-trust-pyramid-guardrails.md)), so changing it
needs an ADR. The same guardrail setup was later added to another, larger Rust repository, and
that work found gaps. Each one below was checked against RackSmith first, and a test reproduced
each real gap (and failed) before the fix:

1. **Gate steps that check nothing.** xtask runs under `cargo run`, so its environment holds
   xtask's package variables (`CARGO_PKG_NAME=xtask`, `CARGO_MANIFEST_DIR=<root>/xtask`, ...), and
   every tool it spawned inherited them. cargo-machete reads `CARGO_PKG_NAME` to decide how to
   parse its arguments: with it set, `cargo machete` scans a directory named `machete`. In the
   other repository that step always passed. Here it failed with "No such file or directory"
   whenever cargo-machete was installed, so `cargo xtask ci` could not pass on a machine that had
   every tool.
2. **Loopholes in the source rules.**
   - `use std::include as load; load!("x.rs")` and `m!(include)` (with
     `macro_rules! m { ($i:ident) => { $i!("x.rs"); } }`) load a file without the `include !`
     token pair that the rule matched.
   - `#[$meta]` or `#[cfg_attr(all(), $meta)]` in a macro can expand to `path = "x.rs"`, which the
     `#[path]` rule could not see.
   - `#[cfg(not(clippy))]`, `#[cfg(clippy)]`, `#[cfg_attr(clippy, ...)]` and `cfg!(clippy)` hide
     code from clippy, or show it code the compiler never builds, and nothing rejected them.
   - These were already covered: `#[path = r"..."]` (the lexer strips raw strings and keeps the
     `path =` tokens), `include!` inside a `macro_rules!` body, and renamed dependencies
     (`alias = { package = "real" }`, in a member or in `[workspace.dependencies]`):
     `cargo metadata` reports the real package name, which the allowlists and bans match.
     Regression tests now pin all three.
   - The fuzz project already had a drift check: its `[lints]` must equal the root
     `[workspace.lints]` exactly, with no exceptions, and the gate runs
     `cargo clippy --manifest-path fuzz/Cargo.toml --all-targets --locked -- -D warnings`. More
     test cases (an added lint, a priority change, a missing group, a missing table) now pin it.
3. **No MSRV check of dependencies.** The CI `MSRV` job builds with Rust 1.98.0, but only on
   Linux, only in CI, and its error is a compile failure deep in some dependency. Nothing named
   the dependency that raised the bar.
4. **Silent "pass" output.** `cargo xtask arch` and `cargo xtask docs` printed "all rules pass"
   without saying what they examined, so a run that loaded no crates, no files or no generated
   sections looked the same as a full run.

## Decision

We will make these changes, each with unit tests in the module it changes:

- **`xtask/src/process.rs`**: every spawned command (cargo, typos, actionlint, zizmor, git, and
  `cargo metadata` through `cargo_metadata`) starts without xtask's package variables:
  `CARGO_PKG_*`, `CARGO_BIN_EXE_*`, `CARGO_MANIFEST_DIR`, `CARGO_MANIFEST_PATH`,
  `CARGO_CRATE_NAME`, `CARGO_BIN_NAME`, `CARGO_PRIMARY_PACKAGE`, `CARGO_TARGET_TMPDIR`,
  `CARGO_RUSTC_CURRENT_DIR` and `OUT_DIR`. We keep `CARGO` on purpose: nested commands run that
  same cargo, and tools read it to detect a `cargo <tool>` call. We also keep `RUSTUP_TOOLCHAIN`
  on purpose: it pins nested calls to the toolchain xtask started with. User configuration such as
  `CARGO_HOME`, `CARGO_TARGET_DIR` or `CARGO_TERM_COLOR` is not touched. `xtask/src/ci.rs`,
  `xtask/src/hooks.rs` and `xtask/src/workspace.rs` build their commands through this function.
- **`xtask/src/source_rules.rs`, `xtask/src/attributes.rs`, `xtask/src/violation.rs`**: the
  identifier `include` is rejected anywhere in code, not only before `!`. An attribute whose meta
  item is a macro fragment (`#[$meta]`, or `$` after a comma in `cfg_attr`) is rejected. A fragment
  in value position, as in `#[doc = $text]`, stays allowed. The identifier `clippy` not followed
  by `::` (only lint paths are) is rejected as a new `LinterCfg` violation.
- **`xtask/src/msrv_rules.rs`** (new), **`xtask/src/workspace.rs`**: `cargo xtask arch` loads the
  locked dependency graph (`cargo metadata --locked`, every platform and feature) and fails if any
  dependency's `rust-version` is above the workspace MSRV (the lowest `rust-version` of any
  member). It also fails if no member declares one. The first run found 130 locked packages. The
  highest requirement is `cargo-platform` 0.3.3 at 1.91, below the MSRV of 1.98. The fuzz project
  is not checked: it builds with a pinned nightly and declares no `rust-version`.
- **`xtask/src/arch.rs`, `xtask/src/docs.rs`**: both commands print what they examined in one line
  (for example `architecture: all rules pass; checked 4 crates, 47 source files, 130 locked
  packages (MSRV 1.98.0; highest dependency rust-version 1.91.0 in cargo-platform 0.3.3)` and
  `docs: 2 generated sections are up to date`). They fail if they examined no crates, source files
  or locked packages, if one crate or the fuzz project has no source files, if there are no
  generated sections, or if a generator printed nothing.
- **`docs/trust-pyramid.md`**: documents the new rules, and gives a manual mutation check for
  property tests (break the invariant, see the property fail, revert). We are not adding
  mutation-testing tools.

These were reviewed and need no change. A manual mutation run made every property test in
`crates/racksmith-domain/tests/` fail at least once. The TLA+ model, the Kani proofs, the fuzz
targets and the conformance test all run against the production `MachineState::apply` and
`TRANSITIONS`, which the API (`MachineRegistry::apply` calls `Machine::handle`) and the CLI also
use. `specs/tla/check.sh` checks the safety invariants and the `BrokenEventuallyReady` liveness
property together.

## Consequences

- A gate step now checks the project it was meant to check. cargo-machete runs correctly under
  `cargo xtask ci`, both on the workspace and on `fuzz/`, which it finds by walking the tree.
- A variable or field named `include`, or a macro that passes whole attributes as fragments, is
  now rejected. If a real use case appears, it needs an xtask change and an ADR.
- `cargo xtask arch` now resolves the dependency graph. On a clone without a registry cache it
  needs network access once, and it fails on a stale `Cargo.lock`, as every other `--locked`
  step already does.
- Raising a dependency past the MSRV is now an explicit `cargo xtask arch` failure that names the
  dependency. To fix it, pin an older version in `Cargo.lock`, or raise `rust-version` (and
  clippy.toml `msrv`) on purpose in its own PR.
- Still open: the fuzz project's dependencies are not checked against the `architecture.toml`
  bans or allowlists. cargo-deny covers its supply chain (`cargo deny --manifest-path
  fuzz/Cargo.toml check`).
