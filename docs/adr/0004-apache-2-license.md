# 0004. License RackSmith under Apache-2.0

- Status: Accepted
- Date: 2026-11-18

## Context

RackSmith is a public open-source project intended for use by companies and individuals running
their own hardware, and for contributions from both. The license should be permissive enough for
commercial adoption, compatible with the Rust ecosystem, and protect users and contributors
against patent claims.

## Decision

We will license RackSmith under the Apache License, Version 2.0.

- The full text is in `LICENSE`, and attribution notices in `NOTICE`.
- Contributions are accepted under the same license by the terms of its section 5. We do not
  require a Contributor License Agreement or a DCO sign-off.
- Dependencies must use licenses compatible with Apache-2.0. The allowlist in `deny.toml`
  (permissive licenses only; copyleft is rejected) is enforced by `cargo deny check` in CI.

## Consequences

- Apache-2.0 includes an explicit patent grant and a patent-retaliation clause, which MIT does not.
- Companies can embed and redistribute RackSmith, including in proprietary products, which
  favors adoption over reciprocity.
- Copyleft crates (GPL, LGPL, AGPL, MPL and similar) are rejected by policy; the license check
  makes this a CI failure rather than a review finding.
