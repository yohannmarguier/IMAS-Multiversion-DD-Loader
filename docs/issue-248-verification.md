# Issue #248 coverage-scope verification

The audit was reproduced at `8f996361ea03d0ba2f22f4bd2584af3a27ba27df`
with the pinned `cargo-llvm-cov 0.9.1`. All 306 Rust unit tests passed, after
which the scope checker exited 2 with these ownership errors:

```text
coverage audit input error: src/conversion/conversion_map.rs: production lines 1965-2340 belong to no group and no exclusion (376 of 2340 unassigned); extend a range or declare a ranged exclusion with its reason
coverage audit input error: src/conversion/known_artifacts.rs: production lines 55-61 belong to no group and no exclusion (7 of 61 unassigned); extend a range or declare a ranged exclusion with its reason
coverage audit input error: src/conversion/seam_policy.rs: production lines 878-879 belong to no group and no exclusion (2 of 879 unassigned); extend a range or declare a ranged exclusion with its reason
```

These ranges contain conversion-map completeness and parsing decisions, the
private XML-fixture lookup's second direction and fallthrough, and read-verdict
assembly. They therefore remain measured in the `conversion` group; no
integration exclusion was added.

After repairing the three ranges, `bash scripts/audit-rust-line-coverage.sh`
passed with the existing 90% aggregate and 80% per-group floors:

```text
conversion: 2004/2298 lines (87.2%) PASS
dd-version: 188/199 lines (94.5%) PASS
context-registry: 170/170 lines (100.0%) PASS
loss: 238/239 lines (99.6%) PASS
artifact-validation: 222/227 lines (97.8%) PASS
deterministic-runtime-binding-policy: 110/110 lines (100.0%) PASS
aggregate: 2932/3243 lines (90.4%) PASS
```

The fixture suite retains exact equality checks at the 90% aggregate and 80%
group thresholds, missing and empty measurement checks, and now explicit
overlapping-range coverage. Its throwaway Rust source also starts from a valid
declared scope, inserts a production decision before the inline test module,
and proves the formerly complete range is rejected as stale.
