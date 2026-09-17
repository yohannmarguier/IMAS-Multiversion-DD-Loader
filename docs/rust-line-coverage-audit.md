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
| artifact-validation | The maintained artifact-coverage validator command. |
| deterministic runtime-binding policy | Fallback constant/error names, library-name choice, version compatibility, and synthesized resolution failure statuses. |

The configuration gives each included source range one owner and ends each
inline-test module before its test implementation. It explicitly excludes
C-ABI pointer marshalling, symbol forwarding, dynamic-library opening and
symbol lookup, and real-Core integration. A file that contains both an ABI
adapter and a decision helper is ranged rather than excluded wholesale:
`src/lib.rs`'s status formatting, `interpose/refusal.rs`'s formatting and
latch gate, `version_stamp.rs`'s pure decoder, and the policy ranges in
`core_binding.rs` — including the public fallback accessors' result-selection
policy — remain measured. Update the assignments with an internal seam
extraction; do not silently shrink the scope.

The checker rejects a missing or empty configured source measurement, malformed
LCOV input, overlapping source assignments, or a scope that does not name all
six groups. A source absent from the scope is an explicit test-layer exclusion
recorded in the same JSON file; it is not part of the denominator.

The floors are 90% aggregate and 80% for every group. They are intentionally
not in CI yet. The compact fixture test is registered as
`rust-line-coverage-audit-fixtures` and proves the aggregate/per-group boundary
and malformed-data behavior without requiring the baseline to pass.

## Initial baseline (2026-09-17)

The pinned tool and Rust 1.88.0 reported the following after 226 Rust unit
tests. The expected nonzero result is the audit's enforcement result, not a
broken command.

| Group | Covered/total | Coverage |
| --- | ---: | ---: |
| conversion | 1,776 / 2,101 | 84.5% |
| DD-version | 115 / 150 | 76.7% |
| context-registry | 183 / 183 | 100.0% |
| loss | 124 / 178 | 69.7% |
| artifact-validation | 0 / 170 | 0.0% |
| deterministic runtime-binding policy | 41 / 111 | 36.9% |
| aggregate | 2,239 / 2,893 | 77.4% |

The aggregate and the DD-version, loss, artifact-validation, and deterministic
runtime-binding-policy groups are below their floors. Later work must raise
coverage or revise this documented scope deliberately; this ticket records the
measurement and its enforcement mechanism without pretending the baseline has
already passed.
