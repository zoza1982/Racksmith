# 0022. Distribute images through content-addressed caches with bandwidth admission at agent Hello

- Status: Accepted
- Date: 2026-11-18

## Context

The §7 target of 1,000 concurrent deploys with p95 under 5 minutes, "bandwidth permitting", means
moving 2 TB for a 2 GB image. In a realistic 120 s transfer window that is about 133 Gb/s; one
100 GbE origin needs about 174 s just to push it. On GPU servers, POST takes 90–240 s per boot; any
bandwidth reservation held during POST is wasted
([system design §9.5](../design/system-design.md#95-distribution-and-admission-for-1000-concurrent-deploys)).

Options considered: **the control plane streams everything**; **a built-in cache role in M1**;
**any HTTP cache in front of a content-addressed origin, with admission control**; **P2P now**. For
admission timing: **reserve at `deploy`** (before power-on) or **reserve at agent Hello** (after POST
and bootstrap).

## Decision

- The control plane hosts an **origin** on its own data runtime
  ([ADR 0024](0024-separate-runtimes-in-one-process.md)) serving immutable, content-addressed rsimg
  blobs with HTTP Range and `sendfile`, marked `Cache-Control: immutable`. Any HTTP cache works in
  front of it; M1 ships an nginx recipe that uses `ngx_http_slice_module` (32 MiB slices, equal to the
  agent's coalesced range), `proxy_cache_lock` with a 30 s lock timeout and age, and long cache
  validity, because stock `proxy_cache` turns Range misses into whole-object upstream fetches and
  releases lock waiters to the origin after 5 s. `ImageSource` objects describe caches with an egress
  budget and a rack/site selector; plans list sources in order.
- **Hedges and failover stay inside admission:** hedged requests are capped at 5% extra bytes per
  agent and never target the origin; a failed source sends the machine back to `AwaitingBandwidth`
  for a token on the next source; the origin answers unadmitted ranges with `429` and `Retry-After`.
- **Stage-2 boot files** (about 46 MB per boot over HTTPS from the control plane) are deducted from
  the origin's egress before slots are computed, unless they move to `imgverify` over plain HTTP.
- **Two-stage admission**, computed by the pure `admission::admit()` in the domain:
  1. a **global slot** at `deploy` (`max_concurrent_deploys`, default 1,000); queued machines stay
     `allocated` in phase `Queued`;
  2. a **bandwidth token** for the selected source at agent Hello, released when `T_stream` ends.
     Tokens per source are `floor(0.9 × egress × W / S)` with a stream window `W` of 30 s.
- The slot figure and the derived in-flight capacity are exported as metrics, printed by the
  benchmark harness and checked by the load lab in an origin-only and a cached topology. The cached
  1,000-concurrent figure is published as `modelled` until a nightly real-bytes test (at least 40
  agents through one cold nginx cache from the recipe) shows origin egress within 1.1 × the blob
  size.
- A per-`(image, pool)` circuit breaker pauses admission when more than 20% of at least 10 deploys
  fail within 10 minutes.
- The `racksmith serve --role cache` role comes in M2/M3 (and becomes the R6 site agent in M3); P2P
  on the same extents comes in M4.

## Consequences

- POST and bootstrap never hold egress, so concurrency is not cut by several times on long-POST
  hardware.
- An origin-only install on 2 × 25 GbE has 78 slots for a 2 GB image: about 700 deploys in flight
  with POST jitter, but not a simultaneous wave of 1,000. Meeting the headline needs rack caches, and
  the docs say so with the published figure.
- Waiting for a token happens with the agent connected (phase `AwaitingBandwidth`), which costs
  sessions but no extra boots.
