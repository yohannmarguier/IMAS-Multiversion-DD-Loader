# Runtime map-acquisition tracer

Issue #211 adds a graph-free, Rust-only tracer at
`conversion::runtime_map`. `RuntimeMapAcquirer::acquire` accepts an IDS and
exact stored/HLI DD endpoints, fetches one complete IDS-scoped fact set through
its `GraphFactsSource`, and returns either the existing validated
`ConversionMap` or an `AcquisitionFailure`. Occurrence opening in the normal
staged and installed ABI calls it for every uncached mismatch. The private
graph-stage C-ABI instance remains controlled mechanism coverage; the private
XML fixture build keeps regression assertions that need the old artifact.

The controlled contract mirrors the selected graph streams: released versions
with optional COCOS conventions; IDS node rows with exact endpoint metadata
(including structures and metadata paths); versioned events; and directed
successors. It validates all row references before handling evidence. The
tracer emits one exact explicit identity rule for every endpoint path whose
metadata has no COCOS label or expression, uses the existing `Retyped` refusal
for a representation difference, and makes an unsupported COCOS-labelled or
expression-bearing path an explicit unmappable refusal. A
provenance-qualified `psi_like` or `dodpsi_like` leaf with exact endpoint COCOS
conventions 11 and 17 instead uses the existing one-step sign flip; raw label
history is not mistaken for a second transform. A caller
path outside the acquired endpoint scope is left unresolved by the existing
resolver rather than being claimed through a document-level identity default.

Unknown semantic event or successor evidence fails explicitly. Raw COCOS-label
and documentation events are corroborating history, not independently executed
transforms. A node missing one requested endpoint returns `UnresolvedEndpoint`;
it does not become an absent counterpart. A source failure remains `Source`,
distinct from construction, scope and evidence failures.

The focused verification is:

```console
cargo test runtime_map --lib
cargo test conversion_map --lib
cargo clippy --all-targets -- -D warnings
```

The original #211 tracer established the complete-map-or-explicit-failure
boundary. The transport, replay, deadline, coordinator and private C ABI
integration described below now consume that same interface.

## Coordinate and timebase evidence (#222)

Coordinate relationships are now projected from `HAS_COORDINATE` separately
from the raw list strings in `coordinates` history events. The relationships
are unversioned and may be omitted or strip index notation, so their absence
or equality as empty lists cannot certify coordinate equivalence. A map marks
one endpoint exact only when nonempty raw coordinate declarations and their
timebase are positionally established as the same paths or as directly
evidenced correspondences, and either each raw dimension is corroborated by a
preserved relationship or an applicable producer verdict classifies it
`Equivalent`. Unequal declarations alone are unresolved, not an inference
that resampling is required. Transport validates relationship dimensions;
targets outside the IDS-node inventory remain local unresolved dependencies.
The live decoder separately identifies producer-labelled `IMASCoordinateSpec`
targets. Complete stable specs can establish representation; path targets
require membership at both endpoints. See the #212 source contract for the
addition-age relationships and event-ledger interpretation.

A producer-established `RequiresResampling` verdict stays localized as the
existing `Unmappable` refusal, while an `UnboundedScope` verdict fails the
whole acquisition. The graph-selected C ABI scenario exercises an unsafe
timebase independently from a safe field in a write and an arraystruct open;
both preserve existing refusal ordering and prevent the Core call. Their
seam-specific CTest names preserve the test-suite grouping convention. See
`docs/history/runtime-map-coordinate-timebase-222.md` for the focused checks,
candidate live evidence and the explicitly unrun real-Core/HLI cases.

## Neo4j acquisition boundary (#212)

`conversion::runtime_map::neo4j_graph` selects the Rust `neo4j` 0.2 Bolt
driver for the pinned local service. `Neo4jScopeSource` fetches the exact
release catalogue and every IDS-scoped node, lifecycle, metadata,
`IMASNodeChange`, and `RENAMED_TO` row using bound `ids`, `skip`, and `limit`
parameters. Every stream is retrieved as count plus complete pages; it rejects
a short/oversized page, duplicate release/node IDs, foreign IDS rows, and
references outside the returned node/release scope. `GraphValue::Null` remains
distinct from missing and empty values, including the snapshot's optional
COCOS convention.

