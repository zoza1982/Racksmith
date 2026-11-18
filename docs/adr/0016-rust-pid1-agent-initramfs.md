# 0016. Boot a Rust PID 1 agent in a minimal initramfs with no subprocesses

- Status: Proposed
- Date: 2026-11-18

## Context

The agent's boot environment decides deploy time (bytes over the wire, kernel and userspace start)
and the agent's trust surface ([PRD](../prd.md) §12 Q2). Ironic's IPA ramdisk is 350–420 MB
compressed; at 1,000 concurrent deploys every 10 MB of initramfs is 10 GB of fleet traffic. The
workspace forbids `unsafe` code. kexec loading has no safe Rust wrapper and is M3 scope
([system design §10.1](../design/system-design.md#101-initramfs-build-answers-prd-12-q2)).

Options considered: **u-root** (Go userland); **LinuxKit** or **Alpine/busybox** (a C userland and a
shell); **mkosi** (a distribution image); **a Rust PID 1 with a distribution kernel** and a curated
module set, with either exec'd static tools (for example kexec-tools) or no subprocesses at all.

## Decision

We will boot `racksmith-agent` as PID 1, a static musl Rust binary, with a pinned Debian stable
signed kernel and a curated module list (common NIC, storage and cpufreq drivers), built by
`cargo xtask initramfs` into a reproducible newc cpio (the `cpio` crate, sorted entries, fixed
metadata, zstd). In M1:

- The initramfs contains **no shell, no busybox, no kexec-tools, no filesystem tools and no
  subprocesses**. Every syscall the agent needs (mount, `finit_module`, netlink, `fallocate`,
  `reboot`) has a safe wrapper in rustix or rtnetlink. Filesystem growth is left to the OS's
  `growpart`.
- PID 1 loads NIC and storage modules first by modalias, sets the `performance` cpufreq governor
  before streaming, configures the interface from the kernel command line, and on a fatal error
  powers the machine off.
- `cargo xtask size` fails CI if the stripped agent exceeds 12 MiB or the compressed initramfs
  exceeds 40 MiB (target 32 MiB).
- Releases ship the initramfs, the kernel, a CycloneDX SBOM and a source offer for the GPL kernel.
- Loading a kexec kernel (M3) is decided by a later ADR among an upstream rustix wrapper, a published
  `racksmith-sys` crate with audited `unsafe`, and a static `kexec-tools` subprocess.

## Consequences

- The agent boots from about 46 MB of kernel plus initramfs, has one binary to audit, and keeps
  `unsafe_code = "forbid"` intact.
- No GPL tool ships inside the M1 initramfs, which keeps the release compliance simple.
- Debugging on a node has no shell; the agent streams its logs to the control plane instead.
- Hardware support is bounded by the curated module list, which must grow with real-hardware
  validation (M3).
