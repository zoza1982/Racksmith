<!--
The PR title becomes the squash commit message and must follow Conventional Commits, e.g.
`feat(api): add machine tags` (enforced by the PR title check).
-->

## What and why

<!-- What does this change, and why? Link the issue: "Closes #123". -->

## How it is verified

<!-- Which tests, property tests, Kani harnesses or TLA+ changes prove this works? -->

## Checklist

- [ ] `cargo xtask ci` passes locally (the local part of the `ci-success` check), with no
      skipped steps.
- [ ] New behavior is covered by tests; invariants are covered by property tests (proptest).
- [ ] Machine lifecycle changes update `TRANSITIONS`, the generated tables
      (`cargo xtask docs --fix`), and the Kani/proptest invariants together (or this PR does not
      touch the lifecycle).
- [ ] Changes to `architecture.toml` come with an ADR under `docs/adr/` (or it is unchanged).
- [ ] Every lint escape uses `#[expect(lint, reason = "...")]` with a real reason; no lint was
      weakened in `Cargo.toml` or `clippy.toml`.
- [ ] User-facing changes are documented.

## AI assistance

<!-- Required. Describe how AI tools were used, e.g. "None", "Claude Code wrote the first draft;
I reviewed every line", "Copilot completions". You are responsible for every line regardless. -->

AI assistance:
