# Occurrence adapter integration coverage

Issue #253 measures the C-facing occurrence and context-lifetime contract that
the scoped Rust unit audit deliberately excludes.  The recording-stub
integration collector selects the exact `occurrence-adapter` CTest ledger in
`cmake/tests/Shim.cmake`; every selected scenario is a separate process, so the
process-wide HLI DD version latch and context registry cannot leak between
cases.

The 16 scenarios are split between two independently instrumented artifacts:

| Contract | Evidence |
| --- | --- |
| accepted, conflicting, invalid-environment and unset latch outcomes | four `hli-dd-version-*` processes |
| write-mode discovery requests `READ_OP` and closes its probe | `version-discovery-write-mode-probe-asks-for-a-read-context` and `graph-runtime-map-write-mode-uses-read-op-stamp-probe` |
| absent, matching, malformed and mismatching stamps across ordinary and plugin opening families | `graph-runtime-map-{absent-stamp-is-a-passthrough,matching-stamp-is-a-passthrough,malformed-stamp-refuses-and-ends-every-family,opening-families-reuse-a-retained-map}` |
| disabled conversion remains a graph-free forward | `graph-runtime-map-conversion-disabled-is-a-plain-forward` |
| acquisition failure closes every opened family and a later open retries | `graph-runtime-map-{failure-closes-every-opening-family,later-open-retries-a-failed-acquisition}` |
| child inheritance, non-LIFO close and recycled IDs | three `context-lifecycle-*` processes |

The collector writes `occurrence-adapter-xml.lcov` and
`occurrence-adapter-graph.lcov`, preserves both raw-profile sets beneath
`occurrence-adapter-profiles/`, and merges only their source counts in
`metadata.json`.  It does not merge incompatible cargo artifacts or fold this
result into the unit-coverage percentage.

## Measured result

The issue #253 implementation run covered 457 of 553 executable lines in
`src/interpose/occurrence.rs`.  The occurrence-specific policy slice —
`decide_occurrence_registration` and `decide_datapath_translation`, executable
lines 98–134 of `src/conversion/seam_policy.rs` — covered 29 of 29 lines.  The
metadata reports those as separate `adapter` and `policy` objects and records
every uncovered adapter line explicitly.

The 96 adapter lines outside this focused ledger are not silently presented as
policy coverage.  They are the private XML artifact-unavailable fallback, a
cached-map failure before a new Core context exists, a forwarded Core-open
failure, null `dataobjectname` handling, invalid internally constructed DD
versions, and the richer arraystruct candidate/refusal/plugin branches.  Those
arraystruct behaviors remain exercised by their existing mechanism and graph
scenario families; issue #253's child-lifetime claim needs the child
registration and close paths, not a duplicate candidate-plan suite.

## Mutation sensitivity

Each mutation was compiled after moving the previously built shared library
out of the stage, so the selected C scenario could not execute a stale binary:

| Mutation | Precise failing assertion |
| --- | --- |
| omitted the global-family refusal `call_end` | `graph-runtime-map-failure-closes-every-opening-family` failed its first expected end-action count |
| changed the shim-owned stamp probe from `READ_OP` | `version-discovery-write-mode-probe-asks-for-a-read-context` failed the recorded mode assertion |
| retained a child record after successful `al_end_action` | `context-lifecycle-ending-child-removes-only-its-own-record` observed translated instead of verbatim forwarding |

These are behavioral mutation observations, not a new mutation-score gate.
The ordinary collector always restores a fresh output tree and fails if either
the scenario ledger or either component report fails.
