# 0028. Amend the supply-chain policy for Linux targets and a single crypto provider

- Status: Accepted
- Date: 2026-11-18

## Context

`deny.toml` is a code-owned guardrail ([ADR 0003](0003-trust-pyramid-guardrails.md)); changing it
needs an ADR. A cargo-deny probe of the planned stack found that:

- `deny.toml` has no `[graph] targets`, so dependencies used only on Windows, macOS or Android are
  checked too; `rustls-platform-verifier` pulls `webpki-root-certs` (CDLA-Permissive-2.0) only on
  non-Linux targets, which fails `kube` and `reqwest`;
- `chrono` became unmaintained on 2026-11-17 (RUSTSEC-2026-0543), which blocks `openraft`,
  `pbjson-types`, `tonic-rest`, `procfs`, `oci-client`, the old `sigstore`, and `fatfs` with its
  default features (`fatfs` 0.3.6 enables `chrono` by default; its last crates.io release is from
  2023-01-17, and the 0.4 rewrite is unreleased);
- rustls can use ring or aws-lc-rs; aws-lc-sys adds C and CMake build paths and complicates static
  musl builds.

RackSmith ships for Linux only (R1: x86_64 and aarch64), while the CI clippy and test jobs also run on
macOS and Windows; how Linux-only crates fit that matrix is
[ADR 0030](0030-linux-only-crates-and-ci-platform-matrix.md).
([system design §3.2](../design/system-design.md#3-crate-and-workspace-layout))

## Decision

- Add `[graph] targets` for `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`,
  `aarch64-unknown-linux-gnu` and `aarch64-unknown-linux-musl`.
- Use **ring** as the only rustls provider; ban `aws-lc-rs` and `aws-lc-sys` in `deny.toml` and in
  `architecture.toml`.
- Keep `chrono` banned in M1: every M1 use is avoidable (hand-mapped well-known types, our own OCI and
  REST code, `fatfs` with `default-features = false` and features `std` and `alloc`, `jiff` or
  `std::time` where a time type is needed). `fatfs` is a maintenance risk on a never-cut agent path:
  if 0.3.x needs a fix, the fallback is a small in-tree FAT16 image writer for the 16 MiB config drive.
- Add no CDLA exception: the control plane uses `rustls-native-certs` or a configured bundle, and the
  agent needs no public roots.
- Any future exception (for example `chrono` through openraft in M2) is dated, scoped to one crate,
  reasoned, time-boxed, and recorded as an amendment to this ADR.

## Consequences

- The policy checks what we actually ship, and `kube` and `reqwest` pass on Linux targets.
- One crypto provider means one set of C sources (ring) to audit and simple static builds.
- Avoiding `chrono` costs some hand-written code now; the M2 Raft dependency still needs a resolution.