The selected release schema is the pinned DD-only v5.3.0 graph described in
`KG_CONVERSION_QUERY_RESEARCH.md`: `DDVersion.id`/`cocos`, `IMASNode` plus
its `INTRODUCED_IN` and `DEPRECATED_IN` lifecycle links, `IMASNodeChange`
with `FOR_IMAS_PATH` and `IN_VERSION`, the measured
`cocos_label_transformation`, `cocos_transformation_expression`, and
`cocos_label_source` properties, NBC fields, and `RENAMED_TO` edges. The
optional `renamed_to` node annotation is projected too. The source does not
substitute newer producer property names for this release contract.

`Neo4jFactsSource` now connects inside the caller's acquisition attempt and
passes `Neo4jRawScope` through the schema decoder into `IdsGraphFacts`.
Historical reconstruction stays in `RuntimeMapAcquirer`; source properties
are distinct from observed endpoint metadata. The driver, decoder, replay,
constructor, coordinator and publication consume one remaining deadline.
Blocked driver workers cannot publish maps after the caller has timed out.

`bash tests/scripts/check-live-acquisition.sh` exercises complete validated
maps for all three reference pairs in both directions, then resolves supported
paths and evidence-dependent refusals. It rejects a zero-test filter. The
pinned archive was acquired and these queries were executed locally during
#212's repair; the earlier raw-only smoke check is no longer the completion
claim. [KG_LIVE_SOURCE_CONTRACT.md](KG_LIVE_SOURCE_CONTRACT.md) records source
property ages, nulls, event provenance, configuration and the verification
matrix. Controlled facts below remain separate mechanism coverage.

## Whole-attempt deadline (#214)

`RuntimeMapAcquirer::new` gives each `acquire` call one fresh five-second
monotonic deadline. An internal caller that needs a different bound constructs
the acquirer with `RuntimeMapAcquirer::with_deadline`; the production occurrence
adapter reads `IMAS_MVDD_GRAPH_DEADLINE_SECONDS` for that bound, while the C ABI
adds no configuration export. The duration covers source work,
scope validation, rule construction, `ConversionMap` validation and the final
publication check. Any expiry becomes `AcquisitionFailure::TimedOut`, which is
distinct from source, incomplete-scope, malformed-evidence and construction
failures. A map that finishes while the deadline expires is rejected at the
publication check rather than returned.

`GraphFactsSource::load_ids_facts` receives the one `AcquisitionAttempt` and
must propagate it to later source stages; it must never make a replacement
attempt or reset the timer. `BoltExecutor::connect` applies the lesser of the
configured `Neo4jConfig::connection_timeout` and that attempt's remainder to
the driver connection and pool acquisition settings. The existing URI,
username and password fields supply normal Neo4j connection/authentication
configuration; credentials stay in the caller's configuration (for the live
check, its `NEO4J_*` environment variables) and are neither logged nor added
to the C ABI. `Neo4jScopeSource::load_raw_scope` then passes the same remaining
time to every read-only server transaction. A blocked synchronous driver call
is run in a worker whose result is awaited only for that remainder. The caller
therefore fails on deadline even if a network/driver call has not returned;
the same transaction timeout asks Neo4j to abort the server-side work, and a
late worker result has no receiver and cannot be decoded, constructed or
published. The worker is deliberately not retained as an in-flight map or a
joinable request; #215 owns that process-life concurrency policy.

The controlled unit tests advance a manual monotonic clock at source,
validation, construction and publication boundaries, and simulate a blocked
transport. They do not sleep or depend on wall-clock timing. The ignored
live-graph acquisition check remains the service integration proof.

## Shared process-life maps (#215)

`RuntimeMapCoordinator::acquire` is the internal complete-map acquisition
entry point for the next occurrence adapter. It keys work and retained maps by
IDS name, stored DD version and HLI DD version. A cache hit returns the
process-life `Arc<ConversionMap>` without contacting the graph, including once
all former callers have dropped their references. Only successful maps enter
that cache.

On a cache miss, one caller owns an `AcquisitionAttempt`; concurrent requests
for that exact key wait for the same terminal result and its original deadline.
The coordinator holds its mutex only to inspect or replace entries: graph I/O,
map construction and waiting happen outside it, so distinct keys can progress
independently. A source, construction or timeout failure wakes every joiner,
is removed rather than cached, and a later request starts a fresh attempt. If
an expired attempt returns late, pointer-identity publication fencing prevents
it from replacing the newer retained result.

