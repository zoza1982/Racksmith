# 0013. Extend the M1 lifecycle with a tainted guard and a release failure exit

- Status: Proposed
- Date: 2026-11-18

## Context

In the M0 lifecycle ([docs/lifecycle.md](../lifecycle.md)) a `deployed` or `deploying` machine can
go `mark_broken → broken → mark_fixed → ready` without passing through `releasing`, and is then
allocatable with the previous tenant's disk contents. The only failure exit of `releasing` is
`mark_broken`, which leads into the same hole. The [PRD](../prd.md) (§8, "Lifecycle evolution")
expects M1–M3 to change the lifecycle; every change is a public API change that runs the full
AGENTS.md checklist (Rust table, conformance test, TLA+ model, Kani proofs, generated docs).

Options considered:

1. **Keep the M0 table.** No verification cost, but the gap above stays open.
2. **The minimal safety change:** a `tainted` flag with a guard, `broken → releasing` and
   `release_failed`.
3. **A full v2 table:** option 2 plus an `enrolling` state, the removal of `new → commissioning`, and
   `abort_deployment`. Enrollment is one store transaction, waiting for approval is not destructive,
   and a cancel is expressible as `mark_broken` then `release`, so the extra states and events add
   verification cost without safety value.

## Decision

We will take option 2 ([system design §4.2](../design/system-design.md#42-the-m1-lifecycle-change)):

- The `Machine` aggregate gains `tainted: bool` ("the disk may hold deployed data"), set by `deploy`
  and cleared only by `release_completed`, and `owner` (with `root_disk`), set by `allocate` and
  cleared by `start_commissioning`, `release_completed` and `mark_fixed`. Clearing on
  `start_commissioning` closes the path `ready → allocate → allocated → mark_broken → broken →
  start_commissioning → commissioning → commissioning_succeeded → ready`, which would otherwise end
  in `ready` with an owner and break I7.
- `mark_fixed` and `start_commissioning` from `broken` are rejected while `tainted` is set.
- New transition `broken → releasing` on `release`.
- New event `release_failed`: `releasing → broken`.
- Releasing a machine that was never tainted completes without booting it.
- New invariant I7: `ready ⇒ ¬tainted ∧ owner = None`.

States stay at 8 and events grow to 12, so 96 `(state, event)` pairs are classified. The guards live
in a second table, `GUARDS`, next to `TRANSITIONS`; `cargo xtask docs` renders both into
`docs/lifecycle.md` and `specs/tla/MachineLifecycle.tla`. The implementing PR updates
`MachineState::apply`, the conformance test, the TLA+ model (new variables `tainted` and `owner`, I7,
I3 re-checked), the Kani proofs and the lifecycle prose and diagram together, as AGENTS.md requires.
The `attesting`, `erasing` and `quarantined` states are drafted in the system design and are not
implemented in M1.

## Consequences

- A machine that may hold a tenant's data cannot return to the pool without a wipe, by construction
  and by model checking.
- The release path has a real failure exit, so `mark_broken` is no longer overloaded.
- This changes the public, model-checked table and is an owner decision; it is on the F1 critical
  path (end of week 2).
- `xtask docs` gains a second generated section (guards), and the TLA+ model gains state.
