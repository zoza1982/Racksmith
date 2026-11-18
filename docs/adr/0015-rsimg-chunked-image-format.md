# 0015. Adopt rsimg v1: chunked zstd frames with a signed per-chunk manifest

- Status: Proposed
- Date: 2026-11-18

## Context

[ADR 0007](0007-block-image-streaming-deploy.md) requires the agent to verify the SHA-256 digest and
the signature before writing, and to stream images to disk. A single whole-file digest cannot be
checked until the last byte has arrived, so as worded the two requirements conflict. The format also
decides decode parallelism, resume, cacheability, sparse writes and later P2P distribution
([PRD](../prd.md) §12 Q3; [system design §9.2](../design/system-design.md#92-image-format-rsimg-v1)).

Options considered:

1. **raw + zstd with a whole-file SHA-256:** simple, but it cannot verify before writing, decodes on
   one core (about 1–1.5 GB/s), and cannot resume or skip holes.
2. **qcow2 on the wire:** cluster order is not disk order, so it cannot be streamed to a block device.
3. **OCI layers as-is:** a transport, not a disk format; it still needs a chunked payload.
4. **Chunked zstd with a signed per-chunk manifest:** independent frames at fixed disk offsets, a
   signed manifest listing every frame's digest, holes recorded at import.

## Decision

We will take option 4 and call it **rsimg v1**:

- The raw disk is split into fixed extents of **4 MiB** (16 MiB is the alternative; a week-3 benchmark
  of decode throughput, manifest size, agent memory and resume granularity decides before F2). Each
  non-zero extent is one independent zstd frame; all-zero extents are holes. Frames are concatenated
  and followed by a zstd seekable seek table, so stock `zstd -d` can decode the blob.
- The manifest is a deterministic protobuf, `racksmith.rsimg.v1.Manifest`, with the virtual size,
  extent size, blob digest, every frame's disk offset, blob offset, lengths and SHA-256 of the
  compressed bytes, the boot-critical extents, the config-drive layout, the ready signal, provenance,
  and a reserved `HandoffHint` for M3 kexec.
- The manifest is signed with ECDSA P-256 in cosign `sign-blob` format. The image's content address
  is the SHA-256 of the manifest bytes.
- The agent verifies the signature against trust anchors from its mTLS plan, then hashes each
  compressed frame before decoding or writing it.
- `racksmith.rsimg.v1` is a stable proto package protected by `buf breaking` from day one.
- raw, qcow2 (through `qemu-img` in M1) and OCI artifacts are import formats only; the agent speaks
  only rsimg. Pure format code lives in `racksmith-rsimg`, shared by the agent and the control plane
  and fuzzed; its prost types are generated into the crate's own `src/gen/` by `cargo xtask codegen`
  ([ADR 0011](0011-evolve-workspace-into-ports-core-adapters.md)).
- An import is re-signed with the catalog key only after its provenance is verified: a pinned
  SHA-256 or a verifiable OpenPGP (`rpgp`) or cosign signature, recorded in `Manifest.provenance`.
  Imports without one need `--allow-unverified-provenance` and are signed with a key that no pool
  trusts by default.

## Consequences

- Verify-before-write holds at 4 MiB granularity, satisfying ADR 0007 literally.
- Parallel decode, out-of-order writes, per-extent retry, HTTP Range resume, multi-source and hedged
  fetch, correct caching by any HTTP cache, sparse writes, and P2P pieces in M4 all follow from one
  format.
- RackSmith owns a format and must keep version 1 readable; every recipe and user image goes through
  an import step.
- A 6 GB raw image has about 1,536 extents and a manifest of about 90 KiB.
