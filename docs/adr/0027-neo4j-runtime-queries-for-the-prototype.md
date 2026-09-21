# Query local Neo4j at runtime in the conversion-map prototype

Accepted for the prototype on 2026-09-16. The prototype uses a local Neo4j
service loaded with a pinned DD-only graph snapshot from IMAS-Codex. Rust
queries that service when an IDS and DD-version pair needs a conversion map
that is not already available in memory, and constructs the map in memory
without generating an XML conversion-map file. This tests runtime graph
queries without requiring Python or an MCP client inside the shim.

A running Neo4j service is therefore a prototype dependency for an uncached
map. This accepts that operational cost in order to test the proposed graph
integration directly; it does not settle the production deployment model.
An approach that queries local data files without a database service remains
a separate deployment alternative.

The concrete snapshot identifier, setup implementation and map validation
remain undecided. This decision
does not establish that every existing XML rule can be derived from the
graph. ADR 0004 continues to describe the existing special-case artifact;
this prototype explores the future map construction left open by that ADR.

## Unavailable graph on an uncached mismatch

When an occurrence's DD-version stamp establishes a mismatch with the HLI
DD version and no conversion map is cached, an unavailable Neo4j service
causes the prototype to refuse the occurrence open. Map acquisition has a
bounded timeout, and the error identifies the IDS and both DD versions.
Already cached maps remain usable without contacting Neo4j.

Passing through unchanged in this case could read misleading values or write
under incorrect paths. For this prototype failure case, this decision
replaces the existing fallback described in ADR 0011 decision 1, where an
unsupported pair has no embedded artifact and passes through. A failed graph
query must remain distinguishable from a successful query that finds no
changes; the latter alone does not establish that an identity map is valid.
Handling incomplete or ambiguous graph evidence is a separate decision.

## DD evidence takes precedence over reproducing the XML

Each conversion derived from the graph must be justified by traceable DD
evidence. If that evidence does not justify an existing XML conversion, the
prototype may decline that conversion even when this reduces currently
supported behavior. The XML is a regression reference for investigating
discrepancies, not an acceptance criterion requiring its assumptions to be
reproduced. In particular, the existing `split-psi-axis` rule's manual
equivalence assumption is not itself sufficient evidence.

This applies ADR 0004's DD-supported-rule requirement to graph-derived maps.
The goal of covering all IDSs and DD versions permits explicit conversion
limitations; it does not promise that every field is convertible.

## Localized unsupported conversions and unchanged paths

An occurrence may open with explicitly unsupported conversions when the
affected paths can be identified reliably. Only operations that depend on
those unsupported conversions refuse; supported paths remain accessible.
If missing evidence prevents identifying which paths are safe, the open
refuses. As in ADR 0019, a later write refusal does not roll back earlier
writes.

An unchanged path is not an unsupported conversion. An empty change-query
result alone establishes neither identity conversion nor a conversion
failure. Map construction must distinguish a known path with no relevant
changes, an established absence in a DD version, unresolved conversion
evidence, and a query failure. Identical path spelling alone is also
insufficient: value transformations may still apply.

The queries needed to establish path presence and applicable changes remain
to be designed against the graph's contract. A reporting tool's empty or
truncated output must not be mistaken for the complete query result.

## Trust the KG as the DD authority

The KG is accepted as the authoritative source of DD facts. The shim project
will not independently validate graph snapshots against source DD versions
or test the correctness of the KG's contents. The proposed prerequisite of
checking snapshot inventories and history against source DD is rejected:
that duplicates the upstream responsibility for the graph.

This trust does not change the distinction between unchanged paths,
established absence, unresolved conversions and retrieval failures. The
shim must interpret each result according to the graph's actual contract.
Testing Rust query selection, version scoping, result handling and conversion
semantics remains the shim project's responsibility. In particular, an empty
change list can establish no recorded changes within the queried scope once
the query's applicability is established; it is not itself a refusal reason.

## One released graph snapshot per prototype run

Setup selects the latest released graph available at that time and treats
it as correct. The selected snapshot remains fixed throughout each HLI
process's lifetime, so cached and newly constructed maps use the same graph.
Updating the graph is an explicit setup action between runs, including when
a newer graph is needed to cover a new DD release. The prototype requires
no live refresh or cache invalidation for graph updates.

The concrete release selected is recorded when setup is performed. This
policy does not assume that upstream graph releases occur only when the DD
releases; it fixes which graph the prototype uses regardless of that schedule.

## Retain constructed maps for the process lifetime

The prototype retains each successfully constructed conversion map until
the HLI process exits, keyed by IDS name, stored DD version and HLI DD
version. Maps are constructed on demand; closing the last context using a
map does not evict it. Context records still disappear on successful close
as before.

This replaces ADR 0003's map-lifetime policy for the prototype. Repeated
opens of the same combination reuse the map without repeating Neo4j queries
and rule construction, and remain possible if Neo4j subsequently becomes
unavailable. The cost is retaining unused maps: memory grows with the distinct
combinations requested during the process. A bounded cache may be considered
later if measurements justify it; it is not part of this prototype decision.

