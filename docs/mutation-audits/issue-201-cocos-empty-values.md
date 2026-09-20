# Issue #201: COCOS inversion and EMPTY-value mutation audit

The focused audit used cargo-mutants' fresh scratch trees after the affected
unit tests passed:

```console
$ cargo mutants --all-features --file src/conversion/conversion_map.rs \
    --re 'ValueTransformation::inverse' -- --lib
$ cargo mutants --all-features --file src/conversion/seam_policy.rs \
    --re 'apply_value_transformation' -- --lib
$ cargo mutants --all-features --file src/conversion/seam_policy.rs \
    --re 'copy_value_transformation' -- --lib
```

| Focus | Caught | Missed | Timed out | Unviable | Classification |
| --- | ---: | ---: | ---: | ---: | --- |
| `ValueTransformation::inverse` | 4 | 0 | 0 | 1 | The unviable `Some(Default::default())` replacement cannot compile because `ValueTransformation` has no default value. The caught set includes the saved `from_cocos != to_cocos` guard mutations; equal conventions refuse while differing conventions invert and round-trip. |
| `apply_value_transformation` | 3 | 0 | 0 | 0 | Mixed read data proves actual values negate and `EMPTY_DOUBLE` does not. |
| `copy_value_transformation` | 9 | 0 | 0 | 0 | Write verdicts prove the shim-owned negated copy, caller preservation, inversion guard and refusal behavior. |

There are no surviving or timed-out mutants, and no equivalent-mutant
exclusions. `is_empty_scalar` generated no mutants in cargo-mutants 27.1.0;
the direct scalar-boundary test remains regression coverage for that dispatch.
