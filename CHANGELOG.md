# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Machine lifecycle state machine (`racksmith-domain`) with validated `MachineId`, `MacAddress`
  and `Hostname` types, property tests and Kani proofs.
- TLA+ model of the machine lifecycle, model-checked by TLC in CI.
- In-memory HTTP API under `/api/v1` (`racksmith-api`).
- `racksmith` CLI with `serve`, `lifecycle` and `validate` commands. `serve` shuts down
  gracefully on Ctrl-C or SIGTERM, drops connections still open after a drain timeout, and stops
  at once on a second signal.
- `Hostname` rejects names whose last label is all digits, such as `192.168.1.1`.
- `cargo xtask` tooling: `ci`, `arch`, `docs` and `install-hooks`.
- Architecture rules (`architecture.toml`), strict workspace lints, cargo-deny policy, fuzz
  targets and CI workflows.
- Project documentation, governance and architecture decision records.

[Unreleased]: https://github.com/zoza1982/Racksmith/commits/main
