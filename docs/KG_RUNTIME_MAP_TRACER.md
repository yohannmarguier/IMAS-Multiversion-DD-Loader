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

Raw lifecycle and change records are deliberately retained as
`Neo4jRawScope`, not converted into endpoint metadata or a map by this ticket:
using current node properties as historical endpoint facts would fabricate a
supported conversion. #213 owns that interpretation and then supplies the
existing `GraphFactsSource`/`RuntimeMapAcquirer` map interface. Consequently,
the three reference pairs have not been frozen into counts or claimed to map
here; their correspondence/history limitations remain explicit input to #213.
