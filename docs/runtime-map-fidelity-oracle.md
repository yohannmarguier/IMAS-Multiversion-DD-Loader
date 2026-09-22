# Runtime-map fidelity oracle

Issue #257 connects the evidence source to the map a context actually uses. It
does not compare graph bytes with XML bytes: the sources are allowed to encode
different justified semantics under ADR 0027.

## Two scopes

The controlled oracle is exhaustive for its deliberately small complete scope.
For each ordered request it independently lists every path present at the HLI
endpoint and checks the selected relation, resolved path or refusal, fidelity,
candidate order and value transformation. The scope contains identity,
renamed, established-absent, retyped-unresolved and COCOS sign-flip cases. A
separate 3.42.0 to 4.1.1 request checks the coexistence candidate plan and its
precedence. Both 4.1.1 to 3.39.0 and 3.39.0 to 4.1.1 are specified; neither is
derived by reversing the other.

The pinned-Neo4j oracle is sampled integration evidence. The setup action
validates the running service against the immutable release, manifest, archive,
producer commit and Neo4j image recorded in `config/dd-graph-release.env`.
`pinned_graph_returns_complete_reference_scopes` then consumes every counted
page for six exact IDS/version/direction requests and records that identity and
those requests in `target/runtime-map-fidelity/live.json`. It trusts the loaded
graph facts and never revalidates them against source DD XML.

The private XML configuration remains separately labelled `xml-fidelity`.
Those tests prove parsed rename, candidate and sign-flip semantics reach a live
context and observable recording-stub calls. They are regression evidence for
the XML mechanism only, not an expectation that production graph results copy
an XML assumption unsupported by graph evidence.

## Context and cache chain

The oracle uses the same `RuntimeMapAcquirer`, `RuntimeMapCoordinator` and
`ConversionMap` as occurrence opening. The `live-graph` CTest selection closes
the final gap through discriminating Core calls: identity and rename operations
in both directions, COCOS payload transformation, coexistence candidate
selection, relative and absolute child reads, cached-datapath translation and
acquisition cleanup.

The retained cache contract is checked at its owner:

- `concurrent_same_key_requests_share_one_construction_and_one_result` covers
  simultaneous callers;
- `successful_maps_outlive_callers_and_do_not_contact_the_graph_again` covers
  warm reuse after caller/context references disappear;
- `fidelity_oracle_cache_keeps_ids_versions_and_direction_in_the_key` covers
  distinct IDS, endpoint and direction keys;
- the graph-runtime-map C scenarios cover child relative/absolute paths and
  reuse after root contexts close.

There is no refresh control: successful maps remain process-lifetime values and
failures remain retryable.

## Negative controls and reports

Freshly compiled tests independently alter candidate order, the COCOS factor,
endpoint hierarchy evidence and cache-key inputs. Each produces a named
mismatch or a changed operation assertion. Neo4j pagination has a separate
`refuses_a_missing_page_instead_of_returning_a_partial_scope` control, so a
truncated response cannot publish a map.

`IMAS_MVDD_FIDELITY_REPORT` asks the controlled test to write JSON containing
the source identity, exact ordered request, path, expected semantics, observed
semantics and mismatches. `tests/scripts/check-live-acquisition.sh` writes the
sampled live report to `target/runtime-map-fidelity/live.json`. CI retains both
JSON files as the `runtime-map-fidelity` artifact, including on failure when a
report was produced.

Local commands:

```console
IMAS_MVDD_FIDELITY_REPORT=target/runtime-map-fidelity/controlled.json \
  cargo test --release fidelity_ --lib
bash tests/scripts/check-live-acquisition.sh
ctest --test-dir build -L 'live-graph|xml-fidelity' --output-on-failure --no-tests=error
```
