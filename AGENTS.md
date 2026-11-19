# AGENTS.md

Instructions for AI coding agents working on RackSmith. Humans should read
[CONTRIBUTING.md](CONTRIBUTING.md); everything here applies to them too.

This file is the **top and weakest layer** of the [trust pyramid](docs/trust-pyramid.md). Most
rules are enforced by the compiler, lints, `cargo xtask` or CI, and this file points at those
mechanisms instead of repeating them. What is left here is judgment that code cannot check.

**What to build** is defined in the [PRD](docs/prd.md): requirement IDs (R1…), milestones and the
decisions in §9. Reference requirement IDs in PR descriptions. Changing scope, targets or a
decision there is a product decision for the owner, never a side effect of a code change.
The M1 work breakdown (task IDs, dependencies, gates) is [docs/plan/m1.md](docs/plan/m1.md).

## The gate

```sh
cargo xtask ci
```

Do not claim a task is done until this passes. It runs every check of the required `ci-success`
check that works on one machine, with the same arguments: format, clippy, tests, doctests,
rustdoc, cargo-deny, architecture, generated docs, spelling, unused dependencies and workflow lint
(for the workspace and for `fuzz/`). Only CI runs the MSRV build, the coverage floor, the macOS
and Windows jobs, and the `Verify` workflow (TLA+, Kani, fuzzing), which is the second required
check, `verify-success`. If the gate reports a skipped tool, say so in your summary; in CI a
missing tool is an error.

A failing check means the change is wrong, not the check. Fix the code. Never pass the gate by
weakening it.

## Architecture

The workspace is layered. A crate may depend only on crates in a strictly lower layer:

| Layer | Crate              | Role                                                       |
|-------|--------------------|------------------------------------------------------------|
| 0     | `racksmith-domain` | Pure model: types, lifecycle state machine. No I/O, async. |
| 1     | `racksmith-api`    | Adapters: HTTP via axum, in-memory registry.               |
| 2     | `racksmith-cli`    | The `racksmith` binary, the composition root.              |
| -     | `xtask`            | Repository tooling. Depends on no product crate.           |

[`architecture.toml`](architecture.toml) is the machine-checked source of truth (layers, per-crate
dependency allowlists, banned crates, banned source patterns) and `cargo xtask arch` enforces it.
See [docs/architecture.md](docs/architecture.md) for the reasoning.

- New domain logic goes in `racksmith-domain`, behind types that make invalid states
  unrepresentable (newtypes with validating constructors, enums, exhaustive `match`).
- Adapters translate; they do not decide. If an adapter grows an `if` about machine states,
  that logic belongs in the domain.
- Adding a dependency: declare it once in the root `[workspace.dependencies]`, inherit it with
  `workspace = true`, and add it to the crate's allowlist in `architecture.toml`.
- Changing `architecture.toml` is an architectural decision: add an ADR in the same PR.

## Lints and escape hatches

The lint policy lives in the root [`Cargo.toml`](Cargo.toml) (`[workspace.lints]`) and
[`clippy.toml`](clippy.toml) (banned types, methods and macros). Read the error, then fix the code.

- If a lint is genuinely wrong for one item, use
  `#[expect(clippy::some_lint, reason = "why this case is the exception")]` on the smallest
  possible item. `expect` fails the build once the exception is no longer needed.
- Lint levels are set only in `Cargo.toml`. `cargo xtask arch` rejects `allow`, `warn`, `deny`
  and `forbid` attributes (including inside `cfg_attr`), inner `#![expect]` and `expect` on a
  whole module.
- Never edit `[workspace.lints]`, `clippy.toml`, `deny.toml`, `architecture.toml`, `rustfmt.toml`
  or CI workflows to make a change pass. Those files are code-owned guardrails; changing them
  needs an ADR and maintainer review.

## Errors

- Libraries (`racksmith-domain`, `racksmith-api`) return typed errors defined with `thiserror`, so
  callers can `match` on them. Name the variant after what went wrong, and keep messages
  lowercase without trailing punctuation.
- `anyhow` is allowed only in `racksmith-cli` and `xtask` (enforced by `architecture.toml`). Add
  `.context(...)` where an error crosses a boundary the reader would not otherwise see.
- No `unwrap`, `expect`, `panic!`, indexing or lossy `as` casts outside tests (lints deny them).
  Propagate with `?`, or restructure so the failure cannot happen.

