# 0001. Record architecture decisions

- Status: Accepted
- Date: 2026-11-18

## Context

RackSmith is developed by people and AI coding agents at high pull request volume. The reasons
behind a guardrail (a lint, a layering rule, a dependency ban) are easy to lose, and a rule whose
reason is unknown is likely to be weakened by the next contributor, human or agent, who finds it
inconvenient.

## Decision

We will record significant decisions as architecture decision records (ADRs), in the format
described by Michael Nygard in
[Documenting Architecture Decisions](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions).

- ADRs live in `docs/adr/` as `NNNN-short-title.md`, numbered sequentially, starting from
  [template.md](template.md).
- An ADR is added in the same pull request as the change it describes, with status `Proposed`,
  and becomes `Accepted` when that pull request merges.
- Accepted ADRs are not rewritten. A decision is changed by a new ADR that supersedes it, and the
  old ADR's status is updated to point at the new one.
- An ADR is required for changes to `architecture.toml`, to the lint policy in `Cargo.toml`,
  `clippy.toml` or `deny.toml`, to public API contracts or the machine lifecycle, and for new
  significant dependencies, protocols or storage technologies.

## Consequences

- Reviewers and agents can find why a constraint exists before proposing to change it.
- Changing a guardrail costs a written argument, which is the point: it should be deliberate.
- The ADR requirement is prose, the weakest tier of the
  [trust pyramid](../trust-pyramid.md). It is backed by CODEOWNERS review of the guardrail files
  and by the pull request checklist.
