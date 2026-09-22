# Rust decision-bearing line-coverage audit

Run the local audit from the repository root:

```console
$ bash scripts/audit-rust-line-coverage.sh
```

It runs two independent configurations and never uses `--all-features`:

- the production audit uses `--no-default-features`, writes
  `target/rust-line-coverage-production.lcov`, and checks
  `coverage/rust-line-coverage-scope.json`;
- the private XML regression audit uses only `xml-fixture-source`, writes
  `target/rust-line-coverage-xml.lcov`, and checks
  `coverage/rust-xml-line-coverage-scope.json`.

Each report prints every group and the aggregate under a configuration heading.
The checker adds covered and total lines; it never averages percentages. Keeping
the reports and scopes separate prevents XML fixture hits from satisfying a
production runtime-map assignment. Before that unfiltered production run, the
audit inventories its library tests and requires the six scenario witnesses in
`coverage/rust-production-line-coverage-tests.json`; a missing, renamed, or
feature-gated witness fails before coverage is measured. A second inventory of
ignored tests ensures every witness will execute in the following unfiltered
coverage run.

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
| conversion | Conversion-map resolution, production graph decoding/map construction/acquisition coordination, path and seam policy, read outcome, the reentry gate, deterministic refusal formatting, and `lib.rs`'s shared status-buffer formatting. The XML scope substitutes `known_artifacts.rs` for the production runtime-map modules. |
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
policy ranges in `core_binding.rs` — including the fallback each unresolvable
IMAS-Core selects — remain measured. The
`validate_equilibrium_coverage` binary is excluded as its command-line,
filesystem, and terminal adapter. Production graph coverage measures the
controlled executor, raw-row validation, wire decoding, historical map
construction, failures, retries and same-key coordination. Its exclusions are
narrowly the live Bolt connection/I/O/value adapter and the process-global
occurrence adapter; live graph and C-ABI suites own those integration layers.
Update the assignments with an internal seam extraction; do not silently shrink
the scope.

**An exclusion may be ranged too.** Where only part of a measured file is
another test layer's business — `version_stamp.rs`'s discovery read, the
process-wide `OnceLock` in `hli_version.rs`, `lib.rs`'s exported entry points,
`core_binding.rs`'s loader, and the three thin dispatchers that read a
process-wide value (`core_binding.rs`'s version accessors,
`refusal.rs`'s seam gate, `loss_file.rs`'s `retain`) — the exclusion carries
`start_line`/`end_line` and
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

Both configurations retain the 90% aggregate and 80% per-group floors.
Ordinary CI runs this same command in its `rust-line-coverage` job and blocks on
either verdict. Both LCOV reports are uploaded even if the audit fails, so
inspect the matching configuration's totals before changing scope or tests.
The compact fixture test is registered as
`rust-line-coverage-audit-fixtures` and proves the aggregate/per-group boundary,
malformed-data behavior, overlapping ownership refusal, and both truncated and
newly stale range refusals against a throwaway source tree, without requiring a
full coverage run. The workflow guard also
parses the audit script: negative fixtures reject removal of the production
run, replacement with `--all-features`, and scoring the XML report where
runtime-map measurement is required. They also reject removal of a required
runtime-map source assignment or test witness.

## Integrated baselines (2026-09-22)

The pinned tool reported 390 passed production tests with three live-service
tests explicitly ignored, followed by 306 passed XML-fixture tests.
`cargo-llvm-cov 0.9.1` is available under Rust 1.88.0, the CI toolchain. Both
configurations pass all floors.

| Group | Production covered/total | Production | XML covered/total | XML |
| --- | ---: | ---: | ---: | ---: |
| conversion | 4,118 / 4,652 | 88.5% | 2,004 / 2,298 | 87.2% |
| DD-version | 188 / 199 | 94.5% | 188 / 199 | 94.5% |
| context-registry | 170 / 170 | 100.0% | 170 / 170 | 100.0% |
| loss | 238 / 239 | 99.6% | 238 / 239 | 99.6% |
| artifact-validation | 222 / 227 | 97.8% | 222 / 227 | 97.8% |
| deterministic runtime-binding policy | 110 / 110 | 100.0% | 110 / 110 | 100.0% |
| aggregate | 5,046 / 5,597 | 90.2% | 2,932 / 3,243 | 90.4% |

The XML denominator retains issue #248's repaired completeness, private-fixture
lookup, and seam-policy ranges. Its fixture guard rejects overlapping ownership
and proves an inserted production decision makes a formerly complete range
stale.
