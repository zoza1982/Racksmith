# 0026. Build racksmith-sim and split the lab into per-PR and nightly tiers

- Status: Proposed (amends the per-PR scope of [ADR 0010](0010-virtual-lab-ci-testbed.md); if accepted,
  ADR 0010's status is updated to point here)
- Date: 2026-11-18

## Context

[ADR 0010](0010-virtual-lab-ci-testbed.md) makes a virtual lab (QEMU VMs, emulated Redfish and IPMI)
and a 10k-machine simulated fleet the CI test bed. Running sushy-tools and VirtualBMC per VM on every
pull request is slow and flaky, a lab of many scenarios per PR does not fit a short gate, and PRs from
the agents' bot account need end-to-end evidence without exposing a self-hosted runner in a public
repository ([system design §13.1–§13.2](../design/system-design.md#13-testing-and-verification-strategy)).

Options considered: **sushy-tools + VirtualBMC per VM on every PR**; **our own emulator only**;
**our own emulator plus nightly conformance against sushy-tools and VirtualBMC as oracles**. For
runners: **self-hosted for PRs**, **GitHub-hosted runners with KVM**, **label-gated ephemeral
runners**.

## Decision

- **`racksmith-sim`** (layer 7) is one Rust tool with two modes. *QEMU mode* exposes a Redfish (and
  optionally IPMI) endpoint per VM, spawns QEMU with network or disk first according to the pending
  one-time override, stops it through QMP, and supports POST-delay profiles (0, 90, 180, 240 s). QEMU
  runs with `-action reboot=shutdown`, so a guest reboot exits QEMU; the sim treats the exit as a
  reset: it applies the pending one-time override, sleeps the POST delay again and respawns. That
  models POST #2 and the real-BMC hand-off (`Disk, Once` then reboot), and the lab asserts that the
  second boot of every deploy came from disk through the override.
  *Fleet mode* serves 10k BMC endpoints on `127.0.0.0/8` with vendor latency and error profiles from
  recorded fixtures, lays the fleet out 40 machines per rack label so the inrush limiter is
  exercised, and runs simulated agents linked against the real `racksmith-agent` library with a fake
  block device and a virtual-image origin, plus a real-bytes subset that includes a cold-cache test
  (40 agents through one cold nginx cache, origin egress within 1.1 × the blob size).
- **Per-PR `lab-e2e`** (target 15 minutes or less): one UEFI VM, the small Debian recipe, proxyDHCP,
  Redfish, commission → deploy → OS-ready → release, plus a `kill -9` of the control plane during
  the write. Artifacts (logs, pcaps, deploy record) are uploaded for reviewers. It is functional
  evidence, not the p95 verifier. It becomes a required check once stable.
- **This narrows ADR 0010**, which asks for full-DHCP boots and the emulated IPMI path on every
  relevant pull request: here they run nightly. The owner decides between this and a second per-PR
  VM (about 8 more minutes per PR).
- **Nightly, split into parallel jobs** under the 6-hour hosted-job limit: `lab-nightly-bench` (the
  2 GB image, `post-0`, 60 runs on a dedicated runner with local NVMe and `cache=none`; the M1 gate is
  the empirical p95 and its one-sided 95% upper confidence bound, both under 300 s);
  `lab-nightly-functional` in three shards (all five recipes, full DHCP, a relayed topology, legacy
  BIOS, UEFI HTTP Boot, IPMI, a BMC-less machine, rogue DHCP, boot-token and session conflicts,
  canary, restore drill, upgrade test, quickstart, Secure Boot smoke, write-zeroes fallback, hand-off
  retry, sushy-tools and VirtualBMC conformance); and the 10k load lab with every §7 gate.
- **Weekly:** POST profiles 90, 180 and 240 s with 20 runs each, one job per profile, published as
  indicative, never as the gate.
- **Runners:** the per-PR job runs on GitHub-hosted Linux runners with KVM enabled, which works for
  bot and fork PRs without secrets; this is verified in week 1. The fallback is an ephemeral
  self-hosted VM runner per job, without secrets or persistent state, started for bot PRs by a
  maintainer label. The load lab runs on a larger or self-hosted runner, scheduled on `main` only. Any
  lab job fails loudly when `/dev/kvm` is missing.

## Consequences

- PRs get end-to-end evidence quickly; breadth moves to nightly, so a regression in a nightly-only
  path is found within a day instead of before merge.
- We own an emulator and must keep it honest; the nightly oracles check our clients against
  independent implementations.
- The hosted-KVM assumption must be verified in week 1; CI cost depends on the outcome, and the
  timed benchmark needs a dedicated runner whose class is recorded with every result.