## Determinism

- Use `BTreeMap`/`BTreeSet`; `HashMap`/`HashSet` are banned so output order, snapshots and
  serialization are reproducible.
- No wall-clock time, randomness or environment reads inside the domain layer; pass values in.
- Tests must not depend on timing, network access or test execution order.

## Testing

- Every behavior change comes with unit tests in a `#[cfg(test)] mod tests` next to the code.
- Invariants get property tests with `proptest` (see `crates/racksmith-domain/tests/`).
  Parsers get a round-trip property: `parse(display(x)) == x`.
- API behavior is tested in-process with `tower::ServiceExt::oneshot`. Only server lifecycle
  tests (`serve`, shutdown, signals) use real loopback sockets or spawn the binary.
- Code that parses untrusted input should have a fuzz target in [`fuzz/`](fuzz/).

### Changing the machine lifecycle

The transition table in [docs/lifecycle.md](docs/lifecycle.md) is the contract. A change to it
must update, in the same PR:

1. `MachineState::apply` and the `TRANSITIONS` table in
   `crates/racksmith-domain/src/lifecycle.rs` (a test keeps the two in agreement).
2. The spec-conformance test in `crates/racksmith-domain/tests/lifecycle_props.rs`.
3. The generated tables: run `cargo xtask docs --fix`, which rewrites the table in
   `docs/lifecycle.md` and the `Transitions` set in `specs/tla/MachineLifecycle.tla` from
   `TRANSITIONS` (`cargo xtask docs` fails CI if either is stale). Then run
   `specs/tla/check.sh`.
4. The TLA+ properties and Kani proofs if an invariant changes.
5. The prose and Mermaid diagram in `docs/lifecycle.md`.

## Completeness

Nothing designed may be missing silently ([ADR 0035](docs/adr/0035-completeness-layer.md)).
`cargo xtask spec` and `cargo xtask arch` enforce it; tracking references are M1 plan task IDs
(`M1-A-01`) from [docs/plan/m1.md](docs/plan/m1.md), never a task that is already struck through.

- **New or renamed design section** in [system-design.md](docs/design/system-design.md): classify
  it in [`requirements.toml`](requirements.toml) as `implemented` (entry points and the tests that
  prove it), `partial` (what is missing and its tasks), `planned` (tasks) or `informative` (why it
  states nothing buildable). Then run `cargo xtask spec --fix` to regenerate
  [docs/requirements.md](docs/requirements.md).
- **New input** (a field or variant of a `Deserialize` or clap type, a `{param}` of a router path,
  or an environment variable read): either `implemented`, with an effect test proving that
  changing it changes an observable outcome, or `rejected` at admission with a typed error naming
  the field, a task and a test. Never accept and ignore an input: a `Deserialize` struct carries
  `#[serde(deny_unknown_fields)]`.
- **New subsystem**: declare it under `[subsystems]` in `architecture.toml` as `wired` (its
  consumer calls the entry outside tests) or `planned` (tasks, and no code yet). Do not land a
  subsystem that nothing calls.
- Implementing a gap is one change: flip its status to `implemented` with tests, remove its
  rejection, and strike its task through in the plan.

## Commits and pull requests

- PR titles follow [Conventional Commits](https://www.conventionalcommits.org/):
  `feat(api): add machine tags`. PRs are squash-merged, so the title is the commit on `main`.
- One logical change per PR. Refactors and behavior changes go in separate PRs.
- Fill in the PR template, including the AI assistance section.
- Agents do not merge, force-push or approve their own work. Agents open pull requests from a
  dedicated bot account, never from a maintainer's account, so that the maintainer's review is an
  independent one (see [GOVERNANCE.md](GOVERNANCE.md)).

## Do not

- Do not leave commented-out code or dead code. A `TODO` or `FIXME` must reference an open
  issue, as in `TODO(#123)` (enforced by `cargo xtask arch`); prefer finishing the work.
- Do not copy a pattern you would not want copied again. The codebase is the primary input for
  every future change: keep it exemplary.
- Do not add a dependency for something a few lines of code can do.
- Do not change public names in the domain API without updating every caller and the docs.
- Do not touch secrets, BMC credentials or real hardware in tests; use fakes.
- Do not "fix" a failing test by changing its expectation unless the old expectation was wrong,
  and say why in the PR.
