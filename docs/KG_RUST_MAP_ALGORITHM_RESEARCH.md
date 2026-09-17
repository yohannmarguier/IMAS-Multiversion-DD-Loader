# From imas-python through the KG to a Rust conversion map

Research completed 2026-09-17. This is an algorithm investigation, not an
implementation specification. It accompanies [the snapshot/query research](KG_CONVERSION_QUERY_RESEARCH.md)
and preserves [ADR 0027](adr/0027-neo4j-runtime-queries-for-the-prototype.md).
No runtime Rust, XML artifact, seam policy, deployment setup or CI was changed.

The later [post-prototype decision review](KG_PROTOTYPE_DECISION_REVIEW.md)
links the completed experiment and records the user's subsequent decisions.
Its accepted absence, COCOS and fidelity criteria supersede experimental
recommendations below where they differ; this note retains the research history.

## Findings that change the implementation premise

Python supplies a useful **correspondence algorithm**, but its exported path
dictionary is not its conversion program. It compares two endpoint inventories,
reads accumulated NBC declarations from the newer endpoint, and adds explicit
special cases. It neither composes every adjacent release nor uniformly derives
conversions from generic DD metadata. Its type handlers, COCOS callbacks and
whole-IDS transformations are separate from the dictionary exported into the KG.
The detailed source trace and executed Python tests are in Appendix A.

The pinned KG contains enough structured evidence for the worked rename,
coexistence, moved-subtree, absence, retype and sign-flip cases below. It also
preserves a real multi-rename history for a second IDS, `pulse_schedule`.
However, it is not a lossless historical serialization of Python's inputs and
outputs. A generic Rust compiler must have a supported result, a localized
refusal, or a construction failure for each relevant piece of evidence; it
cannot treat every missing edge or discarded callback as identity. Appendix B
traces precisely what the producer persists and discards.

Two corrections to the earlier investigation are material:

- Released source retains repeated `path_added` events. The warning about
  suppression describes the **newer checkout**, not the released producer.
  Replay the complete released event ledger, including rename events; do not
  infer all presence from one introduction/removal interval. Export revision
  still does not certify the ingestion revision of every graph record.
- Python does not normalize `../` previous names. The measured equilibrium
  closest-wall-point move is dropped by current Python. A Rust normalization
  algorithm can use the explicit KG declaration without claiming Python parity.

Neither finding changes the accepted read/write/delete policies. The remaining
question is whether a conservative compiler can construct complete, useful maps
within the five-second whole-attempt budget. That needs the bounded experiment
at the end, not a full integration proposal.

## Evidence ledger

