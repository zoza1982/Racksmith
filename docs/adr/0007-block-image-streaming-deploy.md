# 0007. Deploy operating systems by streaming block images from an in-RAM Rust agent

- Status: Accepted
- Date: 2026-11-18

## Context

The MVP must prove RackSmith is the fastest way to take a machine from power-on to a running OS.
The target is under 5 minutes, p95 ([PRD](../prd.md) §7, pain P2). It must also treat every OS
equally (pain P3). Options considered:

1. **Drive native installers** (curtin, kickstart, preseed, autoinstall). Flexible per OS, but
   slow (installers resolve packages and run scripts on every deploy), and each OS needs its own
   integration.
2. **Stream a prebuilt disk image straight to the target disk** from a small agent running in
   RAM, then configure first boot. This is the approach of OpenStack Ironic's IPA and of
   Tinkerbell.
3. **Both**, with installers as a fallback.

## Decision

We will take option 2.

- **The agent** is `racksmith-agent`, a static Rust binary that boots in a minimal initramfs over
  PXE or HTTP boot. It identifies the machine, inventories it, and receives a deploy plan over
  mTLS ([PRD](../prd.md) R27).
- **Writing the image:** the agent streams a compressed image (raw + zstd first; other formats
  are an open question in PRD §12) straight to the target disk. It verifies the SHA-256 digest and
  the signature, and refuses to write an image whose signature doesn't verify. It then grows
  partitions and injects first-boot configuration (cloud-init NoCloud, Ignition).
- **Booting:** after the write, the agent reboots into the OS, or uses kexec where the hardware
  and image allow it ([PRD](../prd.md) R21, M3).
- **Timing:** every deploy phase is timed and exported as a metric, so the benchmark in PRD §7 is
  measured, not estimated.
- **Not in v1:** installer-driven deploys. An OS without an image gets an image recipe, not an
  installer integration.

## Consequences

- Deploy time is bounded by network and disk throughput, not by package installs, and every OS
  goes through one code path.
- Each OS needs a published image. RackSmith ships recipes for Ubuntu, Debian, Rocky/Alma,
  Flatcar and Talos ([PRD](../prd.md) R18) and has to maintain them.
- The agent is a security-critical component: it writes disks and holds deploy credentials. Its
  parsers get fuzz targets, and its enrollment and identity design belongs in the system design.
- Per-machine customization that installers handle at install time (disk layouts, drivers) has
  to be done through image variants or first-boot configuration.
