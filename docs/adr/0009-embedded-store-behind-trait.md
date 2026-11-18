# 0009. Persist state in an embedded store behind a `Store` trait; no external database required

- Status: Accepted
- Date: 2026-11-18

## Context

MAAS requires a PostgreSQL server, one of the moving parts behind pain P1
([PRD](../prd.md) §2). RackSmith must run as one binary (R1) with crash-safe persistent state
(R2). It must also reach high availability in M2 (R5) and manage 10,000 machines per control plane
(§7). Options considered:

1. **Require PostgreSQL** (or another external database). Mature and HA-capable, but it brings
   back exactly the operational burden we attack.
2. **An embedded store with no abstraction.** The simplest start, but it locks in one engine and
   makes HA replication harder to add later.
3. **An embedded store behind a `Store` trait.** HA comes later by replicating the store through
   consensus, and an external database stays possible as just another implementation.

## Decision

We will take option 3.

- **The trait:** the core persists state only through a `Store` trait (a port). Its operations
  are transactional, and changes are visible through a watchable change feed, so the reconcilers
  can be built on watch rather than polling.
- **M1 implementation:** an embedded, crash-safe engine compiled into the binary.
- **Which engine is still open** ([PRD](../prd.md) §12 Q1). Candidates include redb and SQLite.
  The system design chooses one with a benchmark at 10k machines, recorded in its own ADR.
- **M2 HA:** state is replicated across 3 or more replicas through a Raft log, with
  leader-elected reconcilers. The Raft library is chosen in the system design. An external
  database (for example PostgreSQL) stays possible as an alternative `Store` implementation, but
  it is never required.
- **Migrations:** schema migrations are automatic on startup and reversible within one minor
  version (PRD §7, upgrade target).
- **Layering:** the store implementation lives in the adapter layer, and `architecture.toml`
  keeps the domain crate free of storage dependencies.

## Consequences

- Installing RackSmith means one binary and a data directory, with nothing else to operate.
- The `Store` trait must be designed carefully up front (transactions, watch semantics,
  consistency), because the M2 Raft implementation must keep the same semantics.
- Backup and restore become RackSmith's job: snapshot and restore commands are needed by M1's
  end.
- Analytics-style queries that PostgreSQL would make easy need explicit indexes or export
  paths.
