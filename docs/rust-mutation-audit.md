# Rust decision-bearing mutation audit

Run the local audit from a clean repository root:

```console
$ bash scripts/audit-rust-mutation.sh
```

The command refuses a dirty worktree. It then deletes this crate's unit-test
binaries under `target/*/deps/` before anything is built, because a stale test
binary makes a red assertion look green and lags the result by exactly one
iteration — the standing fact recorded in CLAUDE.md. Do the same by hand when
running `cargo mutants` directly on one module. It first asks cargo-mutants for its
candidate list, selects only mutants whose complete source spans belong to the
six groups in `coverage/rust-line-coverage-scope.json`, writes that exact
selection and a generated cargo-mutants filter into a fresh
`target/rust-mutation-audit.XXXXXX/` directory, then runs cargo-mutants in its
own scratch copy. It leaves the candidate list, selection, filter and complete
`mutants.out/` report in that directory and prints its path and elapsed time.
Nothing in the command overwrites an earlier report.

Cargo-mutants itself exits nonzero for every raw missed mutant. The command's
exit status instead comes from the checked-in audit policy after it has applied
the documented dispositions, floors and timeout rule: an accepted equivalent
or integration-only miss can therefore pass, while an unclassified miss,
timeout, incomplete report or below-floor result fails.

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

Selection reads that file's **exclusions** as well as its groups. A candidate
mutant no group owns is dropped only when a declared exclusion covers its
span; one belonging to neither fails the selection naming the hole, because a
candidate quietly skipped is a scope reduction nobody reviewed.

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
selection, the undeclared-candidate refusal and incomplete-run refusal. It does not run a full mutation campaign.

GitHub Actions exposes the same command only through the **Rust mutation
audit** `workflow_dispatch` workflow. It is neither scheduled nor a pull-request
requirement. It retains the complete `target/rust-mutation-audit.*` directory
even when the audit fails; download that artifact to inspect the selected list,
raw cargo-mutants outcomes, and survivor inventory. A raw miss is not silently
accepted: it passes only when `rust-mutation-dispositions.json` supplies a
specific equivalent or integration-only rationale, and any timeout fails.

## Integrated scoped outcome (2026-09-17)

A clean worktree completed 592 selected mutants in 17m13s. Two raw misses are
explicitly classified equivalent fallbacks in
`rust-mutation-dispositions.json`; both remain printed in the survivor
inventory and are excluded from the score. No mutant timed out.

| Group | Caught | Missed | Timed out | Unviable | Excluded | Score |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| conversion | 272 | 0 | 0 | 61 | 2 | 100.0% |
| DD-version | 54 | 0 | 0 | 8 | 0 | 100.0% |
| context-registry | 24 | 0 | 0 | 4 | 0 | 100.0% |
| loss | 26 | 0 | 0 | 5 | 0 | 100.0% |
| artifact-validation | 69 | 0 | 0 | 5 | 0 | 100.0% |
| deterministic runtime-binding policy | 61 | 0 | 0 | 1 | 0 | 100.0% |
| aggregate | 506 | 0 | 0 | 84 | 2 | 100.0% |

The two exclusions are narrow by construction: a changed resolution shape or
write/delete check that makes either fallback reachable returns it to the
score. Do not add a broad exclusion to make a future failed audit pass.
