# 0012. Use redb as the store engine, with openraft for M2

- Status: Proposed
- Date: 2026-11-18

## Context

[ADR 0009](0009-embedded-store-behind-trait.md) requires an embedded, crash-safe store behind a
`Store` trait with transactions and a watchable change feed, Raft-replicated in M2, and leaves the
engine and the Raft library open ([PRD](../prd.md) §12 Q1). The trait's semantics must be fixed
now, because the M2 Raft implementation must keep them; they are recorded separately in
[ADR 0029](0029-fix-store-trait-semantics.md), which F1 freezes, so that this ADR (the engine) can
wait for the week-3 benchmark without holding F1. At 10k machines the live state is about
100–150 MiB and the worst write burst about 2k transactions per second
([system design §6](../design/system-design.md#6-storage)).

Engine options: **redb** (pure Rust, MIT/Apache-2.0, one dependency, MVCC read transactions,
about 1.1k individually fsync'd transactions per second and about 79k keys per second in batches);
**rusqlite** (bundled C SQLite, excellent crash safety, SQL that we do not need, about 142
individually fsync'd transactions per second); **fjall** (blocked by license gaps in `deny.toml`);
**sled** (unmaintained). Raft options for M2: **openraft** (active, pre-1.0, currently fails
`deny.toml` because it depends on the unmaintained `chrono`, RUSTSEC-2026-0543); **raft-rs** (last
crates.io release in 2023, protobuf 2).

## Decision

We will use **redb** in M1, in `racksmith-store-redb`, implementing the `Store` trait of
[ADR 0029](0029-fix-store-trait-semantics.md) and passing its `store_contract` suite.

A single writer on a dedicated OS thread group-commits (5 ms, 256 transactions or 4 MiB, whichever
comes first) and evaluates compares against an in-memory key index. redb's page cache is set to
256 MiB. Writers encode values with `racksmith-storage-model`, so the engine stores opaque bytes.

Acceptance of this ADR is gated on a week-3 benchmark of redb and rusqlite on the CI runner class
(10k machines, 200k index keys, 7 days of events, 50 txn/s steady and a 5,000 txn/s burst with 200
watchers; pass: commit p99 < 20 ms in the burst, file < 1 GiB, snapshot < 5 s, restore < 30 s). If
redb fails, rusqlite implements the same trait.

For M2 we plan **openraft**, pinned exactly and wrapped in `racksmith-store-raft`, with redb as the
state machine and the same evaluate-and-stage function applying log entries on every replica. We
will open an upstream issue to remove the `chrono` dependency; a time-boxed exception, if needed, is
an amendment under [ADR 0028](0028-supply-chain-policy-amendments.md).

## Consequences

- F1 does not wait for this ADR: the trait is frozen by ADR 0029, and a switch to rusqlite after the
  benchmark changes only `racksmith-store-redb`.
- Installing RackSmith stays one binary and one data directory, and the M1 store already behaves
  like a single-replica Raft cluster, so M2 inserts consensus below the apply loop.
- Ad-hoc inspection needs `racksmith admin dump --format jsonl` instead of a SQL shell.
- `redb` is banned outside the store crates ([ADR 0011](0011-evolve-workspace-into-ports-core-adapters.md)),
  so the engine stays swappable.
- The M2 plan carries a dependency risk (openraft is pre-1.0 and blocked by `chrono` today) that
  must be resolved or excepted before M2.
