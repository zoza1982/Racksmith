# Contributing to RackSmith

Thank you for helping. This guide covers setup, the quality gate and how changes are reviewed and
merged. By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
Security issues are reported privately; see [SECURITY.md](SECURITY.md).

## Development setup

1. Install Rust with [rustup](https://rustup.rs/). The first `cargo` command in the repository
   installs the pinned toolchain and components from [`rust-toolchain.toml`](rust-toolchain.toml);
   you never pick a version by hand.
2. Clone the repository and enable the versioned git hooks:

   ```sh
   git clone https://github.com/zoza1982/Racksmith.git
   cd Racksmith
   cargo xtask install-hooks
   ```

   `pre-commit` checks formatting; `pre-push` runs the full gate.
3. Install the optional tools the gate uses. Without them `cargo xtask ci` skips those steps with
   a warning (in CI, where `CI` is set, a missing tool fails the gate instead):

   ```sh
   cargo install --locked cargo-nextest cargo-deny typos-cli cargo-machete
   ```

   plus [actionlint](https://github.com/rhysd/actionlint) and
   [zizmor](https://docs.zizmor.sh/) if you change `.github/`.

Verification tools are needed only when you work on what they check: a Java runtime for the
TLA+ model ([`specs/tla/`](specs/tla/)), [Kani](https://model-checking.github.io/kani/) for
`cargo kani -p racksmith-domain`, and nightly Rust with `cargo-fuzz` for [`fuzz/`](fuzz/).

## The gate

```sh
cargo xtask ci
```

This runs, in order and stopping at the first failure: `cargo fmt --check` and clippy (warnings
denied) for the workspace and `fuzz/`, the tests and doctests, rustdoc with warnings denied,
`cargo deny check` for both lockfiles, `cargo xtask arch`, `cargo xtask docs`,
`cargo xtask spec`, `typos`,
`cargo machete`, actionlint and zizmor, all with `--locked` where cargo resolves dependencies.

CI runs the same commands, plus the jobs that need another toolchain or platform: the MSRV
build, the coverage floor, clippy and tests on macOS and Windows, and the `Verify` workflow
(TLA+, Kani, fuzzing). A PR can merge only when both aggregate checks, `ci-success` and
`verify-success`, are green.

Other `cargo xtask` commands:

| Command                     | What it does                                                    |
|-----------------------------|-----------------------------------------------------------------|
| `cargo xtask arch`          | Enforce [`architecture.toml`](architecture.toml), the MSRV and the exemption ledger [`exceptions.toml`](exceptions.toml). |
| `cargo xtask docs`          | Fail if generated sections are stale.                           |
| `cargo xtask docs --fix`    | Regenerate them (`docs/lifecycle.md`, the TLA+ `Transitions`).  |
| `cargo xtask spec`          | Check [`requirements.toml`](requirements.toml) against the design, the M1 plan, the code and the test listing ([ADR 0035](docs/adr/0035-completeness-layer.md)). |
| `cargo xtask spec --fix`    | Also regenerate the coverage page [`docs/requirements.md`](docs/requirements.md). |
| `cargo xtask install-hooks` | Point git at [`.githooks/`](.githooks/).                        |

### Pinned tool versions

Dependabot updates crates, GitHub Actions and the toolchain in `rust-toolchain.toml`. These pins
live where it cannot see them; bump them deliberately, in a `ci:` or `build:` pull request:

| Pin                         | Where                                        |
|-----------------------------|----------------------------------------------|
| Nightly toolchain for fuzzing | `FUZZ_NIGHTLY` in `.github/workflows/verify.yml` |
| cargo-fuzz                  | `CARGO_FUZZ_VERSION` in `.github/workflows/verify.yml` |
| Kani                        | `KANI_VERSION` in `.github/workflows/verify.yml` |
| actionlint                  | `ACTIONLINT_VERSION` in `.github/workflows/ci.yml` |
| zizmor                      | `version` of the zizmor step in `.github/workflows/ci.yml` |
| TLA+ tools (and SHA-256)    | `TLA_TOOLS_VERSION`, `TLA_TOOLS_SHA256` in `specs/tla/check.sh` |

## Pull requests

1. Open an issue first for anything larger than a small fix, so the approach can be agreed before
   you write code.
2. Keep each PR to one logical change, and fill in the
   [PR template](.github/PULL_REQUEST_TEMPLATE.md).
3. Title the PR as a [Conventional Commit](https://www.conventionalcommits.org/): a type (`feat`,
   `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`), an optional scope
   and a lowercase subject, for example `fix(domain): reject hostnames ending in a hyphen`. A
   check enforces this.
4. PRs are **squash-merged**, so the title becomes the single commit on `main`; your branch's
   intermediate commits do not need to be tidy.
5. Merging needs green `ci-success` and `verify-success` checks and an approving review from a
   code owner ([CODEOWNERS](.github/CODEOWNERS) covers every file, and lists the guardrails —
   lint configuration, `architecture.toml`, the exemption ledger `exceptions.toml`, `deny.toml`,
   `xtask/`, `specs/`, `fuzz/`, `.github/` — explicitly). How this works with a single maintainer is described in
   [GOVERNANCE.md](GOVERNANCE.md).

A Developer Certificate of Origin (DCO) sign-off is **not** required. Contributions are accepted
under the [Apache License 2.0](LICENSE), as stated in its section 5.

## AI-assisted contributions

AI-assisted contributions are welcome. They are held to exactly the same bar as any other:

- Disclose how AI tools were used in the "AI assistance" section of the PR template.
- You are responsible for every line: understand it, and be ready to explain it in review.
- It must pass the same gates. [AGENTS.md](AGENTS.md) is written for coding agents; point yours
  at it.

## Coding conventions

Most conventions are enforced by the compiler, lints and `cargo xtask arch`; read the error and
fix the code. The judgment calls that tools cannot check (where logic belongs, error handling,
testing expectations, how to change the machine lifecycle) are in [AGENTS.md](AGENTS.md) and apply
to human contributors as well. The design is explained in
[docs/architecture.md](docs/architecture.md) and [docs/trust-pyramid.md](docs/trust-pyramid.md).

## Architecture decision records

Significant decisions are recorded as ADRs in [`docs/adr/`](docs/adr/), following
[ADR 0001](docs/adr/0001-record-architecture-decisions.md). Write one when you:

- change [`architecture.toml`](architecture.toml) (layers, allowlists, bans),
- change the lint policy in `Cargo.toml`, `clippy.toml` or `deny.toml`,
- add a significant dependency, protocol or storage technology,
- change a public API contract or the machine lifecycle.

To add one, copy [`docs/adr/template.md`](docs/adr/template.md) to the next free number
(`docs/adr/NNNN-short-title.md`), set its status to `Proposed`, and include it in the PR that
makes the change. It becomes `Accepted` when the PR merges. Accepted ADRs are not rewritten: a
later ADR supersedes one, and the only edit to the old ADR is its Status line, updated to point at
the new ADR.

## Changelog

User-visible changes get an entry under `Unreleased` in [CHANGELOG.md](CHANGELOG.md).
