# Write-path seam policy: best effort, and refuse rather than lie

ADR 0002 put writes out of scope with one line — *"if known versions differ, return failure without calling IMAS-Core"* — implemented as a blanket context-keyed refusal. This ADR replaces that line. It is a decision record only: no code implements it yet.

The governing rule is the caller's, stated as three cases:

> If we can do it safely, we do. If it is dangerous, we do not, and we log it. If it is impossible to write, we do not write, and we log it.

Everything below is that rule applied to one artifact and one ABI.

## What a write can and cannot be

A write only ever converts into an **existing, differently-stamped** IDS occurrence. A fresh occurrence carries no DD-version stamp, so ADR 0007 presumes it matches the HLI, no context record exists, and the seam already forwards. A plain full `put` into a new occurrence is untouched today and stays untouched.

So the workflow in scope is the **append or partial write**: an HLI adds to an occurrence that was stored under a different DD version. The stored DD version does not change, and the DD-version stamp is immutable (see below). **Migrating an occurrence to the HLI's DD version is out of scope.** That is a different operation with a different failure model — it rewrites the stamp, and it must either complete or leave the occurrence untouched, which this ABI cannot promise.

## Decisions

1. **A write is best effort, and every unsafe case refuses before IMAS-Core is called.** The shim never returns `code == 0` for data it did not store. The one exception is the sentinel case in ADR 0018, where IMAS-Core itself would not have stored anything either.

2. **Only a precedence-1 source may write.** Where several HLI-side spellings name one stored slot, a write through any non-primary source (`precedence != 1`) refuses. The supplied 3.39.0⇄4.1.1 artifact has 30 `<from>` sources across its 13 `merged` and 1 `split` rules; 16 are non-primary. Fifteen of those carry `deprecated="yes"` and one — `split-psi-axis`'s `time_slice/global_quantities/psi_magnetic_axis` — does not. Keying the refusal on precedence rather than on the deprecation marker covers all 16 with one predicate instead of a rule plus a special case, and leaves `deprecated="yes"` as corroboration rather than as the test.

   Collision *detection* is not available and is not attempted. Keying on the stored path string would false-fire on every `al_iterate_over_arraystruct` step, because the path is the same and only the element differs; the cursor lives in IMAS-Core and ADR 0002 keeps no AoS element state.

3. **Writing a path the stored DD version has no slot for refuses.** In this artifact that is the 13 `right_only` rules. Rejected: success plus a loss log entry. It is the one undetectable failure mode — `code == 0`, the data gone, and nothing obliged to look at the log.

4. **Where one HLI path names several stored slots, only the precedence-1 slot is written.** The remaining candidates are left as they are, and each earns a `POTENTIALLY_LOSSY` loss log entry. The read path already prefers precedence 1, so the round trip closes. Fan-out would fabricate data into a path the artifact only *assumes* holds the same quantity — `split-psi-axis`'s own note hedges on exactly that — and the artifact's "written to both" remark describes a whole-IDS converter, which the shim is not. Delete answers the same question the opposite way, for reasons ADR 0017 gives.

5. **The DD-version stamp is immutable under a mismatch.** A write to `ids_properties/version_put/data_dictionary` refuses whenever the stored DD version differs from the HLI DD version. The shim does **not** rewrite the value to the stored DD version.

   Rewriting was the obvious alternative and was rejected. It would need a `CHAR_DATA` value transformation the read path never needed, and a length-changing one: a DD 4.1.1 HLI writes `"4.1.1"` with `size[0] == 5` and the stored spelling `"3.39.0"` needs 6, so the shim would have to substitute both the buffer and the caller-owned `size` array. Refusing needs none of that, and it makes the stamp wrong only if the shim writes it — which it now never does.

   This is safe because of how the HLI writes the stamp, which is not what one would guess. `put_slice` never writes it at all: the generated `put_slice_struct_ids_version_dd_al` has an empty body, because the Fortran generator skips non-timed fields in slice mode and the stamp is `STR_0D` and non-dynamic. A full `put` always writes it, from a literal baked in at HLI build time, ignoring whatever the caller placed in the IDS structure. So the refusal never fires on the append workflow this ADR is for, and fires only on a full `put` into an already-stamped mismatched occurrence — which is a replace, i.e. the migration write held out of scope above. The refusal turns that into an early failure instead of a silently mixed-version occurrence.

   The siblings `version_put/access_layer` and `version_put/access_layer_language` describe the writing *library*, not the DD, and forward untouched.