## Store the graph outside Git and cache its archive in CI

The downloaded graph archive and loaded database remain outside Git, in
local storage ignored by the repository where applicable. Git tracks the
selected release reference and immutable digest, the setup scripts, the
Neo4j version and CI configuration. The README documents initial setup,
starting Neo4j and explicitly updating the selected snapshot. Ordinary
compilation does not download the graph.

CI jobs exercising graph-backed conversion use the same acquisition and
loading procedure. They restore the downloaded archive from an Actions
cache keyed by its exact graph digest. On a cache miss, they download the
selected archive from GHCR and save it for subsequent runs. Every fresh
job still loads the archive into Neo4j and starts the service. Cache eviction
must not prevent a clean run: the cache is an optimization, not the only
source of the graph.

Formatting and isolated tests do not require the live graph. Integration
and HLI conversion tests that require it fail if graph setup fails. These
tests check the shim's graph integration and conversion behavior, not the
correctness of the KG's contents.

## Generalize map construction within existing conversion capabilities

The prototype generalizes runtime map construction across IDSs and DD
versions represented in the selected graph. It preserves the current
engine's supported transformations and refusal policies: graph-derived
rules can use existing path conversion and COCOS sign flips, while retypes,
unit redefinitions and other transformations beyond those capabilities
remain unsupported. Learning a change from the graph does not itself make
that change executable.

New numerical or structural transformation mechanisms are a separate effort.
The integration replaces map population: Rust constructs the in-memory
equivalent of the artifact's rules and the existing interpreter applies
the project's read, write and delete conventions. It does not reopen those
operation policies. Manual artifact assumptions are set aside while deriving
the graph-backed rules.
This scope does not permit silently ignoring an unsupported change or
falling back to identity because the engine cannot execute it. The earlier
distinctions between identity, established absence, unresolved conversion
and retrieval failure still apply.

## Completion criteria and regression coverage

The existing equilibrium 3.39.0 ⇄ 4.1.1 tests are the regression baseline.
Tests of the XML loader and rule interpreter continue to exercise the
checked-in artifact. Existing behavioral scenarios must also exercise the
graph-derived maps through the shim, preserving their expected outcomes
where the graph supports the same conversions. Passing only artifact-backed
tests does not prove the new map construction works.

A discrepancy is investigated explicitly, not hidden by removing a test or
weakening its assertion. If the KG cannot justify an XML assumption, the
earlier evidence decision applies: preserve coverage of the interpreter's
mechanism, and document and test the graph-derived behavior separately.
Tests of policies deliberately changed by this ADR, such as map lifetime
and unavailable-graph handling, are updated to the accepted policy. The
artifact is a test fixture, not a runtime fallback for graph acquisition
failures.

Completion additionally requires:

- Graph-derived maps driving real-Core reads, writes and deletes in both
  conversion directions.
- Coverage beyond the equilibrium baseline, including another IDS and
  another version pair selected from graph-supported changes. The map
  builder must remain generic rather than special-case those examples.
- Tests distinguishing identity, established absence, unsupported
  transformations and query failures, and proving map reuse after all
  referencing contexts close.
- At least one real HLI conversion through the installed shim, documented
  graph setup and CI jobs that exercise the graph-backed integration.

These checks verify the shim implementation, not the KG's factual
correctness, and do not claim exhaustive testing of every IDS/version pair.

## Candidates must exist in the exact DD version they represent

Graph-derived maps include only candidates that exist in the exact DD
version of their side. A path's existence elsewhere in the DD lineage is
not sufficient. A valid conversion correspondence and candidate precedence
must still be established; presence alone does not make a path a counterpart.

This corrects a concrete defect in the existing equilibrium artifact's write
behavior. `fold-p2d-bphi` declares `time_slice/profiles_2d/b_field_phi` as its
primary 3.39.0 candidate, although the trusted KG reports that path was
introduced in 3.42.0. The recording-stub write scenario
`write-path-candidate-lands-at-primary-and-retains-unwritten-candidates`
explicitly expects that target under a 3.39.0 stamp. A permissive read plan
can try an absent candidate and fall through, but applying that same primary
to a write creates a field outside the stamped version.

That expectation is not a compatibility requirement for graph-derived maps.
The corrected tests must prove that an absent-version candidate is excluded
and cannot become a write target, while retaining coverage of primary-only
writes and skipped-candidate loss reporting using valid candidates.

## Construct a complete IDS map on the first cache miss

The first mismatched occurrence open for an uncached IDS, stored DD version
and HLI DD version waits for construction of the complete conversion map for
that combination. Completeness includes identifying localized unsupported
conversions; it does not require every path to be convertible. Later
operations use the retained map without resolving additional paths through
Neo4j.

This makes map availability independent of Neo4j after successful
construction and gives operations a stable conversion policy. It accepts
first-open latency and retaining the complete map in memory. Query batching
and parallel construction remain implementation choices to investigate
against the graph's query contract and measurements.

## Share concurrent construction for the same map key

