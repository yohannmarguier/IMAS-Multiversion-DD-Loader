# Rust coverage from C ABI scenarios

The Rust unit-coverage audit deliberately excludes dynamic loading, raw C ABI
adaptation and real-IMAS-Core integration. Collect those lines separately with:

```console
$ IMAS_MVDD_INTEGRATION_CMAKE_PREFIX_PATH=/path/to/imas-core-prefix \
    bash scripts/collect-rust-integration-coverage.sh all
```

`all` runs two independent profiles. `recording-stub` drives the private XML
fixture shim through the existing recording-stub C scenarios. `real-core`
reuses the existing runtime-binding real-Core scenario in an executable linked
to the ordinary production graph-backed shim. The latter needs either an
installed IMAS-Core discoverable by CMake or
`IMAS_MVDD_INTEGRATION_CMAKE_PREFIX_PATH`. Set
`IMAS_MVDD_INTEGRATION_DOWNLOAD_CORE=1` instead to use the repository's pinned
download-and-build mode. Either profile can be run by itself.

Each invocation deletes and recreates only its task-owned directories under
`target/rust-integration-coverage{,-build}/<profile>`. The build inherits
`cargo llvm-cov show-env`, so cargo-c produces an instrumented shared shim.
Raw profile names contain both `%p` and `%m`: concurrent subprocesses and
different instrumented binaries cannot overwrite one another. Build-time
profiles are removed before CTest starts, and `--failure-mode any` rejects a
raw profile that is incompatible with the selected artifact.

The reports remain separate by construction:

| Profile | Rust object | C scenario family | Output |
| --- | --- | --- | --- |
| `recording-stub` | private `xml-fixture-source` shim | recording stub | `target/rust-integration-coverage/recording-stub/` |
| `real-core` | production graph-backed shim | real IMAS-Core | `target/rust-integration-coverage/real-core/` |

Every output directory retains `coverage.lcov`, raw profiles, configure/build
logs, the CTest log, the canonical loaded-library path and `metadata.json`.
The metadata records the Git revision, configuration, artifact path, profile
count and report totals. It also publishes covered and measured line counts for
the read, write and delete adapters and their shared seam policy, so the C ABI
evidence for those intentionally unit-scope-excluded modules is visible without
deriving it from the full LCOV file. These per-source counts are measurements,
not thresholds. The loader-only C probe resolves `al_context_info` with
`dladdr` and fails unless its canonical provider is the exact staged
instrumented library.

The recording-stub selection includes the XML-fidelity scenarios as well as
their `xml-fidelity` label: CTest labels are cumulative, so adding that
diagnostic label must not remove those scenarios from the published profile.
The run also records an executable negative/positive proof.
Loading the shim without calling Rust produces `baseline.lcov` with zero hits
in `src/core/core_binding.rs`; running the existing
`runtime-binding-success` scenario produces `probe.lcov` with positive hits.
Removing that scenario therefore removes those hits rather than leaving an
assumed integration claim.

These reports are evidence, not a second coverage gate. They intentionally set
no whole-crate percentage floor and do not change the scoped 90% Rust-unit
baseline. A CTest failure is returned after any usable report has been written,
so behavioral failure status is never hidden by successful report generation.
CI uploads each profile's complete directory with `if: always()`.
