# Rust decision-bearing mutation audit

Run the local audit from a clean repository root:

```console
$ bash scripts/audit-rust-mutation.sh
```

The command refuses a dirty worktree. It first asks cargo-mutants for its
candidate list, selects only mutants whose complete source spans belong to the
six groups in `coverage/rust-line-coverage-scope.json`, writes that exact
selection and a generated cargo-mutants filter into a fresh
`target/rust-mutation-audit.XXXXXX/` directory, then runs cargo-mutants in its
own scratch copy. It leaves the candidate list, selection, filter and complete
`mutants.out/` report in that directory and prints its path and elapsed time.
Nothing in the command overwrites an earlier report.

Install the pinned tool and use the project minimum toolchain:

```console
$ cargo install cargo-mutants --version 27.1.0 --locked
$ cargo +1.88.0 mutants --version
cargo-mutants 27.1.0
```

The second command was run for this audit on Rust 1.88.0. The wrapper rejects a
different cargo-mutants release and an active Rust compiler below the recorded
minimum; `coverage/rust-mutation-audit.json` records that release, Rust minimum
and the floors.

## Scope and score

The mutation configuration names `rust-line-coverage-scope.json` rather than
copying its groups or source ranges. The checker rejects any other scope. As a
result conversion, DD-version, context-registry, loss, artifact-validation and
deterministic runtime-binding policy retain exactly the line audit's ownership,
including its documented range-level treatment of mixed adapter/policy files.
Raw-pointer adaptation, forwarding, dynamic loading and real-Core integration
remain out of scope for the same explicit reasons recorded by the line audit.

Every group and the aggregate report caught, missed, timed-out, unviable and
explicitly excluded mutants. The score is:

```text
caught / (caught + missed + timed-out)
```

Unviable and precisely excluded mutants are outside both numerator and
denominator. The aggregate floor is 85%; every group must reach 75%; and any
timeout fails even if both percentages pass. Missing baseline success, an
unknown/missing outcome, an incomplete selected list, a group with no selected
mutants, or an output list different from the selected list is an input error,
never a pass.

`coverage/rust-mutation-dispositions.json` contains only exact mutant-identity
exclusions. Each equivalent or integration-only exclusion must state the
observable behavior and its precise rationale. All missed and timed-out mutants
are printed in the survivor inventory. A missed mutant without such an exclusion
is shown as unexplained and remains in the score; it cannot be hidden.

The CTest fixture `rust-mutation-audit-fixtures` verifies threshold boundaries,
timeouts, unviable mutants, a precisely documented equivalent exclusion, scope
selection and incomplete-run refusal. It does not run a full mutation campaign.

## Initial scoped outcome (2026-09-17)

A clean detached worktree at `11457ef` completed 603 selected mutants in
18m18s. The durable [normalized baseline report](mutation-audits/issue-191-scoped-baseline.md)
records its result. No survivor has been excluded:
`rust-mutation-dispositions.json` remains empty, so every missed mutant remains
visible in the command's survivor inventory for follow-up.

| Group | Caught | Missed | Timed out | Unviable | Excluded | Score |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| conversion | 262 | 21 | 0 | 61 | 0 | 92.6% |
| DD-version | 53 | 1 | 0 | 8 | 0 | 98.1% |
| context-registry | 24 | 0 | 0 | 4 | 0 | 100.0% |
| loss | 19 | 9 | 2 | 3 | 0 | 63.3% |
| artifact-validation | 69 | 0 | 0 | 5 | 0 | 100.0% |
| deterministic runtime-binding policy | 57 | 4 | 0 | 1 | 0 | 93.4% |
| aggregate | 484 | 35 | 2 | 82 | 0 | 92.9% |

The aggregate numeric score clears its floor, but the loss group is below 75%
and its two timeouts independently fail the audit. These are baseline findings,
not a reason to lower either floor or classify a survivor without evidence.
