# KG facts for runtime conversion-map construction

Research on 2026-09-17. Scope: replace XML map population with Rust queries
and in-memory construction, preserving the existing interpreter and seam
policies. Manual artifact assumptions are outside this investigation.
The Rust interfaces remain proposed. The follow-up below records direct
Cypher verification against the released snapshot, separately from the
earlier source and MCP investigation.

For the completed experiment and subsequent accepted decisions, see the
[post-prototype decision review](KG_PROTOTYPE_DECISION_REVIEW.md). This note
retains the earlier source and query findings.

The completed [Python → KG → Rust algorithm investigation](KG_RUST_MAP_ALGORITHM_RESEARCH.md)
traces the Python helpers and tests, checks released producer source, derives
a conservative compiler and works the examples through to existing rule
fields. It corrects the newer-checkout reintroduction warning below: the
released producer retains repeated `path_added` events. It also identifies
discarded callbacks, historical metadata gaps and representation limits;
the query boundary alone is not a solved general conversion algorithm.

## Evidence and its limits

Local IMAS-Codex source was inspected at
`abdca7bd4ebdb2aeac6052383f0c13c691eaa7e4`; the shim at
`51e64b05a34e7fb1ebf495885eaae8085903d2b7`. Live observations below use the
available `imas_codex` MCP server. Its deployed code and graph digest were
not identified. Its migration output includes replacement annotations absent
from the local migration renderer, so local source and live deployment must
not be assumed identical.

The existing artifact demonstrates that an agent could derive rules from
MCP results. It does not establish that every rule is stored as a ready-made
KG relationship or that its endpoint inventories were exact. The intended
Rust work is deterministic reconstruction from the underlying facts.

## Where the facts live

IMAS-Codex module paths below are relative to
`/Users/yohann/Documents/Dev/ITER/IMAS-Codex/imas_codex/`; `scripts/` paths
are relative to that repository's root. `src/` paths refer to this shim.

| Fact | Source to query / implementation to study |
|---|---|
| Available releases, order and COCOS convention | `DDVersion.id`, `DDVersion.cocos`; `tools/migration_guide.py:43` (`_resolve_version_range`), `:525` (`_get_version_cocos`) |
| Paths and their metadata | `IMASNode` with `ids = $ids`; `schemas/imas_dd.yaml:690` onward and `graph/build_dd.py:3487` onward |
| Introduction and removal | `INTRODUCED_IN`, `DEPRECATED_IN`; removal-edge producer `graph/build_dd.py:3815` onward |
| Versioned changes | `(IMASNodeChange)-[:FOR_IMAS_PATH]->(IMASNode)` and `-[:IN_VERSION]->(DDVersion)`; producer `graph/build_dd.py:3835` onward |
| Successor edges | `RENAMED_TO`; producer `graph/build_dd.py:3934`, consumer `tools/migration_guide.py:131` onward |
| Additional successor annotation | String property `IMASNode.renamed_to`; producer `graph/dd_lifecycle.py:90` onward |
| DD-declared prior name/type | `change_nbc_version`, `change_nbc_description`, `change_nbc_previous_name`, `change_nbc_previous_type`; `schemas/imas_dd.yaml:727` onward |
| Path COCOS behavior | `cocos_transformation_type`, `cocos_transformation_expression`, `cocos_label_source`; query `tools/migration_guide.py:81` onward |
| COCOS factor calculation | `ids/transforms.py:40` (`cocos_sign`); graph COCOS parameter producer `graph/build_dd.py:3037` onward |

`DEPRECATED_IN` here is populated when a path disappears. It must not be
confused with an obsolescent field that remains legal and populated during
the compatibility period. `lifecycle_status` is current metadata and is also
rewritten by reconciliation; it is not an exact-version presence test.

There are three relevant sources of correspondence, not just rename edges:

1. `RENAMED_TO` edges are loaded from `path_mappings.json`. Its producer,
   `scripts/build_path_map.py:167`, calls
   `imas.ids_convert.dd_version_map_from_factories` against a target DD and
   retains the first mapping per old path. The edge has no version property.
   It may represent a jump to the build target rather than one adjacent step.
2. `graph/dd_lifecycle.py:40` (`dd_path_index`) reads DD NBC previous-name
   attributes, propagates ancestor renames to descendants, and builds full
   old/new paths. Reconciliation writes `n.renamed_to`, a string property,
   for removed nodes. An edge-only query does not retrieve this annotation.
3. NBC properties and `path_renamed` change events can supply declared
   correspondence and chronology. `graph/build_dd.py:1459` (`_detect_renames`)
   and `:1483` (`compute_version_changes`) show how events are produced.
   The detector only considers newly added paths paired with removed paths;
   that alone does not cover a successor introduced earlier during coexistence.

