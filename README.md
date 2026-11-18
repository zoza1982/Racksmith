# RackSmith

[![CI](https://github.com/zoza1982/Racksmith/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/zoza1982/Racksmith/actions/workflows/ci.yml)
[![Verify](https://github.com/zoza1982/Racksmith/actions/workflows/verify.yml/badge.svg?branch=main)](https://github.com/zoza1982/Racksmith/actions/workflows/verify.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/zoza1982/Racksmith/badge)](https://scorecard.dev/viewer/?uri=github.com/zoza1982/Racksmith)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

RackSmith is an open-source, Rust-native alternative to
[Canonical MAAS](https://maas.io/) (Metal-as-a-Service): discovery, commissioning, allocation
and operating system deployment for racks of physical servers.

> [!WARNING]
> **Status: pre-alpha.** The repository is being bootstrapped. Today it contains the machine
> lifecycle model, an in-memory HTTP API and a CLI. It cannot boot, inventory or deploy a real
> machine yet. Do not point it at hardware you care about.

## Goals

MAAS is mature and capable. RackSmith aims at the same job with a different set of trade-offs:

- **One static binary.** No snap, no separate region and rack controller packages to install.
- **Few moving parts.** The goal is to run without a mandatory PostgreSQL server for small
  sites, with an external database as an option for larger ones. The current in-memory store is
  a placeholder until that is designed.
- **A verified core.** The machine lifecycle is a small state machine that is model-checked with
  TLA+ and Kani and property-tested, so the most dangerous logic in a provisioning system (which
  machine may be wiped, when) is the best tested.
- **Built to be developed with AI agents safely.** Strict, mechanically enforced guardrails
  rather than review alone. See [How quality is enforced](#how-quality-is-enforced).

## Quickstart

Install Rust with [rustup](https://rustup.rs/); it picks up the pinned toolchain from
[`rust-toolchain.toml`](rust-toolchain.toml) automatically.

```sh
git clone https://github.com/zoza1982/Racksmith.git
cd Racksmith
cargo run -p racksmith-cli -- serve
```

The API listens on `127.0.0.1:5240` (MAAS's port) by default; pass `--listen` to change it. In
another terminal:

```sh
curl -s localhost:5240/healthz
curl -s -X POST localhost:5240/api/v1/machines \
  -H 'content-type: application/json' \
  -d '{"mac": "52:54:00:12:34:56", "hostname": "node-01"}'
curl -s -X POST localhost:5240/api/v1/machines/m-0000000000000001/events \
  -H 'content-type: application/json' \
  -d '{"event": "start_commissioning"}'
```

Other commands:

```sh
cargo run -p racksmith-cli -- lifecycle                        # print the transition table
cargo run -p racksmith-cli -- validate mac 52-54-00-12-34-56   # normalize a MAC address
cargo run -p racksmith-cli -- validate hostname Node-01        # check an RFC 1123 hostname
```

Set `RUST_LOG` (for example `RUST_LOG=debug`) to control log output.

## Repository layout

| Path                                                 | Contents                                                     |
|------------------------------------------------------|--------------------------------------------------------------|
| [`crates/racksmith-domain`](crates/racksmith-domain) | Pure domain model and the machine lifecycle state machine.   |
| [`crates/racksmith-api`](crates/racksmith-api)       | HTTP API (axum) over an in-memory machine registry.          |
| [`crates/racksmith-cli`](crates/racksmith-cli)       | The `racksmith` binary.                                      |
| [`xtask`](xtask)                                     | Repository tooling: `cargo xtask ci`, `arch`, `docs`.        |
| [`architecture.toml`](architecture.toml)             | Machine-checked crate layering and dependency policy.        |
| [`specs/tla`](specs/tla)                             | TLA+ model of the machine lifecycle.                         |
| [`fuzz`](fuzz)                                       | cargo-fuzz targets for the parsers.                          |
| [`docs`](docs)                                       | Architecture, lifecycle, trust pyramid and [ADRs](docs/adr). |

## How quality is enforced

RackSmith is built so that changes, whether written by people or AI agents, are checked by
machines first. The guardrails form a [trust pyramid](docs/trust-pyramid.md), strongest at the
bottom:

1. **Deterministic constraints.** The type system makes invalid states unrepresentable, and
   strict workspace lints ([`Cargo.toml`](Cargo.toml), [`clippy.toml`](clippy.toml)) deny
   `unwrap`, panics, `HashMap`, `println!` and silent lint suppression.
   [`architecture.toml`](architecture.toml), enforced by `cargo xtask arch`, fixes the crate
   layering, and [`deny.toml`](deny.toml) governs licenses, advisories and dependency sources.
2. **Verification.** Unit, integration and property tests (proptest), fuzzing (cargo-fuzz), Kani
   proofs and a TLA+ model of the [machine lifecycle](docs/lifecycle.md) checked by TLC.
3. **Prose and judgment.** [AGENTS.md](AGENTS.md), [CONTRIBUTING.md](CONTRIBUTING.md) and
   [architecture decision records](docs/adr) cover only what code cannot check.

One command runs the local gate: every check of the required `ci-success` check that runs on one
machine (CI adds the MSRV build, coverage, and macOS and Windows runs):

```sh
cargo xtask ci
```

## Roadmap

- [ ] Discovery: DHCP, PXE and iPXE boot of unknown machines
- [ ] Commissioning: hardware inventory (CPU, memory, disks, NICs) and burn-in tests
- [ ] BMC power control: IPMI and Redfish
- [ ] Image deployment: OS images written to disk, cloud-init configuration
- [ ] API and CLI: authenticated, versioned API with a full-featured client
- [ ] High availability: multiple controllers without a single point of failure

## Contributing

Contributions are welcome, including AI-assisted ones. Start with
[CONTRIBUTING.md](CONTRIBUTING.md), and please follow the
[Code of Conduct](CODE_OF_CONDUCT.md). Report security issues privately as described in
[SECURITY.md](SECURITY.md).

## License

Licensed under the [Apache License, Version 2.0](LICENSE). Unless you explicitly state otherwise,
any contribution you intentionally submit for inclusion in RackSmith is licensed as above,
without any additional terms or conditions.
