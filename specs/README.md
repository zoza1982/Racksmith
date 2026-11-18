# Formal specifications

This directory holds the formal models of RackSmith's most safety-critical logic. They are part of
the verification layer of the [trust pyramid](../docs/trust-pyramid.md): unit and property tests
check the behaviors someone thought to write down, while a model checker explores every reachable
state of the model.

| Model                                              | Mirrors                                              | Checked by                       |
|----------------------------------------------------|------------------------------------------------------|----------------------------------|
| [`tla/MachineLifecycle.tla`](tla/MachineLifecycle.tla) | `MachineState::apply` in `crates/racksmith-domain/src/lifecycle.rs` | TLC, in the `Verify` workflow |

## Running the model checker

```sh
specs/tla/check.sh
```

The script needs bash, curl, `sha256sum` (or `shasum`) and Java 11 or later. On first run it
downloads `tla2tools.jar` (TLA+ tools v1.7.4, the latest stable release) into `specs/tla/.tools/`,
which is gitignored, and refuses to use it unless its SHA-256 matches the hash pinned in the script.
It works from any directory, and passes extra arguments to TLC, for example
`specs/tla/check.sh -coverage 1` for per-action coverage.

The script exits non-zero when TLC finds a false assumption, a violated invariant or property, or a
deadlock. Deadlock checking is deliberately left on: every lifecycle state has an outgoing
transition (`MarkBroken`, or `MarkFixed` from `Broken`), so a deadlock means the model is wrong.

A clean run reports `Model checking completed. No error has been found.` and, with the three
machines in `MachineLifecycle.cfg`, 9,261 distinct states (21 per machine, cubed).

## The machine lifecycle model

`MachineLifecycle.tla` models a finite set of independent machines, each moving through the
lifecycle described in [docs/lifecycle.md](../docs/lifecycle.md). The transition table is the set
`Transitions` of `<<from, event, to>>` triples. It is not written by hand: `cargo xtask docs --fix`
generates it from the Rust `TRANSITIONS` table (between the `BEGIN GENERATED`/`END GENERATED`
comments), in the same order and with the Rust variant names, and `cargo xtask docs` fails CI when
it is stale. TLC therefore always checks the table the code implements. Each TLA+ action is named after, and corresponds to exactly one,
`MachineEvent` variant; the mapping is listed at the top of the module. A rejected event changes
nothing, so it is a stuttering step rather than an action.

Beyond each machine's `state`, the model keeps two history variables so that properties about the
past can be checked as ordinary state invariants:

- `previous[m]`: the state `m` left on its last transition.
- `allocatedSinceReady[m]`: whether `Allocate` fired since `m` last entered `Ready`.

### What is checked

| Lifecycle invariant | TLA+ definition                | Kind                                      |
|---------------------|--------------------------------|-------------------------------------------|
| I1                  | `DeployedOnlyFromDeploying`    | State invariant                           |
| I2                  | `DeployRequiresAllocation`     | State invariant                           |
| I3                  | `ReadyReachableFromEveryState` | `ASSUME`: graph reachability on the table |
| I3                  | `BrokenEventuallyReady`        | Liveness property, under `Fairness`       |
| -                   | `TableWellTyped`               | `ASSUME`: only known states and events    |
| -                   | `TableDeterministic`           | `ASSUME`: one target per `(state, event)` |
| -                   | `TypeOK`                       | State invariant                           |

I4 (no panics, rejected events leave the state unchanged) and I5 (no wildcard match arm) are
properties of the Rust code, not of the model; they are checked by Kani, proptest and the compiler.
See the table in [docs/lifecycle.md](../docs/lifecycle.md#how-the-lifecycle-is-verified).

### Fairness assumptions

`BrokenEventuallyReady` is a liveness property, so it holds only under assumptions about the
environment. `Fairness` states them per machine:

- **Commissioning terminates** (weak fairness on `CommissioningSucceeded \/ CommissioningFailed`):
  a machine does not stay in `Commissioning` forever.
- **Operators eventually repair** (strong fairness on `MarkFixed`): a machine that is `Broken`
  infinitely often is eventually marked fixed. Weak fairness is not enough, because a machine whose
  commissioning keeps failing loops `Broken -> Commissioning -> Broken`, so `MarkFixed` is enabled
  only intermittently. TLC finds that counterexample if `SF_vars` is weakened to `WF_vars`.

Nothing else is assumed to happen: an allocated or deployed machine may legitimately stay that way
forever, so no fairness is placed on allocation, deployment or release.

`SYMMETRY` on `Machines` would shrink the state space, but it is unsound together with liveness
checking, so the configuration does not use it.

## Changing the model

A lifecycle change starts in the Rust table; `cargo xtask docs --fix` then regenerates
`Transitions` here and the table in the docs. The checklist is in
[AGENTS.md](../AGENTS.md#changing-the-machine-lifecycle). After a change to the table or the model:

1. Run `specs/tla/check.sh` and make sure it reports no error.
2. Make sure the check can fail: temporarily break the property you care about (for example, add
   `<<"Ready", "Deploy", "Deploying">>` to the generated `Transitions`, which must violate
   `DeployRequiresAllocation`), confirm TLC reports it, then revert with `cargo xtask docs --fix`.
3. When adding a history variable or property, add it to `TypeOK`, the `.cfg` and the tables above.

To upgrade TLC, change `TLA_TOOLS_VERSION` and `TLA_TOOLS_SHA256` in `check.sh` together, taking the
hash from a download you verified yourself.
