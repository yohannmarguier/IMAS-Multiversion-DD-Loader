# Review follow-up: integrated audit baseline

Run on 2026-09-18 from the clean `unit-test` tip after the two-axis review of
issue #189 and its children, on the completed measurement scope. `cargo-mutants
27.1.0` selected 703 mutants and finished in 24m; `cargo-llvm-cov 0.9.1`
reported 300 passing Rust unit tests on the scoped line command.

```text
line coverage
conversion: 1901/2096 lines (90.7%) PASS
dd-version: 188/199 lines (94.5%) PASS
context-registry: 183/183 lines (100.0%) PASS
loss: 238/239 lines (99.6%) PASS
artifact-validation: 222/227 lines (97.8%) PASS
deterministic-runtime-binding-policy: 110/110 lines (100.0%) PASS
aggregate: 2842/3054 lines (93.1%) PASS

mutation coverage
conversion: caught=278 missed=0 timed-out=0 unviable=61 excluded=3 score=100.0% PASS
dd-version: caught=58 missed=0 timed-out=0 unviable=10 excluded=1 score=100.0% PASS
context-registry: caught=24 missed=0 timed-out=0 unviable=4 excluded=0 score=100.0% PASS
loss: caught=122 missed=0 timed-out=0 unviable=5 excluded=0 score=100.0% PASS
artifact-validation: caught=69 missed=0 timed-out=0 unviable=5 excluded=0 score=100.0% PASS
deterministic-runtime-binding-policy: caught=62 missed=0 timed-out=0 unviable=1 excluded=0 score=100.0% PASS
aggregate: caught=613 missed=0 timed-out=0 unviable=86 excluded=4 score=100.0% PASS
```

The scope grew by 111 selected mutants against issue #204's 592, because three
files' ranges had stopped short of their inline test module. What that exposed,
and what it cost, is the point of recording this run:

- **Two timeouts, which fail the audit by policy.** Mutating the
  `AlreadyExists` guard in `LossFileWriter::create_log` turned `for suffix in
  0_u32..` into an endless retry. The search is now bounded and reports
  exhaustion like any other creation failure.
- **Two survivors in `civil_from_days`' pre-epoch era adjustment**, which
  `utc_timestamp` cannot reach because it divides a `u64` clock reading. The
  branch is gone rather than excluded (ADR 0011).
- **One survivor on `ResolutionError::status`**, now asserted directly.
- **Six survivors on wrappers whose whole body reads a process-wide value** —
  `core_binding`'s four version accessors, `refusal.rs`'s seam gate,
  `loss_file`'s `retain`. No Rust unit test can reach them without settling
  process state, so each is a ranged line-scope exclusion naming the C suite
  that does prove it, rather than a test written to satisfy the number.

The four remaining survivors are all exact entries in
`coverage/rust-mutation-dispositions.json`: three unreachable fallback arms and
one `>`/`>=` boundary where both sides yield a zero-length slice. Each is
printed in the survivor inventory above and excluded from the score; no timeout
and no unexplained survivor remains.
