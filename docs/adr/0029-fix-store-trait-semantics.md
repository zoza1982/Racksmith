# 0029. Fix the Store trait semantics at F1, independently of the engine

- Status: Accepted
- Date: 2026-11-18

## Context

[ADR 0009](0009-embedded-store-behind-trait.md) puts the store behind a `Store` trait with
transactions and a watchable change feed, Raft-replicated in M2. F1 (end of week 2) freezes the
ports so that the core, the store engine and every adapter can be built in parallel
([system design §6.1, §14.2](../design/system-design.md#61-store-trait-semantics-the-contract-m2-must-keep)).
The engine choice, redb or rusqlite, is gated on a week-3 benchmark
([ADR 0012](0012-redb-store-behind-etcd-style-trait.md)). As first written, ADR 0012 carried both
the trait semantics and the engine, so F1 would either freeze semantics under an unaccepted ADR or
slip to week 3 and delay every adapter workstream.

Two gaps in the first draft also had to be closed before freezing:

- Every change must write its actor-attributed `ev/<revision>/<n>` row in the same transaction, but
  the revision is assigned by the writer (M1) or the Raft apply step (M2) after the caller has built
  its `Op::Put` with a literal key, so the caller cannot name the key.
- The draft promised paged lists from one consistent snapshot. API reads come from an in-memory
  object cache that holds only the latest state, and redb cannot reopen an old revision once its read
  transaction is closed, so no component could serve page 2 at revision R without pinning pages
  across client requests.

Options considered: **freeze the semantics with the engine after week 3**; **freeze the semantics
at F1 in their own ADR**, engine-independent, and leave only the engine to the benchmark.

## Decision

We will freeze these semantics at F1, in `racksmith-ports`, with the `store_contract` suite in
`racksmith-testkit` as their executable definition:

- Multi-key transactions with `ModRevision`/`Absent`/`Present` compares, applied atomically at one
  global, monotonic revision; `commit` returns only after the data is durable.
- Three operations: `Put`, `Delete`, and **`Append { prefix, value }`**, whose key is completed as
  `prefix + "<revision>/<n>"` when the revision is assigned (by the writer in M1, by the apply step
  on every replica in M2), deterministically. The `ev/` audit rows use it, so they are written in
  the same transaction as the change without a separate drainer.
- A `Snapshot` is consistent for as long as it is held within one call. **Paged lists are
  consistent per page**, not across pages: each page reports the revision it was served at, a
  client that needs a consistent whole list lists and then watches from the first page's revision,
  and a token older than the compacted revision returns `Compacted`.
- A gap-free, ordered watch per key range from a revision, with a history ring of 200,000 events or
  128 MiB, `Compacted` and `ResyncRequired` errors, progress bookmarks every 10 s, and writers that
  never block on watchers.
- An optional `expected_term` on every transaction (unused in M1), so the M2 leader term needs no
  signature change.
- Limits of 64 KiB per value and 128 operations or 256 KiB per transaction; a bounded proposal
  channel of 4,096 transactions with `Busy` for API writers.

Values are opaque bytes to the trait; their encoding is `racksmith-storage-model`
([ADR 0011](0011-evolve-workspace-into-ports-core-adapters.md),
[ADR 0025](0025-restore-safety-and-reversible-migrations.md)).

## Consequences

- F1 depends only on this ADR; the week-3 benchmark can switch the engine without touching the
  core or any adapter.
- The contract suite gains `Append` checks (keys carry the committing revision and never collide),
  and the M2 Raft store must complete `Append` keys in its apply step.
- Clients lose cross-page snapshot consistency, which nothing in M1 needs; the list-then-watch
  pattern is the documented way to get a consistent view.
