# 0020. Authenticate API callers with static tokens and authorize every RPC by policy

- Status: Accepted
- Date: 2026-11-18

## Context

R28 requires token authentication and role-based authorization (admin, operator, viewer) in M1.
[PRD](../prd.md) §12 Q4 asks whether M1 needs OIDC, and Q5 asks whether the hosted control plane's
multi-tenant model shapes M2's authorization design. M1 runs in CI and small installs; OIDC needs an
identity provider to test against
([system design §11.7](../design/system-design.md#117-authentication-and-authorization-r28)).

Options considered: **OIDC from day one**; **static tokens** with roles; **mTLS user certificates**.

## Decision

- **Tokens:** opaque `rsk_<id>_<secret>` with a 256-bit secret, stored as SHA-256 and compared in
  constant time; role, optional CIDR allowlist, expiry (default 90 days). `racksmith init` writes one
  bootstrap admin token to a 0600 file; there is no default password. The bootstrap admin may also use
  a 0600 Unix socket.
- **Policy as contract:** every RPC declares a `(racksmith.options.v1.policy)` option with an action,
  a minimum role and a resource kind. `cargo xtask codegen` builds the policy table used by one tower
  layer shared by gRPC and REST, and an RPC without the option fails CI (default deny).
- **Decision point in the domain:** `authorize(principal, action, ResourceAttrs { kind, id, pool,
  site, tenant, owner })`, pure and Kani-checked with the actor matrix. M1 checks role only. Every
  v1alpha1 resource carries `tenant` in its metadata, so scoped roles, OIDC groups or Cedar policies
  can land in M2 without touching call sites.
- **Hygiene:** per-token rate limits (100 rps, burst 200), 10 destructive calls per second per
  principal, `dry_run` on every mutating RPC, confirmation plan hashes for destructive changes.
- **OIDC** arrives in M2 behind the `Authenticator` port.

## Consequences

- M1 needs no identity provider and the quickstart stays short.
- Static tokens must be rotated by operators; expiry defaults push them to.
- Q4 is answered (static tokens in M1, OIDC in M2). Q5 is answered as "yes, it shapes M2": the seam
  exists now, but the multi-tenant model itself remains an owner decision.
