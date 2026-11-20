# 0036. Record every lint exemption in a code-owned ledger

- Status: Proposed
- Date: 2026-11-19

## Context

Lint levels are set only in `Cargo.toml`. The one escape hatch in source is
`#[expect(lint, reason = "...")]` on the smallest item
([ADR 0003](0003-trust-pyramid-guardrails.md)). Clippy's `allow_attributes_without_reason` checks
that the reason is **present**, not that it says anything. `#[expect(clippy::x, reason = "x")]`
passes it, and so do `"fix later"` and `"needed"`. An agent optimising for a green build will
find this.

This repository implemented the first half of the bootstrap playbook's escape-hatch principle
(`oss-bootstrap-playbook.md` §0: "explicit, come with a reason, and be approved"): explicit
(`expect`, never `allow`) and with a reason. Nothing mechanical enforced "approved"; it relied
on a reviewer noticing an `#[expect]` somewhere in a diff. The playbook names the gap itself
("checking only that a reason is present is gameable") and answers it with a code-owned
exemption ledger (`arch` rule 8). Its framing is also the right caveat for this ADR: the ledger
works because of the owner's review, not its format. A ledger entry is no harder to write than a
reason string, and an agent can write either. What an agent cannot do is approve its own entry.
Format checks only stop the cheapest filler and make every exemption visible in one diff.

The options were:

1. **Keep the reason check.** It stays gameable, and exemptions remain spread over the tree,
   where a reviewer of a large diff can miss one.
2. **Ban `#[expect]` in source.** This is too strict. A few exemptions are right (an
   exhaustive state-machine match longer than `too-many-lines-threshold`), and the alternative,
   contorting the code, would teach agents worse patterns than the exemption does.
3. **A ledger whose references are checked against a live issue tracker.** This puts network
   access, latency, credentials and nondeterminism into the local gate. `cargo xtask ci` must
   give the same answer offline, on a laptop and in CI.
4. **An offline, code-owned ledger** whose references resolve against files in the repository.

## Decision

We will keep a ledger, [`exceptions.toml`](../../exceptions.toml), at the repository root. It
has one entry for every `#[expect(...)]` attribute in the Rust sources: the workspace crates
(product crates and `xtask`) and `fuzz/`. `cargo xtask arch` enforces it as rule 8
(`xtask/src/expect_attributes.rs`, `xtask/src/exemption_ledger.rs`). `/exceptions.toml` is listed
with the guardrails in `.github/CODEOWNERS`, so every new or changed entry needs the owner's
review.

**Scope includes test code.** Unit-test modules, `tests/`, `benches/` and `examples/` are
scanned too. Agents copy test code as readily as product code, and an exemption in a test
(`#[expect(clippy::too_many_lines)]` on a giant test) is a pattern we do not want copied without
review. The workspace already relaxes the lints that tests legitimately need (`unwrap`, `expect`,
`panic` and indexing in `clippy.toml`), so the cost is small. Inner `#![expect]` and `expect` on a
module are left out, because the source rules already reject them outright.

**Schema** (unknown fields are rejected):

```toml
[[exceptions]]
file = "crates/racksmith-domain/src/lifecycle.rs"   # relative to the root, `/` separators
item = "MachineState::apply"                         # the item's path in the file
lints = ["clippy::too_many_lines"]                   # exactly the attribute's lints, any order
justification = "..."                                # full sentences
reference = "ADR-0013"                               # or "M1-A-01", or "design 4.4"
```

The item is matched by its path inside the file, worked out from tokens. The path is made of the
enclosing `mod`, `impl` (named after its self type), `trait`, `fn` and type bodies, then the item
itself: `MachineState::apply`, `Finding::fmt` for a method of `impl Display for Finding`,
`Tools`, `Config::port` for a field, and `impl Wrapper` for an attribute on an `impl` block. An
attribute on a statement or expression takes the path of the item that encloses it. Matching on
file, item and lint set, rather than on line numbers, keeps entries stable when code above them
moves.

**Rules.** `cargo xtask arch` fails when:

- an `#[expect]` has no entry (the message names the file, item and lints to add);
- an entry matches no attribute (stale), or two entries describe the same attribute;
- two attributes of one file have the same item and lints, so no entry can tell them apart;
- a reference does not resolve. `ADR-NNNN` must name a file in `docs/adr/` whose status is not
  superseded, rejected or deprecated. `M1-X-NN` must be a plan task that exists and is not done
  (reusing `plan_tasks.rs`, as [ADR 0035](0035-completeness-layer.md) does). `design N.M` must be
  a leaf section of the system design (reusing `design_sections.rs`);
- the justification is filler: under 60 characters, under 8 distinct words, a placeholder (`x`,
  `needed`, `see above`, `false positive`, ...), or containing a red-flag phrase as whole words
  (`todo`, `fix later`, `for now`, `temporary`, `bypass`, `hack`, `make clippy happy`,
  `pass ci`, ...);
- the attribute's `reason` is missing or filler under the same rules, with a lower minimum (25
  characters, 4 distinct words). The reason should agree with the justification, but it is a
  one-line summary in the code and need not repeat it.

The scanner works on tokens of the code view (`source_text.rs`, `tokens.rs`, `attributes.rs`), so
an attribute quoted in a comment or a string literal, such as the `reason = "x"` fixtures in
xtask's own tests, is not an exemption. It also finds `expect` inside `cfg_attr`, nested or not.
Hiding an attribute behind a macro fragment (`#[$meta]`) is already rejected by the source rules.

**The count is printed and zero is allowed.** The `arch` summary reports how many exemptions it
checked. Unlike the other counts, zero does not fail the run. A repository can legitimately have
no exemptions, and failing would push it to keep one just to pass. A scan that silently missed
real attributes still cannot pass: the run already fails if it examined no source files, and
every ledger entry whose attribute the scan missed would be reported as stale.

`architecture.toml` is unchanged. The rule lives in xtask, like the other source rules.

## Consequences

- Every exemption is in one reviewable file. A PR that adds an `#[expect]` must also change
  `exceptions.toml`, which needs the code owner's approval, so an exemption cannot slip through
  in a large diff. Removing an exemption means deleting its entry, or the stale check fails.
- The cheapest filler is rejected mechanically: `reason = "x"`, `"fix later"`, `"temporary"`, a
  repeated word.
- References tie each exemption to a recorded decision, an open task or a design section. An
  exemption that rests on a plan task fails when that task is struck through, which forces the
  exemption to be reconsidered once the work it waited for is done.
- Writing an exemption costs more: an entry with a real justification and reference. This is
  intended. The better path is usually to change the code.
- Renaming an item or moving an attribute to another file needs a ledger update in the same PR.
- What it still cannot catch: the length rule, the word rule and the deny-list measure form, not
  truth. A fluent but wrong justification, or one that cites a real but unrelated ADR, passes the
  check. Only the reviewer can reject it. A maintainer who approves a weak justification defeats
  the ledger, and no tool can prevent that. The ledger makes the decision visible and
  attributable; it does not make it.
- Items are named without name resolution. Two `impl` blocks for the same type share a path, and
  attributes on statements share the path of their enclosing function. Identical lints on the
  same path in one file are reported as ambiguous instead of being matched by guesswork.
