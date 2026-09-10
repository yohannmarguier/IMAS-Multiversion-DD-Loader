# A merged/subtree arraystruct open serves a candidate instead of refusing

`fold-constraints-j`, `fold-ggd-j` and `fold-ggd-bfield` are `rel="merged"
subtree="yes"` rules whose anchor is itself an array of structures. Before
this ADR, `al_begin_arraystruct_action` refused every one of them —
"this path is served by several stored candidates, and only a data read can
try them in turn" — because `narrow_context_path` turned every
`Resolved::Plan` into that refusal, unconditionally. An arraystruct open
carries no data, so nothing in the shim could try the candidates the way
`al_read_data` does, and the refusal took the whole subtree with it: every
child leaf under the anchor, `constraints/j_phi/position/psi`'s COCOS rule
included, became unreachable through this shim (issue #178).

Resolved by issue #178, against the tree at `989ea54` (after #176 and #177,
which fixed a related but distinct cause: a merged *scalar* candidate that
could not advance past an absent leaf during `al_read_data` itself).

## The question this had to answer

Two honest answers were on the table: serve the anchor by picking a
candidate some way other than reading data, or confirm the refusal is
correct and change IMAS-Fortran's contract instead. This ADR takes the first
answer, on the strength of one precedent already in this codebase: ADR
0020's stamp-discovery probe already opens a real IMAS-Core context purely to
observe what is stored, when the seam itself has no other way to know. An
arraystruct open can do the same thing for a candidate's *presence*, provided
it respects the same hazard ADR 0020 and ADR 0017 both found: a context
without a real backend reader cannot be trusted to report absence honestly.

## Decisions

1. **A read-mode open tries each stored candidate against IMAS-Core, in
   declared precedence order, keeping the first that reports a populated
   array.** `open_first_populated_candidate`
   (`src/interpose/occurrence.rs`) calls `al_begin_arraystruct_action` once
   per candidate. A candidate whose open fails, or whose reported `size` is
   `0`, is closed (`al_end_action`, through the same call family) and the
   next candidate is tried. The **last** candidate is kept regardless of its
   own size: once every earlier candidate has come back both successful and
   empty, an empty array-of-structures is the honest answer for the whole
   rule, not a reason to refuse. A hard failure on every candidate returns
   the last failure's own status verbatim — a genuine backend error is
   Core's to word, not this seam's to reword.

2. **This is safe only under `READ_OP`, so only a `READ_OP` occurrence
   probes.** `ConversionRecord` gained `opened_read_op: bool`, set once at
   `record_root` from the caller's own `rwmode` and inherited unchanged by
   every child, exactly like `direction_to_stored`. IMAS-Core's HDF5 backend
   only guarantees a reader for a context opened `READ_OP` (ADR 0020); any
   other access mode reports "no reader" as indistinguishable from "no
   data", which would make a write silently prefer whichever candidate a
   missing reader made look emptiest. A non-`READ_OP` open therefore takes
   the declared primary (`precedence == 1`) without probing at all — exactly
   the policy `al_write_data` already applies to an ambiguous plan (ADR 0016
   decision 12), extended here to the seam that opens the context a write
   will later traverse.

   This is cheaper than replicating ADR 0020's probe-through-a-context-of-
   its-own pattern: unlike stamp discovery, which needs an answer *before*
   the caller's own context exists, an arraystruct open already *is* the
   context in question, so trying candidates through it directly costs one
   open/close pair per rejected candidate rather than a second pulse and a
   second occurrence.

