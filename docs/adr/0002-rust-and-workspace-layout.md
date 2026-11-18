# 0002. Use Rust and a layered Cargo workspace

- Status: Accepted
- Date: 2026-11-18

## Context

RackSmith manages bare-metal servers: it answers DHCP and TFTP on the provisioning network, holds
BMC credentials and decides when a disk may be wiped. Bugs here are expensive, and the code parses
untrusted network input. The project also aims to ship as a single binary that is easy to
install, unlike the multi-package, PostgreSQL-backed deployment of MAAS.

Much of the code will be written by AI coding agents, so the language and structure should catch
as many mistakes as possible before review.

## Decision

We will write RackSmith in Rust, edition 2024, with the toolchain pinned in
`rust-toolchain.toml`.

- Rust gives memory safety without a garbage collector, a type system expressive enough to make
  invalid states unrepresentable, exhaustive pattern matching, and static binaries. `unsafe` code
  is forbidden workspace-wide.
- The code is a Cargo workspace of small crates arranged in layers: `racksmith-domain` (layer 0,
  pure model), `racksmith-api` (layer 1, adapters) and `racksmith-cli` (layer 2, the binary). A
  crate depends only on strictly lower layers. Repository tooling lives in `xtask`, which depends
  on no product crate.
- The layering and per-crate dependency allowlists are declared in `architecture.toml` and
  enforced by `cargo xtask arch` in CI, rather than described in prose.
- Every external dependency is declared once in the root `[workspace.dependencies]`, and every
  crate inherits the workspace lints.

## Consequences

- The domain can be tested exhaustively and model-checked because it has no I/O.
- Crate boundaries make dependency violations visible in `Cargo.toml` and checkable by tooling.
- New protocols (DHCP, TFTP, IPMI, Redfish) become new adapter crates, which keeps each one small.
- Rust's compile times and learning curve are a cost for contributors; the pinned toolchain and
  strict lints make the first contribution harder but the hundredth safer.
