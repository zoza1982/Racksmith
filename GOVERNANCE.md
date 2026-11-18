# Governance

RackSmith is a young project run by a single maintainer,
[@zoza1982](https://github.com/zoza1982), who has final say on what is merged and on the
project's direction.

## Decisions

- Day-to-day changes are decided in pull requests that need green `ci-success` and
  `verify-success` checks and an approving code-owner review (see [CODEOWNERS](.github/CODEOWNERS)).
- Significant technical decisions are proposed and recorded as
  [architecture decision records](docs/adr/) in the PR that implements them.
- Disagreements are discussed in the relevant issue or PR; the maintainer decides when consensus
  is not reached.

## Branch rules

`main` is protected by two GitHub repository rulesets, kept as code in
[`.github/rulesets/`](.github/rulesets/). Both are active. Changes to them go through a pull
request that edits the JSON, followed by re-applying it with the command below.

- [`main-checks.json`](.github/rulesets/main-checks.json): changes arrive only through pull
  requests, are squash-merged, need `ci-success` and `verify-success` green, and `main` cannot be
  force-pushed or deleted. **Nobody can bypass it**, including the maintainer.
- [`main-review.json`](.github/rulesets/main-review.json): one approving review from a code owner,
  given after the last push. Repository admins may bypass this ruleset, and only through a pull
  request.

GitHub never counts an author's approval of their own pull request, and today the maintainer is
the only code owner. So:

- AI agents open pull requests from a **dedicated bot account**, never with the maintainer's
  credentials. The maintainer's approval of an agent's work is then a real, independent review.
- Pull requests written by the maintainer personally are merged with the review bypass. They still
  pass every check, because the checks ruleset has no bypass.

When a second maintainer joins, they are added to CODEOWNERS and the review bypass is removed.

Rulesets are (re-)applied with the GitHub API (for an existing ruleset, `PUT .../rulesets/<id>` instead of `POST`):

```sh
for ruleset in .github/rulesets/*.json; do
  gh api --method POST repos/zoza1982/Racksmith/rulesets --input "${ruleset}"
done
```

The repository also requires actions to be pinned to a full commit SHA
(`gh api --method PUT repos/zoza1982/Racksmith/actions/permissions -F enabled=true
-f allowed_actions=all -F sha_pinning_required=true`).

## Becoming a maintainer

Contributors who make sustained, high-quality contributions and reviews may be invited to become
maintainers. New maintainers are added to [CODEOWNERS](.github/CODEOWNERS) and to this file. As
the maintainer group grows, this document will be revised to describe how decisions are made
collectively.

## Code of Conduct

Everyone participating in the project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