6. **A delete that would remove the stamp refuses, with one exception.** The hazard is precisely *the stamp removed while the data remains*: the occurrence becomes unstamped with foreign-version data in it, the live context record goes stale, and the next open makes ADR 0007 read that data as the HLI's own version. So any non-empty path covering the stamp refuses — the leaf, `ids_properties/version_put`, `ids_properties`. The exception is the empty path, "delete the whole DATAOBJECT", which ADR 0017 keeps forwarding unchanged: it removes the data too, so the occurrence is genuinely empty afterwards and nothing is left to misread.

7. **A value transformation carries a direction, and a transformation that cannot be inverted refuses.** The read path could treat the one existing transformation as an involution, because a COCOS sign flip is its own inverse. A write must not inherit that. `ValueTransformation` gains an explicit `ToStored`/`ToHli` sense and an `inverse() -> Option`, and `None` refuses the write. The payoff is the `Option`: the shim can say it cannot invert a transformation instead of guessing. Under an involution assumption, the first unit rescale a future conversion-map generator emits would write ×1000 where it should write ÷1000, silently, with `code == 0`.

8. **Declared fidelity is a statement about reads. The write path derives its own verdict.** `fold-constraints-j` declares `forward="lossy"` with the reason *"both DD3 names may hold different values"* — that is a read ambiguity. Writing one DD3 name into one DD4 slot is one value into one slot, and is exact as a write. Copying the declared verdict onto a write would report loss that did not happen.

9. **The write-side refusals are hoisted, not folded into the shared guard.** `ConversionMap::refusal_before_resolution` stays shared and operation-agnostic: `Retyped` and `Unmappable` refuse for reads and writes alike, and should not grow an operation parameter to keep doing so — it would make the function answer "this rule does not concern you" four times out of six. Instead one `write_refusal_before_resolution` and one `delete_refusal_before_resolution` each call the shared guard first and then add their own rules.

   Order inside the write guard is **pinned by test, not left to the order of the match arms**: shared guard, then non-primary source (decision 2), then no stored slot (decision 3), then non-invertible transformation (decision 7, last, because it needs the resolved stored path to look the transformation up). Review finding P12 exists because this same check was once per-resolver and `resolve_merged`/`resolve_split` forgot it. Hoisting fixes one instance of that shape and creates two more guards a future resolver can bypass; the test on ordering is what keeps that visible.

