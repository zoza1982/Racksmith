# 0010. Use a virtual lab as the CI test bed; label VM benchmarks honestly

- Status: Accepted
- Date: 2026-11-18

## Context

RackSmith boots, wipes and deploys physical machines. Unit and property tests can't show that a
machine really PXE-boots, takes an image and comes up. The project has no dedicated hardware lab
today ([PRD](../prd.md) §10, top risk). Every pull request still needs end-to-end evidence, and
the under-5-minute deploy target (§7) and the 10,000-machine target need measurement. Options
considered:

1. **Wait for real hardware.** That blocks M1.
2. **Unit tests plus mocks only.** Fast, but proves nothing end to end.
3. **A virtual lab:**
   - QEMU/libvirt virtual machines with UEFI firmware that network-boot for real;
   - an emulated Redfish BMC (for example sushy-tools) and virtual IPMI BMCs;
   - a simulated fleet (emulated BMCs and agents, no real VMs) for scale.

## Decision

We will take option 3.

- **End-to-end lab:** a small set of QEMU VMs, driven through emulated Redfish and IPMI. It runs
  real proxyDHCP and full-DHCP boots, image streaming and first boot, in CI on every relevant
  pull request. It is part of the required checks once it is stable.
- **Load lab:** 10,000 simulated machines (emulated BMC endpoints plus lightweight agent
  simulators) exercise the control plane, store and reconcilers against the scale targets in PRD
  §7. It runs nightly and before releases.
- **Benchmarks** from the lab are published as *virtual-lab* numbers and never presented as
  real-hardware results. The MAAS comparison runs MAAS and RackSmith on the same lab.
- **Hardware fidelity:** drivers are tested against fixtures recorded from real BMCs (Redfish
  responses from several vendors), so behaviour that differs by vendor is covered without the
  hardware itself.
- **Real hardware:** rented hourly bare-metal servers are used for release benchmarks, and a
  partner or donated lab is sought before M3 (GPU, DPU, firmware), which can't be validated
  virtually.

## Consequences

- Every pull request gets end-to-end evidence (boot logs, timings) that reviewers can inspect
  instead of reading every line, which fits the verification tier of the
  [trust pyramid](../trust-pyramid.md).
- The lab is significant infrastructure (nested virtualization in CI, image builds, sushy-tools),
  and it has to be built first in M1, before the boot and deploy code it tests.
- VM timings differ from real hardware (firmware POST time, NIC speeds, disk throughput), so the
  real-hardware claim stays unproven until the rented or partner hardware runs.
- GPU, DPU and firmware features (M3) are blocked on real hardware. That is accepted and tracked
  as a risk.
