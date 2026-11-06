# Artifact validation executable coverage

Issue #256 measures the public `validate_equilibrium_coverage` executable:

```console
bash scripts/collect-rust-integration-coverage.sh artifact-command
```

The collector builds with no private fixture feature, using the pinned
coverage tool. It starts with an empty task-owned build/report directory,
removes build-time raw profiles, and executes the resulting binary directly
through `tests/coverage/check-artifact-command.py`. No library helper or Rust
unit test can substitute for these invocations.

The nine scenarios check default and explicit approved artifacts, an unknown
argument, a missing `--artifact` value, missing and unreadable inputs, malformed
XML, a reduced rule set below the rename floor, and a single unclaimed rule
that fails completeness. A directory supplies the unreadable-input case even
when CI runs as root. Expected inventory totals come from the checked-in text
inventories; rename totals come from their independent baseline. The supported
floors remain 346 and 339, matching the existing CMake contract. The tests check
numeric accounting and stable diagnostic/report fields, not snapshots of
temporary paths or timings. Existing CLI semantics are unchanged.

The November 6 implementation run (Darwin, Rust 1.97.1,
cargo-llvm-cov 0.9.1) passed all nine scenarios and covered all 41 executable
lines in `src/bin/validate_equilibrium_coverage.rs`. The whole linked binary
also contains library code; its aggregate is diagnostic and does not replace
this command-specific measurement.

`metadata.json` records the revision, toolchain, feature configuration, binary,
raw-profile count and command source coverage. `scenarios.json` retains each
subprocess's result and output. CI uploads both with the LCOV/raw profiles,
including on failure. Checker fixtures reject helper-only and missing-command
measurements; workflow fixtures reject removal of the command run or upload.

Two fresh-build mutations were killed after deleting the executable before
each rebuild: returning success for errors failed the exact exit-status
assertion, and suppressing the diagnostic failed the stderr assertion. The
original command was restored, rebuilt, and passed all nine cases.
