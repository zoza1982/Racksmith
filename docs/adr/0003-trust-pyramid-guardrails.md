# 0003. Enforce quality with a trust pyramid of guardrails

- Status: Accepted
- Date: 2026-11-18

## Context

AI coding agents can produce many pull requests quickly. Line-by-line human review does not scale
to that volume, and agents copy whatever patterns the codebase already contains, good or bad.
Natural-language rules are followed inconsistently and go stale.

## Decision

We will rely on guardrails ordered by strength, described in
[docs/trust-pyramid.md](../trust-pyramid.md):

1. **Deterministic constraints** first: the type system, strict workspace lints in `Cargo.toml`
   (clippy `all`, `pedantic` and `cargo` as errors, plus selected restriction lints), API bans in
   `clippy.toml`, architecture rules in `architecture.toml` enforced by `cargo xtask arch`, and
   supply-chain policy in `deny.toml`.
2. **Verification** next: unit, integration and property tests, fuzzing with cargo-fuzz, Kani
   proofs and a TLA+ model of the machine lifecycle checked by TLC.
3. **Prose** last: `AGENTS.md`, `CONTRIBUTING.md` and ADRs, only for judgment that code cannot
   check.

Supporting rules:

- A rule that can be expressed as a lint or check is expressed that way, not as prose.
- Lint levels are set only in `Cargo.toml`: `allow`, `warn`, `deny` and `forbid` attributes are
  rejected in source, and exceptions use `#[expect(lint, reason = "...")]` on the smallest item.
- `cargo xtask ci` runs locally every gate that fits on one machine, and CI exposes the gates as
  two required checks, `ci-success` and `verify-success`.
- The guardrail files are owned in `.github/CODEOWNERS`, so weakening one requires maintainer
  approval and an ADR.
- The seed code is held to the standard we want copied: no dead code, no TODOs, documented items.

## Consequences

- Most defects are rejected by tools before a reviewer sees them, so review can focus on design.
- Strict lints make some code more verbose, and occasionally need a justified `#[expect]`.
- Formal verification and fuzzing add CI time to every pull request; the nightly schedule adds
  longer fuzzing on top.
- The guardrails themselves must be maintained; changing them is a deliberate, reviewed act.
