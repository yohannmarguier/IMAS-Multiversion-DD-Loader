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
