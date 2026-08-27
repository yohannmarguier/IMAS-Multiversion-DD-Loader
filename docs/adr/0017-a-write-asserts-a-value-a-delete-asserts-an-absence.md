# A write asserts a value, a delete asserts an absence

Where one HLI path resolves to several stored candidate paths, a write and a delete take **opposite** answers to what looks like the same question. A write writes only the precedence-1 candidate (ADR 0016 decision 4). A delete deletes **all** of them. This ADR exists because that asymmetry looks like an inconsistency and is not, and because a reader who assumes symmetry will "fix" one of the two. Issue #129 implements the safe single-path leaf case and its stamp guard; issue #130 adds candidate-plan fan-out. Subtree handling remains deferred to the following ticket.

## Why the answers differ

The two operations assert different things about the candidates they do not touch.

A **write** asserts a value. Fanning out means writing the caller's value into a path the conversion-map artifact only *assumes* holds the same quantity. `split-psi-axis`'s own note hedges on exactly that assumption and names the fallback if it is wrong. So fan-out on a write fabricates data, and the shim cannot know it is wrong.

A **delete** asserts an absence. Fanning out means removing every path that could satisfy a later read of the same HLI path — which is exactly what the caller asked for. Deleting only precedence 1 would be actively harmful, not merely incomplete: the read path falls back through the candidate list in declared order, so a stale precedence-2 candidate left behind would be **served as live data** after a delete the caller was told had succeeded.

So the asymmetry is not a compromise between the two. Each direction is the faithful one for its own operation.

## Decisions

1. **A delete fans out over every resolved candidate. A write does not.** ADR 0016 decision 2's precedence-1-only refusal applies to delete as well: a delete addressed through a non-primary source refuses, the same as a write.

2. **A delete fan-out deletes every candidate without a presence probe.** `al_delete_data` has no not-found outcome — only `code == 0` and `code != 0` — but an attempted delete is preferable to a fabricated successful no-op. The former probe read through the caller's context. Under real IMAS-Core's HDF5 `WRITE_OP` context, its reader group is absent, so every candidate looked absent, no delete was forwarded, and the shim returned `code == 0` for data it had not deleted (issue #138).

   Opening a shim-owned read context mid-delete is not safe either: closing it releases the pulse's per-IDS file handle while the caller's write context remains live. Nor can the old fixed `DOUBLE_DATA`, rank-0 scalar probe establish presence for arbitrary candidate shapes. The conversion-map artifact has neither a type nor a rank, so there is no sound probe shape to supply. The shim therefore calls `al_delete_data` for each candidate directly, retains the first nonzero result, and continues through later candidates as decision 3 requires. A missing candidate can consequently look like a backend failure, but that is an honest limitation of the ABI; reporting success after forwarding nothing is not.

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
- **Treat a failed delete of a never-written candidate as success** — rejected. The ABI cannot distinguish it from a real backend failure, so that would swallow genuine errors to compensate for its missing not-found outcome.
- **Track which candidates were written** — rejected. That is per-occurrence state the shim does not keep, it would have to survive across processes to be correct, and ADR 0003 confines the registry to live contexts.
- **Allow a subtree delete and translate each rule's targets individually** — rejected for now. It is the more capable answer, but it makes one caller request into an unbounded set of deletes across disjoint subtrees, with no way to report which parts happened.

## Consequences

- **A delete fan-out makes one delete call per candidate.** It can report a failure for an absent candidate because the ABI has no not-found result, but it never claims success without forwarding the candidate plan.
- **On the only backend that implements delete, the per-path fan-out has no per-path effect.** Real IMAS-Core's `HDF5Writer::deleteData` ignores its `path` argument and removes the whole IDS pulse file plus its master-file link, so the first candidate takes the occurrence with it and the rest find nothing. Decision 2's fan-out is still the right shape — it is the ABI contract this shim is written against, and a backend that honoured `path` would need it — but no test can observe a *per-candidate* deletion until the backend is fixed. This decision's removal of the presence probe is what made that reachable: the probe's silence used to stop the fan-out before Core, at the cost of the false `code == 0` this decision exists to eliminate. Tracked as issue #139, stated for users in README.md's "Scope and limitations", and pinned by `write-delete-oracle-reverse-delete-fan-out-reaches-disk` as behaviour of record rather than as desired behaviour.
- **"Escaping rule" enters the vocabulary** and is recorded in `CONTEXT.md`. The term is needed because the predicate is not "does this rule apply" but "does this rule's *target* leave the subtree the caller named".
- **The delete seam never writes to the loss log.** A fan-out is faithful, and a partial failure is reported through `al_status_t`. Only the write seam logs, and only for ADR 0016 decision 4.
- **A caller who wants a mismatched occurrence migrated has exactly one legitimate route**: delete the whole DATAOBJECT and write it fresh. That is a data-losing operation they must ask for explicitly, which is the right shape for it.
