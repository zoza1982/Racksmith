# 0035. Make what is missing deterministic with a completeness layer

- Status: Proposed
- Date: 2026-11-18

## Context

Every guardrail so far finds what is **wrong** in the code: lints, `cargo xtask arch`, tests,
property tests, fuzzing, Kani and TLA+. None of them finds what is **missing**. A retrofit of the
same guardrail setup onto another repository found spec fields that admission accepted and the
scheduler silently ignored, user policies that were never evaluated, and whole subsystems that
were designed and built but never called. Each passed every check, because a check can only judge
the code that exists.

The playbook these guardrails follow (`oss-bootstrap-playbook.md` §4.1, "Completeness: make
what's missing deterministic") answers this with four mechanisms, strongest first:

1. Fail closed on unimplemented inputs: an input that the implementation does not honour is
   rejected at admission with a typed error naming the field and its tracking reference, never
   accepted and ignored.
2. Requirements traceability as code: a `requirements.toml` maps every section of the
   authoritative design document to a status, and a check fails on an unclassified section, an
   "implemented" claim whose test does not exist, or a bad tracking reference. A generated coverage
   table is drift-checked.
3. Subsystem wiring in `architecture.toml` (`arch` rule 7): each subsystem names its entry symbol
   and the package that must call it, or is planned.
4. Field-effect property tests: the same workload runs twice with one input changed, and the
   observable outcome must differ.

RackSmith is about to build M1 from a large design
([system design](../design/system-design.md), 102 leaf sections) with agent fleets working in
parallel ([M1 plan](../plan/m1.md)). That is exactly when a section can be skipped or an input
accepted without effect, and nobody notices until the benchmark or a user does. `xtask/` and
`architecture.toml` are code-owned guardrails ([ADR 0003](0003-trust-pyramid-guardrails.md)), so
adding the layer needs this ADR.

The options were: adopt the playbook as written (tracking references are GitHub issues); adopt it
with the M1 plan as the tracker; or rely on review. Review is what the retrofit had, and it missed
all three kinds of gap.

## Decision

We will add the completeness layer of the playbook, with the adaptations below.

**Tracking references are M1 plan task IDs, not GitHub issues** (owner decision). The plan
already breaks M1 into one-PR tasks with IDs, and the repository has no issue backlog yet. A
reference is written `M1-A-01` or `M1-H-01`. The plan's work-breakdown tables omit the prefix
(`A-01`) and its lab tables write it (`M1-H-01`). `xtask/src/plan_tasks.rs` reads every table row
of [docs/plan/m1.md](../plan/m1.md) whose first cell is a task ID. A task is done when that cell
is struck through (`~~A-01~~`). Referencing a malformed, unknown or done task is an error, so a
gap cannot stay "tracked" by a task that is already closed. The check needs no network access and
gives the same answer locally and in CI.

**`cargo xtask spec`** (`xtask/src/spec.rs`, `requirements.rs`, `design_sections.rs`,
`rust_items.rs`, `test_listing.rs`, `finding.rs`) checks [`requirements.toml`](../../requirements.toml):

- `[sections."<N.M>"]`: one entry for every leaf section of the system design. A leaf is every
  numbered `### N.M` heading, plus every numbered `## N.` heading without numbered `###`
  children. Unnumbered headings and headings inside code blocks are not sections. A heading that
  starts with a digit but is not numbered in one of those forms (`## 17 Title`, `### 4.1`,
  `### 4.1.2 Title`) is an error, and so is a `### N.M` under a `## K.` with `K != N`, so no
  section can drop out through a typo. `title` must
  equal the heading text, so a renamed or renumbered section fails until someone re-checks its
  status. `status` is one of:
  - `implemented`: needs `entry_points` and `tests`;
  - `partial`: needs `tasks` and `missing`, and may have `entry_points` and `tests`;
  - `planned`: needs `tasks`;
  - `informative`: needs `reason`.

  Every other field is forbidden for that status. An unknown key, a missing leaf and a title
  mismatch are errors.
- `informative` is a deliberate extension of the playbook. Some sections state no buildable
  requirement: risks, references, decision logs, ownership lists, open questions. Calling them
  "implemented" would be false, and calling them "planned" would invent tasks. `informative`
  makes the claim explicit and asks for the reason, so a reviewer can disagree with it.
- `entry_points` are paths that start with a product crate (`racksmith-*`) written as an
  identifier (`racksmith_api`). Every later segment must be an identifier defined (`fn`,
  `struct`, `enum`, `trait`, `type`, `const`, `static`, `mod`, `union` or `macro_rules!`) in the
  non-test sources of that crate, so a name defined only in another crate does not count.
- `tests` and `effect_tests` are `<package>::<libtest path>`, as
  `cargo test -p <package> -- --list` prints the path. Every named test must exist in a product
  crate (a guardrail's own unit test proves nothing about the product), and must not be
  `#[ignore]`d, because the gate never runs an ignored test.
- `[inputs."<package>::<Type>.<member>"]`: one entry for every field and every variant of every
  type in a product crate's non-test sources that derives serde `Deserialize` or clap `Parser`,
  `Args`, `Subcommand` or `ValueEnum`. The fields of a variant are inputs too, keyed
  `<Type>.<Variant>.<field>` for a struct-like variant and `<Type>.<Variant>.0` for a tuple
  variant. Fields marked `skip` or `skip_deserializing` are not inputs. Inputs that no derive
  declares are classified the same way: `<package>::route.<pattern>.<param>` for every
  `{param}` segment of a router path pattern (`.route("/machines/{id}", ...)`, also `nest`), and
  `<package>::env.<NAME>` for every environment variable read at run time (`env::var("NAME")`,
  `env::var_os`, `EnvFilter::from_env*` and `from_default_env`, which read `RUST_LOG` unless a
  `with_env_var` names another, and `#[arg(env = "NAME")]`). An environment read whose name is
  not a string literal, `env::vars()` and a bare `#[arg(env)]` are errors, because what they read
  cannot be classified. A `Deserialize` type with named fields must carry
  `#[serde(deny_unknown_fields)]` and no `#[serde(flatten)]` field (which defeats it): otherwise
  it accepts and ignores every field it does not know. A status is one of:
  - `implemented`: needs `effect_tests`, which prove that changing the input changes an
    observable outcome (mechanism 4);
  - `rejected`: needs `tasks`, and `tests` proving that admission rejects the input with a
    typed error naming the field (mechanism 1).

  An unclassified, unknown or malformed input is an error.
- `docs/requirements.md` is generated: a summary, one row per section and one per input.
  `cargo xtask spec` fails if it is stale, and `cargo xtask spec --fix` rewrites it.
- The command prints what it checked: sections and inputs classified, tests and tasks resolved.
  It fails if it found no sections, no inputs or no tests, because then it looked in the wrong
  place.

Code is read with the token-level tools that `cargo xtask arch` already uses, so a name in a
comment or a string is never a definition or a reference. Test-only code is excluded: `tests/`,
`benches/` and `examples/`, items under `#[cfg(test)]` (and `#[cfg(kani)]`, or `all(...)` with one
of them as a conjunct), and modules declared `#[cfg(test)] mod name;`. An item under
`cfg(any(test, feature = "x"))` is product code, because the feature builds it.

**The test listing comes from `cargo test --no-run --message-format=json` and each binary's
`--list`, not from cargo-nextest.** nextest is optional in `cargo xtask ci`, and a step whose tool
is missing is skipped locally. The traceability check must never be skippable. Cargo reports
which package built each test binary, so integration tests with the same file name in different
packages stay apart. The same tests are built with the same arguments as the gate's test step, so
the step costs little beyond listing. Doctests are not referenced.

**`arch` rule 7** (`xtask/src/subsystem_rules.rs`, `architecture.rs`, `violation.rs`): each
`[subsystems.<name>]` of `architecture.toml` has a `status`, an `entry` identifier and a one-line
`reason`.

- `wired` needs `crate`, the package whose non-test sources define `entry`, and `consumer`, the
  package whose non-test sources must use it. A use is an identifier match outside comments,
  strings and test code, other than the definition itself. The consumer must declare `crate` as
  a normal dependency and must not define an item named like `entry`, which would make its uses
  its own. The consumer may be the defining crate only if that crate is the composition root (the
  single highest-layer product crate, with a binary target), and then the use must be in its
  binary targets (`src/main.rs`, `src/bin/`, or every file of a crate without `src/lib.rs`).
- `planned` needs `tasks` (open plan tasks), and `entry` must appear nowhere in the workspace,
  tests included: no half-wired code.

Unknown keys are rejected, and `cargo xtask arch` prints how many subsystems it checked and fails
if there are none, because then rule 7 examined nothing. The
subsystem entries themselves are an architectural record, added to `architecture.toml` with this
ADR.

**What we are not adding yet**:

- No fake clock. Mechanism 4 asks for a deterministic simulation with a fake clock, but nothing
  in the product is time-based yet, and unused code is banned (dead code fails the lints). The
  clock arrives with the first time-based behaviour (leases, retries, timeouts), together with its
  effect tests.
- No `NotYetSupported` error variant. It arrives with the first input that is actually
  `rejected`, for the same reason. Until then, every input must be `implemented` with an effect
  test.

The gate runs the check: `cargo xtask ci` has a "requirements traceability" step after "generated
docs", and the `arch` job of `.github/workflows/ci.yml` runs `cargo xtask spec` after
`cargo xtask docs`. AGENTS.md, CONTRIBUTING.md and [docs/trust-pyramid.md](../trust-pyramid.md)
point at it.

## Consequences

- A gap is now visible and cannot ship by accident. A new design section without a status, an
  "implemented" claim without a test, a new input that nobody classified, or a subsystem that is
  built but never called fails CI. The fix is one of three explicit choices: build it, reject it,
  or track it.
- Each feature lands as one change. It flips its `requirements.toml` status to `implemented`
  with tests, removes its admission rejection, adds its effect test, and strikes its task through
  in the plan. The diff of `requirements.toml` shows reviewers what a PR claims.
- Editing the design or the plan now has a cost. Renaming a section, renumbering it, or striking a
  task through requires updating `requirements.toml` (and the subsystem entries) in the same PR.
  This is intended: the classification must be re-checked when its basis changes.
- `cargo xtask spec` builds every test binary, so it is slower than the other xtask checks. In
  the gate the tests are already built, so the cost is the listing.
- The token-level analysis has no name resolution. Two input types with the same name in one
  package are rejected, because their keys would collide, and so is an input type named `route`
  or `env`. A use of `entry` through a glob or a re-export under another name is not seen, which
  fails loudly. Two limits remain that can pass silently: a wired consumer that depends on
  `crate` and names `entry` without calling it (for example an unrelated method of the same name
  on another type) counts as a use, and an entry point's segments are checked for being defined
  in the named crate, not for being nested in each other (`racksmith_api::AppState::serve` passes
  if both names exist there). An environment variable read through an imported function
  (`use std::env::var; var("X")`) is not seen as an input either. Review covers these.
- The checks prove that a test exists, not that it proves the claim. Whether an effect test
  really shows an effect, or an `informative` reason holds, stays with review, as in the rest of
  the trust pyramid. Mutation-checking effect tests (break the effect, see the test fail) follows
  the manual procedure in [docs/trust-pyramid.md](../trust-pyramid.md).
- Follow-up: once GitHub issues track M1 work, references could also accept issue numbers and,
  in CI, reject closed issues, as the playbook describes. That would need a new ADR.
