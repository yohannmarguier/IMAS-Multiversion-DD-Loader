# Issue #204: integrated audit baseline

Run on 2026-09-17 from clean `unit-test` revision `a4818e1`, before this CI
enforcement change. `cargo-llvm-cov 0.9.1` reported 299 passing Rust unit tests
on the scoped line command; `cargo-mutants 27.1.0` selected 592 mutants and
finished in 17m13s. Both tool pins are available under Rust 1.88.0, the
workflow's configured toolchain.

```text
line coverage
conversion: 1900/2095 lines (90.7%) PASS
dd-version: 154/161 lines (95.7%) PASS
context-registry: 183/183 lines (100.0%) PASS
loss: 148/149 lines (99.3%) PASS
artifact-validation: 222/227 lines (97.8%) PASS
deterministic-runtime-binding-policy: 102/110 lines (92.7%) PASS
aggregate: 2709/2925 lines (92.6%) PASS

mutation coverage
conversion: caught=272 missed=0 timed-out=0 unviable=61 excluded=2 score=100.0% PASS
dd-version: caught=54 missed=0 timed-out=0 unviable=8 excluded=0 score=100.0% PASS
context-registry: caught=24 missed=0 timed-out=0 unviable=4 excluded=0 score=100.0% PASS
loss: caught=26 missed=0 timed-out=0 unviable=5 excluded=0 score=100.0% PASS
artifact-validation: caught=69 missed=0 timed-out=0 unviable=5 excluded=0 score=100.0% PASS
deterministic-runtime-binding-policy: caught=61 missed=0 timed-out=0 unviable=1 excluded=0 score=100.0% PASS
aggregate: caught=506 missed=0 timed-out=0 unviable=84 excluded=2 score=100.0% PASS
```

The raw mutation report retained two missed fallback-arm deletions, both exact
entries in `coverage/rust-mutation-dispositions.json`. Each is an unreachable
equivalent after the shared-refusal check; the audit prints both in its survivor
inventory and excludes no timeout or unexplained survivor.
