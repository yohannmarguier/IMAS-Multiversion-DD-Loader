# Issue #246 validation and source-assignment ledger

This ledger records the corrective CI/HLI integration for issue #246. It
distinguishes checks executed locally from Linux-only graph/Core/HLI checks
that must run in the hosted workflows after the commit is published.

## Reviewed baseline and reproduction

Implementation started from `2d2b7cd`, a descendant of the reviewed #232
measurement commit `a1bc5aa` and #233 production-cutover commit `feb843e`. It
also contains the integrated #244 and #245 repairs.

The two review findings reproduced before the change:

- **F9:** `IMAS_MVDD_GRAPH_TEST_SOURCE=live` registers
  `live-graph-core-coexistence-{forward,reverse,nested}` in
  `cmake/tests/RealCore.cmake`, while the full job selected five names beginning
  `read-coexistence-`, `write-coexistence-` or `write-delete-coexistence-`.
  The mismatch is structural: none of the live names can match that filter.
- **F12:** pinned IMAS-Fortran `cd6ea111948bbff7b07b36b39992c56b565dea9b`
  and IMAS-Cpp `4c1109c2471c403f2675100d2f844776877386eb`
  both register `stamp-mismatch-no-artifact` as clean XML-era passthrough.
  Their sources require success, no skipped paths and an absent renamed field,
  whereas production `feb843e` acquires a graph map or refuses an uncached
  mismatch. This was static source evidence; the old hosted configuration did
  not reach a meaningful production assertion.

## Source matrix

| Profile | Shim source | Required direct selection | Contract |
|---|---|---|---|
| Full real-Core | Installed production graph source | 3 live coexistence scenarios | Forward, reverse and nested live graph behavior; exact enabled inventory before execution. |
| Controlled real-Core | Private `graph-test-source` stage | 5 controlled coexistence scenarios | Original deterministic candidate assertions, isolated from the live profile. |
| Fortran legacy | Private `xml-fixture-source` package | 144 registered, 20 disabled at the pinned fork | Original suite and XML-specific assertions remain unchanged, including no-artifact passthrough. |
| Fortran production | Installed production graph source | 5 enabled scenarios | Graph-supported COCOS write/read plus version-unset, equal-stamp, absent-stamp and malformed-stamp behavior. |
| C++ legacy | Private `xml-fixture-source` package | 65 enabled at the pinned fork | Original Tier-1 suite, examples and XML-specific assertions remain unchanged. |
| C++ production | Installed production graph source selected by loader path, verified with `ldd` | 5 enabled scenarios | Cross-DD COCOS round trip plus version-unset, equal-stamp, absent-stamp and malformed-stamp behavior. |
| C++ production refusal | Installed production graph source, fresh process with `NEO4J_PASSWORD` absent | 1 dedicated HLI probe | Uncached equilibrium 3.40.0 → 4.1.1 acquisition refuses, leaves the IDS untouched and emits the reason, IDS and both versions. |

`scripts/prepare-private-xml-fixture-package.sh` copies the already installed
package metadata into a job-private prefix and replaces only its library with
the existing `xml-fixture-source` stage. The destination must not already
exist. The ordinary install remains graph-backed, and no build option,
installed setting or runtime fallback can select the XML source.

The pinned HLI revisions do not change. No unavailable or mutable fork commit
is introduced. Both profiles use IMAS-Core
`3e5871a844c594491ab9e5365b63576f552bf50f`, DD 4.1.1 and graph release
`v5.3.0` at manifest
`sha256:dc90975cb9fa0c7b08e9e4809640d01e41d927b4162200c13eec5076e030329b`.

## Requirement-to-check mapping

| Issue #246 criterion | Check |
|---|---|
| AC1 | Baseline ancestry checks and the two reproductions above. |
| AC2 | `check_ctest_inventory.cmake` requires the exact three live names, enabled and nonempty, before the full job runs them. |
| AC3 | A separate controlled real-Core build requires and runs the exact five original names. |
| AC4 | The live selection no longer exits at a zero-match grep; the complete suite, install and consumer steps remain after it. |
| AC5 | The source matrix above; workflow guards require both private-fixture and production prefixes. |
| AC6 | Fortran and C++ production conversion selections plus `cpp_graph_acquisition_refusal.cpp`. |
| AC7 | Both production HLI selections include version-unset, equal-stamp, absent-stamp and malformed-stamp cases; existing failed-open checks remain in the repository suites. |
| AC8 | The XML library is overlaid only into a private CI prefix; the installed production package and runtime configuration are unchanged. |
| AC9 | Workflow linkage checks retain the committed HLI/Core pins; C++ additionally resolves the production shim path with `ldd`. |
| AC10 | Hosted Fortran/C++ steps assert exact registered/enabled selections, execute with `--no-tests=error`, and write source pins plus graph identity to the job summary. |
| AC11 | The final local and hosted command ledger below. |
| AC12 | MATLAB and Java jobs are untouched; no deadline or operation/scientific policy changes are made. |
| AC13 | This document is the review ledger. |

`check_ctest_inventory.cmake` is itself covered against valid, empty, missing,
disabled and unexpectedly expanded inventories, so a zero-test or renamed-away
selection cannot become a green workflow edit.

## Execution ledger

Executed locally on the combined revision:

```console
cmake -S . -B build-merge-219 -DIMAS_MVDD_REAL_CORE_TESTS=OFF
# 265 tests registered

ctest --test-dir build-merge-219 \
  -R '^(ci-workflow|hli-validation-workflow|ctest-inventory-contract|private-xml-fixture-package)$' \
  --output-on-failure --no-tests=error
# 4/4 passed

cargo check --all-targets
# passed

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features xml-fixture-source -- -D warnings
# passed

cmake --build build-merge-219
ctest --test-dir build-merge-219 --output-on-failure --no-tests=error
# 265/265 passed
```

The hosted Linux checks are deliberately not reported as passed before a
published run exists. Required post-publication evidence is:

- full CI: 3/3 live coexistence scenarios, 5/5 controlled coexistence
  scenarios, then the complete real-Core/package/install/consumer sequence;
- Fortran: 144 registered, 124 enabled in the private XML profile, plus the
  exact five production scenarios and their fixture setup checks;
- C++: 65 registered/enabled/passed in the private XML profile, the exact five
  production scenarios, and the dedicated unavailable-acquisition probe;
- job summaries and diagnostics carrying the HLI, Core and graph identities
  above.

Until those runs exist, this ledger is implementation and local-regression
evidence, not a claim that issue #246 or parent #205 can be closed.
