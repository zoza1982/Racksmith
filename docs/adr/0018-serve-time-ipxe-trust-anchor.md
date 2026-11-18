# 0018. Inject the installation's trust anchor into release-built iPXE at serve time

- Status: Proposed
- Date: 2026-11-18

## Context

Everything after the iPXE binary (script, kernel, initramfs) should be authenticated in M1 even
though stage 1 cannot be until Secure Boot (M2). iPXE authenticates HTTPS servers against roots
compiled in with `TRUST=`, stored as SHA-256 fingerprints. Each installation has its own root CA
([ADR 0017](0017-agent-enrollment-and-built-in-ca.md)). R1 promises a single binary and §7 promises
install to first deploy in under 15 minutes
([system design §8.3](../design/system-design.md#83-m1-trust-anchor-serve-time-injection-into-a-release-built-ipxe)).

Options considered:

1. **Plain HTTP** for script, kernel and initramfs. Simple; no integrity after stage 1.
2. **A per-install iPXE build at `racksmith init`** from pinned sources in a container. Strong, but it
   needs a C toolchain or container runtime on the operator host and puts the 15-minute install at
   risk.
3. **Serve-time injection:** release iPXE builds with a placeholder root whose fingerprint is a known
   32-byte marker, replaced with this installation's root fingerprint when the binary is served.
4. **DHCP-delivered trust override:** an iPXE build with `ALLOW_TRUST_OVERRIDE`, with the trust setting
   sent by proxyDHCP.

## Decision

We will take option 3, proven by a week-1 spike. The injected fingerprint is that of the installation's
**server root**, which signs only server certificates, so no agent or enrollment certificate can chain
to what iPXE trusts ([ADR 0017](0017-agent-enrollment-and-built-in-ca.md)). `racksmith-netboot`
replaces the marker when TFTP or HTTP serves the binary (about 50 lines plus a CI test that boots the patched binary in the lab); UEFI
does not check PE checksums for unsigned images while Secure Boot is off. The boot HTTPS server sends
its full chain including the root. Fallback 1 is option 4, which has equivalent strength because
stage 1 is unauthenticated anyway; fallback 2 is option 1 with the residual risk documented.

## Consequences

- `racksmith init` needs no toolchain and builds nothing; the release ships ready-made iPXE binaries
  for x86_64 EFI, arm64 EFI and BIOS.
- Stage 2 is authenticated against the installation's own root in M1.
- Stage 1 remains unauthenticated until the M2 Secure Boot chain; an attacker on the provisioning VLAN
  can substitute the iPXE binary. In M1 that attacker can be any deployed OS on the provisioning
  network ([ADR 0033](0033-provisioning-network-boundary-and-os-ready-path.md)), so M1 claims no
  isolation from deployed machines.
- The release process must build iPXE with the placeholder and publish its GPL source offer.
