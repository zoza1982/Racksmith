# 0014. Drive each machine through one pure decide step with operation records, fencing and grants

- Status: Accepted
- Date: 2026-11-18

## Context

Reconcilers cause the dangerous side effects: BMC power changes, boot overrides, plans that write
disks. They must be idempotent and crash-safe (R39), safe against stale state after a restart, a
restore or (in M2) a deposed leader, and they must never write the wrong disk. AGENTS.md already says
adapters translate and the domain decides. The [PRD](../prd.md) principle "nothing destructive
happens without an explicit state transition" needs a mechanism that code can check.

Options considered:

1. **Per-flow reconcilers** that each contain their own orchestration logic. Familiar, but the logic
   spreads across async code that is hard to model-check.
2. **A workflow engine** with persisted steps. Powerful, but a large dependency and a second model of
   state next to the lifecycle.
3. **One actor per machine around one pure, total function** in `racksmith-domain`, with operation
   records written before effects, a fencing tuple, and capability types for destructive effects.

## Decision

We will take option 3 ([system design §4.3–§4.4, §7.3](../design/system-design.md#43-operations-fencing-and-grants)):

- **`decide(machine, observations, now, timeouts) -> Decision`** is pure and total. A decision holds
  at most one lifecycle event, one operation-record change and one side effect. `MachineReconciler`
  is the only component that causes side effects for a machine, and it only executes decisions.
- **Two-step protocol:** a reconcile that starts work commits an operation record (the intent) and
  emits no effect; a later reconcile, restoring the machine from that committed record, emits the
  effect. `decide()` never returns an effect for an uncommitted operation (invariant I12).
- **Fencing:** every operation carries `Fencing { term, generation, op, attempt }`; every plan and
  agent report carries the same tuple, and report dedup is by `(op, attempt, seq)` with the high-water
  mark reset when `attempt` changes, so a fresh agent on a retry is not mistaken for a replay and a
  late report from an earlier attempt is rejected. In M1 `term` is always 0 and
  the `LeaderGate` port is `AlwaysLeader` (plus an observe-only switch used after a restore), so M2 Raft
  adds leadership without signature changes. Agents reject plans whose term is below the highest they
  have seen, and the gateway pushes a plan only if `(generation, op, attempt)` still match and only
  on the session bound to the attempt ([ADR 0017](0017-agent-enrollment-and-built-in-ca.md)).
- **Effects are observed, not assumed:** `Observations` carries a `BmcObservation` (power, the boot
  override as read back, and the outcome of the last action for `(op, attempt, phase)`). An
  `*Issuing` phase advances only on matching evidence, and failed outcomes are classified in the
  domain (retry with backoff, wait with a condition, or fail the attempt), never in the adapter.
  Hand-off retries after `ImageWritten` have their own phases (`HandoffPowerOff`,
  `HandoffOverride`, `HandoffPowerOn`), because `EnsurePower(On)` is a no-op on a hung machine.
- **Effects never block reconcile workers:** an action is handed to a per-BMC effect executor (or to
  the agent session hub) with a non-blocking send; only executors wait on BMC semaphores, rate
  limiters, timeouts and vendor retries, and their outcomes return as observations. Work-queue lanes
  use weighted fair dequeue with reserved workers for user intents and agent reports.
- **Grants:** `DiskWriteGrant` and `PowerGrant` have no public constructor, no `Clone` and no
  `Deserialize`; only `Machine::grant_disk_write` and `Machine::grant_power` mint them, for the open
  operation of the right kind in the right state (Kani-proved). `PowerDriver::ensure_power` and
  `set_boot_override` take a `&PowerGrant`, and disk-writing plans are built from a `DiskWriteGrant`.
  The agent's `Plan::validate` is a consistency check, not an authorization; a signed grant can be
  added later.
- **Target disk pinned:** commissioning records a stable `DiskId` (WWN or EUI-64/NGUID, otherwise
  model + serial); a disk with neither is not eligible. `resolve_root_disk` resolves the pool's rule
  to exactly one `DiskId` at `allocate`; the agent writes only to a device whose identity matches and
  otherwise fails with `TargetDiskNotFound`. There is no fallback.
- **Verification:** a hand-written `specs/tla/DeployProtocol.tla` (crash, stale agent, user
  `mark_broken` race, attempt retry, hand-off retry, `NewLeader`) checks
  `WritesOnlyUnderCommittedOp`, `NoStaleCompletion`, `NoDeposedLeaderWrite`,
  `PhaseAdvancesOnEvidence`, `NoSecondWriteAfterImageWritten` and `OpEventuallyClosed` on every PR; a
  conformance test checks `decide()` against its `DECIDE_RULES` table; deterministic simulation
  injects crashes at every await point. Generating the TLA+ step relation from `DECIDE_RULES` is
  stretch work.

## Consequences

- Orchestration is testable without I/O, model-checkable, and reviewable in one place; adapters and
  reconcilers stay thin.
- Every effect costs one extra store commit and one reconcile round (the two-step protocol), which is
  milliseconds against deploys measured in minutes.
- A destructive call without a lifecycle reason does not compile, and the wrong-disk failure mode is
  closed, at the cost of commissioning every machine before its first deploy.
- `DeployProtocol.tla` is a second formal model to maintain alongside the lifecycle model.
