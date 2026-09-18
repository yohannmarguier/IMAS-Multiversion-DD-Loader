# Runtime-map COCOS factors — issue #220

Issue #220 attaches the first graph-derived value transformation to the
existing `ConversionMap` executor. It adds no resolver, numerical-expression
engine, scale converter, or operation-policy variant.

## Delivered behavior

`EndpointMetadata` records typed COCOS-label provenance. A leaf path obtains
the existing sign flip exactly once when both endpoints carry the same
supported `psi_like` or `dodpsi_like` class, each class has `xml` or
`inferred_sign_flip` provenance, neither endpoint carries an expression, and
the exact endpoint conventions are 11 and 17 in either order. Equal known
conventions establish the factor `+1`, so no value transformation is attached.
The builder emits at most one `TypedSignFlip` for a resolved stored path, which
deduplicates corroborating endpoint evidence before it reaches the executor.
Only the selected HLI/stored endpoints and events within that endpoint interval
participate; an unrelated release cannot make an independently exact pair refuse.
Raw `cocos_label_transformation` history is tracked as raw XML evidence but
does not overwrite a separately provenance-qualified backfill; documentation
and raw-label events therefore cannot create a second sign flip. A raw
replacement of one non-empty label with another is incompatible evidence and
refuses that path rather than being merged into the inferred factor.

Unknown labels or provenance, compound expressions, a structure-level label,
different endpoint classes, missing conventions, and convention pairs outside
the supported factor all become localized `Unmappable` rules. They do not make
independently established paths unavailable. COCOS factors never use the
historical version-based 11/17 fallback.

The existing read/write engine supplies the transformation: reads flip once,
writes use its inverse on shim-owned storage, and `EMPTY_DOUBLE` stays unchanged
both in a scalar and inside an array. Refusals leave caller-owned read/write
buffers alone and retain the normal C ABI loss entries.

The controlled graph C-ABI tracer now covers both pinned-graph psi directions,
array and scalar `EMPTY_DOUBLE`, the `DOUBLE_DATA` gate, plus unknown-label,
compound-expression, and missing-convention refusals. The ignored pinned-Neo4j
CI check verifies the same `equilibrium/time_slice/profiles_1d/psi` evidence:
`psi_like`/`inferred_sign_flip`, no expression, COCOS 11 at 3.39.0 and 17 at
4.1.1, and retained raw-label history. It also keeps the independent `field`
and `timebase` arguments on their existing map paths; the retained plugin/reentry
suites continue to cover their shared seam behavior.

## Limits retained deliberately

Only the two listed COCOS classes and factor signs are supported. Expressions,
other classes, scales, reshaping, resampling, and candidate-specific factors
remain outside the existing value engine and refuse rather than becoming
identity conversions. The tracer's controlled source is the existing C-ABI
acquisition seam; production source cutover remains outside this ticket.

## Checks run

- `cargo test --all-targets`
- `cargo test --all-targets --features graph-test-source`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cmake -S . -B build-issue-220 -DCMAKE_BUILD_TYPE=Debug -DIMAS_MVDD_REAL_CORE_TESTS=OFF`
- `cmake --build build-issue-220 -j2`
- `ctest --test-dir build-issue-220 --output-on-failure` (220/220 passed)

The real-IMAS-Core profile is not run here; the implementation’s new coverage
uses the existing recording-stub C ABI harness.
