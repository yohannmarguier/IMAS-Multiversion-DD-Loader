# Issue #194: DD-version-stamp mutation audit

The focused audit ran in a fresh scratch tree after the affected unit tests
passed:

```console
$ cargo mutants -f src/version/version_stamp.rs --all-features \
    --output /private/tmp/imas-mvdd-issue194-mutations-final \
    -- --lib version_stamp
```

It generated seven mutants: three caught, one missed equivalent, zero timed
out, and three unviable. The baseline build and focused tests passed.

| Mutant | Outcome | Classification |
| --- | --- | --- |
| `decode` replaced with `None` | caught | A reported valid release must produce `Stored`. |
| `decode` replaced with `Some(Default::default())` | unviable | `DdVersion` has no default value. |
| `classify_discovery_read` replaced with `Default::default()` | unviable | `StampOutcome` has no default value. |
| `reported_extent > 0` replaced with `== 0` | caught | A positive reported extent must expose its valid bytes. |
| `reported_extent > 0` replaced with `< 0` | caught | A positive reported extent must expose its valid bytes. |
| `reported_extent > 0` replaced with `>= 0` | missed, equivalent | At zero both branches decode an empty slice as malformed; positive and negative extents take the same branches. |
| `discover` replaced with `Default::default()` | unviable | `StampOutcome` has no default value. |

The sole surviving mutation is excluded narrowly as equivalent: no caller can
observe a difference because the zero-length slice decodes to the same
malformed-stamp refusal in either form. The source comment beside the bound
records the same reasoning.
