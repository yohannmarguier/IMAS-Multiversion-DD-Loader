# A write asserts a value, a delete asserts an absence

Where one HLI path resolves to several stored candidate paths, a write and a delete take **opposite** answers to what looks like the same question. A write writes only the precedence-1 candidate (ADR 0016 decision 4). A delete deletes **all** of them. This ADR exists because that asymmetry looks like an inconsistency and is not, and because a reader who assumes symmetry will "fix" one of the two. Issue #129 implements the safe single-path leaf case and its stamp guard; issue #130 adds candidate-plan fan-out and probes. Subtree handling remains deferred to the following ticket.

## Why the answers differ

The two operations assert different things about the candidates they do not touch.

A **write** asserts a value. Fanning out means writing the caller's value into a path the conversion-map artifact only *assumes* holds the same quantity. `split-psi-axis`'s own note hedges on exactly that assumption and names the fallback if it is wrong. So fan-out on a write fabricates data, and the shim cannot know it is wrong.

A **delete** asserts an absence. Fanning out means removing every path that could satisfy a later read of the same HLI path — which is exactly what the caller asked for. Deleting only precedence 1 would be actively harmful, not merely incomplete: the read path falls back through the candidate list in declared order, so a stale precedence-2 candidate left behind would be **served as live data** after a delete the caller was told had succeeded.

So the asymmetry is not a compromise between the two. Each direction is the faithful one for its own operation.

## Decisions

1. **A delete fans out over every resolved candidate. A write does not.** ADR 0016 decision 2's precedence-1-only refusal applies to delete as well: a delete addressed through a non-primary source refuses, the same as a write.

2. **A delete probes before it deletes, to manufacture the outcome the ABI does not give it.** `al_delete_data` has no not-found outcome — only `code == 0` and `code != 0` — so a fan-out delete would fail every time a precedence-2 candidate had simply never been written. Before deleting, the shim issues an *unconverted* `al_read_data` per resolved candidate and classifies the result through the existing read-outcome classifier (ADR 0012 decision 3), then deletes only the candidates that hold data.

   **What the probe's shape does not say (review finding P8, issue #138).** That probe is issued at one fixed shape — `DOUBLE_DATA`, rank 0, against a caller-owned `f64` — whatever the candidate's real DD type and rank are. Every candidate a fan-out can currently reach is FLT, so the datatype is right; the *rank* is not, and 8 of the ~10 reachable targets in the shipped artifact are FLT_1D or FLT_2D (`profiles_2d/b_field_*`, `profiles_1d/j_phi`, …). What real IMAS-Core answers when asked for a rank-0 read of stored array data is not recorded in `docs/IMAS-CORE_FUNCTIONALITY_INVENTORY.md` and is therefore not known here. If it answers not-found, a present candidate is skipped and the delete returns `code == 0` having left data behind, which is exactly what ADR 0016 decision 1 forbids.

   This is deliberately not fixed here, and the reason is not cost. The probe cannot reach IMAS-Core meaningfully at all until issue #138 lands — the caller's write-mode context has no reader group, so every candidate already reports not-found for an unrelated reason — so any fix would be unverifiable, and the recording stub is shape-blind (`compute_read_response` overwrites `*data` regardless of `dim`), so it cannot witness the difference either. Nor is there a rank source to fix it *with*: the conversion artifact carries no DD types, and ADR 0013's inventories are bare path lists. Issue #138's own notes name this same unsoundness; it belongs to that work, together with the option #138 raises of dropping the probe entirely.

   This invents no machinery. `read_data_unconverted` already exists for version-stamp discovery, the classifier already owns the three-way outcome, and the probe enters the reentry counter (ADR 0014) like every other internal read.

3. **A partial failure attempts every candidate and then returns the failure.** There is no rollback in either direction, so stopping at the first failure only leaves more data behind than continuing does. The caller gets the failure; the log gets nothing, because a delete that reports failure has told the caller everything the shim knows.

4. **A subtree delete refuses when an escaping rule exists.** An **escaping rule** is a rule whose HLI-side selector is at or under the requested path `P`, but at least one of whose stored-side targets lies outside the resolved stored subtree `S`. The delete is trivial — and proceeds — precisely when every rule with a selector at or under `P` has all of its stored targets at or under `S`.

   Per rule kind: `moved` is the only kind that escapes in the supplied artifact; `renamed` escapes only if it crosses the boundary, at which point it is a move in all but name; `merged` and `split` escape only if a candidate lands outside `S`; `left_only` and `right_only` never escape; `retyped` refuses anyway on shape.

   **Leaf deletes are always trivial**, so this only ever bites a structure path. On the supplied artifact it refuses `time_slice/boundary_separatrix` for a DD3 HLI and `time_slice/boundary` for a DD4 HLI, and allows `time_slice`, `time_slice/constraints`, and every leaf.

   The check is deliberately conservative in one place: it runs *before* resolution, so a `merged` candidate landing outside `S` refuses even where a fan-out would have worked. Refusing a delete that would have succeeded costs the caller one retry with a narrower path; permitting one that silently leaves a subtree half-deleted costs them the occurrence.

5. **The empty path forwards unchanged.** `al_delete_data` accepts an empty path meaning "delete the whole DATAOBJECT". It needs no translation, no rule matches it, and afterwards the occurrence is unstamped — so ADR 0007 makes it match the HLI on the next open. That is the migration ADR 0016 held out of scope, arriving legitimately and by the caller's explicit request rather than by the shim guessing.

   This is also the one exception to ADR 0016 decision 6, which otherwise refuses any delete covering the DD-version stamp. The hazard there is the stamp removed *while the data remains*; the empty path removes the data too, so nothing is left to misread.

## Considered Options

- **Fan out on both, for symmetry** — rejected. It fabricates data on the write path, and the artifact itself flags the assumption it would rest on.
- **Precedence-1 only on both, for symmetry** — rejected, and the worse of the two symmetric options. It leaves a stale candidate that the read path's own fallback then serves as live data after a successful delete.
- **Delete without probing, and treat a failure on a never-written candidate as success** — rejected. It cannot distinguish that case from a real backend failure, so it would swallow genuine errors to work around a missing outcome.
- **Track which candidates were written, to avoid the probe** — rejected. That is per-occurrence state the shim does not keep, it would have to survive across processes to be correct, and ADR 0003 confines the registry to live contexts.
- **Allow a subtree delete and translate each rule's targets individually** — rejected for now. It is the more capable answer, but it makes one caller request into an unbounded set of deletes across disjoint subtrees, with no way to report which parts happened.

## Consequences

- **A delete is the only seam that reads before it writes.** The probe means a delete's cost scales with the number of candidates, and it can fail for a reason that is a read failure rather than a delete failure. The refusal message must say which.
- **"Escaping rule" enters the vocabulary** and is recorded in `CONTEXT.md`. The term is needed because the predicate is not "does this rule apply" but "does this rule's *target* leave the subtree the caller named".
- **The delete seam never writes to the loss log.** A fan-out is faithful, a probe-and-skip is faithful, and a partial failure is reported through `al_status_t`. Only the write seam logs, and only for ADR 0016 decision 4.
- **A caller who wants a mismatched occurrence migrated has exactly one legitimate route**: delete the whole DATAOBJECT and write it fresh. That is a data-losing operation they must ask for explicitly, which is the right shape for it.
