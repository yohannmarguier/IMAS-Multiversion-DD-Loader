# Issue #197: loss-file failure mutation audit

The focused audit used cargo-mutants 27.1.0 in a fresh scratch tree after the
loss-file unit module passed:

```console
$ cargo mutants --all-features --file src/loss_file.rs \
    --re 'LossFileWriter<F, E>::(retain|create_log|report_failure)' \
    --output /private/tmp/imas-mvdd-issue197-writer-mutations \
    -- --lib loss_file
```

It generated 13 mutants: 10 caught, zero missed, two timed out and one was
unviable. The timeout outcome is a caught result here: both mutations broaden
the `AlreadyExists` retry guard, so the deterministic non-collision creation
effect is retried through the unbounded suffix loop until cargo-mutants stops
the test after 20 seconds.

| Focus | Caught | Missed | Timed out | Unviable | Classification |
| --- | ---: | ---: | ---: | ---: | --- |
| Writer retain, creation and failure latch | 10 | 0 | 2 | 1 | The tests catch lost delivery, duplicate-key reversal, lost log creation, inverted directory checks and rejection of an actual collision. The two timeout mutants retry a `PermissionDenied` creation failure indefinitely. The `if let`-chain `&&` to `||` replacement is invalid Rust. |
| Production preamble and append effects | 2 | 0 | 0 | 0 | The exact preamble and complete appended records catch both no-op effects. |
| One-diagnostic failure latch | 2 | 0 | 0 | 0 | Injected creation, clock and directory failures observe one diagnostic and disabled subsequent delivery. |

There are no surviving mutants and no equivalent-mutant exclusions. The
scratch tree prevents a stale test binary from contributing to an outcome.
