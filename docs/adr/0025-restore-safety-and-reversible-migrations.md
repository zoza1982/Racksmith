# 0025. Make restore a safety event and keep storage migrations reversible

- Status: Proposed
- Date: 2026-11-18

## Context

[ADR 0009](0009-embedded-store-behind-trait.md) makes backup and restore RackSmith's job and requires
automatic migrations that are reversible within a minor version. A snapshot is older than reality:
restoring one and acting on its open operation records could power-cycle or wipe a machine that has
since been redeployed. prost drops unknown fields, so an older binary cannot preserve fields it does
not know ([system design §6.6–§6.7](../design/system-design.md#66-backup-restore-and-restore-safety)).

Options considered: **plain restore and forward-only migrations**; **restore as a safety event with
reversible, typed migrations**.

## Decision

- **Storage encoding** is an internal proto (`racksmith.storage.v1`) kept out of the public API in its
  own pure crate, `racksmith-storage-model` (layer 1), with the key schema, the codec and the
  migrations, so public `v1alpha1` changes do not force migrations and every writer and both store
  engines share one codec. `Machine` is `Serialize` but not `Deserialize`
  and is rebuilt only through `Machine::restore`, which re-validates every invariant.
- **Migrations** are typed `up`/`down` pairs in a `MIGRATIONS` table. On startup, older data triggers
  an automatic pre-migration snapshot and then `up`; newer data makes the binary refuse to start and
  print the `racksmith admin migrate --to <N>` command to run with the newer binary. A minor release
  may contain only migrations with a `down`. Tests check `down(up(x)) == x`; a nightly job runs
  N-1 → N → N-1 with live data.
- **Snapshots** (`racksmith admin snapshot`, scheduled every 6 h, keep 8) contain only ciphertext for
  secrets and never the KEK.
- **Restore** (`racksmith admin restore`) always: bumps a restore epoch; supersedes every open
  operation and moves machines with an open deploy or release to `broken` with reason
  `restored_from_snapshot` (keeping `tainted`); bumps every machine's `enrollment_generation`; and
  starts the server in **observe-only mode** (`LeaderGate` refuses effects) until an admin runs
  `racksmith admin resume`.
- **Restore fences every machine, not only those with open operations.** A machine that was `ready`
  or `allocated` in the snapshot may have been deployed since, and a `deployed` one may have been
  redeployed for someone else. Every machine that is not `new` gets the condition `RestoreUnverified`,
  and machines in `allocated`, `deployed` or `broken` are also marked `tainted`. While the condition
  is set, `decide()` emits no event except `mark_broken` and no action, so nothing is allocated,
  released or wiped. It clears per machine through `racksmith admin restore-verify` after
  `racksmith admin restore-report` has shown the observed BMC power and identity; a machine observed
  powered on with no open operation must be verified by name. The report and the verify command are
  part of the never-cut restore minimum. The nightly restore drill deploys after the snapshot and
  checks that the restored machines cannot be allocated or released before verification.

## Consequences

- A stale snapshot cannot replay old operations against live machines, and old agent certificates
  and plans die with the generation bump.
- After a restore, an operator must look, resume and verify; machines that were mid-deploy need a
  release, and at 10k machines verification relies on `restore-verify --all` for machines whose
  observed state matches.
- Every migration costs a `down` implementation and tests; reversibility comes from code, not from
  field preservation.
