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

The configuration gives each included source range one owner and ends each
inline-test module before its test implementation. It explicitly excludes
C-ABI pointer marshalling, symbol forwarding, dynamic-library opening and
symbol lookup, and real-Core integration. A file that contains both an ABI
adapter and a decision helper is ranged rather than excluded wholesale:
`src/lib.rs`'s status formatting, `interpose/refusal.rs`'s formatting and
latch gate, `version_stamp.rs`'s pure decoder, `artifact_validation.rs`'s
calculation, and the policy ranges in `core_binding.rs` — including the public
fallback accessors' result-selection policy — remain measured. The
`validate_equilibrium_coverage` binary is excluded as its command-line,
filesystem, and terminal adapter. Update the assignments with an internal seam
extraction; do not silently shrink the scope.

The checker rejects a missing or empty configured source measurement, malformed
LCOV input, overlapping source assignments, or a scope that does not name all
six groups. A source absent from the scope is an explicit test-layer exclusion
recorded in the same JSON file; it is not part of the denominator.

The floors are 90% aggregate and 80% for every group. Ordinary CI runs this
same command in its `rust-line-coverage` job and blocks on its verdict. Its
LCOV report is uploaded even if the audit fails, so inspect the group totals
before changing scope or tests. The compact fixture test is registered as
`rust-line-coverage-audit-fixtures` and proves the aggregate/per-group boundary
and malformed-data behavior without requiring a full coverage run.

## Integrated baseline (2026-09-17)

The pinned tool reported the following from the integrated 299-unit-test audit.
`cargo-llvm-cov 0.9.1` and `cargo-mutants 27.1.0` are both available under
Rust 1.88.0, which is the CI toolchain; the recorded line run also passes on
the local Rust toolchain. All floors pass, so the command is suitable for CI
enforcement.

| Group | Covered/total | Coverage |
| --- | ---: | ---: |
| conversion | 1,900 / 2,095 | 90.7% |
| DD-version | 154 / 161 | 95.7% |
| context-registry | 183 / 183 | 100.0% |
| loss | 148 / 149 | 99.3% |
| artifact-validation | 222 / 227 | 97.8% |
| deterministic runtime-binding policy | 102 / 110 | 92.7% |
| aggregate | 2,709 / 2,925 | 92.6% |
