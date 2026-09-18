# Runtime map-acquisition tracer

Issue #211 adds a graph-free, Rust-only tracer at
`conversion::runtime_map`. `RuntimeMapAcquirer::acquire` accepts an IDS and
exact stored/HLI DD endpoints, fetches one complete IDS-scoped fact set through
its `GraphFactsSource`, and returns either the existing validated
`ConversionMap` or an `AcquisitionFailure`. Nothing calls this interface from
the occurrence seams yet: embedded-artifact selection and the public C ABI are
unchanged.

The controlled contract mirrors the selected graph streams: released versions
with optional COCOS conventions; IDS node rows with exact endpoint metadata
(including structures and metadata paths); versioned events; and directed
successors. It validates all row references before handling evidence. The
first tracer deliberately accepts only an event- and successor-free identity
scope. It emits one exact explicit identity rule for every endpoint path whose
metadata has no COCOS label or expression, uses the existing `Retyped` refusal
for a representation difference, and makes a COCOS-labelled or
expression-bearing path an explicit unmappable refusal until a supported factor
is proven. A caller
path outside the acquired endpoint scope is left unresolved by the existing
resolver rather than being claimed through a document-level identity default.

Unprocessed event or successor evidence fails explicitly. A node missing one
requested endpoint returns `UnresolvedEndpoint`; it does not become an absent
counterpart. A source failure remains `Source`, distinct from construction,
scope and evidence failures.

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
check still requires CI's pinned Neo4j service. This tracer deliberately
serves only its controlled equilibrium identity scope, not a live graph or the
unimplemented semantic mappings.

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
