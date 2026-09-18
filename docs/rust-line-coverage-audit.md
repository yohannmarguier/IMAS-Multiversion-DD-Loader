# Rust decision-bearing line-coverage audit

Run the local audit from the repository root:

```console
$ bash scripts/audit-rust-line-coverage.sh
```

It runs `cargo llvm-cov --all-features --lcov`, writes the generated LCOV file
to `target/rust-line-coverage.lcov`, then checks it against
`coverage/rust-line-coverage-scope.json`. The report always prints each group
and the aggregate before returning failure, so it remains useful while the
baseline is below the floors. It adds covered and total lines; it never averages
percentages.

Install the checked-in tool version and its LLVM component for the Rust
toolchain you use:

```console
$ cargo install cargo-llvm-cov --version 0.9.1 --locked
$ rustup component add llvm-tools-preview --toolchain 1.88.0
```

The repository minimum is Rust 1.88. `cargo +1.88.0 llvm-cov --version` was run
with this pinned `cargo-llvm-cov 0.9.1`; the scope records both versions and the
wrapper refuses another `cargo-llvm-cov` release. The ordinary command uses the
active Rust toolchain, whose version Cargo checks against `rust-version =
"1.88"`.

The scope has six groups:

| Group | Assigned behavior |
| --- | --- |
| conversion | Conversion-map resolution, path and seam policy, read outcome, the reentry gate, deterministic refusal formatting, and `lib.rs`'s shared status-buffer formatting. |
| DD-version | DD-version parsing, the latch policy, and pure stamp decoding. |
| context-registry | Context lifecycle, root ownership, occurrence cache, map cache, and loss retention. |
| loss | In-memory loss encoding/retention and append-only loss-file behavior. |
| artifact-validation | The artifact-coverage calculation and ADR 0013 completeness proof. |
| deterministic runtime-binding policy | Fallback constant/error names, library-name choice, version compatibility, and synthesized resolution failure statuses. |

The configuration gives each included source range one owner and runs to the
start of each file's inline test module, so no production line falls out of the
denominator by being left unnamed. It explicitly excludes C-ABI pointer
marshalling, symbol forwarding, dynamic-library opening and symbol lookup, and
real-Core integration. A file that contains both an ABI adapter and a decision
helper is ranged rather than excluded wholesale: `src/lib.rs`'s status
formatting, `interpose/refusal.rs`'s formatting and latch gate,
`version_stamp.rs`'s decoder *and* its read classifier, `artifact_validation.rs`'s
calculation, `loss_file.rs`'s rendering, naming and append behavior, and the
policy ranges in `core_binding.rs` — including the public fallback accessors'
result-selection policy — remain measured. The
`validate_equilibrium_coverage` binary is excluded as its command-line,
filesystem, and terminal adapter. Update the assignments with an internal seam
extraction; do not silently shrink the scope.

**An exclusion may be ranged too.** Where only part of a measured file is
another test layer's business — `version_stamp.rs`'s discovery read, the
process-wide `OnceLock` in `hli_version.rs`, `lib.rs`'s exported entry points,
`core_binding.rs`'s loader — the exclusion carries `start_line`/`end_line` and
its own reason beside the ranges that *are* measured. That is the only
supported way to leave production code out of a measured file: a range that
simply stops short, with nothing saying why, is the failure mode this format
exists to prevent.

The checker rejects a missing or empty configured source measurement, malformed
LCOV input, overlapping source assignments, or a scope that does not name all
six groups. It also rejects **a measured file whose production lines are not
fully assigned**: it reads each measured source, takes its inline test module
as the end of production, and requires every line before that to fall in a
group range or in a declared exclusion. That is what makes a silently truncated
range impossible rather than merely discouraged. A source absent from the scope
is an explicit test-layer exclusion recorded in the same JSON file; it is not
part of the denominator.

The floors are 90% aggregate and 80% for every group. Ordinary CI runs this
same command in its `rust-line-coverage` job and blocks on its verdict. Its
LCOV report is uploaded even if the audit fails, so inspect the group totals
before changing scope or tests. The compact fixture test is registered as
`rust-line-coverage-audit-fixtures` and proves the aggregate/per-group boundary,
malformed-data behavior, and the truncated-range refusal against a throwaway
source tree, without requiring a full coverage run.

## Integrated baseline (2026-09-18)

The pinned tool reported the following from the integrated 298-unit-test audit,
over the completed scope. `cargo-llvm-cov 0.9.1` and `cargo-mutants 27.1.0` are
both available under Rust 1.88.0, which is the CI toolchain; the recorded line
run also passes on the local Rust toolchain. All floors pass, so the command is
suitable for CI enforcement.

| Group | Covered/total | Coverage |
| --- | ---: | ---: |
| conversion | 1,901 / 2,101 | 90.5% |
| DD-version | 189 / 200 | 94.5% |
| context-registry | 183 / 183 | 100.0% |
| loss | 237 / 239 | 99.2% |
| artifact-validation | 222 / 227 | 97.8% |
| deterministic runtime-binding policy | 106 / 130 | 81.5% |
| aggregate | 2,838 / 3,080 | 92.1% |

The denominator grew by 155 lines against the first recorded baseline: the
loss-file's rendering, naming and append behavior, the DD-version read
classifier and the moved runtime-binding fallback policy were all production
code that earlier ranges stopped short of. Deterministic runtime-binding policy
now carries the four public accessors' forwarding arms, which only the C ABI
suites can reach, and sits closest to its floor because of it.
