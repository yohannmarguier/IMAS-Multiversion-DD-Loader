# KG-to-Rust map prototype: post-prototype decision review

Reviewed 2026-09-17 through grill-with-docs. This records the completed
experiment's implications and the subsequent user decisions, not an
implementation or published specification. No prototype code is reusable.

## Authority and preserved scope

[ADR 0027](adr/0027-neo4j-runtime-queries-for-the-prototype.md) remains the
decision record. Preserve its all-KG-covered-IDS/version objective, complete
first-open map, exact endpoint candidates, immutable snapshot, process-life
cache, shared construction, retries and configurable five-second whole-attempt
deadline. Preserve its real-Core, installed-HLI, setup and CI completion
criteria. The three experimental pairs are samples, not the feature's scope.
The ADR still does not settle a general production deployment model.

Evidence read for this review:

- [Original grilling handoff](/private/tmp/imas-mvdd-kg-grilling-handoff.md),
  historical; its pending granularity question was subsequently settled.
- [Research handoff and preservation audit](/private/tmp/imas-mvdd-rust-map-research-handoff.md).
- [Completed prototype results](/private/tmp/imas-mvdd-rust-map-prototype-results.md)
  and [retained measurements](/private/tmp/imas-mvdd-rust-map-prototype-evidence.json).
- [Algorithm research, including both appendices](KG_RUST_MAP_ALGORITHM_RESEARCH.md)
  and [snapshot/query research](KG_CONVERSION_QUERY_RESEARCH.md).
- Current repository instructions, [glossary](../CONTEXT.md), relevant ADRs
  and the existing interpreter, operation policy and map-acquisition source.

Earlier research statements that no Rust experiment had run are historical.
The completed experiment supplies that evidence; it did not test production
cache/concurrency, real-Core operations or installed HLI integration.

## Decisions closed with the user

The final three sections of ADR 0027 record the accepted decisions in full:

1. Missing historical COCOS stays unknown. Do not adopt the historical 11/17
   fallback or retain the experimental numeric zero. Refuse conversions whose
   required factor cannot be established; independently supported paths remain
   usable under the existing localization rule.
2. Proven removal plus an exhausted correspondence search remains unresolved.
   It does not establish absence of a counterpart. Apply the same threshold
   to removed children beneath a proven parent move and to other removals.
3. Derive fidelity per requested path. An independently exact surviving field
   does not inherit loss solely from a different child. Changes limited to
   the resulting loss report are accepted. Populate the existing interpreter
   through a new module and adapter, allowing necessary construction and
   integration edits while preserving execution semantics.

These decisions do not approve every experimental classification. In
particular, prototype absence totals and blanket unit refusals are not
acceptance targets. Existing XML behavior remains a regression fixture;
graph-derived discrepancies must be documented and tested explicitly.

## Factual evidence gaps, not preference questions

### Absence and unresolved conversions

The released producer computes removal from inventory differences, while its
successor importer creates edges only for nonempty destinations. It does not
preserve the reason for an exported no-copy result. The producer contract
therefore does not turn an unsuccessful mapping search into a positive
no-counterpart declaration. See algorithm research Appendix B and
[released builder](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py).

The prototype's `gap/identifier` NoSource result needs the same evidence review
as the 586/632 removed-without-established-correspondence groups. A removed
spelling, a missing expected child and a missing successor do not suffice by
themselves under the newly accepted threshold. This review establishes no
generic positive absence certificate for this snapshot. Do not force a
nonzero absence count or invent correspondence/absence declarations.

### Coordinate evidence