| Key | Fixed primary source / scope |
|---|---|
| P | IMAS-Python `fbc39ebc6a30dabbe3041d8480ce4e3f4e282f6c`, [ids_convert.py](https://github.com/iterorganization/IMAS-Python/blob/fbc39ebc6a30dabbe3041d8480ce4e3f4e282f6c/imas/ids_convert.py); local checkout `/Users/yohann/Documents/Dev/ITER/IMAS-Python` |
| R | IMAS-Codex release `ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e`, inspected with `git show`, [builder](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py) |
| C | Newer IMAS-Codex `abdca7bd4ebdb2aeac6052383f0c13c691eaa7e4`; distinct from R and the remote MCP deployment |
| S | Shim `51e64b05a34e7fb1ebf495885eaae8085903d2b7`; sources below use repository-relative links. |
| G | DD-only v5.3.0, OCI manifest `sha256:dc90975cb9fa0c7b08e9e4809640d01e41d927b4162200c13eec5076e030329b`; archive/image provenance in the companion note |
| Q | Read-only Cypher executed this session against G; query contracts and selected raw rows reproduced below. Temporary probe output `/private/tmp/imas-mvdd-algorithm-probe-output.txt`; no remote MCP result substituted for G. |

The deliberately separate P/R identities matter: R's lockfile pins
`imas-python==2.2.0`, whereas the current P checkout is the algorithm studied
and tested here. The export manifest does not record the Python environment
that produced each edge. No byte-for-byte producer reproduction is claimed.
[Producer provenance, Appendix B](#appendix-b-released-producer-contract).

## Input/output crosswalk

This table concerns the **measured release schema**, not current helper names.
Read Appendix A for the owner of each Python behavior and Appendix B for each
persistence claim.

| Python input or result | Released KG channel | Rust consequence |
|---|---|---|
| Exact endpoint factories and version ordering | `DDVersion.id`; node/event lifecycle | Parse numeric `(major,minor,patch)`; require both exact releases. Catalogue presence alone is not a per-IDS completeness certificate. |
| Endpoint field inventories | Accumulated `IMASNode.id`, `ids`; `path_added`, `path_removed`, `path_renamed` events; introduction/removal edges | Reconstruct exact presence. Include structures, errors and metadata. |
| New endpoint NBC description/date/previous-name lists | `change_nbc_description`, `change_nbc_version`, `change_nbc_previous_name` | Decode aligned lists and version applicability; do not split and discard ordering. No historical updates of these properties are persisted. |
| Prior type declaration | `change_nbc_previous_type` | Corroborating declaration; Python actually compares endpoint datatypes. |
| Endpoint datatype/rank | `data_type`, `ndim` and versioned events, including field-qualified `structure_changed` IDs | Reconstruct endpoint values; refuse incompatible shape using existing Retyped. Never infer exact endpoint type from a uniformly “current” property model. |
| Parent nesting | Path segments, `HAS_PARENT` | IDS-qualified KG path becomes IDS-relative shim path. Segment-aware ancestry; no raw string-prefix matching. |
| Timebase/context maps (`tbp`, `ctxpath`) | Not exported from Python map; `timebasepath` property/events and path hierarchy are separate evidence | Shim already handles caller context and independently resolves its timebase; do not import Python object traversal semantics. Timebase changes requiring resampling remain unsupported. |
| Coordinates | `HAS_COORDINATE` edges with `dimension`, target IMASNode or IMASCoordinateSpec; `coordinates` events | There is no reliable `n.coordinates` property: R builds a list but persists relationships. Historical list events retain raw strings; current edges can have stripped index notation and are not versioned. |
| Identifier metadata, max occurrence | Payload contains fields; event IDs distinguish enum/rank and `maxoccur_changed` | R's node SET does not persist every payload field (including enum/maxoccur in this insertion path). A queried null is unavailable metadata, not proof of no constraint. |
| Generic rename outputs | `RENAMED_TO`, NBC declarations, occasional `path_renamed` | Edges are unversioned, flattened and forward-only exports. Both tested IDSs have zero `path_renamed` events despite many useful correspondences. |
| Reverse map and coexistence ordering | Not persisted as Python reverse semantics or precedence | Build a versioned endpoint relation; use existing successor-first shim precedence. Python's “last copied child wins” is not that contract. |
| None/no-copy result | Not imported as a successor edge | Distinguish absence from unsupported/unresolved using inventories and other evidence. |
| COCOS callback | `DDVersion.cocos`, `cocos_label_transformation`, label source, expression, label events | Use release property names. Backfilled label is not the same thing as historical raw label. |
| Python extra sign table | Selected consequences backfilled as `psi_like` / `inferred_sign_flip` | Table is a producer input; Rust needs persisted evidence and provenance, not Python at runtime. Structure-level table entries require care. |
| Type callbacks / whole-IDS callbacks / contour operations / pulse resampling | Not exported by path-map channel | A path edge cannot certify a value conversion. Classify recorded semantic/type/timebase changes; unsupported or insufficient evidence must not fall through to identity. |
| Fidelity, rule kind, source precedence | No ready-made graph fields | Derive rule cardinality from exact endpoints and use existing artifact/interpreter conventions. No new operation policy. |

The coordinates/payload distinctions follow R's [node insertion and coordinate
relationships](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L2985).
Q returned `equilibrium/code` as `data_type=STRUCTURE` but `is_leaf=true`.
Therefore even that convenience flag is unsuitable for the shim's
leaf-versus-structure safety decision; use reconstructed datatype/hierarchy.

## A concrete, conservative Rust algorithm

The following is a proposed derivation, not code already implemented. Its
refusal states are essential parts of the result, not deferred TODOs.

### Typed facts and invariants

Use `Version(u32,u32,u32)`, validated `IdsName`, IDS-relative `DdPath`,
`EventId`, and a fixed `SnapshotId`. Keep wire null distinct from an empty
string and from a missing row. Suggested pure input/result shapes:

```text
IdsFacts {
  snapshot, versions,
  nodes: Map<Path, NodeFacts>,
  events: Vec<{id, path, version, field, kind, old, new, annotations}>,
  successors: Map<Path, Set<Path>>
}
EndpointField { present, datatype, rank, unit, timebase, coordinates, evidence }
Correspondence { left: RoleAtVersion, right: RoleAtVersion, evidence, transform }
PathVerdict = Identity | Supported(Relation) | AbsentCounterpart
            | Unsupported(reason,evidence) | Unresolved(reason,evidence)
BuildResult = CompleteMap(map,diagnostics) | AcquisitionFailure
            | IncompleteScope | UnrepresentableScope
```

A `RoleAtVersion` is more than a path string: the spelling `name` can mean the
old description or the new identifier. Preserve direction, date and semantic
role when connecting vertices. An undirected union of all `RENAMED_TO` edges
would incorrectly fuse those roles (Appendix A, name/identifier migration).
No generated target or candidate may be absent from its own endpoint.
No duplicate candidate, missing precedence, contradictory event, dangling
path ID or unsupported wire encoding may silently become a supported rule.

### Fetch contract and completeness

Fetch the small full version catalogue, then three IDS-scoped streams: nodes,
all events, and successors. **All history** is the simple safe starting point:
restricting events to `(old,new]` cannot establish an endpoint's presence,
initial datatype or historical parent spelling. Below are parameterized query
shapes executed with `ids='equilibrium'`, `after=''`, `page_size=2`. Diagnostic
node projections were also exercised without filtering any node category.

```cypher
MATCH (v:DDVersion)
RETURN v.id AS version, v.cocos AS cocos ORDER BY version;

MATCH (n:IMASNode {ids:$ids}) WHERE n.id > $after
RETURN n.id AS path,
       n{.id,.ids,.data_type,.ndim,.node_type,.unit,.timebasepath,
         .change_nbc_version,.change_nbc_description,
         .change_nbc_previous_name,.change_nbc_previous_type,
         .cocos_label_transformation,.cocos_label_source,
         .cocos_transformation_expression} AS metadata,
       [(n)-[:INTRODUCED_IN]->(v)|v.id] AS introduced,
       [(n)-[:DEPRECATED_IN]->(v)|v.id] AS removed
ORDER BY path LIMIT $page_size;

MATCH (n:IMASNode {ids:$ids})<-[:FOR_IMAS_PATH]-(c:IMASNodeChange)
WHERE c.id > $after
RETURN c.id AS event,n.id AS path,properties(c) AS facts,
       [(c)-[:IN_VERSION]->(v)|v.id] AS versions
ORDER BY event LIMIT $page_size;

MATCH (n:IMASNode {ids:$ids}) WHERE n.id > $after
RETURN n.id AS path,[(n)-[:RENAMED_TO]->(s)|s.id] AS successors
ORDER BY path LIMIT $page_size;
```

Node projection may add coordinates as
`[(n)-[r:HAS_COORDINATE]->(q)|{dimension:r.dimension,target:q.id}]` and
parents as `[(n)-[:HAS_PARENT]->(p)|p.id]`; these relationship fields are
source-backed extensions, not part of the executed pagination probe.
Avoid `properties(n)` in the runtime contract: it can retrieve embeddings and
unrelated enrichment. `properties(c)` is bounded event metadata; an explicit
projection can replace it after schema validation. Do not use reporting-tool
category filters, twenty-event caps, or ten-hop limits.

Required cardinalities: unique version IDs; exactly one node row per path;
one event owner and one event version per unique event ID; one successors row
per node, including empty lists. To enforce event ownership, extend the event
projection with `[(c)-[:FOR_IMAS_PATH]->(p)|p.id] AS owners` and require exactly
the returned path; the executed IDS-scoped MATCH alone cannot detect an extra
owner outside that IDS. This is a proposed validation extension, not a measured
cardinality assertion. Orphan events are not discoverable through this scoped
stream, so it cannot certify their absence. Query counts of nodes, events and successor edges
and compare them with fully consumed distinct rows. For events additionally
check ownership across returned IDs; known endpoints must resolve in the
catalogue. Q measured 2,019 nodes/3,584 events/143 successor edges for equilibrium,
and 1,369/2,534/308 for pulse_schedule, with distinct event IDs equal to counts.

Suggested page size is 512, **an experiment parameter**, not an accepted tuning
decision. Use keyset pages (`id > last_id`), consume through a final short or
empty page, and apply one shared remaining deadline to every query and local
stage. String ordering is only the pagination cursor; versions are sorted
numerically in Rust. Pattern-comprehension lists have no guaranteed order:
sort/deduplicate in Rust. The database must stay immutable during the attempt
and process, per ADR 0027. The same pinned snapshot makes sequential pages
consistent; no concurrent graph mutation is supported.

Counts prove retrieval completeness, not upstream DD accuracy. Trust the KG,
as accepted; do not compare it against source DD XML. Conversely, the release
has no per-IDS/per-version extraction-completeness certificate. A missing
catalogue release, missing inventory seed, contradictory lifecycle, or inability
to localize affected paths must produce `IncompleteScope`, not an identity map.
A present catalogue and successful empty change query alone do not satisfy this.

### Presence and endpoint metadata

1. Normalize versions numerically; choose chronological left/right independently
   of HLI/stored direction. Reject malformed or unsupported release IDs.
2. For each node, establish the earliest known state from its addition event or
   introduction edge, then replay dated `path_added`/`path_removed` events.
   Addition means present **at** the version; removal means absent **at** it.
   Split a `path_renamed(old,new,v)` event into the corresponding presence
   effects where the release omitted separate add/remove events. Multiple
   appearance intervals are valid. Obsolescence is not removal.
3. Reconcile lifecycle edges with the event ledger. Missing redundant events
   can be filled by explicit introduction/removal edges when unambiguous;
   contradictory events or an unlocatable reintroduction cannot be guessed.
   Newer-producer suppression must not be applied to this release contract.
4. Reconstruct tracked metadata separately within each presence interval, with
   the event's old value before its first change and new values after each
   change. R compares metadata only for common paths: a reappearance can have
   different metadata without a change event. Never chain events across an
   absence gap. Each interval needs a usable initial-value anchor; otherwise
   the field is unresolved. With no event, use a persisted property only where
   its age/contract supplies that anchor. Decode empty old/new strings according
   to each field, not with one global null rule.
5. Coordinate event values may be Python-list text. A small literal-list parser
   can accept strings/escapes only and reject other syntax; never evaluate it.
   Raw coordinate history and stripped relationship targets are not equivalent.
   NBC lists and COCOS expressions have no complete event history, so their
   applicability requires the preserved declaration dates/provenance; absent
   historical declarations cannot be reconstructed by pretending latest is old.

These are interpretations of R's [event comparison](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L1383)
and [event persistence](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L3281),
not a factual audit of upstream inventories.

### Correspondences across history

Decode previous names and NBC versions together, requiring equal lengths,
valid dates and monotonic order. Admit the known rename descriptions as rename
instructions; contour and structural/value descriptions are not rename aliases.
For each endpoint path, work backwards through applicable declarations, including
ancestors, to find its spelling/role at the other endpoint. A declaration's
stored final path is a **witness**, not necessarily an endpoint candidate.
Thus `beam` may supply the history needed to relate `antenna` and `launcher`
even when `beam` is absent from both requested endpoints.

For a list `(v1,old1),(v2,old2)` on final `new`, the local name timeline is
`old1 --v1--> old2 --v2--> new`. For a selected old endpoint, Python chooses the
first date greater than it; Rust can use the same selection with inventory
checks, while separately retaining coexistence. Rewrite each ancestor's name
at the same historical stage. Apply the most-specific child declaration before
propagating its parent substitution, so a child exception is not overwritten.
Process simultaneous parent/child changes as one version step. Memoize results
and detect cycles/conflicting histories rather than arbitrarily ordering them.

Normalize a previous name relative to the **declaring node's parent**. Treat
`../` segment-wise, permit multi-segment previous names such as `reference/data`,
and reject escape above the IDS root. Strip an IDS prefix exactly once from KG
IDs; never strip a matching substring in the middle. A normalized historical
name becomes a candidate only if the inventory says it exists at that endpoint.

Combine evidence in this order: dated NBC/history, independently persisted
successor correspondence, exact endpoint membership. A flattened edge can
corroborate a direct old/new correspondence but does not supply missing dates,
callbacks or value semantics. Two old paths sharing a flattened final successor
are not automatically a proven historical pair. If dated metadata cannot
establish their relevant roles, retain an unresolved correspondence. Do not
perform unrestricted transitive closure through future semantic changes.

For a supported parent relation, propagate its suffix only to descendants
actually present at the relevant endpoints, then let explicit child relations,
retypes, removals and unsupported changes override it. Construct the endpoint
relation at individual path granularity before compressing selectors. This
follows Python's inventory-limited propagation, adding the normalization it
lacks and the shim's refusal semantics. See P:217–361 in Appendix A.

### Group into existing rule fields

Let L/R be **version-qualified, evidence-supported** member sets for one
semantic role, not all paths in an undirected component.

| Endpoint shape | Existing representation |
|---|---|
| 1:1, same spelling and no change | Identity default; or exact `Renamed(p,p)` if a future typed constructor uses explicit identity entries (there is no `Rel::Identical`) |
| 1:1, changed terminal name | `Renamed`, exact/exact fidelity |
| 1:1, changed parent | `Moved`, exact/exact unless declared subtree loss applies |
| m:1 | `Merged {right, froms:left}`, successor-first precedence; declared forward Lossy/reverse Exact as in artifact; interpreter narrows merge loss to PotentiallyLossy |
| 1:n | `Split {left, froms:right}`; mirror declaration forward Exact/reverse Lossy |
| One-sided, proven absence | `LeftOnly` or `RightOnly`; present-side Lossy, opposite declaration Unmappable; produces NoSource for reads through present side |
| Type/rank change | `Retyped {left,right}`; existing unconditional UnservableRetype refusal |
| Required unit redefinition | Existing right-path redefine entry, existing UnitRedefinition refusal |
| Unsupported/unresolved but localizable | Existing applicable rule with Unmappable fidelity; one-sided rule can carry Unmappable when its spelling is genuinely absent opposite |
| General m:n or incompatible transforms | Not generally representable as one current rule; local refusals where valid existing selectors can cover it, otherwise fail map construction |

Sources: [Rule/Rel definitions](../src/conversion/conversion_map.rs),
`Rel` at 365, `Rule` at 414, `resolve_merged` at 1158, `resolve_split` at 1204,
`refusal_before_resolution` at 1110;
[path fidelity narrowing](../src/conversion/path_conversion.rs) at 990;
[artifact conventions](3.39.0--4.1.1.xml) at 92 and 242.

Do not select precedence alphabetically. A dated linear successor chain orders
newest surviving spelling first; exact-version filtering precedes numbering.
If evidence supplies only incomparable branches and cannot identify a primary,
that plan is unresolved. This preserves established precedence without inventing
new read/write/delete selection rules.

Use exact per-path rules as the correctness baseline, including AoS anchors.
A structure rewrite must still exist for opening its child context. Compress
identical suffix families to subtree selectors only after checking both endpoint
inventories and all exceptions. Exact selectors override subtree selectors;
within subtrees the longest anchor wins. Duplicate same-stage anchors are
invalid; nested anchors are allowed by the actual validator. Do not describe
all nested selectors as forbidden overlap. Preserve escaping child rules for
the existing delete safety check. [Matching/validation](../src/conversion/conversion_map.rs),
705, 1005 and 1382.

For a removed child under a moved parent, emit its explicit absence override;
for a differently renamed child, emit its own correspondence; for a removed
container with escaped children, never let its absence hide the children's
rules. Do not use a subtree rule to fabricate a target child. A typed builder
must validate **every candidate** against its endpoint: existing
`check_completeness` does not inspect all `FromEntry` targets and is not enough
for ADR 0027's candidate-presence condition. Its side-only check also forbids
using `LeftOnly(p)` when p exists on the right, even if the intent was a generic
refusal. Use a truthful supported relation with Unmappable fidelity, or fail
construction when the current representation cannot express a local refusal.
[Completeness implementation](../src/conversion/conversion_map.rs), 1520–1635.

### Values, deduplication and representation limits

Resolve paths and exact endpoint types **before** deciding transforms. For a
known COCOS class, calculate the factor from endpoint conventions; for the
verified 11→17 transition, `psi_like` and `dodpsi_like` give -1. The
[Codex factor calculator](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/ids/transforms.py#L40)
is distinct from Python's label/table callbacks. Unknown labels must not use
its fallback factor 1.

A supported expression needs an explicitly parsed, whitelisted algebra of
known classes, or it is unsupported. Taking the first `_like` token from an
expression is not evaluating it. A factor +1 requires no numerical transform;
-1 maps to the existing sign flip; any required factor other than ±1, reshape,
resampling or geometry-dependent transform is outside the current engine.
For historical versions with null convention, a documented Codex fallback is
11 before 4.0 and 17 after; using it would be an explicit source-derived
compatibility rule, not a graph-provided fact. Without it, mark COCOS-dependent
paths unresolved. [Fallback source](https://github.com/iterorganization/imas-codex/blob/abdca7bd4ebdb2aeac6052383f0c13c691eaa7e4/imas_codex/cocos/calculator.py#L128).

Raw COCOS label events and backfilled class properties differ: Q shows `psi`
losing its raw label at 4.0 while retaining an inferred `psi_like` property.
That is not evidence its physical sign transform disappeared. Use the recorded
class provenance and endpoint conventions. Attach one transform per resolved
right-side path. A label, an inferred Python-table label and a documentation
sign-change event can be evidence of **the same change**: deduplicate by
transition/path and corroborate; do not multiply -1 three times. Independent
successive transformations compose only when independently established.

The existing `sign_flips: HashMap<right_path,(left_cocos,right_cocos)>` cannot
encode different left-candidate transforms for the same right path. Every
left source of a merged right path therefore must require the same factor;
otherwise that group cannot be served by the unchanged interpreter.
`ValueTransformation` supports None and SignFlip only. A label on a structure
also does not authorize multiplying a structure pointer: Python's magnetics
handler explicitly flips `.data`; persist/derive the actual leaf relation or
refuse. [Transform application/keying](../src/conversion/conversion_map.rs),
331, 774, 1358 and 1420; Appendix A, P:1130.

### Construction pseudocode and cost

```text
build(ids, hli, stored, snapshot, deadline):
  facts = fetch_complete_ids_streams(snapshot, ids, deadline)
  validate_wire_contract_counts_references(facts)
  L,R = sort_numeric(hli,stored)
  endpoints = replay_presence_and_tracked_metadata(facts,L,R)
  history = decode_dated_nbc_and_successor_evidence(facts)
  relation = resolve_versioned_roles(history,endpoints)
  relation = propagate_only_real_descendants_and_apply_child_exceptions(relation)
  verdicts = classify_identity_absence_retype_semantic_change_and_gaps(relation)
  transforms = derive_supported_factors_and_deduplicate(verdicts,facts,L,R)
  groups = partition_representable_endpoint_relations(verdicts,transforms)
  rules = emit_exact_rules_and_proven_subtree_compressions(groups)
  ensure_every_endpoint_path_has_supported_absent_or_explicit_refused_outcome()
  ensure_every_target_and_every_candidate_exists_at_its_endpoint()
  ensure_no_selector_conflicts_or_right_path_transform_conflicts()
  map = validated_typed_constructor(sides,rules,transforms,redefines,defaults)
  compare_resolutions_both_directions_to_compiler_verdicts(map)
  return complete_map_or_localization_failure_before_deadline()
```

The required constructor is a refactoring of XML loading's validation/index
assembly at [ConversionMap::load](../src/conversion/conversion_map.rs), 794–956;
it must expose no partially valid map and must not serialize intermediate XML.
Provenance/diagnostics can accompany map construction without entering the
interpreter. Accepted process caching, single-flight construction and failures
remain ADR 0027 responsibilities outside this pure compiler.

With N nodes, E events, K correspondences, D path depth and R emitted rules,
fetch/storage is O(N+E+K) plus strings. Sorting history is O(E log E);
memoized ancestry work is roughly O(ND+K) for acyclic unambiguous histories.
Naively enumerating all possible ambiguous paths can explode: bound graph
work and reject ambiguity instead. Current resolver scans source entries, so
validating every path against R exact rules is O(NR); retaining thousands of
exact rules can also increase per-seam lookup cost. This is a real cost of
preserving the interpreter, to measure before assuming compression is optional.
Keyset pagination with ORDER BY may rescan/sort without a suitable index;
inspect plans during the experiment rather than asserting linear DB time.

Measured: a small Cypher-shell batch (two rows per stream plus
IDS counts) took 0.98 seconds wall time once warm; earlier related probes took
1.16–1.60 seconds. Those include JVM client startup and **are not** Bolt query
latencies or complete-map construction benchmarks. No full IDS transfer,
compiler time, peak Rust memory, or first-open time has been measured. Five
seconds remains the accepted prototype default, not a demonstrated result.

## Worked derivations into current rule fields

In this section L is the earlier DD, R the later. Paths are IDS-relative in
rules; Q's graph IDs include the IDS prefix. `Exact(p)` / `Subtree(p)` mean
current Selector variants. These are predicted compiler outputs, not results
from an implemented Rust compiler. Raw presence/NBC facts are Q measurements
or the positive examples in the companion note; Python results were executed
separately as described in Appendix A.

### One rename history, two endpoint pairs: equilibrium current constraint

Facts: `time_slice/constraints/j_tor` introduced 3.39.0, removed 4.0.0;
`j_phi` introduced 3.42.0; successor edge j_tor→j_phi and NBC previous name
j_tor/date 3.42.0. Both names coexist at 3.42.0. The raw edge has no date.

For **3.39.0→4.1.1**:

```text
Rule {
 id: deterministic("constraints-j",L,R), rel: Renamed,
 left: Subtree(time_slice/constraints/j_tor),
 right: Subtree(time_slice/constraints/j_phi), froms: [],
 fidelity_forward: Exact, fidelity_reverse: Exact
}
```

Every mapped descendant still needs inventory/transform checks and exceptions.
There is no j_phi candidate on L. This is the same correction mechanism as
ADR 0027's b_field_phi example: the artifact's claimed merge cannot override
exact endpoint membership. Python measured a bijective rename for this pair.

For **3.42.0→4.1.1**:

```text
Rule {
 rel: Merged, left: None,
 right: Subtree(time_slice/constraints/j_phi),
 froms: [ {selector:Subtree(.../j_phi),precedence:1},
          {selector:Subtree(.../j_tor),precedence:2} ],
 fidelity_forward: Lossy, fidelity_reverse: Exact
}
```

Rust derives this by intersecting the known correspondence with endpoint
presence, not by copying Python's 3.41 fallback or reverse-map behavior.
Existing resolver yields one destination Forward and ordered candidates
Reverse. Existing seam policy determines reads/writes/deletes. No new policy
choice is needed. Source convention: artifact rule `fold-constraints-j`;
measured Python behavior: Appendix A's coexistence table.

### Multi-rename and nested child exception: pulse_schedule

Q returned these exact structured facts:

| Path | Introduced | Removed | Previous name / NBC dates | Successor |
|---|---|---|---|---|
| ec/antenna | 3.22.0 | 3.26.0 | null | ec/beam |
| ec/launcher | 3.26.0 | 3.40.0 | null | ec/beam |
| ec/beam | 3.40.0 | none | antenna,launcher / 3.26.0,3.40.0 | none |
| ec/antenna/launching_angle_pol | 3.22.0 | 3.26.0 | null | ec/beam/steering_angle_pol |
| ec/launcher/steering_angle_pol | 3.26.0 | 3.40.0 | null | ec/beam/steering_angle_pol |
| ec/beam/steering_angle_pol | 3.40.0 | none | launching_angle_pol / 3.26.0 | none |

For **3.25.0→3.40.0**, parent's first later NBC selects antenna. Proposed parent
rule: Renamed(Subtree(ec/antenna),Subtree(ec/beam)), Exact/Exact. The child's
explicit event gives `ec/beam/launching_angle_pol`, then the historical parent
substitution gives `ec/antenna/launching_angle_pol`. Its more-specific rule is
Renamed(Subtree(ec/antenna/launching_angle_pol),
Subtree(ec/beam/steering_angle_pol)), Exact/Exact before descendant-specific
changes. A suffix-only parent rule would choose the wrong child spelling.

For **3.30.0→3.40.0**, parent's first later event selects launcher. The child's
3.26 change predates L, so the counterpart is
`ec/launcher/steering_angle_pol`. Parent rule maps launcher→beam; this child
needs no special name override beyond the parent's suffix propagation.

For **3.25.0→3.30.0**, the same retained history relates antenna→launcher and
launching_angle_pol→steering_angle_pol, although beam is absent at both
endpoints. This is the recommended additional IDS/pair for focused prototype
coverage; it exercises historical resolution rather than merely a current-target
edge. P's `test_multi_rename` covers these endpoint combinations.
An additional direct Python probe confirmed this 3.25→3.30 parent, child and
`/reference` mapping in both directions, with no type/postprocessing callbacks
on those entries and empty whole-IDS callback lists.

The two 3.40 examples establish **path correspondence**, not blanket support
for every leaf in the IDS: `reference/data→reference`, datatype/timebase changes
and Python's heterogeneous-time resampling require separate classification.
Do not call the entire 3.25→3.40 conversion exact because its AoS name is exact.
Pre-3.35 null COCOS values also need the explicit fallback or localized COCOS
uncertainty described above; structural/string examples require no sign factor.

### Moved subtree with a missing nested child

Q and the prior snapshot probe establish:

```text
new: equilibrium/time_slice/boundary/gap
previous_name: ../boundary_separatrix/gap
nbc_version: 4.0.0
old gap/identifier: introduced 3.31.0, removed 4.0.0, no successor
new boundary/gap/identifier: no node
new boundary/gap/r: introduced 4.0.0
```

For 3.39.0→4.1.1, normalize the previous name relative to
`time_slice/boundary`, producing `time_slice/boundary_separatrix/gap`.
The existing artifact's output shape is:

```text
Moved { left:Subtree(time_slice/boundary_separatrix/gap),
        right:Subtree(time_slice/boundary/gap), froms:[],
        fidelity_forward:Lossy, fidelity_reverse:Exact }
LeftOnly { left:Subtree(time_slice/boundary_separatrix/gap/identifier),
           right:None, froms:[],
           fidelity_forward:Lossy, fidelity_reverse:Unmappable }
```

The more-specific child rule returns NoSource, never a fabricated
`boundary/gap/identifier`. A call on `gap/r` follows the move, subject to its
own metadata/value checks. Here the parent's Lossy declaration preserves the
artifact convention for a subtree losing a child; exact per-leaf expansion
may separate those verdicts more precisely and must be compared explicitly,
not passed off as identical artifact fidelity. See artifact 242–254.

This is also the requested **known absence** case: the full inventory and
absence of a supported correspondence establish the dropped identifier under
this parent mapping. In contrast, the eight alias anchors below remain
explicitly unresolved equivalences. The two classifications must not be
conflated simply because both lack successor edges.

### Same spelling, changed type

Q returned event
`equilibrium/grids_ggd/grid/space/coordinates_type:data_type:4.0.0`,
old `INT_1D`, new `STRUCT_ARRAY`. For 3.39.0→4.1.1:

```text
Retyped { left:Subtree(grids_ggd/grid/space/coordinates_type),
          right:Subtree(grids_ggd/grid/space/coordinates_type),froms:[],
          fidelity_forward:Exact,fidelity_reverse:Exact }
```

Both directions refuse as UnservableRetype regardless of declared fidelity;
new identifier children remain under that refusal rather than being treated
as ordinary independent additions. Python instead installs
`_type_changed_to_identifier`, invisible in the exported path dictionary.
Same spelling and an absent rename edge therefore cannot establish identity.
Sources: Q event; P:183–204; artifact `retype-coordinates-type`; S:1110.

### Same spelling, COCOS sign flip

Q returned `time_slice/profiles_1d/psi.cocos_label_transformation='psi_like'`.
DD3.39.0 carries 11, DD4.1.1 carries 17. Raw label history has addition at 3.28.1
and clearing at 4.0.0, while backfill preserves the class property.
COCOS parameters are `(sigma_bp,e_bp)=(+1,1)` and `(-1,1)`, so
`(-1/+1)*(2*pi)^(1-1)=-1`.

Keep the identity path result and populate:

```text
sign_flips["time_slice/profiles_1d/psi"] = (CocosConvention("11"),
                                           CocosConvention("17"))
```

S:1420 orients the read transformation from stored to HLI: Forward means
17→11, Reverse means 11→17; write requests the inverse. The scientific value
is flipped once. Existing EMPTY-sentinel handling stays in seam policy.
Do not construct a second path rule just because documentation also describes
the COCOS change. P's callback dictionary similarly deduplicates by key.

### Concrete negative cases and remaining representational limits

The eight unresolved structured correspondences from the companion note are
`profiles_1d/{b_average,b_max,b_min}`, `profiles_2d/{b_r,b_z,b_tor}`,
`global_quantities/magnetic_axis/b_tor` and `global_quantities/w_mhd`, all
under equilibrium/time_slice. Their supposed equivalences are not declared
by successor/NBC facts in G. Their documentation is not a machine-readable
conversion instruction. P's measured b_average mapping is None too. Keep
these as scoped unresolved evidence, not guessed merges and not proof that
the entire graph lacks correspondence data.

Other limits are different in kind:

| Limit | Classification / consequence |
|---|---|
| Querying `cocos_transformation_type` on G | Schema-name mismatch; use measured `cocos_label_transformation` |
| Empty per-path report or twenty-event cap | Reporting limitation; use complete raw streams |
| Unrecorded updates to NBC declarations/expressions | Missing historical metadata; cannot promise arbitrary pair reconstruction |
| Unversioned flattened edge hides callback | Lost algorithm output; endpoint types/events may reveal refusal, but edge alone is insufficient |
| Manual psi-axis equivalence/fidelity assumptions | Manual semantic assumption excluded; an observed successor does not prove all rule claims |
| Reshape, contour construction, resampling, non-sign scale | Unsupported transformation, even when correspondence is known |
| Many-to-many relation; merged candidates need different factors | Existing representation limit, not a missing Neo4j query |
| Same-spelling semantic reuse | Role ambiguity; do not apply identity before inspecting the dated relation |
| Undetectable scope of lost semantics | Whole construction must fail rather than mark arbitrary paths safe |

The investigation does not claim that every Python special case can be
identified solely from the released KG. In particular, bare forward edges do
not disclose their generating callback. The conservative algorithm is buildable
for supported evidence and explicit refusals; universal all-IDS successful
construction is **not proven**. This is a bounded limitation of the evidence,
not a reason to replace established seam policies or add prose heuristics.

## One bounded next experiment

Question: **Can a small Rust fact compiler build complete, validated maps for
these fixed IDS/pairs, preserving exact endpoint candidates and localizing
unsupported evidence, without changing the interpreter?**

Use the pinned G service, existing Rule/Selector/interpreter types through a
minimal validated typed-constructor experiment, and no HLI/Core integration,
cache service, setup automation or generated XML. Exercise equilibrium
3.39→4.1.1 and 3.42→4.1.1 plus pulse_schedule 3.25→3.30. Feed full IDS streams,
not a hand-selected path map. This is a recommended throwaway prototype,
**not work performed or authorization inferred from this research**.

Measurable success criteria:

1. Every endpoint path is classified as supported, known absence or a traced
   local refusal; zero silently defaulted unsupported changes. Any unlocalizable
   scope failure is reported with evidence, not hidden to make the experiment pass.
2. Both directions reproduce the worked candidate/path/refusal/transform results;
   no endpoint-absent candidates, duplicate selectors or fabricated descendants.
   Compare artifact discrepancies explicitly, including corrected j_phi/b_field_phi
   membership and missing alias equivalences.
3. A graph-free compiler fixture can replay captured typed facts deterministically,
   shuffle row order without changing output, and distinguish missing page/query
   failure from a complete empty result. This tests the shim's interpretation,
   not KG factual correctness.
4. Measure complete connection+fetch+decode+compile+validation wall time, retained
   bytes/rule count, and resolver lookup cost. Report cold/warm observations for
   at least five fresh map builds each; reuse of a Neo4j page cache is labelled.
   Compare with the five-second default; a miss is evidence, not permission to
   silently increase the deadline or stop fetching before completeness.
5. Enumerate unsupported/unresolved counts and precise evidence causes per IDS.
   If current map representation cannot localize them honestly, stop the
   experiment at that demonstrated boundary; do not redesign the interpreter
   inside this prototype.

No Bolt driver recommendation is needed to settle the algorithm. The client
needs parameter binding, complete streaming/page consumption, typed nulls,
error propagation and a shared remaining deadline. Driver/runtime selection
can follow this experiment's actual cancellation and query requirements.

## Verification performed and scope

Nine targeted Python behavior tests passed, with bytecode/cache writing
disabled; Appendix A records selection and timings. Direct G queries confirmed
multi-rename history, coexistence evidence, moved-child absence, retype and
COCOS facts. Parameterized first-page query shapes and complete IDS counts
were executed. Release source was inspected read-only without switching the
Codex checkout. No full Rust map builder, all-pair proof, end-to-end shim test
or complete-map latency benchmark exists from this research.

The sections below retain the detailed primary-source traces in this same
file so the research result does not depend on temporary background reports.

## Appendix A: Python algorithm source trace

Research 2026-09-17 for the KG-backed Rust-map investigation. Source checkout
`/Users/yohann/Documents/Dev/ITER/IMAS-Python`, revision
`fbc39ebc6a30dabbe3041d8480ce4e3f4e282f6c`. Applicable repository `AGENTS.md`
was read; its graph-first lookup was run (`graphify query 'DDVersionMap rename
mapping conversion'`), then the actual source inspected. No tracked source
changes were made. This revision is evidence about current Python, **not** proof
of which Python revision produced the released KG. Source references below are
relative to that checkout; `P` abbreviates `imas/ids_convert.py`, `T` abbreviates
`imas/test/test_ids_convert.py`, `N` abbreviates `imas/test/test_nbc_change.py`.
Read the shim's CONTEXT, ADR 0027, KG research and producer investigation as
constraints. The Python DD XML probes below investigate Python behavior only;
they do not independently validate KG correctness.

### The actual construction pipeline

1. `IDSFactory` loads one exact DD XML tree, records its `<version>` and checks
   a supplied version agrees (`imas/ids_factory.py:53–76`). `dd_zip` delegates
   released XML and parsing to `imas_data_dictionaries` (`imas/dd_zip.py:16`,
   `:83–90`). `dd_version_map_from_factories` parses both versions, selects the
   numerically older factory, builds **both** directions, and returns a boolean
   telling whether its first factory was older (`P:519–534`). It uses a
   128-entry `_DDVersionMap` LRU (`P:112–116`), not the shim's accepted cache
   contract. Missing IDS at either endpoint raises (`P:142–147`).
2. The map makes dictionaries of **every field** in the two endpoint IDS XML
   trees; paths are IDS-relative (`P:206–215`). It does not fetch or compose all
   adjacent DD releases. Generic construction reads accumulated NBC metadata
   **from the newer endpoint tree**, compares it with the older endpoint
   version, and consults the two endpoint inventories.
3. Iterate newer fields with `change_nbc_description` in XML tree order
   (`P:238–309`). Parse comma-separated NBC versions, assert they are sorted,
   and ignore a field if its last NBC date is <= the old version. Invalid
   version strings log an error and are ignored. Accepted generic rename
   descriptions are `aos_renamed`, `leaf_renamed`, `structure_renamed`
   (`P:122`). For these, zip the corresponding comma-separated previous names
   with the dates and choose the **first date strictly greater than the old
   version** (`P:257–267`). This is how several historical renames collapse
   directly to the endpoint mapping.
4. Compute the previous path from `parent(new_path) + previous_name`, then
   apply the nearest already-mapped structure/AoS ancestor rename (`P:217–236`).
   Look it up in the old endpoint inventory. If absent, log and skip; if its
   type is compatible/supported, install the rename (`P:267–277`).
5. `_add_rename` inserts forward and normally reverse entries, plus target
   timebase and nearest-AoS-relative context path (`P:333–353`, `:497–516`).
   It recursively propagates an ancestor rename to old descendants **only
   while each corresponding descendant path exists in the new inventory**
   (`P:355–361`). Later child NBC entries refine the mapping. It does not
   manufacture an arbitrary wildcard rule over all descendants.
6. Check data types at remaining common-spelling paths that were not already
   claimed (`P:311–315`), then record missing paths compactly (`P:317–319`,
   `:474–494`). A missing structure is omitted from the None map if it is an
   ancestor of another map entry, allowing traversal to reach relocated
   children. A wholly missing subtree is represented by its highest skipped
   ancestor, not by one None per descendant.
7. Only when old major==3 and new major==4, run `_apply_3to4_conversion`, then
   run old-side missing-path compaction again (`P:321–331`). This is a
   substantial additional source of behavior, not generic NBC decoding.

**Important correction:** `get_old_path` does **not** normalize `../`.
It concatenates strings and substitutes parents. Although `Path` is imported,
it is used for conditional contour handlers, not rename-path normalization.
For the real equilibrium previous name
`../boundary_separatrix/closest_wall_point`, the generic lookup tries
`time_slice/boundary/../boundary_separatrix/closest_wall_point`, which is not
an inventory key. This was measured in the local Python environment: for
3.39.0→4.1.1 both corresponding directional map entries are None. Rust's
normalized moved-path reconstruction therefore goes beyond Python's actual
generic algorithm while following explicit KG NBC evidence; it must not be
described as copying Python behavior. (`P:228–236`; probe below.)

### Maps and behavior are not interchangeable

`NBCPathMap.path` is a partial `path -> optional path` dictionary. Omission
means default unchanged traversal, string means a destination, None means no
copy. Separate maps retain timebase/context, type-change callbacks,
postprocessing callbacks, whole-IDS callbacks, and warnings to suppress
(`P:54–103`). A `path` dictionary alone is not Python's conversion program.
For example, same-spelling `coordinates_type` can be absent from `path` while
having a type callback; same-spelling `psi` can be absent from `path` while
having a sign-flip callback.

`_check_data_type` compares parsed endpoint types, **not** the
`change_nbc_previous_type` attribute (`P:150–204`). It supports structure↔AoS,
0D↔1D of matching primitive families, INT_1D→identifier if the new field has
`doc_identifier`, and integral float↔integer scalar. An unsupported type
records `path=None` and `type_change=None`. Type changes beneath propagated
renames are not independently checked by `_add_rename`, and later common-path
checking skips already mapped names; Rust should explicitly classify each
endpoint pair rather than reproduce that omission (`P:311–315`, `:355–361`).

Unknown NBC descriptions log an error and are ignored (`P:307–309`); this can
leave same-spelling fields copied unchanged. That is expressly incompatible
with ADR 0027's unsupported-change handling. Python's None conflates dropped
data/absence and unsupported behavior unless `type_change` is also examined.
The shim must preserve established absence, unresolved conversion,
unsupported transformation and retrieval failure separately.

During execution `_get_target_item` logs a once-per-path warning for None
and skips it; a mapped string is followed with `IDSPath.goto`; default lookup
uses the source node's name. If a missing parent structure encloses renamed
children, default lookup can return the current target and keep descending
(`P:681–718`). `_copy_structure` visits nonempty source children, applies
type callbacks, copies/resizes recursively, then postprocesses (`P:721–765`).
This is whole-object copying, **not** the shim's candidate read/write/delete
policy and not a specification for it.

### Coexistence is a hardcoded recovery in Python

For DD3.42+→DD4, the normal generic pass skips the 3.42 rename metadata, since
that NBC date is no later than the older endpoint. The new spelling exists
at both endpoints and remains implicit identity; the retired spelling is
initially None. `_apply_3to4_conversion` explicitly loads **DD3.41.0**,
builds its direct map to the new endpoint, and uses that map to replace
old-side None entries and add mapped descendants (`P:399–423`). It does not
add the same alias on the reverse side and does not reconstruct generic
compatibility periods for all major versions. Only `.path` entries are copied
by this recovery, not tbp/context or callback maps.

Measured `time_slice/constraints/j_tor` / `j_phi`:

| Endpoints | Old inventory | New inventory | Python old→new | Python new→old |
|---|---|---|---|---|
| 3.39.0, 4.1.1 | j_tor only | j_phi only | j_tor→j_phi | j_phi→j_tor |
| 3.42.0, 4.1.1 | both | j_phi only | j_tor→j_phi; j_phi implicit identity | j_phi implicit identity |

The newer field carries `aos_renamed`, date `3.42.0`, previous name `j_tor`.
Thus a Rust graph compiler can derive the shim's valid 1:1 versus 2:1 rule
from correspondence plus **exact endpoint membership**, without copying
Python's hardcoded 3.41 factory dependency.

Python's tests explicitly assert `j_phi` wins when both are populated and
call it a "happy accident" of DD attribute order (`T:538–584`), not an
ordered-candidate field of the mapping. `iter_nonempty_` follows metadata
children order (`imas/ids_structure.py:260–265`), which originates in XML
field order (`imas/ids_metadata.py:248–254`); `_copy_structure` later writes
overwrite earlier ones (`P:737–763`). The shim already has explicit
successor-first precedence; use that existing convention, not XML order.

### Multiple names, nested exceptions and intermediate structure removal

Measured pulse_schedule's `ec/beam` in DD3.40.0 has:

```
change_nbc_description = aos_renamed
change_nbc_version = 3.26.0,3.40.0
change_nbc_previous_name = antenna,launcher
```

For 3.25.0→3.40.0 the first later event selects antenna, producing
`ec/antenna ↔ ec/beam`. For 3.30.0→3.40.0 it selects launcher, producing
`ec/launcher ↔ ec/beam`. No intermediate DD3.26 XML is loaded. `N:287–320`
tests every pair among 3.25, 3.30, 3.39 and 3.40, including netCDF reads.

The nested new `ec/beam/steering_angle_pol` has NBC previous name
`launching_angle_pol`, date 3.26.0. For the early pair, its provisional old
path `ec/beam/launching_angle_pol` is rewritten using the mapped parent to
`ec/antenna/launching_angle_pol`; that is the explicit child exception to
simple suffix propagation. For the later pair the child event is ignored
(already before 3.30), and propagated identity suffix yields
`ec/launcher/steering_angle_pol ↔ ec/beam/steering_angle_pol`. Both results
were measured. `N:112–136` is the parent-rename test, but despite its docstring
mentioning angle structures it populates/asserts only AoS names; the measured
map probe adds actual evidence for this child exception.

If a KG build flattens both antenna and launcher to beam and drops event
dates, it loses the **order** antenna→launcher→beam, the interval when each
name is valid and which child transformation predates a selected endpoint.
Endpoint presence plus common successor may still identify a family, but
cannot in general prove transition chronology or value behavior. Preserve
dated NBC lists where present and do not mistake a flattened edge for an
adjacent event. Whether the selected KG retains these particular lists must
be queried separately.

Measured neutron_diagnostic 3.40.1→3.41.0 provides another real nested case:
`synthetic_signals` disappears, while
`synthetic_signals/fusion_power→fusion_power` and
`synthetic_signals/total_neutron_flux→neutron_flux_total`. The parent is
deliberately omitted from the None map so execution can descend to mapped
leaves (`P:474–494`, `:712–718`). This prevents a blanket removed-subtree
rule swallowing valid escaped children. This is a useful second-IDS example
if graph facts are available; pulse_schedule's two-pair example tests more
of the requested historical algorithm.

### DD metadata versus special tables and callbacks

| Behavior | Owner/input | Shim implication |
|---|---|---|
| Explicit rename history | New endpoint NBC description/date/previous-name lists and both endpoint inventories, P:238–277 | Suitable algorithm reference, with normalization and explicit endpoint gates |
| Type change | Parsed endpoint datatype; doc_identifier for one supported Python case, P:150–204 | Compile current shim retype refusal; do not port reshape callbacks |
| Unconditional contour point repetition | NBC description, P:280–283; callbacks P:996–1011 | Requires shape/value change unavailable in current shim |
| Conditional contour closure | NBC descriptions plus live closed sibling/child and geometry values, P:284–306, P:1014–1127 | Not a path rewrite; localized unsupported conversion |
| DD3→4 generic COCOS | OLD fields with labels psi_like/dodpsi_like, P:363–375 | Python does not evaluate general COCOS formulas; KG factor analysis is separate |
| Extra COCOS | `_3to4_sign_flip_paths`, 22 IDS table entries at P:769–873 | Lookup is Python code, not automatically persisted KG metadata |
| Magnetics flux workaround | Structure `flux_loop/flux` flips its `.data`, P:1130–1143 | Field-level label on a structure cannot be naively applied to its AoS/structure data pointer |
| Equilibrium contour_tree | Whole-IDS Python callback, P:378–383, P:1275–1339 | Depends on magnetic-axis/boundary values, makes new structures; out of scope |
| pf_active connections | IDS/path hardcode P:384–388; matrix conversion P:1146–1164 | Sign reversal is not this matrix transform |
| source→provenance | Hardcoded forward pseudo-rename plus callback, P:390–397, P:1180–1193 | A `path` string alone hides required allocation and conflict check |
| DD3.42 aliases | Explicit 3.41 factory fallback P:399–423 | Generalize through graph history; do not carry runtime Python dependency |
| Magnetics bpol_probe/method | Forward-only hardcodes P:427–432; method callback P:1342–1350 | A persisted rename edge may hide required field remapping |
| name/identifier→description/name | Sibling shape pattern, P:434–472; empty-identifier callback P:1167–1177 | Same-spelling name changes semantic role; graph rename family alone may be insufficient |
| pulse_schedule resampling | Outside map building, convert_ids branch P:616–630, helpers P:1196–1272 | Timebase merging/interpolation unavailable to shim; missing callback in flattened map is material |

The pulse_schedule branch's executed condition is old <3.40 and new >=3.40
with heterogeneous time (`P:617–621`); its helper comment says DD4, which is
less general than the code. Prefer the condition. Similarly, the
name/identifier block's comment promises to skip existing mappings, but the
executed body checks only sibling presence/index exclusion/target presence,
then unconditionally calls `_add_rename`; do not treat that comment as a
conflict guard (`P:441–472`).

COCOS uses dictionaries keyed by source path, so duplicate generic/table
entries overwrite the same callback rather than flip twice (`P:369–375`).
The newer-side callback key is the old-to-new resolved destination. This
provides a concrete deduplication requirement for Rust's transform attachment.
The hardcoded equilibrium table covers boundary/psi, q_min/psi, ggd/psi/values
(`P:809–813`). Supported generic psi and dpressure_dpsi flips and roundtrips
are tested at `T:353–394`; table path existence at `T:650–666`. COCOS is
gated by DD major versions in Python, **not** DDVersion.cocos values.

Same-spelling changes require special care: coordinate identifier, units or
type history and COCOS can invalidate identity; name/identifier sibling
migration can give the same spelling different roles. A connected-component
alias graph must not collapse those roles indiscriminately. A flattened
path dictionary also cannot prove whether a string→structure edge was a
true rename or a pseudo-rename coupled to a callback. Endpoint types and
other recorded changes must retain refusal semantics.

### Executed evidence

Used existing `/Users/yohann/Documents/Dev/ITER/IMAS-Python/.venv/bin/python`;
`imas.__file__` resolved to that checkout. Read-only map probe retained at
`/private/tmp/imas-python-map-probe.py`. It exercised equilibrium
3.39/3.42→4.1.1, pulse_schedule 3.25/3.30→3.40 and neutron_diagnostic
3.40.1→3.41. Source data confirms each endpoint/member and outputs above.
For coordinates_type the path entry is omitted with callback
`_type_changed_to_identifier` both directions; psi has omitted path with
`_cocos_change` both directions. The old b_average alias maps to None, so
Python does not silently solve that missing-KG alias case.

Targeted tests were executed with `PYTHONDONTWRITEBYTECODE=1`, pytest's cache
provider disabled, no source edits:

```
test_ids_convert.py -k 'cocos_change or migrate_deprecated_fields or ggd_space_identifier or name_identifier'
6 passed, 103 deselected in 1.15s

test_nbc_change.py -k 'multi_rename or change_aos_renamed or change_leaf_renamed'
3 passed, 593 deselected in 1.35s
```

These validate the cited Python behavior only, not Rust map construction or
KG factual correctness. No full suite, Rust compiler or end-to-end shim
experiment was run by this investigator.

## Appendix B: released producer contract

Read-only investigation, 2026-09-17. Release source `ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e` (R) was inspected with `git show`; current checkout `abdca7bd4ebdb2aeac6052383f0c13c691eaa7e4` (C) was not changed. No graph queries were run by this investigator. Source links below fix R unless explicitly marked C. The downloaded graph's export revision is evidence about its exporter, **not proof of the revision/environment that originally populated every record**.

### Material correction to the earlier source report

The released builder **does not suppress subsequent `path_added` events**. It persists each event as `path:field:version`; additions after removal therefore have distinct IDs. Introduction edges deliberately remain first-only, and every removal gets a `DEPRECATED_IN` edge. A presence algorithm for this release can replay the full addition/removal event ledger, augmented with `path_renamed` events (the detector excludes matching additions/removals from separate events). In contrast, the newer checkout adds a filter suppressing `path_added` once one already exists. Do not attribute that newer defect to the released source. This is a producer capability, not an assertion that all graph histories are complete or that the snapshot was rebuilt from scratch using R. [R event persistence](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L3281), [R introduction/removal edges](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L3202), [C event suppression](https://github.com/iterorganization/imas-codex/blob/abdca7bd4ebdb2aeac6052383f0c13c691eaa7e4/imas_codex/graph/build_dd.py#L3892).

### What the Python map exporter loses

`scripts/build_path_map.py` is unchanged between R and C. It selects all source versions lexically less than one target, calls `dd_version_map_from_factories` independently for each source/target/IDS, and reads **only** `version_map.old_to_new.path`. Consequently it drops the independent Python reverse mapping, type-conversion handlers, post-processing handlers, ignored paths, and the per-pair applicability of each map. Equal path spellings are skipped; the first encountered old path wins for all sources. A `None` destination survives JSON but produces no edge. Therefore neither absence of an edge nor a plain same-spelling path proves no transformation. [Exporter loop](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/scripts/build_path_map.py#L153).

JSON includes `new_path`, `deprecated_in`, `last_valid_version`, but the latter is actually the **first accepted source version**, not the last version in which the path exists. Deprecation is searched forward from that source. JSON `new_to_old` is synthesized by inversion, not taken from Python's `version_map.new_to_old`. The graph importer retains only old/new endpoints, discarding both JSON dates and file metadata. Both endpoint nodes must already exist. Missing JSON yields an empty map rather than failure. Edges are thus unversioned source-to-target correspondences, not adjacent transition records or Python's full conversion program. [JSON construction](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/scripts/build_path_map.py#L177), [JSON load](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L1532), [edge import](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L3368).

### Metadata is neither uniformly first nor uniformly current

The release walks extracted versions and inserts/upserts nodes for the added set. Persisted type, rank, documentation and related ordinary metadata therefore come from first appearance **or the most recent reappearance**, because readdition uses the same upsert. Introduction remains the first-existing edge (R does not correct it to the chronological minimum as C does). Paths missing from an extraction are diffed as absent, and failed version extraction is logged and skipped; the next successful version is compared with the previous successful one. Exact-history completeness is not certified by the existence of DDVersion nodes. [Extraction/build](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L1686), [upsert](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L2962).

A final pass updates selected properties for latest-present paths: NBC version/description/previous-name/previous-type, expression, lifecycle, timebase, coordinate aliases and others. It writes null for missing members when another member triggers the update. Removed nodes do not receive that pass. Event comparison tracks units, documentation, type, node type, COCOS label, lifecycle, coordinates, timebase, max occurrence, rank and identifier enum; it **does not track NBC attributes or COCOS expressions**. Consequently historical changes to these declarations cannot generally be recovered from the graph's one property value. `structure_changed` conflates rank and identifier-enum event types; the event ID retains their original field name. Old/new event values are strings, including Python-list representations of coordinates. [Final pass](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L1869), [comparison](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L1383).

Raw comma-separated NBC attributes are preserved as strings; they are not normalized into transition objects. The rename event detector tests the raw previous-name against IDS-prefixed removed paths, and only for newly added nodes. It does not resolve relative names or coexistence. Thus `path_renamed` events are an insufficient correspondence source even where node properties or exporter edges contain usable evidence. R has no `graph/dd_lifecycle.py` reconciliation/string-successor channel; C adds it. [Raw extraction](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L1097), [rename detector](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L1359), [C lifecycle helper](https://github.com/iterorganization/imas-codex/blob/abdca7bd4ebdb2aeac6052383f0c13c691eaa7e4/imas_codex/graph/dd_lifecycle.py).

### COCOS persistence and interpretation limits

R uses `cocos_label_transformation`, including in change-event names, not C's renamed `cocos_transformation_type`. It clears labels not present in the latest extracted DD, then backfills current paths from latest 3.x labels, first expression token matching `{..._like}`, then Python's hard-coded `_3to4_sign_flip_paths`. These sources are tagged `xml`, `inferred_forward`, `inferred_expression`, `inferred_sign_flip`; imported Python table paths become `psi_like`. Original expression text is separately stored. A first-token label cannot encode multiplication/division or leading minus in a compound expression; labels are class evidence, not an executable expression equivalent. Historical label events are generated before backfill and must not be mistaken for the history of the later inferred properties. [Refresh/backfill](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L897), [Python table helper](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/cocos/transforms.py#L12).

Version COCOS is read from XML and may remain null. R creates COCOS parameter nodes and HAS_COCOS links only for non-null conventions. The `ids/transforms.py:cocos_sign` helper is a separate calculator, not the Python DD conversion algorithm: it supports sign/normalization factors but silently returns one for unknown labels. That fallback should not certify unsupported labels as exact identity in Rust. [Version nodes](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L2540), [factor calculator](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/ids/transforms.py#L40).

### What source provenance does and does not establish

R's lockfile specifies `imas-python==2.2.0` and `imas-data-dictionaries==4.1.1` with distribution hashes. That identifies the reproducible environment intended by this checkout; it does not identify the installed packages that originally generated `path_mappings.json` or accumulated graph records. The exporter manifest records package version, current git commit/tag, export timestamp and format, not producer-package versions, DD XML hashes, map JSON hash or per-record ingestion revisions. The map JSON itself records target/source versions and timestamp but no Python dependency revision. These missing provenance inputs prevent claiming byte-for-byte reproduction of the graph from the local current Python checkout. [Locked Python](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/uv.lock#L1461), [locked DD](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/uv.lock#L1449), [export manifest](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/cli/graph/data.py#L441).

The exact released-source contract supports a richer presence replay than the previous current-source warning suggested. It still lacks a complete versioned Python conversion program: independent reverse mappings, transforms, ignored sets, historical NBC/expression updates, and per-record provenance are not persisted by this path-map channel. Rust must consume recoverable evidence, preserve unsupported/unresolved distinctions, and not infer that a flattened edge contains those discarded facts.
