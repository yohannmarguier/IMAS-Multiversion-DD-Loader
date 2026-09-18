# Historical pulse_schedule real-Core oracle — #229

Issue #229 extends the graph-selected runtime-map test instance to the second
IDS pair, `pulse_schedule` 3.25.0 ↔ 3.30.0.  It does not change runtime map
construction, conversion policy, or the production map-source selection.

## Scenario ledger

`graph_runtime_map_oracle_test` creates one private HDF5 pulse per scenario
through the public C ABI, then writes its stored DD-version stamp directly as
oracle setup.  The operation under test runs only through the graph-selected
shim; raw HDF5 is used afterwards only to inspect the stored effect.

| Direction | Read | Write | Delete |
| --- | --- | --- | --- |
| 3.30.0 HLI → 3.25.0 stored | `ec/launcher/steering_angle_pol` reads the stored `ec/antenna/launching_angle_pol` value | writes only `ec/antenna/launching_angle_pol` | removes that exact stored leaf and the mapped trivial `ec/launcher` structure |
| 3.25.0 HLI → 3.30.0 stored | `ec/antenna/launching_angle_pol` reads the stored `ec/launcher/steering_angle_pol` value | writes only `ec/launcher/steering_angle_pol` | removes that exact stored leaf and the mapped trivial `ec/antenna` structure |

All eight cases assert that the stored DD-version stamp survives and an unrelated
stored scalar remains `99.0`.  The two historical C-ABI tracer scenarios also
exercise the mapped parent anchor with relative and absolute child fields in
both directions.  They keep `ec/beam` out of both endpoint plans: it is a
later witness for the dated correspondence, not a 3.25.0 or 3.30.0 candidate.

The scenario has no value transformation.  The historical angle path is
structural/numeric data whose endpoint correspondence is established without a
COCOS factor; it does not treat missing historical COCOS metadata as a
factor-one fallback.  The retained graph-runtime C-ABI refusal scenarios remain
the coverage for unknown COCOS and unresolved coordinate/timebase or unit
evidence, where the affected operation refuses before Core while independent
paths remain available.

## Verification

On the pinned IMAS-Core source revision from `IMAS_CORE_REF`, the focused
real-Core run registered and passed six cases:

```console
ctest --test-dir build-issue-229-real -R '^(read|write|delete)-graph-pulse-schedule-' --output-on-failure
# 8/8 passed
```

The graph-selected recording-stub tracer also passed its two historical nested
direction cases.  The test instance is deliberately an internal controllable
graph source: live-Neo4j setup remains CI-gated by the graph-required workflow,
and the installed-shim production source has not switched away from its
existing map selection.
