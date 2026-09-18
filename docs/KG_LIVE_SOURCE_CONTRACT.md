# Live source boundary (issue #212)

The pinned producer is IMAS-Codex v5.3.0, commit
`ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e`; the archive and Neo4j image
identities remain in `config/dd-graph-release.env`. The contract below comes
from its `graph/build_dd.py` and queries of that exact archive. It describes
interpretation of the trusted KG, not an audit against DD XML.

`Neo4jFactsSource` connects inside `GraphFactsSource::load_ids_facts` using
the caller's `AcquisitionAttempt`. `Neo4jScopeSource` retrieves and validates
all streams, then `neo4j_facts` decodes them. `RuntimeMapAcquirer` alone
reconstructs endpoint state and builds the existing `ConversionMap`.
`RuntimeMapCoordinator` retains successful maps. No field operation queries
Neo4j. Ordinary production builds still select XML (#233).

| Raw projection | Fact and temporal meaning | Null/empty/error handling |
| --- | --- | --- |
| `DDVersion.id`, `toString(v.cocos)` | Numeric release order; exact release's convention. The stored COCOS property is integer; Cypher normalizes it to decimal text. | Null convention stays unknown; missing, empty or invalid release/convention fails retrieval. No 11/17 historical fallback. |
| `IMASNode.id`, `ids` | IDS-qualified graph identity becomes an IDS-relative map path; all nodes, including structures and metadata, are retrieved. | Empty/malformed identity, duplicate path, or escaped ownership fails. |
| `data_type`, `ndim`, `unit AS units` | Ordinary properties originate at addition or latest readdition. They are source metadata, never synthetic endpoint observations. Type determines leaf versus structure. | Required type/rank and event values must decode consistently; ordinary properties must match replay at the latest addition. Unit null and empty remain distinct in raw facts. |
| `timebasepath AS timebase` | Refreshed for latest-present nodes; applicable history supplies the earlier value. Removed nodes retain their last property. | Nullable, with field-qualified history taking precedence. Missing column fails. |
| `HAS_COORDINATE.dimension`, target ID | One-based unversioned relationships created at addition; converted to zero-based fact dimensions. Target labels distinguish paths from specs. Targets may be `IMASCoordinateSpec` (`1...N`) or node paths. | Empty edges alone do not certify array coordinate equivalence. Complete dimensions, presence interval, target endpoint membership and event history are required. Raw coordinate history remains distinct from stripped targets. |
| `INTRODUCED_IN`, `DEPRECATED_IN` | Lifecycle anchors supplement all addition/removal/rename events. Removal means absent at that release; obsolescence does not. | Lists, including empty lists, must be present. Null lists, duplicate versions or unknown references fail. |
| NBC description, aligned dates and previous names | Dated rename declarations on a witness, possibly outside both requested endpoints. Parent declarations apply to independently verified children; explicit child declarations retain priority. | Only rename descriptions form aliases. Misaligned dates/names fail; missing correspondence stays unresolved. Prior type is retained at the wire boundary; actual type changes use the event ledger. |
| COCOS label, expression, source | Producer class/provenance, distinct from raw XML label events. `inferred_forward` retains an earlier XML class; `inferred_sign_flip` records the producer's explicit table evidence. | Missing provenance or unsupported expression/class cannot certify a factor. No first-token evaluation of expressions. |
| Event ID, kind, path, release, owners, releases | Entire field-qualified event identity and exact ownership/cardinality. Includes history outside the requested interval. | Unknown required semantics, duplicates, conflicting rows, absent/multiple owners or releases fail. A scoped query does not certify absence of upstream orphan events. |
| Event old/new strings | Field-specific scalar values or Python string-list literals. Replay stays within an appearance interval. | Typed null remains missing evidence; empty coordinate text is the producer's absent declaration encoding. List parsing never evaluates code. |
| `semantic_type` | Producer classification of documentation changes; sign-convention events corroborate COCOS instead of multiplying its factor. | Non-scientific clarification is not a sign change. Unknown scientific behavior cannot establish identity. |
| `unit_change_subtype` | `cosmetic`, `sentinel_resolved`: declaration-only; `dim_equivalent`: dimensions only. | Incompatible or unknown classifications remain unresolved. The release cannot supply the controlled fixture's `RequiredScaleOrOffset` verdict; the adapter never fabricates it. |
| `RENAMED_TO` endpoints | Unversioned, flattened correspondence corroboration, not an adjacent release transition or endpoint presence proof. | Empty stream is valid. Duplicate/dangling/cross-scope edges fail. Unsuccessful search is not NoSource. |

All count/page queries use bound parameters and the same remaining deadline.
Count agreement checks complete retrieval, not upstream factual completeness.
The selected graph must remain immutable throughout the process. Decoder,
replay, validation and publication consume the same attempt; an expired
attempt cannot become a retained map.

Controlled sources remain available for evidence the release does not carry,
including positive absence and explicit resampling/scale classifications.
They must not be described as live service evidence.

## Calling the live path

Provision the pinned archive with `scripts/dd-graph.sh setup` (see README),
then supply `NEO4J_URI`, `NEO4J_USERNAME`, `NEO4J_PASSWORD` and optionally
`NEO4J_DATABASE` at execution. Credentials are not CMake cache entries.

```sh
bash tests/scripts/check-live-acquisition.sh
cmake -S . -B build-live -DCMAKE_BUILD_TYPE=Release \
  -DIMAS_MVDD_GRAPH_TEST_SOURCE=live
cmake --build build-live
IMAS_MVDD_GRAPH_DEADLINE_SECONDS=120 ctest --test-dir build-live \
  --output-on-failure --no-tests=error
cmake --build build-live --target imas_mvdd_graph_test_package
```

The ordinary `stage` and installation still select XML; only `graph-stage`
and `graph-package` select the requested private feature. The default private
source is `controlled`. For the existing pinned Fortran command in the tracer,
point `CMAKE_PREFIX_PATH` at this live `graph-package`, and export the same
connection settings and explicit deadline when running CTest. A missing graph
fails acquisition; there is no XML fallback.

The Rust entry for #232 is
`RuntimeMapCoordinator::with_deadline(Neo4jFactsSource(Neo4jConfig { ... }), bound)`
then `acquire(&MapRequest { ids, stored_dd, hli_dd })`. The configuration owns
connection settings, page size and connection timeout. The outer bound covers
the entire attempt, including connection. Use `RuntimeMapCoordinator::new`
for the unchanged five-second default. The live functional test explicitly
uses 120 seconds to establish semantics; it is not a performance result.
#232 still owns measurement and reporting, and #233 owns production selection.

The destructive-service lifecycle test is ignored in ordinary test runs. Run
it only against an isolated task-owned container, with no concurrent live
checks; it stops and restarts that explicitly named service:

```sh
IMAS_MVDD_TEST_GRAPH_CONTAINER=<task-container> cargo test --release \
  pinned_live_map_survives_graph_shutdown --lib -- --ignored --nocapture
```

## Evidence-dependent outcomes

| Case | Live evidence and resulting behavior |
| --- | --- |
| `j_tor` / `j_phi` | Dated 3.42 rename, successor edges and independently present child endpoints. 3.39↔4.1 is a rename; 3.42↔4.1 has successor-first candidates only on the coexistence side. Unknown descendants receive no manufactured selector. |
| `b_field_tor` / `b_field_phi` | COCOS expression/provenance does not establish the engine's supported factor. Local refusal; controlled B-field operation cases remain separate. |
| boundary descendants | Stable `boundary/outline/r` with a coordinate spec is supported. `boundary/gap` lacks corroborating correspondence and refuses; failed search never asserts absence. |
| `grids_ggd/grid/space/coordinates_type` | Reconstructed representation change refuses; the installed Fortran partial-read scenario observes the refusal. |
| `profiles_1d/psi` | `psi_like` with `inferred_forward` provenance and endpoint conventions supplies one sign flip. Documentation corroborates it rather than multiplying it. |
| pulse_schedule antenna/launcher/beam | Dated ancestor and child declarations plus endpoint membership resolve antenna↔launcher, including name and angle `envelope_type` descendants. Beam remains an intermediate witness, never an endpoint. |
| coordinates/units | Complete in-scope path relationships require presence at both endpoints; producer-labelled specs retain their identity. Missing/foreign/dangling evidence refuses locally. `dim_equivalent` alone never certifies factor one. |
| scientific changes | Unsupported documentation, node type and identifier enum changes refuse. Unknown historical COCOS stays unknown. |

## Executed verification (2026-09-18)

All commands used the repair worktree `/private/tmp/imas-mvdd-issue-212-repair`.
The live service was `bolt://127.0.0.1:17688`, backed by the exact archive
manifest `dc90975cb9fa0c7b08e9e4809640d01e41d927b4162200c13eec5076e030329b`
and the Neo4j image pinned in `config/dd-graph-release.env`. Real Core was
installed from the pinned fork commit
`dae4abdd9428bd28f47063f8f575bdc8abd915f2`. Fortran was the unchanged pin
`cd6ea111948bbff7b07b36b39992c56b565dea9b` with DD 4.1.1.

| Gate | Path/source and executed command | Result / limitation |
| --- | --- | --- |
| R1 | `cargo test --release pinned_graph_returns_complete_reference_scopes --lib -- --ignored --nocapture` | One test acquired and validated all six directional maps; supported and refused path assertions passed. |
| R2–R3 | `cargo test runtime_map --lib --offline`; raw-response `neo4j_graph` tests feed the same acquirer/resolver | Malformed types/ranks, missing columns/pages, duplicate/conflicting records, ownership/cardinality, query failure, shuffled releases, addition anchors, coordinate membership and unsupported semantics covered. Controlled history tests retain removal/reappearance and exceptions. |
| R4 | Live CTest build `/private/tmp/imas-mvdd-212-real-build`; `ctest --output-on-failure --no-tests=error -j2` with connection settings and 120-second bound | Recording stub, genuine Core, nested j candidates, both equilibrium directions and pulse_schedule reads/writes/deletes. HDF5 independently checks stored effects, stamps and unrelated data. Successful absolute reads beneath a child remain blocked by pinned Core; direct-Core comparison below reproduces the limitation. |
| R5 | `cargo test --release pinned_live_map_survives_graph_shutdown --lib -- --ignored --nocapture`, naming `imas-mvdd-212-live` | Passed: caller references released, retained map reused offline, uncached failure, explicit later retry after restart. Deterministic coordinator tests cover same-key sharing, deadlines, blocked worker, no late publication and registry independence. |
| R6 | Installed `/private/tmp/imas-mvdd-212-live-build/graph-package`; `ctest --test-dir /private/tmp/imas-fortran-issue-230-debug -R '^al-fortran-test-shim-graph-runtime$' --output-on-failure --no-tests=error` | Fixture, loss-log cleanup and generated Fortran conversion passed (3 tests). Graph-required CI selects `live`; fast CI keeps controlled facts. Hosted CI itself has not run this unpushed branch. |
| R7 | Configuration and direct coordinator call above; nonzero script used in CI | Callable live path delivered. No benchmark claims or production cutover included. Native downstream blockers remain until the repair is landed/resolved. |
| R8 | `cargo fmt --check`, `cargo clippy --all-targets --all-features --offline -- -D warnings`; controlled and live CMake suites; package checks; two-axis code review | Final counts and package results recorded below. Review corrections include metadata consistency, coordinate provenance, enum refusal and exact refusal text. |

The graph is treated as trusted input; none of these checks compares its facts
with XML inventories. Existing XML artifact mechanism coverage is retained.

### Remaining R4 limitation: absolute reads beneath child contexts

The live coexistence oracle reads `reconstructed` beneath a `j_phi` stored
context and obtains the seeded value 47 in both directions. Calling the
same Core library directly through its `al_plugin_read_data` symbol also returns 47
for that relative spelling, but returns `EMPTY_DOUBLE` for the stored absolute
spelling `/time_slice/constraints/j_phi/reconstructed`. The converted absolute
call reproduces that EMPTY result. Thus these checks establish the limitation,
not successful absolute-path access. Recording-stub coverage separately proves
absolute-path translation.
The direct baseline uses the plugin twin to avoid Core's public wrapper
reentering the shim through an interposable symbol on ELF platforms.

In pinned Core `dae4abdd9428bd28f47063f8f575bdc8abd915f2`,
`src/hdf5/hdf5_reader.cpp`, `HDF5Reader::read_ND_Data`, replaces every slash
with `&` and unconditionally prepends the current array-structure context.
It does not reset to the occurrence root for a leading slash. Owner: IMAS-Core
HDF5 path handling; #212 retains this acceptance blocker until that behavior
is corrected or the required operation contract is explicitly resolved.
Changing child-context operation policy in the shim would exceed this repair's
scope. Keep #212 open and retain #232/#233's blockers; a green limitation
regression must not be represented as closing this gate.

Final suite results: controlled profile **309 checks**, live profile **285
checks**. Initial runs exposed three controlled fixture/CI-guard failures and one
live diagnostic-wrapping guard failure; each was corrected. Final full runs
passed all 309 controlled and 285 live checks, with the final direct-backend baseline also
rerun separately. The live suite includes **20 live recording-stub checks** and
**53 real-Core checks** (the latter includes retained XML regressions). The
controlled profile retains **50 controlled graph ABI checks** and **47
real-Core checks**. Installed consumers passed through pkg-config and CMake;
the DESTDIR check passed. Formatting and all-feature clippy passed.

The two-axis review reported one standards violation (missing exact refusal
text), one design heuristic (source properties reuse the metadata value
shape), and three spec defects (invalid replay values, coordinate target
membership, identifier enum semantics). The violation and defects were fixed
with acquisition/ABI regressions. The internal metadata value shape is retained
with explicitly distinct `source_metadata` storage and addition-age replay;
its `release` is never accepted as an observed endpoint. A follow-up review
found a dangling-coordinate fallback, now refused by a dedicated raw test.
