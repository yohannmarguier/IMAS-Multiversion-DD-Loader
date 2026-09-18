# Runtime-map ambiguous arraystruct contexts — #224

Graph-derived coexistence now emits a subtree candidate plan only when the
dated predecessor and successor are both evidenced structures. Leaf
coexistence remains an exact plan. An exact child-specific rule still wins
over this subtree selector, so the structure relationship does not erase a
complete graph scope's more-specific outcome.

The existing `al_begin_arraystruct_action` policy is unchanged. In `READ_OP`,
it tries candidates in precedence order, closes empty rejected contexts, keeps
the first populated candidate, and keeps the final candidate when all are
empty. In non-read modes it opens the primary once without probing. The child
record retains the stored candidate that actually opened. Relative descendants
therefore filter siblings outside that fixed anchor: reads and deletes can use
the reachable candidate, while a relative write that no longer has the
precedence-one source refuses and records its existing root-shared loss.
Absolute arguments still resolve from the IDS root. Field and timebase retain
their independent resolution.

The controlled graph source adds a separate structure-coexistence scope so
the existing leaf coexistence/delete contract remains its own regression. The
shared C ABI harness covers first-populated selection, a wholly empty but
successful subtree, write-mode primary-only selection, fixed-anchor relative
read/write/delete behavior, absolute reading, and nested/root loss lookup.
The construction test separately proves that an evidenced structure plan
resolves its descendants as successor-first candidates.

Focused checks run locally:

```console
cargo test --features graph-test-source acquisition_extends_an_evidenced_coexisting_structure_to_its_descendants -- --nocapture
cmake -S . -B build-issue-224 -DIMAS_MVDD_REAL_CORE_TESTS=OFF -DCMAKE_BUILD_TYPE=Debug
cmake --build build-issue-224 --target graph_runtime_map_test -j2
ctest --test-dir build-issue-224 --output-on-failure -R '^arraystruct-graph-runtime-map-coexistence'
```

The scenarios use controlled acquisition evidence; no live graph rule was
invented for coverage. The graph-backed real-Core and installed-HLI matrix,
including plugin-twin acceptance beyond its existing shared seam coverage,
remain handoff work for the combined integration matrix.
