# 0005. Build one front-end-agnostic core; ship the single binary first and the Kubernetes operator in M2

- Status: Accepted
- Date: 2026-11-18

## Context

The product interview ([PRD](../prd.md) §3, §4) set two ways to run RackSmith "from day one":

- a single static binary for anyone who wants zero dependencies;
- a Kubernetes operator for platform teams that already run a management cluster.

The project has one maintainer plus AI agents and a target of about 3 months for the MVP
(milestone M1). Options considered:

1. **Binary only.** The fastest MVP, but the operator later means refactoring a core shaped
   around one front end.
2. **Operator only.** A natural fit for platform teams, but it needs a Kubernetes cluster before
   the first machine can boot. That brings back the operational weight that pain P1 attacks.
3. **Both, fully, in M1.** Doubles packaging, testing and documentation within the MVP window.
4. **One core built for both from day one, shipped one front end at a time.**

## Decision

We will take option 4.

- **The core** is a set of library crates holding the domain, the reconcilers, the ports (traits)
  and the adapters. It has no knowledge of how it is hosted: no CLI parsing, no process
  lifecycle, no Kubernetes types.
- **Front ends** are thin composition roots. `racksmith` (the binary) ships in M1. A
  `racksmith-operator` crate ships in M2 and maps CRDs onto the same declarative resources
  (see [ADR 0006](0006-protobuf-contracts-grpc-api.md)).
- **A conformance test suite** runs against every front end. A front end is done when it passes
  the same suite.
- **Enforcement:** `architecture.toml` gives front ends the top layer. Core crates are denied
  front-end dependencies (CLI parsing crates, `kube`, `k8s-openapi`, process-signal handling), so
  `cargo xtask arch` fails if hosting concerns leak into the core.

## Consequences

- The M1 scope stays realistic, and the operator arrives without a core rewrite.
- The core must expose a reconciler interface that suits both a simple in-process loop and the
  operator's informer and work-queue model. That is the main design constraint for the system
  design doc.
- The declarative resource schema has to be shared by both front ends from M1 on, so changing it
  later becomes an API-versioning problem.
- Until M2, Kubernetes users run the binary (for example as a StatefulSet) instead of a native
  operator.