10. **`timebase` inherits the read path wholesale.** `field` and `timebase` resolve independently, a refusal on either refuses the write, and both contribute to the fidelity verdict. In this artifact `time` is identity and no rule touches a timebase path, so there is nothing here to decide yet. The hazard is recorded as a limitation rather than guessed at: a write whose timebase resolves to a *different* candidate than the neighbours already in the occurrence would attach its value to a different time basis. Inventing a rule for an unreachable case would be uncovered code, which ADR 0011 forbids.

    **Note (issue #134).** This is the **one open forward exposure** the #122 series leaves behind. Every other decision here is either implemented and covered, or asserted unreachable by a test that fails when a future artifact makes it reachable (decision 12's shape). This one is neither: nothing fails when a timebase path acquires a rule, because there is no mechanism to notice. So the obligation is on the author of that artifact — the first conversion-map artifact whose rules touch a timebase path must reopen this decision rather than treat the silence above as a finding that timebase conversion is safe. It is a finding that the question has not been asked.

11. **`rwmode` on `al_begin_global_action` stays ignored *by the policy*.** A write-mode open of a new occurrence reads no stamp, so no record is created and the seam forwards; a write-mode open of an existing mismatched one registers and translates exactly as a read-mode open does. The stamp decides whether conversion applies, not the mode, and this ADR's append-only scope means a write-mode open never *creates* a mismatch, only inherits one.

    **Correction (ADR 0020).** The second half of that was false as written, against real IMAS-Core's HDF5 backend, for as long as discovery read the stamp through the caller's own context: that backend initializes a reader only under `READ_OP`, so the read came back not-found and every write through a write-mode open forwarded unconverted. The policy above was right and the mechanism underneath it was not. It holds now because ADR 0020 gives discovery a read-mode context of its own whenever the caller's access mode is not `READ_OP` — so `rwmode` is still not a policy input, but it *is* now an input to which context the stamp is read through.

12. **The write path emits one fidelity verdict, and the other is asserted unreachable.** Decision 4's unwritten candidate is `POTENTIALLY_LOSSY` — ADR 0008's unverified bucket, exactly as a `merged` read already uses it. Decision 2's refusal removed the only case that would have been certainly lossy, so **`LOSSY` has no write-side producer**. Per ADR 0011's standard and the precedent of review finding P7, that is asserted by a test that fails and tells the reader to add real coverage if a future artifact makes it reachable — not assumed to be in use.

## Two channels, and which one carries what

The refusal message and the loss log are not redundant, and the split is not arbitrary:

- **The refusal message** — code `-1000` plus the reason, DD path and both DD versions — carries every case where the shim declined to write. The caller sees it immediately.
- **The loss log** carries the cases where the write *succeeded*. Decision 4 is the only one: `code == 0` was returned, so there is no message to carry anything, and the log is the sole record. This is what justifies the log existing on the write path at all.

A refused write appears in both, which is redundant rather than wrong. ADR 0012 records why that redundancy is tolerated instead of removed.

## Considered Options

- **Keep ADR 0002's blanket refusal** — rejected, because it makes a mismatched occurrence permanently append-only, which is the whole capability being asked for.
- **Translate on the way down and let IMAS-Core sort out the rest** — rejected. The collisions in decision 2 have no correct answer at the ABI, and IMAS-Core cannot see that two writes it received named one quantity.
- **Success plus a loss log entry wherever a write cannot be served** — rejected, and this is the sharpest rejection in the ADR. It is the only failure mode that is invisible at the call site, and the loss log carries no obligation on any caller to read it.
- **Rewrite the DD-version stamp to keep it truthful** — rejected in decision 5, on machinery cost and on the observation that an immutable stamp cannot be made wrong by the shim.
- **Support the migration write now** — rejected as out of scope. It needs an all-or-nothing guarantee the ABI does not offer, and ADR 0017's empty-path delete already provides a legitimate way to reach a fresh occurrence.

## Consequences

- **ADR 0002's `al_write_data` / `al_delete_data` row is superseded**, and that ADR's title no longer describes its whole content. The file is not renamed: every cross-reference in `CLAUDE.md`, the ADRs and the source comments names `0002-read-path-seam-policy.md`, and the churn of renaming outweighs the inaccuracy of a title.
- **A DD 4.1.1 HLI cannot append its DD4-only fields to a DD 3.39.0 occurrence.** All 13 `right_only` paths sit under `time_slice`, the dynamic AoS, so decision 3 lands on `put_slice` and not only on a full `put`. It fires only for fields the caller actually filled, but `time_slice/profiles_1d/psi_norm` and `time_slice/boundary/phi` are ordinary things for a DD4 code to fill. ADR 0019 is what makes this survivable.
- **The refusal count on the write path is higher than on the read path**, by design. A read that cannot be served costs one field; a write that cannot be served, left to succeed, costs the caller's belief that the data is stored.
- **ADR 0007 needs a write-side paragraph.** Its argument for forwarding an unstamped occurrence is a read argument, and on the write path a wrong presumption permanently creates a mixed-version occurrence.
- **Proof needs a native oracle, not a round trip.** A shim round trip cannot verify a sign flip at all: the write flips HLI→stored, the read flips stored→HLI, and the caller's own value comes back whether or not the sign on disk is right. Only reading the file natively proves that the stored name holds the value, that the sign on disk is the stored convention, that the stamp still reads the stored version after a `put_slice`, and that the precedence-2 candidate was left empty. The round trip is a consistency check, and must be labelled as one so that nobody deletes the native oracle as redundant.
