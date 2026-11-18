# 0030. Mark Linux-only crates and check only portable crates on macOS and Windows

- Status: Accepted
- Date: 2026-11-18

## Context

The required `ci-success` check runs `cargo clippy --workspace --all-targets --all-features` on
`ubuntu-latest`, `macos-latest` and `windows-latest`, and `cargo nextest run --workspace` plus
doctests on Ubuntu and macOS (`.github/workflows/ci.yml`). RackSmith ships for Linux only (R1), and
the M1 design puts Linux-only APIs in workspace crates
([system design §3.1](../design/system-design.md#31-layered-crate-map)): netlink, `finit_module`,
`reboot`, `mount` and block-device `fallocate` in the agent; `sendfile` in the image service;
`SO_BINDTODEVICE` in boot services. Those crates do not compile for macOS or Windows, and the
server and CLI link them. [ADR 0028](0028-supply-chain-policy-amendments.md) limits `deny.toml` to
Linux targets but does not touch the CI matrix. CI workflows are code-owned guardrails
([ADR 0003](0003-trust-pyramid-guardrails.md)): without a decision, the first agent or boot-services
pull request fails the required check, and the only quick fix available to an agent is to weaken CI,
which AGENTS.md forbids.

Options considered:

1. **Drop the macOS and Windows jobs.** Simplest, but loses the check that the portable crates
   (domain, ports, proto, rsimg, core, power drivers, PKI, API) stay portable, which keeps them easy
   to test and fuzz on any developer machine.
2. **`cfg(target_os = "linux")`-gate every Linux call** and provide stubs elsewhere. Keeps the
   whole workspace compiling everywhere, but adds stub code that is never shipped or run, which the
   project's no-dead-code rule argues against.
3. **Mark Linux-only crates** in `architecture.toml` and have the non-Linux jobs check only the
   portable crates.

## Decision

We will take option 3, landing before the first Linux-only crate (before F1):

- `architecture.toml` gains a per-crate `platforms` field. `racksmith-agent`, `racksmith-netboot`,
  `racksmith-imagesvc`, `racksmith-sim`, `racksmith-server`, `racksmith-cli` and (in M2)
  `racksmith-operator` are `platforms = ["linux"]`. `cargo xtask arch` rejects a dependency from an
  unmarked (portable) crate on a Linux-only crate.
- `cargo xtask platform-exclude` prints the `--exclude` list derived from `architecture.toml`.
- `ci.yml` is amended in the same pull request: the macOS and Windows clippy jobs and the macOS test
  job run with `--workspace` plus that `--exclude` list; the Linux jobs are unchanged and still cover
  every crate. `cargo xtask ci` runs on Linux and is unchanged.
- Inside portable crates, Linux-specific behaviour (for example a socket option) is behind a port or
  an explicit `cfg(target_os = "linux")` item with no stub; inside Linux-only crates no gating is
  needed.

## Consequences

- The required check keeps meaning something on every OS, and Linux-only crates never fail it.
- The exclude list cannot drift from the code, because it is derived from `architecture.toml`,
  which `cargo xtask arch` checks.
- This changes a CI workflow and `architecture.toml`, both code-owned guardrails, so it needs the
  owner's review and acceptance.
