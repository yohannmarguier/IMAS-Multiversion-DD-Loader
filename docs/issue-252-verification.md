# Issue 252 verification

Issue 252 keeps dynamic binding and C-facing forwarding outside the scoped Rust
unit gate. This record makes their behavioral coverage and C-driven Rust hits
explicit without converting either into a misleading whole-crate percentage.

## Behavioral contract

`tests/shim/runtime_binding_test.c` runs every scenario in a fresh process.
It checks the successful ordinary and plugin ABI routes against a recording
Core: selected symbol, every input argument, return status and output storage.
The failure scenarios assert the following public results:

| Core condition | `al_context_info` result | Status-less accessors |
| --- | --- | --- |
| library is absent | code `-1`, override diagnostic | not called by the scenario |
| version major differs | code `-1`, both versions named | the ADR 0001 fallback values |
| bootstrap `getALVersion` is null | code `-1`, null-version diagnostic | not called by the scenario |
| version-compatible library lacks `al_context_info` | code `-1`, missing-symbol diagnostic | `getALVersion`, `getDDVersion`, `const2str` and `err2str` are null |

`tests/stub/missing_symbol_stub.c` deliberately exports only the bootstrap
`getALVersion`. It proves that a successful bootstrap does not make a partial
library usable.

`tests/shim/reentry_guard_test.c` supplies the stored spelling
`beta_normal` from below Core and asserts that it arrives once, unchanged. A
following HLI-side write still resolves `beta_tor_norm` to `beta_normal`, which
proves both that the guard depth was restored and that passthrough did not
alter the mismatched context's registry record.

## Measured C-driven coverage

The recording-stub command below produced an LCOV file with cargo-llvm-cov
0.9.1 on Darwin. The real-Core command is the reproducible full-profile
workflow; the production adapter measurement below drove its staged pinned-Core
shim through `runtime-binding-production-real-core`. The retained reports live
under `target/rust-integration-coverage/` and are intentionally untracked.

```console
$ bash scripts/collect-rust-integration-coverage.sh recording-stub
$ IMAS_MVDD_INTEGRATION_DOWNLOAD_CORE=1 \
    bash scripts/collect-rust-integration-coverage.sh real-core
```

The pinned Core production build was additionally driven through
`runtime-binding-production-real-core`, which passes a legal HDF5 lifecycle
through the staged production shim. Its LCOV report, and the recording-stub
profile's complete 195-scenario report, produced these source-level hits:

| Adapter | Recording stub | Real-Core forwarding |
| --- | ---: | ---: |
| `src/core/core_binding.rs` | 177/221 | 65/221 |
| `src/interpose/dispatch.rs` | 98/98 | 0/98 |
| `src/interpose/reentry.rs` | 11/11 | 0/11 |
| `src/interpose/passthrough.rs` | 97/97 | 3/97 |

The smaller production set is deliberate rather than a test gap. A complete
real Core cannot supply the partial-library failures; that requires the
minimal fixture. The production forwarding scenario has no controllable plugin
callback, so dispatch-family selection and reentry are measured through the
recording Core that can call back through the public shim. Its three
passthrough hits are the legal real-Core lifecycle's context-info route; the
remaining verbatim exports require sentinel arguments and return buffers that
the recording Core exposes for exact inspection. The stub contract closes
those observability gaps without treating a fabricated Core as a production
oracle.

## Fresh-build mutation witnesses

Both mutations were compiled in a new CMake build directory before the named
behavioral scenario ran:

| Mutation | Assertion that killed it |
| --- | --- |
| route `al_plugin_read_data` to `al_read_data` | `runtime-binding-plugin-forwarding` failed its plugin call-count assertion |
| return code `0` from runtime-binding `failure()` | `runtime-binding-missing-symbol` failed its exact `-1` assertion |

This is a behavioral proof: neither witness relies on Rust implementation
coverage or an internal mock.
