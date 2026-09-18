# Runtime map-acquisition tracer

Issue #211 adds a graph-free, Rust-only tracer at
`conversion::runtime_map`. `RuntimeMapAcquirer::acquire` accepts an IDS and
exact stored/HLI DD endpoints, fetches one complete IDS-scoped fact set through
its `GraphFactsSource`, and returns either the existing validated
`ConversionMap` or an `AcquisitionFailure`. The private graph-stage C-ABI test
instance calls it from occurrence opening; production embedded-artifact
selection and the installed public C ABI remain unchanged.

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

The tracer is intentionally not a graph transport, historical reconstruction,
deadline/single-flight implementation, runtime source switch, or C ABI
adapter. Those additions must retain this complete-map-or-explicit-failure
boundary and consume a shared attempt deadline rather than resetting it.

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
targets outside the IDS-node inventory (including `IMASCoordinateSpec`) are
local non-corroborating facts, not scope failures. It deliberately leaves
them unversioned raw facts until an adapter can establish their historical
meaning.

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

The driver applies bounded TCP connection and pool acquisition settings, a
read-only server transaction timeout from the caller's supplied remaining
attempt time, and consumes each response fully before accepting it. Those are
the cancellation/remaining-time facilities handed to #214; per-query limits
do **not** constitute its future whole-attempt deadline.

The graph-required CI scenario provisions the pinned service and runs
`pinned_graph_returns_a_complete_equilibrium_scope`. It verifies the live
catalogue contains 3.39.0 and 4.1.1, that the stamp metadata path is present,
and that the unfiltered event and successor streams are non-empty. The local
unit suite supplies shuffled pages and schema-faithful malformed rows for the
same boundary. This machine did not have a running pinned graph, so the live
scenario is recorded as CI-required rather than claimed as locally executed.

Raw lifecycle and change records remain `Neo4jRawScope` at the transport
boundary. The controlled acquisition facts replay numeric-version additions,
removals and field-qualified metadata events into interval-local endpoints;
they never treat a node's latest property as historical evidence. A reused
spelling across an absence interval stays an explicit path-local refusal until
correspondence evidence proves its role. The direct raw-to-fact adapter remains
separate, and the three reference pairs have not been frozen into counts or
claimed to map here.

## Whole-attempt deadline (#214)

`RuntimeMapAcquirer::new` gives each `acquire` call one fresh five-second
monotonic deadline. An internal caller that needs a different bound constructs
the acquirer with `RuntimeMapAcquirer::with_deadline`; this tracer has no
environment-variable or C-ABI deadline setting because graph-backed runtime
source selection remains outside its scope. The duration covers source work,
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

The production staged and installed shim still selects only the embedded XML
artifact. CMake additionally builds the same crate into a private
`graph-stage/` test instance with Cargo's internal `graph-test-source` feature;
that instance selects a controlled complete graph-fact source through
`RuntimeMapCoordinator`. There is no installed source-selection option, new C
export, or second harness.

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

The retained XML mechanism scenarios keep their normal staged library and
unchanged expectations. The broader opening-family/probe/concurrency matrix is
#225, and production source cutover remains #233.

Verified in the recording-stub profile with `cmake -S . -B build-issue216
-DCMAKE_BUILD_TYPE=Debug -DIMAS_MVDD_REAL_CORE_TESTS=OFF`, `cmake --build
build-issue216 -j2`, and `ctest --test-dir build-issue216 --output-on-failure`
(215 passing tests). The same change passed `cargo test --all-targets`,
`cargo clippy --all-targets -- -D warnings`, and `cargo clippy --all-targets
--features graph-test-source -- -D warnings`; the one ignored live-graph unit
check still requires CI's pinned Neo4j service. This tracer deliberately uses
controlled graph facts rather than a live graph; its equilibrium scope now
covers identity operations, direct renames, unit classifications, opening
lifecycle, and the pinned graph's `profiles_1d/psi` COCOS evidence. Production
source cutover and remaining semantic mappings stay outside its scope.

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
expectation: every scenario executed by `graph_runtime_map_test` inherits the
`graph-runtime-map` label from that target, while the retained mechanism suites
continue to link the ordinary staged XML-selected shim and have no such label.
The graph-labelled matrix is therefore selectable without copying the shared
recording-stub harness or turning the installed shim into a runtime source
switch.

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
| Plugin twins, reentry, passthrough and lifecycle | plugin-arraystruct coexistence, `reentrant-read-*`, `passthrough-*`, opening-family, failure-cleanup and retry cases | XML passthrough and mechanism suites remain unchanged; no source selection reaches the installed ABI. |
| Evidence-specific candidates and transformations | coexistence fallback/order, direct renames, psi sign flips, unknown/compound/missing COCOS and unit/timebase refusals | Unproven aliases remain refusals; unresolved removal does not become graph absence. Declaration-only units are exact, while required or insufficient numerical evidence refuses. |

This makes the final-cutover assignments mechanical: keep the ordinary staged
target for XML fixtures, move only `graph-runtime-map` labelled scenarios when
the production source changes, and preserve any documented expectation
difference instead of silently changing an XML assertion.

At registration, the Debug recording-stub profile selected and passed 50/50
`graph-runtime-map` CTests. The pinned live graph was not started for that
local matrix run; `graph-abi` makes its setup and complete-scope acquisition a
CI prerequisite rather than silently treating a controlled source as a live
graph result.