3. **A successful child records which stored candidate it actually opened,
   not just its own HLI-DD spelling.** `ConversionRecord` gained
   `stored_path: String` — `resolved_path`'s counterpart in the stored DD's
   own spelling, empty for a root exactly like `resolved_path`, and fixed at
   `record_child` to whichever candidate's complete stored-DD path
   (`Candidate`/`ContextCandidate`'s `stored_dd_path`) the opening seam
   actually used.

   This was not optional bookkeeping. `stored_anchor` used to *re-derive* a
   context's stored anchor from the map on every relative argument, which
   only ever had one answer for a renamed/moved anchor. A merged anchor has
   no single map-derivable answer: the map alone cannot say whether IMAS-Core
   is holding `time_slice/constraints/j_phi` or `time_slice/constraints/j_tor`
   — only the seam that just opened it knows, and only because it tried.
   Without recording that choice, every relative read under a served
   merged-subtree anchor (`measured`, `position/psi`, ...) would immediately
   hit the same "several stored candidates" refusal the arraystruct open had
   just gotten past, at one call further down.

4. **A relative candidate that does not lie beneath the context's own fixed
   anchor is dropped from the plan, not treated as a refusal.** Once
   `arraystruct_ctx` is opened against `time_slice/constraints/j_tor`, a
   relative read for `measured` resolves `time_slice/constraints/j_phi/measured`
   as the HLI path (subtree selectors still match both aliases) and produces
   two candidates — one under `j_phi`, one under `j_tor` — from the *rule's*
   declared list. Only the second lies beneath this context's actual,
   already-fixed stored anchor; the first names a sibling group IMAS-Core
   never opened from here at all. `stored_c_path` returns `Ok(None)` for a
   relative path that does not strip against `record.stored_path`, and the
   plan-building step in `resolve()` filters those out rather than failing
   the whole collection — a candidate not reachable from this context is not
   a reason to refuse a candidate that is. Only if every candidate is
   filtered out does this refuse (a real anchor mismatch, not a plan the
   artifact ever intended to reach from here).

5. **A refused arraystruct open now retains a loss.** `contextual_refusal`
   (`src/interpose/refusal.rs`) — the one refusal path both arraystruct
   arguments (`path` and `timebase`) share — retains an `UNMAPPABLE` loss
   (`LossOperation::Read`) before formatting the refusal, mirroring
   `write.rs`'s `finish_write_refusal`. This was the one gap issue #173 left
   unaudited: a refused write and a refused delete both already reached the
   loss log and the loss log file; a refused context open reached neither.
   `LossOperation::Read` because every reachable refusal at this seam is
   refusing to *read* through the context that would have opened — there is
   no write in flight to misname it as.

## What this does not change

- `al_write_data`'s own ambiguous-plan policy (ADR 0016 decision 12) is
  unchanged; decision 2 above only extends its reasoning to arraystruct
  opens, it does not touch the leaf-write seam itself.
- A `timebase` candidate plan is unreached by the shipped artifact — no
  rule's timebase is `merged`/`split` — so `begin_arraystruct_action_impl`
  takes its declared primary without probing, regardless of `rwmode`. Decision
  2's probe is reserved for `path`, which is where issue #178's three rules
  actually need it.
- ADR 0021's shape survives: `ContextPathResolution` still has exactly the
  role ADR 0021 gave it, extended with a `Candidates` variant carrying
  `ContextCandidate { path, stored_dd_path }` in place of the old blanket
  refusal — the same "one `resolve`, one narrowing per seam" split, not a
  reopening of it. `Translated` grew the same `stored_dd_path` payload for
  the same reason: a renamed/moved anchor's child needs its own stored
  spelling remembered exactly as a merged one does, it simply never
  disagreed with what the map could already re-derive.

## Consequences

- `IMAS-Multiversion-DD-Loader/docs/HLI_INTEGRATION_CONTRACT.md`'s refusal
  table entry for "this path is served by several stored candidates..." now
  names only what still triggers it: a context open resolving to a
  `merged`/`split` plan whose candidates need a value transformation (an
  artifact this shim has never shipped), not a plain candidate plan.
- `al-fortran-test-shim-structural-rules` and
  `al-fortran-test-shim-cocos-rules` are expected green: the parent
  `time_slice/constraints/j_phi`, `time_slice/ggd/j_phi` and
  `time_slice/ggd/b_field_phi` subtrees open, and
  `cocos-j-phi-position-psi`'s COCOS rule becomes reachable underneath the
  first.
- `tests/shim/arraystruct_path_test.c` covers all four decisions above
  through the public C ABI: falling through an empty precedence-1 candidate,
  succeeding empty when every candidate is, taking the primary without
  probing under `WRITE_OP`, and the loss-log entry a refusal now leaves.
- Issue #139's on-disk delete hazard and the timebase-inheritance exposure
  recorded elsewhere in CLAUDE.md are unaffected: neither rule this ADR
  serves has a delete or timebase concern the shipped artifact reaches.

## Considered Options

- **Confirm the refusal is correct and ask IMAS-Fortran to change its
  contract instead.** Rejected: a working, if slower, path-conversion answer
  was available (decisions 1–2), so declaring the subtree permanently
  unservable would have been a shim limitation stated as a fact about the
  ABI.
- **Probe every access mode alike.** Rejected in decision 2: it is exactly
  the mistake ADR 0020 corrected for stamp discovery and ADR 0017 removed
  from delete's own fan-out probe — a write-mode open's "empty" is not
  evidence of absence.
- **Re-derive a merged anchor's stored spelling from the map on demand,
  matching the existing (unchanged) behaviour for a renamed anchor.**
  Rejected in decision 3: the map has no single answer for a merged anchor:
  which candidate IMAS-Core is holding is a runtime fact this open decided
  once, not a static property the map can restate.