These sources need to be reconciled, with provenance retained, rather than
treating an absent edge as proof that a removed path has no counterpart.

## Live MCP observations

Requests used `get_dd_version_context` for the named paths, with
`from_version=3.39.0`, `to_version=4.1.1`, and rename-chain following;
and `get_dd_migration_guide` for that pair with `ids_filter=equilibrium`.

| Observation | Consequence for map population |
|---|---|
| `constraints/j_tor` introduced 3.39.0, obsolescent 3.42.0, removed 4.0.0; `constraints/j_phi` introduced 3.42.0 | Both names cannot be candidates on the 3.39.0 side. They can coexist on the 3.42.0 side. |
| Migration guide names `constraints/j_phi` as replacement for `constraints/j_tor`, including descendant paths | Replacement information is available through the MCP; absence from the per-path edge report does not prove missing correspondence. |
| `profiles_2d/b_field_tor` removed 4.0.0; `profiles_2d/b_field_phi` introduced 3.42.0 | Confirms ADR 0027's invalid-3.39.0-candidate example. |
| Guide names `beta_tor_norm` as replacement for `beta_normal`; version context introduces the former at 4.0.0 | Concrete rename evidence for the baseline. |
| `grids_ggd/grid/space/coordinates_type` changes `INT_1D` to `STRUCT_ARRAY` at 4.0.0 | Enough to identify a retype that the existing engine refuses; no new reshape implementation is needed. |
| Guide returns COCOS 11 → 17 and explicit sign-flip paths | Existing sign-flip representation can be populated from COCOS facts. Exact endpoint filtering remains necessary. |

The version tool also returned changes before the requested interval. This
matches the local per-path query's lack of a version predicate and its
`collect(...)[..20]` cap (`tools/version_tool.py:155`). It must not be ported
verbatim as the runtime query. The live version catalog reported 35 versions
from 3.22.0 through 4.1.1; this observation is not a universal coverage promise.

## Proposed Rust boundary

Names below are illustrative interfaces, not a commitment to file layout or
a particular driver. All queries are scoped to the requested IDS or the small
version catalog; complete results may be fetched in bounded pages.

| Function | Returns |
|---|---|
| `load_dd_versions()` | Released versions, ordering information and COCOS metadata; validate both requested endpoints |
| `load_ids_nodes(ids)` | Full path IDs, parent links, types/ranks, units, coordinates/timebase, lifecycle links, NBC fields, successor property, COCOS fields |
| `load_ids_changes(ids, versions)` | Untruncated change IDs, path IDs, version IDs, change types, old/new values and semantic classifications |
| `load_ids_successors(ids)` | Direct successor edges plus provenance; combine with node annotations and NBC facts in Rust |
| `compile_conversion_map(facts, left, right)` | Validated existing map structures, with rules, candidate order, fidelity and transformations |

The first four operations collect facts. The last is pure Rust work. It
does not contact Neo4j or apply read/write/delete policy. A single IDS-scoped
edge fetch followed by local traversal avoids per-path database round trips
and the reporting tool's arbitrary ten-hop lineage limit.

Representative query shape for history (not executed against Neo4j here):

```cypher
MATCH (p:IMASNode {ids: $ids})<-[:FOR_IMAS_PATH]-(c:IMASNodeChange)
MATCH (c)-[:IN_VERSION]->(v:DDVersion)
WHERE v.id IN $versions
RETURN p.id AS path, c.id AS change_id, v.id AS version,
       c.change_type AS change_type,
       c.old_value AS old_value, c.new_value AS new_value,
       c.semantic_type AS semantic_type,
       c.unit_change_subtype AS unit_change_subtype
ORDER BY p.id, c.id
```

Use numeric version ordering in Rust. Determine the chronological interval
independently of conversion direction. Requested-pair changes are in
`(earlier, later]`; reconstructing endpoint metadata from a current node may
also require history outside that interval. Current node properties must not
silently stand in for historical types or lifecycle states.

Nor are all direct node properties uniformly current: the builder first
writes nodes on addition, then refreshes selected properties in later passes
(`graph/build_dd.py:1906`, `:1995`). Endpoint reconstruction must follow
each property's actual contract. Preserve change IDs: `ndim` and identifier
enum changes both become `structure_changed`, while their IDs retain the
original field name. Event old/new values are strings, including Python-list
representations of coordinates; typed Rust decoding must account for that.

The node query must include structures, error fields and metadata paths.
The MCP search/list presentation filters are inappropriate for complete map
construction. Missing facts, absent endpoints and retrieval failures remain
distinct under ADR 0027.

## Constructing the existing map

