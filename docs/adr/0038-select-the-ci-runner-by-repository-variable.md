# 0038. Select the CI runner by repository variable

- Status: Proposed
- Date: 2026-11-26

## Context

Every Linux job in `ci.yml`, `verify.yml` and `pr-title.yml` ran on `ubuntu-latest`, a
GitHub-hosted runner, and the M1 plan kept them there: standard hosted runners are free for a
public repository, and self-hosted capacity was reserved for the KVM lab (owner decision 10,
ADR 0026). Another of the maintainer's repositories, a private one, moved its CI to the home K3s
cluster when the account's billing blocked its hosted runners. This repository is not billed for
standard hosted runners, so it does not need to move, but it should be able to move in one step
if hosted runners ever become unavailable to it (billing state, outages, queueing). The owner
decided (2026-11-26) to keep a self-hosted runner on standby for that case.

This repository is **public**. A self-hosted runner executes whatever workflow code a run brings,
and with `pull_request` a fork's run uses the fork's version of the workflows. GitHub's own
guidance is that self-hosted runners should not serve public repositories' fork pull requests.

Options considered:

1. Hard-code a self-hosted label in every `runs-on`. Fork pull requests would reach the runner,
   and moving back would need another workflow change.
2. Read the label from a repository variable for every run, as the other repository does. Its
   ADR notes that this is safe only because that repository is private.
3. Read the label from a repository variable, but only for runs whose code comes from this
   repository; fork pull requests always use `ubuntu-latest`.

`.github/workflows/` is a code-owned guardrail (AGENTS.md), so the choice needs this record.

## Decision

We will take option 3. Every Linux `runs-on` in `ci.yml`, `verify.yml` and `pr-title.yml` is

```yaml
runs-on: ${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository) && vars.CI_RUNNER || 'ubuntu-latest' }}
```

and the `clippy` and `test` matrices substitute it only for their `ubuntu-latest` entry:

```yaml
runs-on: ${{ matrix.os == 'ubuntu-latest' && (<same condition>) && vars.CI_RUNNER || matrix.os }}
```

- **Same-repository runs** (pushes to `main`, the merge queue, the nightly schedule, manual
  dispatch and pull requests from branches of `zoza1982/Racksmith`) use the runner named by the
  repository variable `CI_RUNNER`. Only accounts with write access can create such runs.
- **Fork pull requests** always run on `ubuntu-latest`. The condition is evaluated by GitHub from
  the event, before any job starts. A fork that edits the expression changes its own copy only,
  and its run still needs approval under the fork-approval setting (W0-5).
- **Unset `CI_RUNNER`** sends every job back to `ubuntu-latest`, exactly as before this ADR.
- macOS and Windows jobs, `codeql.yml` and `scorecard.yml` stay on GitHub-hosted runners: the
  first two need those operating systems, and Scorecard's results publishing requires a hosted
  runner.

The standby runner is `racksmith-dietpi-6`: an ARC scale set registered to this repository only,
with ephemeral runner pods (one per job, destroyed after it), at most one at a time, capped at
4 CPU and 8 GiB, pinned to one amd64 node and given no Kubernetes service-account token. It is
defined in the cluster's repository (`helm/apps/arc/racksmith-runner-scale-set-values.yaml`). Its
image is the official ARC runner image plus what the hosted image provides and these jobs assume
(a C toolchain, jq, zstd, Go, shellcheck, Python with pip) and the toolchains CI installs
(rust-toolchain.toml's channel, the MSRV and the fuzz nightly), so the jobs and the gate's
arguments are unchanged.

**`CI_RUNNER` is unset by default**, so CI runs on hosted runners, which are free for this
repository, run about 15 Linux jobs at once and add no load to the maintainer's node. Setting
`CI_RUNNER=racksmith-dietpi-6` switches same-repository runs to the standby runner; it was set
once to prove the path (the Kani, fuzz, rustdoc and PR-title jobs passed on it, 2026-11-26) and
then removed.

This scale set is for ordinary CI. The KVM lab keeps its own scale set and runner model
(M1-H-01, M1-H-04): it adds `/dev/kvm`, network namespaces and the job-started hook, none of
which this runner has.

## Consequences

- CI keeps working if hosted runners become unavailable: setting one variable moves Linux jobs
  to the standby runner, and deleting it moves them back.
- While the variable is set, one runner on one home node is slower than hosted runners: a push
  starts about 15 Linux jobs, which run one at a time, and Kani and coverage compete with nothing but each other. If the
  node is down or reboots (it overheats under sustained load), jobs queue or fail and are re-run.
  Raise `maxRunners` only after measuring the node's temperature under load.
- Contributors from forks get hosted runners and so see the same checks with different timing;
  the required checks `ci-success` and `verify-success` are unchanged.
- Same-repository branches can reach the runner, so write access to this repository is now also
  access to a pod on the maintainer's cluster. Agents keep opening pull requests from the bot's
  fork without write access (M1 plan W0-1), whose runs land on hosted runners.
- Workflows triggered by Dependabot may not receive repository variables; they then fall back to
  `ubuntu-latest`.
- The runner image must be rebuilt when rust-toolchain.toml's channel, the MSRV or the fuzz
  nightly changes; a stale image still works but downloads the toolchain on every job.
