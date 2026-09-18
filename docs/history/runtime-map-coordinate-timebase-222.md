# Coordinate and timebase evidence — #222

Issue #222 extends the controlled runtime-map acquisition boundary without
adding a resampler, a contour synthesizer, an upstream graph repair, or any
new seam policy. `EndpointMetadata` still carries raw/replayed coordinate
paths and a timebase path; `GraphEvent` now carries only a producer-established
coordinate/timebase verdict where one exists. The graph transport projects
`HAS_COORDINATE` relationships separately from historical coordinate event
values, because the former are unversioned and can have omitted or
index-stripped targets.

## Delivered behaviour

- A nonempty, positionally matching coordinate declaration plus matching (or
  directly evidenced renamed) timebase/coordinate paths is exact only when a
  preserved `HAS_COORDINATE` relationship corroborates every raw dimension or
  an applicable producer-established `Equivalent` verdict does. A raw match
  alone remains unresolved.
- Equal empty coordinate collections and unequal collections without further
  evidence are both localized `Unmappable` refusals. Neither proves an
  equivalent representation or a need for resampling.
- `RequiresResampling` is a path-local unsupported result. The existing
  resolver and seam policy turn it into the ordinary safe-conversion refusal;
  no numerical interpolation is attempted.
- `UnboundedScope` aborts map acquisition rather than certifying arbitrary
  neighbouring paths. Independent established paths remain usable when the
  uncertainty is bounded to a path.
- The C ABI graph-test source contains one classified resampling timebase.
  Its shared-harness scenario proves a safe `field` does not mask the unsafe
  `timebase` for either `al_write_data` or
  `al_begin_arraystruct_action`: neither forwards to Core and caller buffers
  remain unchanged. They are registered as separate write and arraystruct
  scenarios, so CTest continues to identify the seam under test.

## Evidence and limits

The checked-in investigation remains the evidence for the live candidates:
the pinned equilibrium scope has 2,019 nodes, 3,584 events and 143 successor
edges; pulse_schedule has 1,369 nodes, 2,534 events and 308 successor edges
([algorithm research](../KG_RUST_MAP_ALGORITHM_RESEARCH.md)). It confirms that
equilibrium's `coordinates_type` is a type change and that pulse_schedule's
old-<3.40 to new->=3.40 heterogeneous-time branch requires resampling. The
latter is therefore an unsupported candidate, not a map identity default.

This workspace had no recorded local graph selection, graph home, password or
running task-owned Neo4j container while #222 was implemented, so no new live
query, real-Core run or installed-HLI run is claimed. The complete live
equilibrium query remains CI-gated; a future session should provision the
pinned graph through `scripts/dd-graph.sh`, then test only producer-classified
coordinate/timebase evidence. A raw relation omission, a raw history string
and a flattened path edge remain insufficient on their own.

## Verification

The focused checks were:

```console
cargo test runtime_map --lib
cargo test node_query_keeps_unversioned_coordinate_relationships_distinct_from_raw_history --lib
cmake -S . -B build-issue-222 -DCMAKE_BUILD_TYPE=Debug -DIMAS_MVDD_REAL_CORE_TESTS=OFF
cmake --build build-issue-222 --target graph_runtime_map_test -j2
ctest --test-dir build-issue-222 -R '^(write|arraystruct)-graph-runtime-map-timebase-resampling-refuses-without-forwarding$' --output-on-failure
```