## Graph-selected C-ABI tracer (#216)

At the #216 milestone, CMake additionally built the same crate into a private
`graph-stage/` test instance with Cargo's internal `graph-test-source` feature;
that instance selects a controlled complete graph-fact source through
`RuntimeMapCoordinator`. #233 made that coordinator the normal staged and
installed source for every uncached mismatch. There is no installed
source-selection option, new C export, or second harness; XML is now private
fixture coverage only.

`graph_runtime_map_test` links that private library and the existing recording
stub. Its two identity scenarios open 4.1.1 → 3.39.0 and 3.39.0 → 4.1.1
occurrences, then read, write and leaf-delete `time` through the normal C ABI,
asserting the exact Core payloads and an empty loss log. Its unavailable-IDS
scenario proves that a failed uncached acquisition after Core opened a context
returns the standard refusal naming the IDS and both versions, ends that exact
Core context, and leaves no context loss record. The occurrence adapter
acquires a ready map before `record_root`; on failure it forgets the cached
mismatch before asking the matched call family to clean up the just-opened
context.

The retained XML mechanism scenarios link their private fixture library and
keep their expectations. The broader opening-family/probe/concurrency matrix
is #225.

Verified in the recording-stub profile with `cmake -S . -B build-issue216
-DCMAKE_BUILD_TYPE=Debug -DIMAS_MVDD_REAL_CORE_TESTS=OFF`, `cmake --build
build-issue216 -j2`, and `ctest --test-dir build-issue216 --output-on-failure`
(215 passing tests). The same change passed `cargo test --all-targets`,
`cargo clippy --all-targets -- -D warnings`, and `cargo clippy --all-targets
--features graph-test-source -- -D warnings`; the one ignored live-graph unit
check still requires CI's pinned Neo4j service. That first tracer deliberately
used controlled graph facts rather than a live graph; its equilibrium scope now
covers identity operations, direct renames, unit classifications, opening
lifecycle, and the pinned graph's `profiles_1d/psi` COCOS evidence. Remaining
semantic mappings stayed outside that milestone's scope.

## Evidenced direct rename tracer (#217)

`GraphRename` carries a dated NBC previous-name declaration on the newer
node. `RuntimeMapAcquirer` now emits an exact `Renamed` rule only when that
declaration falls inside the requested chronological interval, its local or
IDS-qualified spelling normalizes to an exact older endpoint member, and the
flattened `RENAMED_TO` stream corroborates the same direct pair. Both endpoint
metadata representations must match without COCOS evidence before the rule is
servable. The existing resolver remains the only rule executor.

The graph-stage source uses `beta_normal` → `beta_tor_norm` as its controlled
tracer. The C ABI scenarios prove reads, writes and leaf deletes reach the
actual stored spelling in both 4.1.1 → 3.39.0 and 3.39.0 → 4.1.1 directions.
The `j_tor` → `j_phi` declaration is deliberately present but COCOS-labelled:
it is traced as an `UNMAPPABLE` read refusal naming the caller path, without a
Core read. An uncorroborated declaration, conflicting/missing endpoint evidence
or a representation/value mismatch likewise remains localized to refusal;
neither an alias nor a missing target becomes identity or a write target.

The focused Rust test also reverses release, node and successor input order and
asserts identical resolution. XML-backed mechanism scenarios remain on their
unchanged source path. Raw Neo4j transport still ends at `Neo4jRawScope`; this
tracer consumes the same complete `GraphFactsSource` boundary used by the
controlled graph-stage source, rather than adding a second map interpreter or
a graph-specific C ABI.
## Opening families and lifecycle around acquisition (#225)

The graph-stage C ABI tracer now covers the opening adapter around the same
controlled complete-map interface. One source scope can deliberately become
unavailable after its first successful load, proving that global, slice,
timerange, plugin-global and plugin-slice openings retain and reuse the map
after their earlier roots close. Reopening the same occurrence also exercises
the cached-mismatch global `datapath` route, using #217's already-evidenced
rename rule. The #225 availability cases themselves remain identity-only, so
they verify adapter routing and retention without adding a scientific
conversion rule.

