# 0024. Isolate control, boot services and image data in separate runtimes inside one process

- Status: Proposed
- Date: 2026-11-18

## Context

R1 puts the API, reconcilers, boot services and store in one binary. The §7 targets require API read
p99 under 50 ms at 10k machines while 1,000 deploys run. Two workloads threaten that: a site power
restore in which 10k machines DHCP at once, and the image origin pushing gigabytes per second. CERN's
Ironic conductors crashed when image bytes went through them. `fsync` on a tokio worker blocks other
tasks ([system design §2.3](../design/system-design.md#23-process-layout-four-isolated-execution-contexts)).

Options considered: **one tokio runtime for everything**; **one runtime plus the blocking pool**;
**separate execution contexts** for control, boot services and image data, plus a dedicated store
writer thread.

## Decision

`racksmith-server` builds four contexts:

| Context | Default threads | Hosts |
|---|---|---|
| Control runtime | `min(8, ncpu/2)` | API, agent gateway, reconcilers, observers, power drivers, object cache |
| Boot runtime | 2 | DHCP, TFTP, boot HTTP(S), phone-home |
| Data runtime (blocking pool) | `min(16, ncpu/2)` | image origin (`sendfile`), import, zstd encoding |
| Store writer | 1 OS thread | redb group commit and `fsync` |

They communicate only over bounded channels. Boot services ask the core for decisions through a
bounded channel of 4,096 requests and drop packets when it is full, which DHCP and TFTP clients
retry. Thread counts are configurable. Runtime metrics are exported per context.

## Consequences

- A DHCP herd or bulk image I/O cannot take workers from the API; the load lab's `power-restore`
  scenario checks that API p99 stays under 50 ms.
- More configuration and more moving parts inside one process; the defaults must suit small and large
  hosts.
- The same layout serves the M2 cache role, which runs only the data runtime.
