# 0027. Define the deploy timing schema and the benchmark method

- Status: Accepted
- Date: 2026-11-18

## Context

The MVP must prove that RackSmith is the fastest path from power-on to OS-ready, reproducibly (PRD
§1, §4 principle 6, §7, R20). Firmware dominates end-to-end time on real servers (two POSTs on a GPU
server alone exceed 5 minutes), and the M1 lab is virtual
([ADR 0010](0010-virtual-lab-ci-testbed.md)). The MAAS comparison must be fair and attributable
per phase ([system design §9.1, §12.4, §13.7](../design/system-design.md#91-timing-schema-and-phase-budget-prd-7-power-on--os-ready-p95--5-min)).

Options considered: **seven coarse phases**; **fourteen fine phases** with clock-skew correction.

## Decision

- **Phases:** `T_queue`, `T_inrush`, `T_power`, `T_firmware`, `T_pxe`, `T_ipxe`, `T_kernel`, `T_enroll`,
  `T_bwwait`, `T_prep`, `T_stream`, `T_finalize`, `T_handoff`, `T_firstboot`, each bounded by the clock
  that observes it (control-plane BMC calls, boot-service packets per MAC, agent reports, OS-ready).
- **Skew correction:** agent monotonic offsets are converted onto the control-plane clock with
  Cristian's algorithm over the session's `Ping`/`Pong` (minimum-RTT sample), with an error bound of
  ± RTT/2; each phase records its clock.
- **Two headline numbers:** end to end, and the RackSmith-controlled subtotal (`T_ipxe` through
  `T_finalize` minus `T_bwwait`, plus `T_firstboot`), exported as `racksmith_deploy_controlled_seconds`.
  The software target is ≤ 90 s p95 on 10 GbE and NVMe hardware. The planning budget, built from the
  boot-budget research's p95s, puts it at about 142 s, so the target is not claimed as met until it is
  measured on rented hardware.
- **What is gated, and how:** the M1 gate is the **measured** end-to-end p95 of the nightly lab
  benchmark (2 GB image, `post-0`, 60 runs), together with its distribution-free one-sided 95% upper
  confidence bound (the largest of 60 samples). Sums of per-phase p95s are planning figures only:
  quantiles do not add, and rare tail events in different phases push the end-to-end p95 above the
  sum. Hardware columns come from a Monte Carlo model over per-phase distributions with explicit
  tail events (`cargo xtask bench model`), labelled `modelled`.
- **Regression rule:** PRD §11's rule (benchmark p95 tracked per release, no regression over 10%) is
  applied at release time. The nightly gate goes further, as a design choice: a metric fails when its
  median and p95 are both more than 10% worse than the median of the previous 14 nights and a
  one-sided Mann–Whitney test at the 5% level agrees.
- **Labels:** every published number carries `environment` (`virtual-lab`, `simulated`,
  `rented-hardware`, `modelled`), `post_profile` and an environment fingerprint that includes the
  runner class and disk attachment. The lab runs POST profiles 0, 90, 180 and 240 s; the non-zero
  profiles run weekly with 20 samples and are published as indicative.
- **MAAS comparison:** MAAS 3.x and RackSmith on the same host and VM shapes, each on its own
  provisioning bridge so two PXE responders never answer the same VM, power through the same emulated
  Redfish endpoints, one base image build in two variants that differ only in the cloud-init
  datasource (`[NoCloud]` for RackSmith, `[MAAS]` as a MAAS custom `dd-gz` image), and timing
  measured identically from outside (emulator log, pcap, a neutral phone-home collector). A week-2
  spike proves the MAAS side works in the lab. Method, raw
  data and scripts go in `docs/benchmarks/`, and MAAS maintainers are invited to review.

## Consequences

- The benchmark attributes time to the party that controls it, so the claim stays honest on every
  hardware class.
- The schema is part of the public API (`PhaseTimings`) and of the metric names frozen at F3.
- Asking the owner to restate the §7 target per hardware class follows from this decision.