The tracer separately preserves matching and absent stamps, malformed-stamp
cleanup, conversion-disabled forwarding, a Core slice-open failure, and the
non-read `READ_OP` stamp probe. It also proves that a reentrant Core read and
the untranslated plugin-binding seam retain their existing bypass contracts
while a graph-backed root is live. An unavailable IDS refuses each linkable
opening family after it opened Core, through that family's matching end seam,
without a loss record. A transient controlled failure succeeds only on a later
opening, proving that a terminal failed attempt is not retained as either a
map or an occurrence-cache mismatch.

The coordinator's focused Rust tests remain the synchronization evidence: they
cover same-key joining, registry access while a leader and joiner wait for a
map, independent different-key progress, shared deadlines, retention after
graph shutdown, shared failures and later retry, and fencing of late expiry.
The public C ABI scenarios supply the opening-family, cleanup and lifecycle
evidence; no test controls were added to the shipped ABI.

Verified in the recording-stub profile with `cargo fmt --check`, `cargo test
runtime_map --lib` (34 passed; one pinned-live-graph check ignored), `cargo
clippy --all-targets --features graph-test-source -- -D warnings`, then
`cmake -S . -B build-issue225 -DCMAKE_BUILD_TYPE=Debug
-DIMAS_MVDD_REAL_CORE_TESTS=OFF`, `cmake --build build-issue225 -j2`, and
`ctest --test-dir build-issue225 --output-on-failure` (234 passed). The live
Neo4j, real-Core and HLI completion obligations remain outside this controlled
tracer ticket.

## Moved descendants, exceptions and per-field fidelity (#218)

The runtime adapter now normalizes every non-absolute NBC previous name from
the declaring node's parent, segment by segment. `..` may walk only within the
IDS root; an escaping declaration never forms a correspondence. An evidenced
structure relation whose parent changes becomes an exact `Moved` rule, while
an ordinary direct relation remains an exact `Renamed` rule. A move never
creates a wildcard suffix mapping: every descendant needs its own
endpoint-backed correspondence. That preserves an independently evidenced
child, an unresolved one-sided child, and a child that escapes the parent's
target subtree.

The controlled source supplies both a synthetic `moved_descendants` IDS and a
graph-derived declaration of `boundary_separatrix/gap` → `boundary/gap`.
Nested C ABI scenarios prove relative arraystruct opening, relative reads,
independent absolute field/timebase writes, and deletes in 4.1.1 → 3.39.0;
the reverse direction proves the existing escaping-subtree delete refusal.
The graph `gap/r` read retains the XML fixture's stored path, payload and
success status but has exact fidelity and no loss entry, while the XML suite
continues to assert its inherited `LOSSY` loss separately. `gap/identifier`
and an IDS-root-escaping declaration remain `UNMAPPABLE` rather than becoming
absent or inheriting parent support.

Verified with `cargo fmt --check`, `cargo test --features graph-test-source
runtime_map --lib` (40 passed, one pinned-live-graph test ignored), `cargo
clippy --all-targets --features graph-test-source -- -D warnings`, and the
recording-stub CMake profile's seam-named moved/fidelity C ABI scenarios. This does
not add multi-release reconstruction, candidate anchors or a scientific
transformation; those remain the bounded work of #219 and #224.

## Dated historical roles (#219)

The map acquirer now projects a rename witness's dated NBC declarations back
to each requested endpoint. It selects the first declaration after that
endpoint in numeric release order and applies the deepest child declaration
before an ancestor substitution at the same stage. The projected spellings
must both exist at their exact endpoints with the same representation, and
each non-witness spelling needs a `RENAMED_TO` edge to corroborate its role.
The later witness is never emitted as an endpoint rule merely because it
connects the history.

The controlled `pulse_schedule` scope records antenna → launcher → beam and
the `launching_angle_pol` → `steering_angle_pol` exception. Its 3.25.0 ↔
3.30.0 maps therefore relate antenna/launcher and their child spellings while
leaving beam unclaimed at both endpoints. Conflicting same-date declarations,
successor cycles, reused spellings, self-referential history, and missing
endpoint anchors stay localized as `UNMAPPABLE`; reordering releases, nodes,
successor rows, or aligned declarations changes no result. The graph-stage C
ABI tracer exercises the same maps through nested arraystruct opening plus
child reads, writes and deletes in both directions.

