# 0006. Make protobuf the API source of truth, served as gRPC with a generated REST gateway

- Status: Accepted
- Date: 2026-11-18

## Context

Many clients consume RackSmith's API: the CLI, the Kubernetes operator (M2), the Terraform/OpenTofu
and Cluster API providers, the in-RAM deploy agent, and users' own automation. Deploys are
long-running and need live progress ([PRD](../prd.md) R20). Options considered:

1. **REST/JSON with OpenAPI generated from code.** The easiest to consume, but streaming is
   awkward and the schema follows whatever the server code happens to do.
2. **Kubernetes-style resources only.** A good fit for declarative state, but a poor fit for
   imperative operations and streams such as deploy progress and agent check-in.
3. **Protobuf contracts as the source of truth, served as gRPC with a REST/JSON gateway generated
   from them.**

## Decision

We will take option 3.

- **The contracts** are `.proto` files in a dedicated `proto/` tree, versioned by package
  (`racksmith.<service>.v1alpha1`, later `v1`). Rust types are generated from them; nobody writes
  wire types by hand.
- **Servers:** gRPC for clients and agents, including server streaming for deploy progress. A
  REST/JSON gateway and an OpenAPI document are both generated from the same contracts.
- **Declarative resources** (Machine, MachinePool, OSImage, FirmwareBaseline, …) use the same
  spec/status shape as Kubernetes inside the protobuf messages, so the operator's CRDs can map
  onto them one to one ([ADR 0005](0005-one-core-two-front-ends.md)).
- **Contract checks:** CI runs a protobuf linter and a breaking-change check against `main`.
  Breaking changes are only allowed in `alpha` packages, or with a new version package. The exact
  tool (for example buf) is chosen in the system design.
- **Layering:** generated types stay in an adapter-layer crate. The domain crate never depends
  on protobuf types; conversions are explicit, which matches the existing DTO pattern in
  `racksmith-api`.

## Consequences

- Clients are typed in every language, deploy progress streams over the same API, and one schema
  serves the CLI, the operator and the providers.
- Adds a code-generation step to the build and to `cargo xtask ci`, and probably a `protoc` or
  buf dependency in CI.
- The current axum JSON API in `racksmith-api` is an M0 placeholder. It will be replaced by the
  gRPC service and the gateway, and its routes are not a compatibility promise.
- Browser clients have to use the REST gateway (or gRPC-Web). That is acceptable while v1 has no
  web UI.
