# RackSmith Product Requirements Document

| | |
|---|---|
| **Status** | Draft v0.1 (from the product interview of 2026-11-18) |
| **Owner** | @zoza1982 |
| **Scope** | Product vision, v1 MVP, and milestones after the MVP |
| **Changes** | Pull requests only. A change to scope, targets or a decision in §9 is a product decision and needs the owner's approval |

> This document is the product source of truth for humans and AI agents. Architecture decisions
> that follow from it are recorded as ADRs in [`docs/adr/`](adr/). When code and this PRD disagree,
> raise it in an issue. Never silently "fix" either one.

---

## 1. Summary

RackSmith is an open-source (Apache-2.0), Rust-native bare-metal provisioning system that does
the job of [Canonical MAAS](https://maas.io/) for **hyperscale and GPU clusters**.

It targets the four places MAAS hurts most:
- **Operational complexity:** RackSmith ships as one binary or as a Kubernetes operator.
- **Deploy speed:** block-image streaming takes a machine to OS-ready in under 5 minutes.
- **OS lock-in:** any OS image is first-class.
- **Imperative workflows:** declarative fleet state, reconciled from Git.

Above that base, it adds a GPU-fleet layer:
- firmware and BIOS baselines;
- GPU, fabric and DPU readiness checks;
- health-driven automatic quarantine;
- reprovisioning a node for a new tenant in seconds;
- hardware-rooted attestation.

**The MVP proves one thing:** RackSmith is the fastest way to take a rack of machines from power-on
to a running OS, and anyone can reproduce that benchmark.

## 2. Problem: what we attack in MAAS

MAAS is mature and widely deployed, and we respect it. These are the trade-offs we make
differently. Each pain maps to a requirement in §6 and a measurable target in §7.

| # | MAAS pain | Why it hurts at GPU/hyperscale | RackSmith answer |
|---|---|---|---|
| P1 | **Install and operations complexity.** Snap/deb packaging, separate region and rack controllers, a required PostgreSQL server, and upgrades that touch all of them. | Every component is another thing to deploy, monitor, back up and upgrade across many sites. | **One static binary** with embedded storage, or the same core as a **Kubernetes operator**. Upgrades are replacing the binary, with zero-downtime upgrades in HA mode. |
| P2 | **Slow deployment.** Each deploy PXE-boots an ephemeral environment and then runs an installer (curtin), which commonly takes many minutes per machine. | Idle GPU nodes are expensive. Slow reprovisioning blocks tenant churn, recovery and fleet-wide upgrades. | **Stream block images** straight to disk from an in-RAM agent, then kexec or reboot. Images are cached, compressed and verified. Target: **under 5 minutes from power-on to OS-ready**, with 1,000 machines deploying at once. |
| P3 | **Ubuntu-first OS support.** Other operating systems need custom images built separately (for example with packer-maas). | GPU fleets run RHEL-family, Rocky, Talos, Flatcar, vendor images and custom golden images. | **Any OS is first-class.** Ubuntu gets no special status. Built-in support for cloud-init, Ignition and kickstart-free configuration. |
| P4 | **Imperative, database-centric workflows.** State lives in the MAAS database and is driven by UI or API calls. A Terraform provider exists, but nothing continuously reconciles firmware, BIOS and OS state. | Fleets drift. Nobody can review, diff or roll back hardware state in Git. | **Declarative desired state** (machines, pools, firmware baselines, OS assignments) kept in Git and **continuously reconciled**, in the same spec/status shape as Kubernetes. |
| P5 | **No first-class GPU-fleet lifecycle.** Firmware baselines, GPU and fabric burn-in, and health-driven quarantine are only possible through custom commissioning scripts. | GPU clusters live or die by firmware consistency, ECC/XID health and fabric (NVLink/InfiniBand/RoCE) correctness. | **Declarative firmware and BIOS lifecycle, GPU readiness checks, and automatic remediation** are built-in features. |

## 3. Target users

| Persona | Needs | Priority |
|---|---|---|
| **GPU/HPC fleet operator** at an AI lab, neocloud or enterprise AI cluster (1k–10k+ nodes) | Fast reprovisioning, firmware consistency, node health, multi-tenancy, automation everywhere | **Primary** |
| **Platform engineer** building Kubernetes or a cloud on bare metal | API-first, Cluster API, Terraform, GitOps | Primary |
| **Hyperscale infrastructure SRE** | HA, observability, predictable upgrades, works with existing NetBox, IPAM and DHCP | Secondary |
| **Homelab / small lab** | Easy install | Served by the single binary, but never shapes trade-offs |

## 4. Product principles

1. **Speed is a feature.** Treat deploy time as a benchmark tracked in CI. Speed regressions are bugs.
2. **Declarative first.** Every capability is expressible as desired state. Imperative commands are a convenience on top.
3. **Pluggable, with batteries included.** Every external system (power, DHCP, IPAM, DNS, image store, inventory) sits behind a trait. A built-in default ships and works out of the box.
4. **One core, two front ends.** The single binary and the Kubernetes operator are thin shells over the same core library and reconcilers.
5. **Safety over convenience.** Wiping disks, changing firmware and power-cycling are guarded by the model-checked lifecycle. Nothing destructive happens without an explicit state transition.
6. **Verifiable claims.** Every performance claim comes with a public, reproducible benchmark harness.

## 5. Goals and non-goals

**v1 goals:** P1–P4 solved for the deploy path. P5 and the GPU layer are designed for in the
architecture and delivered after the MVP (§8).

**Non-goals (for v1):**
- A full web UI. v1 is API and CLI only; a read-only dashboard may come later.
- Virtual machine management (RackSmith provisions bare metal; KVM and LXD hosts are out of scope).
- Replacing NetBox as the DCIM source of truth (we integrate with it).
- Windows images. Planned, not in the MVP.

## 6. Requirements

Priorities: **P0** is the MVP, **P1** is the next milestone, **P2** comes later.

### 6.1 Control plane and deployment model
- **R1 (P0)** A single static `racksmith` binary (Linux x86_64 and aarch64) runs the API, the reconcilers, the boot services and the embedded store.
- **R2 (P0)** Embedded, crash-safe persistent store behind a `Store` trait. No external database is needed.
- **R3 (P0)** The core is a library crate set with no front-end assumptions, so a second front end needs no core changes.
- **R4 (P1)** The Kubernetes operator: CRDs (`Machine`, `MachinePool`, `OSImage`, `FirmwareBaseline`, …) reconciled by the same core.
- **R5 (P1)** HA: 3 or more replicas with consensus-replicated state (Raft) and leader-elected reconcilers. Rolling upgrades with zero downtime.
- **R6 (P1)** Multi-site: a site agent sits next to each rack's network, with one control plane for all sites.

### 6.2 Discovery, boot and network (pluggable)
- **R7 (P0)** Built-in boot services: proxyDHCP (default, so it coexists with existing DHCP), full DHCPv4 server mode, TFTP, HTTP(S) boot, and iPXE chainloading. UEFI first, legacy BIOS supported.
- **R8 (P0)** An unknown machine that PXE-boots is auto-enrolled: it boots the RackSmith agent and reports its identity (MAC, system UUID, serial) and basic inventory.
- **R9 (P0)** `Ipam`, `Dhcp` and `Dns` traits, each with a built-in implementation.
- **R10 (P1)** Adapters: Kea DHCP, NetBox (IPAM plus inventory sync, both directions), PowerDNS or RFC 2136 dynamic DNS.
- **R11 (P2)** DHCPv6 and IPv6-only boot.

### 6.3 Hardware control
- **R12 (P0)** Redfish driver: power on, off and cycle, one-time boot override (PXE/HTTP), and system inventory.
- **R13 (P0)** IPMI driver: power and boot device only.
- **R14 (P0)** BMC credentials are encrypted at rest and never logged. They are redacted in every API response.
- **R15 (P1)** Redfish virtual media boot, for networks without PXE.
- **R16 (P2)** DPU/SmartNIC (NVIDIA BlueField and similar): provision the DPU OS, and use the DPU for isolated network boot.

### 6.4 OS image deployment
- **R17 (P0)** The `racksmith-agent` is a static Rust binary in a minimal initramfs. It streams block images (raw or qcow2, zstd-compressed) straight to the target disk, checks the SHA-256 and the signature, grows partitions, and injects configuration.
- **R18 (P0)** Image catalog: OS images from HTTP(S) or OCI registries, with signatures verified before any write to disk. Built-in recipes: Ubuntu, Debian, Rocky/Alma, Flatcar and Talos.
- **R19 (P0)** First-boot configuration: cloud-init (NoCloud), Ignition (Flatcar/Talos), and SSH keys.
- **R20 (P0)** Deploy progress is streamed over the API (gRPC server streaming), with per-phase timings recorded for benchmarking.
- **R21 (P1)** Fast reprovisioning: kexec into the new image when the hardware allows, keep an image cache on the node and on the site agents, secure-erase between tenants (NVMe format/sanitize, ATA secure erase), and restore a known-good firmware state.
- **R22 (P2)** Distributing images peer-to-peer or by multicast, for 1,000+ concurrent deploys.

### 6.5 Commissioning, firmware and health (the GPU layer)
- **R23 (P0)** Basic commissioning inventory: CPU, memory, disks, NICs, PCI devices and GPUs.
- **R24 (P1)** Declarative BIOS settings and firmware baselines (BMC, BIOS, NIC, GPU) through the Redfish UpdateService. Drift detection and rolling remediation with a configurable blast radius.
- **R25 (P1)** GPU readiness: driver and firmware checks, DCGM diagnostics and burn-in, NVLink/NVSwitch topology checks, and InfiniBand/RoCE link and fabric checks.
- **R26 (P1)** Health and remediation: ECC/XID error and thermal signals, automatic quarantine (a `Quarantined` lifecycle state), and hooks for RMA workflows.

### 6.6 Security and attestation
- **R27 (P0)** mTLS between the control plane and the agents. The agent identity is bound to the enrollment.
- **R28 (P0)** API authentication with tokens; authorization by role (admin, operator, viewer).
- **R29 (P1)** Secure Boot (signed iPXE and shim chain). TPM 2.0 measured boot with remote attestation; a machine that fails attestation can't be allocated.
- **R30 (P1)** Automatic BMC credential rotation. Signed images and provenance (Sigstore/cosign).
- **R31 (P1)** An audit log of every state transition and destructive action.

### 6.7 APIs, declarative state and integrations
- **R32 (P0)** The gRPC API, defined in protobuf, is the source of truth. A REST/JSON gateway and an OpenAPI spec are generated from it. API versioning: `v1alpha1` → `v1`.
- **R33 (P0)** The `racksmith` CLI covers every API capability. Output is human-readable or JSON.
- **R34 (P0)** Declarative resources in spec/status shape, applied with `racksmith apply -f`. The schema is the same one the operator's CRDs use.
- **R35 (P1)** GitOps reconciler: a Git repository is the desired state, continuously reconciled, with drift reported and corrected.
- **R36 (P1)** A Terraform/OpenTofu provider and a Kubernetes Cluster API infrastructure provider.
- **R37 (P2)** Slurm integration: drain, reprovision and return to service.

### 6.8 Observability and operations
- **R38 (P0)** Structured logs (tracing), Prometheus metrics (deploy phase durations, success rate, queue depth), and OpenTelemetry traces across the control plane, agent and BMC calls.
- **R39 (P0)** Every reconciler is idempotent and crash-safe: a control-plane restart mid-deploy resumes or safely rolls back.

## 7. Non-functional targets (benchmarked; release gates)

| Metric | MVP target | Measured in |
|---|---|---|
| Time from power-on to OS-ready, p95, single machine | **under 5 minutes** (2 GB compressed image, 10 GbE equivalent) | Virtual lab (QEMU + emulated Redfish), every release |
| Concurrent deploys per control plane | 1,000 with p95 still under 5 minutes, bandwidth permitting | Load lab (simulated BMCs and agents) |
| Machines managed per control plane | **10,000** | Simulated fleet: 10k emulated BMCs and agents |
| API latency, p99, reads | under 50 ms at 10k machines | Load lab |
| Control-plane memory at 10k machines | under 2 GB RSS | Load lab |
| Install to first deployed machine | under 15 minutes for a new user | Documented quickstart, tested in CI |
| Upgrade | Replace the binary and restart. Store migrations are automatic and reversible within one minor version | CI upgrade test |

**Head-to-head benchmark:** a public harness deploys the same image to the same virtual lab with
MAAS 3.x and RackSmith and reports per-phase timings. The results and the method go in
`docs/benchmarks/`.

## 8. Milestones

The plan assumes one maintainer plus AI agent fleets, with every change going through the
trust-pyramid gates.

| Milestone | Scope | Exit criteria |
|---|---|---|
| **M0 Bootstrap** ✅ | Workspace, guardrails, CI, lifecycle model (TLA+/Kani), governance | Done 2026-11-18 |
| **M1 Fast-deploy MVP** (~12 weeks) | R1–R3, R7–R9, R12–R14, R17–R20, R23, R27, R28, R32–R34, R38, R39. **Virtual lab in CI** (QEMU/libvirt VMs, an emulated Redfish BMC such as sushy-tools, virtual IPMI BMCs). Benchmark harness and simulated 10k-node load lab | A machine goes from power-on to OS-ready in under 5 minutes in the virtual lab, in CI; 5 OS recipes deploy; the 10k simulated fleet meets the §7 targets; MAAS comparison published |
| **M2 Declarative platform** | R4, R5, R10, R15, R29–R31, R35, R36 | Operator and binary pass the same conformance suite; a 3-node HA cluster survives losing its leader mid-deploy; the CAPI provider creates a Kubernetes cluster |
| **M3 GPU fleet** | R21, R24–R26, R6 | Firmware drift is remediated across a pool; a node that fails DCGM is quarantined automatically; a tenant reprovision takes under 60 s on hardware that supports kexec |
| **M4 Scale and edge** | R11, R16, R22, R37 | 1,000 concurrent deploys using peer-to-peer image distribution; DPU-isolated boot |

### Lifecycle evolution

The current lifecycle model (`docs/lifecycle.md`) is the M0 seed. M1–M3 add states such as
`Enrolling`, `Erasing`, `Quarantined` and `Attesting`. Every change updates the Rust transition
table, the conformance test, the TLA+ model and the generated docs together, and CI enforces this.

## 9. Key decisions and their rationale

| # | Decision | Rationale | ADR |
|---|---|---|---|
| D1 | Core designed for both front ends from day one; **the M1 release ships the single binary, the operator ships in M2** | The user asked for both from day one. Making the core front-end-agnostic in M1 keeps that promise without doubling the MVP scope for a solo maintainer. *The owner can revisit this.* | to write |
| D2 | **gRPC/protobuf contracts as the source of truth**, with a REST gateway | Typed clients, streamed deploy progress, and a single schema for the CLI, operator and Terraform | to write |
| D3 | **Block-image streaming** from an in-RAM Rust agent, not OS installers | Fastest and works for any OS; installers are the main cause of P2 | to write |
| D4 | **Pluggable network services** with built-in defaults (proxyDHCP by default) | Fits into existing DHCP/IPAM at hyperscale sites, and still works with zero dependencies | to write |
| D5 | **Embedded store** behind a trait, with Raft for HA in M2 | Removes the PostgreSQL dependency (P1); the trait keeps an external database possible | to write |
| D6 | **Virtual lab is the CI test bed** | No dedicated hardware lab yet (§10) | to write |
| D7 | **OSS plus a support and hosted business model**: everything stays Apache-2.0 | No open-core feature fences; the hosted control plane is the commercial product | [0004](adr/0004-apache-2-license.md) |

## 10. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| **Virtual lab only.** No real Redfish, GPU or DPU hardware | VM timings don't prove the real-hardware claim; the GPU/DPU work in M3 can't be tested | Design every driver against recorded real BMC responses (fixture-based tests); rent hourly bare-metal servers for benchmarks before releases; ask for community or vendor lab donations before M3; label VM benchmarks as such |
| Scope versus a solo maintainer | The MVP slips | The M1 scope above is fixed; anything else waits for M2. Agent fleets work against contracts in this PRD |
| Running DHCP/TFTP is a security exposure | A rogue boot path could wipe machines | proxyDHCP by default; network-scoped listeners; mTLS for agents; signed images; the model-checked lifecycle guards destructive transitions; fuzz every protocol parser |
| Redfish implementations differ by vendor | Drivers break on some hardware | A vendor quirk layer, driven by a conformance suite of recorded responses |
| The MAAS comparison is seen as unfair | Credibility | Open harness, identical lab, method published, MAAS maintainers invited to review |

## 11. Success metrics

- **Adoption:** GitHub stars and contributors; production deployments reported (target: 3 by the end of M2).
- **Performance:** benchmark p95 tracked per release, with no regression over 10%.
- **Quality:** change-revert rate under 5%; zero lifecycle-invariant violations in fuzzing and TLA+ runs.
- **Business:** inbound requests for support or hosting after M2.

## 12. Open questions

1. Embedded store choice (redb, SQLite, or something else) and the Raft library for M2. This needs an ADR with a benchmark.
2. Agent initramfs base: a custom minimal image (u-root style) or a minimal Linux distribution? This decides image size and boot time.
3. Image format standard: raw+zstd only, or also qcow2 and OCI-packaged disk images from the start?
4. Authentication for M1: static tokens only, or OIDC from the start?
5. The hosted control plane in D7 needs a multi-tenant model. Does it shape M2's authorization design?
6. Hardware partners: which vendors or labs to approach for real-hardware validation before M3?
