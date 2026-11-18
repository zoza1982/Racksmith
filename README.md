# RackSmith

[![CI](https://github.com/zoza1982/Racksmith/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/zoza1982/Racksmith/actions/workflows/ci.yml)
[![Verify](https://github.com/zoza1982/Racksmith/actions/workflows/verify.yml/badge.svg?branch=main)](https://github.com/zoza1982/Racksmith/actions/workflows/verify.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/zoza1982/Racksmith/badge)](https://scorecard.dev/viewer/?uri=github.com/zoza1982/Racksmith)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

RackSmith is an open-source, Rust-native alternative to
[Canonical MAAS](https://maas.io/) (Metal-as-a-Service), built for hyperscale and GPU clusters:
discovery, commissioning, firmware lifecycle, allocation and fast operating system deployment for
racks of physical servers.

> [!WARNING]
> **Status: pre-alpha.** The repository is being bootstrapped. Today it contains the machine
> lifecycle model, an in-memory HTTP API and a CLI. It cannot boot, inventory or deploy a real
> machine yet. Do not point it at hardware you care about.

## Why RackSmith: the MAAS pains we attack

MAAS is mature and capable, and we respect it. RackSmith is built for **hyperscale and GPU
clusters**, where these trade-offs hurt the most:

| MAAS pain | RackSmith answer |
|---|---|
| **Complex to install and operate.** Snap/deb packages, separate region and rack controllers, a required PostgreSQL server, and upgrades that touch all of them. | **One static binary** with embedded storage, or the same core as a **Kubernetes operator**. Upgrading means replacing the binary. |
| **Slow deployments.** An ephemeral boot followed by an installer run (curtin) takes many minutes per machine. | **Block-image streaming** from an in-RAM Rust agent, then kexec or reboot. Target: **under 5 minutes from power-on to OS-ready**, with 1,000 machines deploying at once and a public, reproducible benchmark. |
| **Ubuntu-first.** Other operating systems need separately built custom images. | **Any OS is first-class:** Ubuntu, Debian, RHEL-family, Flatcar, Talos and your own golden images, configured with cloud-init or Ignition. |
| **Imperative and database-centric.** State lives in the MAAS database and is driven by UI or API calls; nothing continuously reconciles it. | **Declarative desired state in Git**, continuously reconciled, in the same spec/status shape as Kubernetes. |
| **No first-class GPU-fleet lifecycle.** Firmware baselines, burn-in and quarantine are only possible through custom scripts. | **Firmware and BIOS baselines with drift remediation, GPU/fabric readiness checks (DCGM, NVLink, InfiniBand), and automatic quarantine of unhealthy nodes.** |

Also built in from the start:
- a pluggable network stack (proxyDHCP, TFTP, HTTP boot, iPXE, with Kea and NetBox adapters);
- Redfish and IPMI drivers;
- a gRPC API with a REST gateway;
- mTLS between the control plane and its agents.

Later milestones add TPM attestation and Cluster API / Terraform providers.

The core is also **verified**: the machine lifecycle (which machine may be wiped, and when) is
model-checked with TLA+ and Kani and property-tested. And it is **built to be developed safely
with AI agents**; see [How quality is enforced](#how-quality-is-enforced).

The full product plan is in the [Product Requirements Document](docs/prd.md).

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

See the [PRD milestones](docs/prd.md#8-milestones) for scope and exit criteria.

- [x] **M0 Bootstrap:** workspace, guardrails, CI, model-checked lifecycle
- [ ] **M1 Fast-deploy MVP:** single binary, proxyDHCP/TFTP/HTTP boot/iPXE, Redfish + IPMI, image-streaming agent, 5 OS recipes, gRPC API + CLI, virtual lab in CI, under 5 minutes to OS-ready, 10k-node load lab, MAAS benchmark
- [ ] **M2 Declarative platform:** Kubernetes operator, GitOps, HA (Raft), Cluster API + Terraform, NetBox/Kea, Secure Boot + TPM attestation
- [ ] **M3 GPU fleet:** firmware/BIOS baselines, DCGM and fabric checks, automatic quarantine, reprovisioning in seconds, multi-site
- [ ] **M4 Scale and edge:** peer-to-peer image distribution, DPU-isolated boot, Slurm, IPv6

## Contributing

Contributions are welcome, including AI-assisted ones. Start with
[CONTRIBUTING.md](CONTRIBUTING.md), and please follow the
[Code of Conduct](CODE_OF_CONDUCT.md). Report security issues privately as described in
[SECURITY.md](SECURITY.md).

## License

Licensed under the [Apache License, Version 2.0](LICENSE). Unless you explicitly state otherwise,
any contribution you intentionally submit for inclusion in RackSmith is licensed as above,
without any additional terms or conditions.
