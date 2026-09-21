# Runtime-map correctness repairs — #244

The repair branch starts from `feb843e`, which contains the #232 measurement
commit `a1bc5aa` and the #233 graph-only production cutover. It preserves the
single runtime-map interpreter, existing resolver and C ABI operation policies.

## Finding-to-regression handoff

| Finding | Checked-in regression | Repair commit |
| --- | --- | --- |
| F1 | `acquisition_withholds_leaf_certification_from_a_leaf_with_a_descendant` | `994e90b` |
| F4 | `timed_out_bolt_queries_close_the_socket_and_leave_no_blocked_worker` | `fb71c01` |
| F5 | `terminal_selection_rechecks_expiry_after_a_success_was_ready_to_publish`; `publication_completion_includes_cache_admission_and_rolls_back_expired_success` | `cea5d8e` |
| F6 | `a_conflicting_child_declaration_cannot_inherit_its_parent_correspondence` | `d3a85f4` |
| F7 | `acquisition_localizes_present_and_unanchored_coexistence_without_panicking` | `0bccaa6` |
| F8 | `coexistence_candidates_survive_when_the_declaration_predates_both_endpoints`; `an_older_declaration_does_not_cross_a_removal_and_reappearance_boundary` | `341bdfc` |

Each test was first run against its unrepaired slice: F1 certified the
contradictory ancestor as a leaf; F4 kept the fake peer's socket open; F5
admitted a success after the controlled clock expired; F6 resolved the child
through its parent; F7 panicked at the coexistence invariant; and F8 returned
no candidates at 3.42.1. Each then passed with its corresponding repair.

## Publication and cancellation

The coordinator always acquires its state mutex before an attempt's result
mutex. While both are held, it rechecks expiry, selects the sole terminal
result, removes the in-flight attempt, tentatively admits successful maps, and
observes publication completion. Completion-time expiry removes that tentative
map before replacing the selected result with the shared timeout and waking
waiters. A waiter that expires first selects the same terminal timeout; a late
leader can only remove the in-flight entry and return that already-selected
result. Failed attempts are not retained, so a later open may make one new
bounded attempt.

The production transport no longer runs a synchronous driver in a detached
thread. One current-thread async runtime owns the Bolt connection/query/result
future, bounded by the remaining whole-attempt duration. Timeout cancels that
future and drops the sole graph/pool owner. The localhost Bolt regression
stalls after handshake and HELLO, observes the connection close, joins the
peer, and repeats the complete sequence three times.

## S1 disposition

S1 is explicitly deferred. The five metadata names occur at distinct trust
boundaries: graph-event validation, historical replay, typed metadata access,
typed mutation and wire-scope validation. A shared enum could reduce spelling
repetition, but it would couple those deliberately different accepted subsets
without fixing a demonstrated correctness defect. Folding that refactor into
the six behavioral repairs would enlarge their review surface. If pursued, it
should be a separate behavior-preserving change with boundary-specific tests;
this deferral is not an unresolved runtime-map correctness failure.

## Scope

The repair adds no public control surface, fallback, guessed alias or second
interpreter. Existing controlled C ABI coexistence scenarios remain the oracle
for read fallback, primary-only writes and skipped-path loss, and delete
fan-out/failure aggregation. Live Neo4j, real Core and HLI combined validation
remains the #246 integration stream's responsibility; this handoff does not
declare parent #205 complete.

The combined runtime revision handed to #246 is `fb71c01` on
`feat/runtime-conversion-mapping-issue-244`; it contains all six behavioral
repair commits. Later commits on that branch are handoff documentation only.

## Verification

Executed in `/private/tmp/imas-mvdd-issue-244`:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets` — 303 passed, 3 ignored live-service tests
- `NEO4J_URI=bolt://127.0.0.1:17688 NEO4J_USERNAME=neo4j NEO4J_PASSWORD=... NEO4J_DATABASE=neo4j cargo test --release pinned_graph_returns_complete_reference_scopes --lib -- --ignored --nocapture` — 1 passed, acquiring all six directional maps from the pinned Neo4j 2026 service
- `cmake -S . -B /private/tmp/imas-mvdd-issue-244-build -DCMAKE_BUILD_TYPE=Debug -DIMAS_MVDD_REAL_CORE_TESTS=OFF` — 262 tests registered
- `cmake --build /private/tmp/imas-mvdd-issue-244-build -j2`
- `ctest --test-dir /private/tmp/imas-mvdd-issue-244-build --output-on-failure --no-tests=error -j2` — 262 passed, including 50 `graph-runtime-map` tests

The first controlled CTest run exposed that rejecting every contradictory
endpoint hierarchy also rejected a hierarchy in the approved XML inventory.
The final F1 repair therefore takes the permitted conservative route: the map
remains loadable, but a leaf-labelled node with a descendant is not certified
as a leaf delete. The shared C harness proves that unsafe case refuses before
Core and that an unambiguous coexistence leaf still fans out.

The live graph completeness check was executed because a compatible pinned
service was already running. The two other ignored checks (measurement output
and graph-shutdown lifecycle), real-Core matrix and HLI matrices were not
executed here. Those remaining combined checks belong to #246 as specified by
#244.