The focused checks are `cargo test
acquisition_relates_dated_historical_endpoints_without_promoting_witnesses
--lib`, `cargo test acquisition_localizes_unreliable_historical_roles --lib`,
and the two `graph-runtime-map-historical-nested-operations` CTests in the
recording-stub profile. The live Neo4j source still stops at its raw-scope
boundary, and real-Core persistence plus the remaining pulse-schedule
scientific classifiers/oracle remain for #229.

## Graph/fixture matrix registration (#226)

The CTest source assignment is explicit rather than inferred from an XML
expectation: controlled graph-fixture scenarios executed by
`graph_runtime_map_test` inherit the `graph-runtime-map` label, while the
retained mechanism suites link a private XML fixture stage. The `live-graph`
matrix links the ordinary staged production shim, so no test source selection
becomes an installed runtime option.

The `graph-abi` CI job provisions the committed graph snapshot, proves the
live complete equilibrium scope at the Rust boundary, then builds the
recording-stub profile and runs `ctest -L graph-runtime-map --no-tests=error`.
CTest retains the executed scenario count in the job log; an empty label
selection, failed provisioning, or failed acquisition is a failed job. The
normal `fast` job remains graph-service-independent and continues to run the
XML fixtures, explicit NoSource scalar/array ABI cases, formatting, and
isolated Rust tests.

The selected graph scenarios are the requirement ledger, not a second map
oracle:

| Requirement | Graph-selected shared-harness scenarios | XML fixture assignment or documented difference |
| --- | --- | --- |
| Read, primary-only write, delete fan-out and losses | `coexistence-*`, `renamed-*`, `psi-*`, and `unit-refusal-*` | XML retains its artifact candidate and NoSource scalar/array behavior. Graph excludes unsupported scientific paths and records graph candidate paths in write/delete losses. |
| Nested stored anchors and historical paths | `moved-parent-*`, `historical-nested-operations-*`, and arraystruct coexistence cases | XML retains its `move-gap` inherited loss; graph `gap/r` is exact and deliberately has no loss. |
| Plugin twins, reentry, passthrough and lifecycle | plugin-arraystruct coexistence, `reentrant-read-*`, `passthrough-*`, opening-family, failure-cleanup and retry cases | XML passthrough and mechanism suites remain private fixtures; the installed ABI has no source-selection knob. |
| Evidence-specific candidates and transformations | coexistence fallback/order, direct renames, psi sign flips, unknown/compound/missing COCOS and unit/timebase refusals | Unproven aliases remain refusals; unresolved removal does not become graph absence. Declaration-only units are exact, while required or insufficient numerical evidence refuses. |

The completed cutover keeps XML scenarios on their private fixture target,
uses the ordinary staged target for all live graph scenarios, and preserves
documented expectation differences instead of silently changing an XML
assertion.

At registration, the Debug recording-stub profile selected and passed 50/50
`graph-runtime-map` CTests. The pinned live graph was not started for that
local matrix run; `graph-abi` makes its setup and complete-scope acquisition a
CI prerequisite rather than silently treating a controlled source as a live
graph result.

## Installed Fortran graph scenario (#230)

The private `graph-test-source` package can now be assembled under a build
tree with `cmake --build <shim-build> --target
imas_mvdd_graph_test_package`. It uses the same Cargo-c package shape as the
normal install — library, generated header, pkg-config metadata and CMake
package configuration — but never changes `cmake --install` or production
XML selection. Its build-tree prefix is `<shim-build>/graph-package`.

The pinned IMAS-Fortran revision `cd6ea111948bbff7b07b36b39992c56b565dea9b`
on `feat/runtime-conversion-mapping-issue-230` adds the opt-in
`AL_SHIM_GRAPH_RUNTIME_SCENARIO`. With that switch on and
`CMAKE_PREFIX_PATH=<shim-build>/graph-package`, its
`al-fortran-test-shim-graph-runtime` test drives generated `ids_put_slice`
and `ids_get` calls against a private copy of the DD-3.39.0 equilibrium
fixture. It writes `time_slice/profiles_1d/psi` with `[-2.5, 7.25]` through a
DD-4.1.1 HLI, then reads the same values back after the graph-derived COCOS
write inverse and read flip. The scope also exposes `coordinates_type` as a
retype: the HLI reports `PARTIAL_READ` and retains the relative leaf spelling
plus the `container changed shape` reason in its skip log. This is the HLI's
honest refusal surface; it does not claim the shim C ABI's joined loss path.