The release creates HAS_COORDINATE relationships during batched node
insertion. Its target lookup uses MATCH, so a relationship is not created
when the target is unavailable at that step. The relationships are not
versioned, and index notation may be stripped. The exact source is
`_batch_create_path_nodes`, steps 5 and the surrounding batch loop, in the
[released builder](https://github.com/iterorganization/imas-codex/blob/ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e/imas_codex/graph/build_dd.py#L2962).

Consequently, an absent relationship is not proof that the DD field has no
coordinate. This source finding identifies a possible loss mechanism; it
does not establish the actual cause of the seven measured pulse_schedule
mismatches. Nor do unequal relationship sets alone prove resampling is needed.
Historical events, dimension information, timebase and established path
correspondences must be interpreted according to their own contracts. Equal
empty relationship sets are not automatically proof of equivalent coordinates.

Where equivalence cannot be established, retain a localized unresolved
conversion. Where evidence establishes a required unsupported transformation,
classify that separately as unsupported. If uncertainty cannot be localized,
the accepted construction-failure policy applies. No user choice to ignore
coordinates or add resampling is needed or authorized.

### Unit declarations and other semantic changes

The prototype compared unit declarations conservatively, including equivalent
spellings and sentinel resolution. The released producer distinguishes
cosmetic, sentinel-resolved, dimensionally compatible and incompatible changes
in `_units_changed`. Its dimensional-compatibility test does not establish
that numerical scale or offset is unchanged. See the same released builder
and the algorithm research's endpoint-metadata contract.

Determine the required value behavior from sufficient evidence. A proven
declaration-only change does not require a numerical transformation; an
actual unsupported unit transformation still refuses. Insufficient evidence
remains unresolved. Do not infer unchanged values from dimensional
compatibility alone, or adopt raw-string inequality as a permanent refusal
policy. The prototype's chi-squared refusals do not settle those specific
fields. This review has not resolved each unit discrepancy.

### Runtime unit-evidence handoff (#221)

The runtime-map boundary receives a producer classification with the unit
event; endpoint unit strings alone are never used to derive a numerical
factor. Its controlled source contract records these evidence-backed outcomes
in both map directions:

- `Cosmetic` (`m` to `metre`) and `SentinelResolved` (`s` to `second`) are
  declaration-only and resolve exactly.
- `DimensionallyCompatible` (`m` to `cm`) remains a localized `Unmappable`
  refusal: compatible dimensions do not prove factor one or zero offset.
- `RequiredScaleOrOffset` (`m` to `cm`) is a localized
  `UnitRedefinition` refusal because the numerical operation is known but not
  executable by this shim.

The query contract already retains `semantic_type` and `unit_change_subtype`
for a live source. No Neo4j executor or synthetic conversion factor is added
here; an adapter that cannot supply one of the classifications must leave the
path unresolved. The controlled examples are source-contract fixtures, not a
claim that any particular live DD path has that evidence.

Discarded callbacks, unrecorded historical metadata updates, coordinate-
convention documentation events and role reuse remain bounded evidence
limitations described in the research. They are not permission for identity
defaults, guessed aliases or a universal conversion claim.

## Existing contract versus prototype choices

- **NoSource and refusal differ.** The interpreter refuses Unmappable fidelity
  before resolving a side-only rule; an ordinary side-only rule produces
  NoSource. The read policy returns EMPTY/not-found for NoSource. Unclaimed
  reads also narrow to not-found, so every unresolved endpoint path must be
  explicitly claimed by a refusal rather than left unclaimed. This is an
  adapter/completeness obligation, not a request to change unknown-path policy.
- **Fidelity is declared input.** The XML's `move-gap` declaration currently
  produces a lossy `gap/r` read, explicitly asserted by the nested-read test.
  The accepted per-path graph declaration can change that report without
  changing the resolver or read loop. Exact field fidelity never certifies
  the containing subtree. Preserve the XML test and add the graph expectation.
- **Candidate precedence remains explicit.** Apply the accepted successor-first
  convention only where chronology establishes it, after endpoint filtering.
  The XML's manual psi-axis exception is not a requirement to reproduce that
  exception in graph-derived maps. Incomparable candidates remain unresolved.
- **Map contents can change scientific behavior.** Endpoint corrections,
  supported extra sign flips, explicit refusals and evidence-derived candidate
  declarations can differ from XML under ADR 0027. The user's agreement about
  loss-report-only changes does not reclassify every discrepancy as cosmetic.
- **Representation limits remain limits.** General many-to-many relations and
  incompatible candidate-specific transforms must be honestly localized as
  refusals, or construction must fail. Do not redesign execution to force a
  positive result.

Source checks: [map resolution](../src/conversion/conversion_map.rs),
[path narrowing and fidelity](../src/conversion/path_conversion.rs),
[operation loops](../src/conversion/seam_policy.rs),
[nested-read expectations](../tests/shim/nested_context_read_test.c),
[XML fixture declarations](3.39.0--4.1.1.xml), and ADRs 0004, 0006, 0008, 0012.

## Implementation details and specification follow-through

The adapter needs a validated typed construction interface shared with XML
fixture loading. Acquisition is fallible, bounded by one deadline, and retained
by `RuntimeMapCoordinator` outside the registry mutex; Neo4j work must stay out
of registry state management. Delete safety needs exact endpoint
structure/leaf information rather than an equilibrium-only fixture inventory.
Unknown COCOS needs an honest type.

These are necessary construction/integration changes, not new read/write/delete
policies. Driver, runtime, query batching, module layout and constructor type
details remain engineering choices. Experimental HTTP transport, page size 512
and exact-rule expansion are evidence, not mandatory implementation choices.

No additional policy choice was identified in the reviewed coordinate and
unit gaps: their safe outcomes follow from the accepted evidence/refusal rules.
Specification may describe these limitations without pretending the gaps are
resolved or freezing the experimental coverage counts. It must retain the full
integration acceptance criteria and the no-code-reuse constraint. No commit,
production implementation or upstream KG change was made by this review.

## Staged ready-map registration handoff (#208)

`ContextRegistry::record_root` accepts a ready `Arc<ConversionMap>` and only
records context state. It neither constructs maps nor waits for or performs
KG I/O. `RuntimeMapCoordinator` acquires and retains successful maps by exact
IDS/stored/HLI key before calling `record_root`, so live roots and children
share the ready map without making the registry a cache owner.

#216 supplies a complete or failed acquisition result to this same handoff:
it must acquire before registry mutation, pass a successful ready map into
root registration, and clean up an already-opened Core context on failure.
#215 replaces the legacy cache behavior with the accepted process-life,
bounded shared-attempt policy; this staged refactor does not impose that new
retention policy on artifact-backed maps.

### #208 verification and remaining boundary

In an isolated worktree from `feat/runtime-conversion-mapping`, the following
checks passed: `cargo fmt --check`; `git diff --check`; `cargo test
a_root_record_uses_the_callers_ready_shared_map --lib`; `cargo test
context_registry --lib`; `cargo test occurrence --lib`; `cargo clippy
--all-targets -- -D warnings`; and a Debug CMake build configured with
`-DIMAS_MVDD_REAL_CORE_TESTS=OFF`, followed by its 211-test `ctest
--output-on-failure` suite. That suite includes the occurrence-discovery,
plugin-family, and context-lifecycle C ABI regressions.

The validation build intentionally did not run real-IMAS-Core tests. Map
acquisition remains artifact-backed and weakly retained here; #215 supplies
the accepted process-life shared-attempt policy, and #216 supplies the
fallible KG acquisition/cleanup path.

## Typed-map construction handoff (#206)

`ConversionMap::from_typed(TypedConversionMap)` is the map-source boundary for
future KG acquisition. It accepts typed endpoint sides, rules, ordered
merge/split entries, established sign flips and unit-redefinition refusals,
then validates and builds the resolver's existing source indexes. Endpoint
COCOS is `Option<CocosConvention>`: `None` records unknown metadata and does
not create a transformation or prevent independently established paths.

`ConversionMap::load` now only decodes XML into that typed description before
calling the same constructor. XML keeps its required known COCOS attributes;
the historical fixture and interpreter contract remain unchanged.

## Endpoint delete classification handoff (#207)

Typed maps carry one endpoint inventory per map side: exact nodes classified
as `Leaf` or `Structure`, plus a completeness fact. A converted delete consults
the HLI-facing side selected by its map direction; only endpoint evidence an
acquisition adapter has validated as complete can certify an exact `Leaf` and
bypass the existing escaping-subtree check. Missing or incomplete metadata
therefore fails closed as a structure, while a trivial structure delete,
whole-DATAOBJECT delete, stamp protection and candidate fan-out retain their
existing policies. Contradictory classifications of the same exact endpoint
path reject typed-map construction.

The legacy equilibrium adapter seeds those map-owned inventories from the
checked-in 3.39.0 and 4.1.1 leaf inventories, preserving its XML-backed
runtime behavior. A KG acquisition adapter must supply complete, validated
endpoint datatype/hierarchy facts to certify leaves; this ticket neither
acquires KG facts nor selects maps at runtime.

## Specification follow-through

The user subsequently invoked to-spec and confirmed all three testing
interfaces: the existing public C ABI as the main acceptance interface,
existing ConversionMap construction/resolution tests, and one new
map-acquisition interface for controlled evidence, failure, cache, concurrency
and deadline tests. The resulting
[specification is issue #205](https://github.com/yohannmarguier/IMAS-Multiversion-DD-Loader/issues/205),
published with `ready-for-agent` on 2026-09-17. Its complete body and label were
read back and verified. Testing-interface confirmation is no longer pending;
implementation tickets and implementation remain subsequent work.
