# Fuzz targets

Coverage-guided fuzzing of the RackSmith domain crate with
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) and libFuzzer. Fuzzing is part of the
verification layer of the trust pyramid ([`docs/trust-pyramid.md`](../docs/trust-pyramid.md)): it
checks properties over inputs nobody thought to write a test for.

This directory is a standalone cargo project, excluded from the root workspace, because libFuzzer
instrumentation needs a nightly compiler while the workspace is pinned to stable
([`rust-toolchain.toml`](../rust-toolchain.toml)). `fuzz/Cargo.lock` is committed so every run
builds the same dependency versions. The project is still held to the workspace rules:
`cargo xtask arch` requires its `[lints]` table to equal the root `[workspace.lints]` and applies
the source rules to its targets, and `cargo xtask ci` (like CI) runs clippy and cargo-deny on it.

## Targets

| Target               | Code under test                                   | Properties checked                                                                                                                                       |
|----------------------|---------------------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------|
| `parse_mac`          | `MacAddress::from_str`                            | Never panics. An accepted input equals its canonical form up to case and `-` separators. `Display` round-trips.                                         |
| `parse_hostname`     | `Hostname::from_str`, `Hostname::try_from`        | Never panics; both constructors agree. An accepted name is the input lowercased and satisfies RFC 1123 (last label not all digits), re-checked independently. `Display` round-trips. |
| `parse_machine_id`   | `MachineId::from_str`                             | Never panics. Only the canonical form is accepted: a parsed id displays as exactly the input.                                                           |
| `lifecycle_sequence` | `MachineState::apply`, `Machine::handle`          | From any start state, `apply` never panics and accepts exactly the rows of `TRANSITIONS`. `Deployed` is entered only from `Deploying`. A failed `handle` leaves the machine unchanged. |

Every target asserts its properties with `assert!`/`assert_eq!`, so a violated property is a
crash that libFuzzer records just like a panic in the code under test.

## Running locally

Install a nightly toolchain and cargo-fuzz once:

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
```

Then, from the repository root:

```sh
cargo +nightly fuzz list                                    # show the targets
cargo +nightly fuzz run parse_hostname                      # fuzz until stopped with Ctrl-C
cargo +nightly fuzz run parse_hostname -- -max_total_time=60   # fuzz for 60 seconds
```

The corpus grows in `fuzz/corpus/<target>/` and is reused by later runs. A failing input is saved to
`fuzz/artifacts/<target>/`. Both directories are git-ignored.

To investigate a failure:

```sh
cargo +nightly fuzz run parse_hostname fuzz/artifacts/parse_hostname/crash-<hash>   # reproduce
cargo +nightly fuzz tmin parse_hostname fuzz/artifacts/parse_hostname/crash-<hash>  # minimize
```

Fix the bug, then add the minimized input as an ordinary unit or property test in the domain crate
so the regression is caught by `cargo xtask ci` on every pull request, not only by fuzzing.

## Adding a target

Code that parses untrusted input or enforces an invariant should have a target (see
[`AGENTS.md`](../AGENTS.md)). To add one:

1. Create `fuzz_targets/<name>.rs` with a `//!` comment that lists the properties it checks.
2. Add a matching `[[bin]]` entry to [`Cargo.toml`](Cargo.toml).
3. Assert properties, not only the absence of panics: round-trips, agreement with an independent
   oracle (as `lifecycle_sequence` does with `TRANSITIONS`), and invariants on accepted values.
4. Pick structured values from the domain's `ALL` constants with `Unstructured::choose`, as
   `lifecycle_sequence` does, rather than adding fuzzing-only trait implementations to the domain
   crate.

CI discovers targets with `cargo fuzz list`, so a new `[[bin]]` entry is fuzzed automatically.

## Continuous fuzzing (the outer loop)

The [`Verify`](../.github/workflows/verify.yml) workflow fuzzes every target:

- on every pull request, in the merge queue and on `main`, for 60 seconds per target;
- nightly, for 300 seconds per target.

Every run starts from the corpus that earlier nightly runs built up (kept in the GitHub Actions
cache), and the nightly run minimizes it with `cargo fuzz cmin` and saves it again, so coverage
accumulates across runs. Only the scheduled run on `main` writes that cache.

All targets run even if one crashes. When a run finds a crash, the job fails and uploads
`fuzz/artifacts/` as the `fuzz-artifacts` workflow artifact. Today a maintainer takes it from there, following the steps under
[Running locally](#running-locally).

The planned outer loop automates that hand-off without automating the merge: an agent downloads the
artifact, minimizes the input, turns it into a regression test, proposes a fix, and opens a pull
request that goes through exactly the same gates as any other change, for a human to approve. See
"Future work: the outer loop" in [`docs/trust-pyramid.md`](../docs/trust-pyramid.md).