Executed on macOS with the isolated package, the exact
`IMAS_CORE_REF` fork `dae4abdd9428bd28f47063f8f575bdc8abd915f2`, HDF5, and
DD 4.1.1:

```console
cmake -S . -B build-shim -DCMAKE_BUILD_TYPE=Release \
  -DIMAS_MVDD_REAL_CORE_TESTS=OFF
cmake --build build-shim --target imas_mvdd_graph_test_package -j2

cmake -S hli -B hli/build -DCMAKE_BUILD_TYPE=Debug \
  -DAL_USE_MULTIVERSION_SHIM=ON -DAL_SHIM_GRAPH_RUNTIME_SCENARIO=ON \
  -DCMAKE_PREFIX_PATH="$PWD/build-shim/graph-package" -DDD_VERSION=4.1.1 \
  -DAL_BACKEND_HDF5=ON -DAL_BACKEND_MDSPLUS=OFF -DAL_BACKEND_UDA=OFF \
  -DAL_TESTS=ON -DAL_EXAMPLES=OFF -DAL_PLAYGROUND=OFF -DAL_PLUGINS=OFF \
  -DAL_HLI_DOCS=OFF \
  -DAL_CORE_GIT_REPOSITORY=https://github.com/yohannmarguier/IMAS-Core.git \
  -DAL_CORE_VERSION=dae4abdd9428bd28f47063f8f575bdc8abd915f2
cmake --build hli/build --target al-fortran-test-shim-graph-runtime -j2
cmake --build hli/build --target al-core-runtime -j2
ctest --test-dir hli/build -R '^al-fortran-test-shim-graph-runtime$' \
  --output-on-failure --no-tests=error
```

The selected CTest ran its fixture copy, loss-log cleanup and the nonzero HLI
scenario: all 3 passed. This is an installed graph-selected test instance with
controlled complete facts; it is not a production source switch or a claim
that the scenario itself queried live Neo4j.

## Installed Fortran graph validation (#231)

The Fortran HLI CI job now starts the committed DD-only graph through the
digest-keyed `setup-dd-graph` action before it builds either HLI configuration.
That action restores or acquires only the immutable archive, then always loads
a fresh task-owned database and query-smoke-checks it; a cache hit cannot skip
startup, and a setup failure fails the job.

The existing HLI configuration consumes the ordinary installed graph-backed
package and runs its full asserted suite. A second `build-graph` configuration
enables only `AL_SHIM_GRAPH_RUNTIME_SCENARIO` and finds that same installed
package.
It builds the same pinned Core fork, verifies that checkout's commit, rejects
an empty `al-fortran-test-shim-graph-runtime` selection, then executes that
generated-HLI conversion scenario with the normal HDF5 backend. Its job summary
records the Fortran and Core pins, selected graph release and manifest digest,
and selected scenario count without reporting credentials.

The generated Fortran scenario acquires its map from live Neo4j; the same
ordinary production artifact is selected in `graph-abi` and the installed-Core
CI profile. Functional validation explicitly selects a 120-second deadline;
the default remains five seconds. XML fixture tests remain graph-service-
independent. One selected
snapshot applies for an HLI process; map acquisition has its shared configurable
five-second whole-attempt deadline and successful maps are reused for that
process lifetime. Operators start or update the selected snapshot explicitly
between HLI processes as documented in README.md; no graph archive, database or
credential belongs in Git.

## Live repair verification (#212)

The earlier #216–#231 entries describe the controlled-source milestones at
the time they landed. Their mechanism tests remain. The current live variant
also runs genuine Core operations on all three reference pairs and the pinned
installed Fortran scenario. The source contract and executed-command matrix
are in [KG_LIVE_SOURCE_CONTRACT.md](KG_LIVE_SOURCE_CONTRACT.md); neither an
available Neo4j service nor a controlled test alone counts as live evidence.
