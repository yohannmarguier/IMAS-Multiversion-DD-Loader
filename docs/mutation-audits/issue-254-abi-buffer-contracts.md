# Issue #254: C ABI buffer-contract mutation witnesses

Each focused mutation was built after deleting the affected C test executable,
so the result could not come from a stale binary. The recording-stub scenarios
then failed at the public C ABI boundary, rather than at a Rust-only policy
unit test.

| Operation | Mutation | Failing scenario | Observed failure |
| --- | --- | --- | --- |
| Read | Write `0.0` instead of `EMPTY_DOUBLE` for a shim-decided rank-zero absence in `src/interpose/read.rs`. | `read-path-no-source-scalar-receives-the-empty-sentinel` | The caller's scalar was not the exact `-9e40` EMPTY sentinel. |
| Write | Preserve the value instead of negating it in `copy_value_transformation` in `src/conversion/seam_policy.rs`. | `write-path-plugin-cocos-sign-flip-uses-a-shim-owned-copy` | The plugin recording stub did not receive the expected transformed `-1.25`. |
| Delete | Limit the candidate loop in `run_delete` to its first entry. | `delete-path-reports-a-failure-and-continues` | The later candidate failure was not retained as status `-24`. |

All three mutations were restored immediately after their focused red run.
The normal sources were rebuilt and the corresponding scenarios passed before
the final suite.