For the supervisor's A → B compatibility transition, collect the replacement
relation and exact-version presence. A-only → B-only yields a one-to-one
rule; A-and-B → B-only yields a merged rule. Populate successor-first
candidate precedence and fidelity according to the existing artifact's
conventions. Do not require a KG property named `merged`, or reopen the
interpreter's operation policies. Longer correspondence histories must be
resolved before selecting candidates that exist at the requested endpoints.

Do not infer a correspondence merely because two paths share documentation
or because one was added when another was removed. Likewise, retain more
specific exceptions under renamed/moved subtrees; a broad subtree rule must
not make a removed child appear to exist in the other version.

The shim already has `Rule`, `FromEntry`, `Selector`, `Side`, directional
fidelity and transformation storage (`src/conversion/conversion_map.rs:403`,
`:414`, `:771`). However, `ConversionMap::load` is currently the XML-only
constructor and several fields are private. Implementation needs a validated
typed constructor shared with the XML parser, including source-index
construction and ambiguity checks. Generating XML text merely to parse it
again is unnecessary. This constructor refactoring preserves the interpreter.

For COCOS, port the small supported factor calculation from
`ids/transforms.py:40`, using KG conventions and path labels. A factor of
`-1` populates the current sign-flip table; `1` needs no transform. Other
factors/unsupported expressions follow the already accepted refusal scope.
Do not copy the Python helper's unknown-label fallback to `1`. Deduplicate
the same sign change reported by both COCOS labels and documentation events.
The current COCOS query receives `to_versions` but does not filter on it;
endpoint applicability belongs in the new builder.

## Direct verification against released DD-only v5.3.0

On 2026-09-17, downloaded the anonymously accessible OCI artifact
`ghcr.io/iterorganization/imas-codex-graph-dd` and fixed the selection to
manifest digest
`sha256:dc90975cb9fa0c7b08e9e4809640d01e41d927b4162200c13eec5076e030329b`.
The 2,091,378,730-byte archive passed its layer digest check:
`sha256:2514cbd89c525131130f592f6f7596c700187a4eba43ba2d0e8c623fe32b38ed`.
The archive manifest identifies git tag `v5.3.0`, commit
`ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e`, and export time
`2026-04-10T15:36:08.811578+00:00`.

Loaded the dump into a new temporary database and queried it using
Neo4j `2026.01.4-community`, image digest
`sha256:657e0b601f09da7ef1bd51eebfe3758c3123eb362249767d8476500c95ea810e`.
The container had no external network or published ports and a ten-second
transaction timeout. Queries were bounded to named paths, the version
catalog or equilibrium (2,019 nodes and 3,584 change events). The temporary
service was stopped after verification. Data and acquisition scripts remain
under `/private/tmp/imas-mvdd-kg-verify.U2NOq5/` for reuse; no production
service, source DD, or shim implementation was modified.

### Verified positive examples

| Rule mechanism | Raw evidence | Result |
|---|---|---|
| Rename | `beta_normal -[:RENAMED_TO]-> beta_tor_norm`; successor NBC previous name `beta_normal`, version 4.0.0 | Correspondence recoverable without prose inference |
| Coexistence merge | `constraints/j_tor -[:RENAMED_TO]-> constraints/j_phi`; old introduced 3.39.0 and removed 4.0.0, successor introduced 3.42.0; NBC previous name `j_tor` | A-only → B-only for 3.39.0 → 4.1.1, A-and-B → B-only for 3.42.0 → 4.1.1 |
| Moved subtree | `boundary/closest_wall_point.change_nbc_previous_name = '../boundary_separatrix/closest_wall_point'`, NBC version 4.0.0; no successor edge | Normalize the previous name relative to the new parent. Same evidence exists for `dr_dz_zero_point` and `gap` |
| Retype | `coordinates_type:data_type:4.0.0`, old `INT_1D`, new `STRUCT_ARRAY` | Populate the existing retype refusal |
| COCOS flip | `profiles_1d/psi.cocos_label_transformation = 'psi_like'`; versions 3.39.0 and 4.1.1 carry COCOS 11 and 17 | Factor is derivable using the supported calculation |
| Intermediate version change | `boundary/rho_tor` introduced 4.1.0 with NBC previous name `../global_quantities/rho_tor_boundary`; the latter introduced 3.40.0 and still present | Version-scoped NBC facts can resolve a change without a flattened successor edge; coexistence must be preserved |

Paths in this table omit the common `equilibrium/time_slice/` prefix except
`coordinates_type`, which is `equilibrium/grids_ggd/grid/space/coordinates_type`.

### Snapshot-specific query contract corrections

