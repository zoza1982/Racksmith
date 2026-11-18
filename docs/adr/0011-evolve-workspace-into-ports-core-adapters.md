# 0011. Evolve the workspace into ports, a core and adapters

- Status: Accepted
- Date: 2026-11-18

## Context

The M0 workspace has three layers: `racksmith-domain` (0), `racksmith-api` (1) and
`racksmith-cli` (2). M1 adds a store, boot services, two power drivers, an image service, a PKI,
an in-RAM agent, a simulator and a reconciler engine ([system design §3](../design/system-design.md#3-crate-and-workspace-layout)).
Three layers cannot hold ports, a core and adapters without the core depending on adapters or the
reverse. The work is done by one maintainer and fleets of AI agents, so crate boundaries are also
the unit of parallel work and of review.

Options considered:

1. **A few large crates** (domain, server, agent). Fewer manifests, but every workstream edits the
   same crates, and layering inside a crate cannot be checked by `cargo xtask arch`.
2. **Ports, a core and adapters in strict layers**, with a dev-only test kit below everything that
   tests against it, so the existing rule ("depend only on strictly lower layers, dev-dependencies
   included") keeps working.

## Decision

We will take option 2, with this layer map:

| Layer | Crates |
|---|---|
| 0 | `racksmith-domain` (pure) |
| 1 | `racksmith-ports` (traits only, including the cross-workstream seams `BootDecisions`, `BootTokens` and `AgentSessions`), `racksmith-proto` (generated code + conversions), `racksmith-rsimg` (pure image format), `racksmith-storage-model` (the `racksmith.storage.v1` encoding, key schema, codec and migrations, shared by every writer and both store engines), `racksmith-netboot-core` (sans-IO DHCP/TFTP decisions) |
| 2 | `racksmith-testkit` (fakes and port contract suites; dev-only) |
| 3 | `racksmith-core` (reconcilers, queues, admission, sessions) and the adapters `racksmith-store-redb`, `racksmith-redfish`, `racksmith-ipmi`, `racksmith-netboot`, `racksmith-imagesvc`, `racksmith-pki` (later `racksmith-store-raft`) |
| 4 | `racksmith-api` (gRPC services, REST transcoder, agent gateway; replaces the M0 axum placeholder) |
| 5 | `racksmith-server` (wiring library: no CLI parsing, no signals) |
| 6 | `racksmith-cli`, `racksmith-agent` (lib + PID 1 binary), `racksmith-operator` (M2) |
| 7 | `racksmith-sim` (BMC emulator, agent fleet, benchmark harness) |

We will add these rules to `architecture.toml` and `cargo xtask arch` in the PR that lands this
ADR, and add crate entries as each crate is created:

- `clap`, `kube`, `kube-runtime`, `k8s-openapi` only in layer 6; `tokio::signal` source-banned below
  layer 6 (enforces [ADR 0005](0005-one-core-two-front-ends.md)).
- `redb` only in the store crates; `reqwest` only in `racksmith-redfish` and `racksmith-imagesvc`.
- Source bans on `std::fs`, `std::net`, `std::process` in `racksmith-core`; on `tokio` in
  `racksmith-ports`; the domain's full source bans in `racksmith-rsimg` and `racksmith-netboot-core`.
- `racksmith-agent` may not depend on the core, store, power, API, PKI or image-service crates.
- A new `dev-only` rule kind: `racksmith-testkit` and `racksmith-sim` may appear only as
  dev-dependencies of product crates.
- A new `generated` path rule: generated files live only under the generated roots listed in
  `architecture.toml` (M1: `crates/racksmith-proto/src/gen/`, `crates/racksmith-rsimg/src/gen/` and
  `crates/racksmith-storage-model/src/gen/`), all written by `cargo xtask codegen` (policy in
  [ADR 0019](0019-protobuf-codegen-and-rest-transcoder.md)). Several roots are needed because the
  L1 crates that own a signed or persisted proto cannot depend on each other, and nobody writes prost
  types by hand ([ADR 0006](0006-protobuf-contracts-grpc-api.md)).
- A new per-crate `platforms` field: crates that call Linux-only APIs are marked
  `platforms = ["linux"]` and may be depended on only by crates with the same mark
  ([ADR 0030](0030-linux-only-crates-and-ci-platform-matrix.md)).
- `aws-lc-rs` and `aws-lc-sys` banned everywhere ([ADR 0028](0028-supply-chain-policy-amendments.md)).
- `expose_secret` source-banned outside `racksmith-pki`, `racksmith-redfish`, `racksmith-ipmi` and
  `racksmith-agent`.
- A new `cargo xtask size` gate, run by `cargo xtask ci`: the stripped x86_64 musl agent binary must
  be at most 12 MiB and the compressed initramfs at most 40 MiB (target 32 MiB).

## Consequences

- Each layer-3 crate is an independent work package whose "done" is a contract suite in
  `racksmith-testkit`; an agent working on one adapter cannot break another, and a PR that crosses a
  boundary fails `cargo xtask arch` instead of review.
- About 18 crates by the end of M1 means more manifests and allowlist entries to maintain. Crates are
  created only when a workstream needs them.
- `cargo xtask arch` needs new rule kinds (`dev-only`, `generated` with several roots, `platforms`)
  and the size gate; that tooling work is on the F1 critical path.
- The seams between same-layer crates built by different workstreams (netboot and core, the agent
  gateway and the session hub, netboot and the PKI's token minting) are ports frozen at F1, so they do
  not surface as conflicts at the first end-to-end deploy.
- `racksmith-server` exists so that the M2 operator reuses the wiring without depending on the CLI.