Concurrent occurrence opens needing the same uncached IDS, stored DD version
and HLI DD version share one in-flight map construction. One request builds
the map while the others wait for its result. Different keys may build
independently. Database queries, map construction and waiting do not hold
the context-registry lock.

This prevents duplicate acquisition and construction for a shared cache miss
without making an unrelated occurrence wait behind another map's database
work.

## Retry after a failed construction

A failed construction delivers its failure to every request waiting on that
attempt, but is not retained in the process cache. A later occurrence open
may start a new bounded construction attempt for the same key. Only
successfully constructed maps are retained for the process lifetime.

This lets service recover after Neo4j becomes available again without
requiring an HLI restart. Waiters on a failed attempt do not automatically
retry within that same open; retries are triggered by later opens.

## Bound the whole construction attempt to five seconds by default

Each map acquisition and construction attempt has one configurable deadline,
defaulting to five seconds. It covers the whole attempt, including connection,
queries and in-memory construction; individual queries do not restart the
budget. Requests joining an in-flight attempt share its remaining time.

This bounds a stalled occurrence open against the local database. Five
seconds is an initial prototype default, not a measured performance claim;
measure complete-map construction before adjusting it. Expiration follows
the failed-attempt policy above, allowing a later open to retry.

## Missing historical COCOS remains unknown

Accepted during the post-prototype review on 2026-09-17. A missing DD-version
COCOS convention remains unknown. Do not substitute the Codex calculator's
version-based fallback (11 before DD 4.0.0, 17 thereafter), infer identity
from missing conventions, or use a numeric placeholder as a scientific
convention. The prototype's inert `0` was a disposable representation
workaround, not an accepted value.

When a conversion's required COCOS factor cannot be established because of
that missing information, classify the conversion as unresolved and refuse
the affected operation. Paths whose conversion is independently established
remain usable. Missing convention metadata alone does not refuse the entire
IDS when the uncertainty can be localized; the existing rule for uncertainty
whose scope cannot be localized still applies.

This chooses evidence-backed conversions over the additional coverage a
historical compatibility assumption could provide. It does not change
conversions whose required convention information is available, nor the
existing sign-flip or operation policies. The in-memory representation must
express unknown metadata honestly; its concrete type is an implementation
choice.

## Removal without an established counterpart remains unresolved

Accepted during the post-prototype review on 2026-09-17. Establishing that a
path disappeared, then exhausting the relevant KG correspondence evidence
without finding a replacement, is not sufficient to establish absence of a
counterpart. Classify that conversion as unresolved. A read of that path
refuses rather than returning EMPTY/not-found on the strength of an
unsuccessful correspondence search.

Established absence remains a distinct outcome when evidence justifies that
there is no counterpart. Apply the same evidence threshold to all paths,
including removed children under a proven parent move. The prototype's
`gap/identifier` absence and its broader unresolved-removed-path category
are experimental classifications to review under this criterion, not two
separate accepted policies. Its measured absence counts are not acceptance
targets for the implementation.

This accepts reduced conversion coverage rather than presenting an
undiscovered correspondence as missing data. It preserves the existing
interpreter's distinction between NoSource and refusal and the accepted
localization policy; it changes neither read/write/delete operation policy
nor the decision to trust KG facts without auditing them against source DD.

## Derive fidelity per requested path while preserving execution semantics

Accepted during the post-prototype review on 2026-09-17. Graph-derived maps
declare fidelity for the requested path's evidenced conversion. A surviving
field that converts exactly does not inherit a loss declaration solely
because a different child of its containing structure cannot be converted.
Each affected child receives its own supported, absent or refused outcome.
An exact field read does not certify that its entire subtree is convertible.

For example, the XML's `move-gap` subtree rule declares forward loss because
of `gap/identifier`, and consequently also reports a read of `gap/r` as lossy.
A graph-derived rule may declare the latter exact when its own path and value
conversion are established. The user accepts that this changes the loss log:
for an otherwise identical rule, the stored path, returned value and success
status remain the same, but that loss entry disappears. It does not authorize
changing candidate precedence, transformations or refusal outcomes merely to
improve fidelity. Existing conditional-loss semantics for merges and splits
remain unchanged.

The implementation follows a new graph-query and evidence-analysis module
with an adapter into the existing ConversionMap representation. Keep one
interpreter and preserve its matching, candidate execution, transformations
and read/write/delete policies. Equivalent maps must have equivalent
execution regardless of how they were populated. New map contents can still
produce the explicitly accepted evidence-driven differences from the XML.

Preserving execution semantics does not require leaving every existing file
untouched. Necessary construction and integration changes are allowed: a
validated typed construction interface shared with XML loading, occurrence
map acquisition and the already accepted cache/failure/deadline behavior,
honest unknown-COCOS metadata, and generalized endpoint information for
delete safety. These changes do not authorize a second interpreter or a
redesign of operation policy. Module layout and concrete types remain
implementation details.

Keep the XML fixture's interpreter tests and document graph-derived fidelity
differences in separate expectations. All implementation must be written
independently from the retained findings; no discarded prototype code is to
be recovered or reused.