- Equilibrium has **143 `RENAMED_TO` edges**, **35 NBC previous-name
  properties**, and **zero `renamed_to` string properties**. The second
  successor channel exists in the newer source implementation, but is not
  populated in this released snapshot. An empty string-property query is
  therefore not evidence that the edge relation is absent.
- It has **zero `cocos_transformation_type` properties**, but **151
  `cocos_label_transformation` properties**, 151 provenance properties and
  97 expressions. Inspecting `keys(n)` confirmed the older property name.
  The corresponding event type is also `cocos_label_transformation`.
  The first query's null labels were a schema-version mismatch, not missing
  COCOS evidence. Bind the adapter to the selected snapshot's contract.
- There are **zero `path_renamed` change events** for equilibrium despite
  the 143 successor edges and the NBC metadata. A query selecting only
  that event type would silently discard recoverable conversions.
- DD versions before 3.35.0 have null `DDVersion.cocos` in this snapshot.
  Supporting them needs the documented historical convention fallback or
  additional graph evidence; null must not mean identity.

Representative executed query shape (the probes supplied literal path
lists; a Rust client would bind `$paths`):

```cypher
UNWIND $paths AS pid
MATCH (n:IMASNode {id: pid})
RETURN pid AS path,
       n.change_nbc_previous_name AS previous_name,
       n.change_nbc_version AS nbc_version,
       [(n)-[:RENAMED_TO]->(s) | s.id] AS successors,
       [(n)-[:INTRODUCED_IN]->(v) | v.id] AS introduced,
       [(n)-[:DEPRECATED_IN]->(v) | v.id] AS removed
```

### Concrete correspondence gap in the baseline

Eight old alias anchors in the artifact have no outgoing successor edge or
successor string in this snapshot:

- `profiles_1d/b_average`, `profiles_1d/b_max`, `profiles_1d/b_min`;
- `profiles_2d/b_r`, `profiles_2d/b_z`, `profiles_2d/b_tor`;
- `global_quantities/magnetic_axis/b_tor`;
- `global_quantities/w_mhd`.

Their expected counterparts have no NBC previous-name link identifying
these aliases. Both members of the older alias pairs already exist at the
snapshot's earliest DD, 3.22.0. The `b_field_tor → b_field_phi` NBC link is
present, but does not establish the older `b_tor → b_field_tor` link.
Examined all relationships and property keys for representative missing
aliases, plus all equilibrium NBC previous-name values. The records contain
documentation, lifecycle and semantic-cluster information, not an explicit
replacement declaration for these eight anchors.

For example, `b_average` says "Flux surface averaged B", while
`b_field_average` describes the averaged modulus and positivity. An agent
can use that prose to propose a correspondence; the data is not an
executable equivalence declaration. This explains why `source='derived'`
in the hand-authored artifact does not ensure deterministic recovery from
the structured graph. These are concrete missing declarations under the
accepted evidence policy, not a reason to change read/write/delete rules.
They can remain unresolved for the prototype; full recovery requires such
declarations upstream in the KG rather than new shim heuristics.

### What this verification does and does not settle

The representative query mechanisms are now directly verified, and an
initial Rust adapter can be designed against this exact snapshot. No
equilibrium node had multiple introduction edges, multiple removal edges,
or multiple `path_added` events in the bounded checks. That does not prove
all-IDS history completeness, particularly given the producer's event
deduplication. Exhaustive parity and arbitrary-version coverage belong in
prototype validation, not an assertion from these samples.

## Remaining prototype validation

The query families and in-memory destination are identified. This research
has not established that every non-manual artifact rule is recoverable from
every released KG snapshot, nor that all intermediate-version histories are
complete enough for an arbitrary pair. In particular:

- Extend the verified snapshot adapter across the required IDS/version
  cases. Do not substitute current-checkout property names for the measured
  release contract or infer the remote MCP's deployment from this snapshot.
- Check correspondence through intermediate versions: a current-target
  flattened edge cannot alone establish each historical transition.
- Check lifecycle/history behavior for removal and reintroduction. The local
  producer suppresses subsequent `path_added` events once one exists
  (`graph/build_dd.py:3892`), but the released producer does not. The follow-up
  algorithm investigation establishes full event replay as the released-source
  approach; a simple lifetime interval is insufficient for reintroductions.
- Compare representative rename, coexistence merge, moved subtree, absence,
  retype and sign-flip outcomes with the artifact, excluding `source=manual`
  and reviewing `decision=yes` assumptions. Retain ADR 0027's correction of
  invalid endpoint candidates.

These are checks of the shim's use of the KG contract, not independent
validation of KG facts against source DD XML. No runtime code or
shim tests were changed during this research. The direct follow-up used the
isolated temporary database setup described above.
