# Handoff — code review findings, `feat/path-conversion` (spec #43 / tickets #44–#69)

**Written:** 2026-08-17. **Repo:** `/Users/yohann/Documents/Dev/ITER/IMAS-Multiversion-DD-Loader`
**Purpose:** a fresh agent should be able to (a) explain any finding below to the user, or (b) fix one
or a batch of them, without re-running the review.

---

## 1. What was reviewed

Two-axis review (`/code-review` skill: Standards + Spec, run as two independent parallel sub-agents,
deliberately **not** merged or cross-ranked).

- **Fixed point:** `main` = `5e54257`
- **Head:** `feat/path-conversion` = `f8764de` (merge of PR #94)
- **Diff:** `git diff main...feat/path-conversion` — 60 files, +17,318 / −182, 126 commits
- Working tree during review was `fix/pr94-scoped-passthrough` (`af32154`); its `main...HEAD` diff is
  **byte-identical** to `main...feat/path-conversion`, so either ref reproduces the review.

**Baseline state at review time (verified, do not re-derive):** `cargo fmt --check` clean;
`cargo test --lib` = 149 passed / 0 failed. Anything `cargo fmt`/`cargo clippy` enforces was
deliberately out of scope for both axes.

### Sources — reference, don't duplicate

| What | Where |
|---|---|
| Parent spec | GitHub issue #43 `Spec: read-path DD conversion in the shim` (`gh issue view 43 --comments`) |
| Implementation tickets + acceptance criteria | issues #44 … #69 (all closed, all sub-issues of #43) |
| Design decisions | `docs/adr/0001` … `docs/adr/0013` |
| Repo standards | `CLAUDE.md` / `AGENTS.md` (byte-identical, and the sync rule *was* honoured), `CONTEXT.md`, `README.md` |
| Conversion artifact under test | `docs/3.39.0--4.1.1.xml`, `docs/inventory/equilibrium-{3.39.0,4.1.1}.txt` |

The review's fetched copies of the issue text live in an **ephemeral job dir**
(`/Users/yohann/.claude/jobs/96126597/tmp/{spec-43.md,issues-44-69.md}`) — assume gone; re-fetch with
`gh issue view <n> --json number,title,state,body,comments`. Note plain `gh issue view <n> --comments`
was returning empty intermittently on this machine; the `--json` form is reliable.

---

## 2. How to read the finding table

- **CONFIRMED** — I independently re-checked the code and the claim holds as stated. Evidence noted.
- **REPORTED** — a review sub-agent's claim, plausible and specific, but **not** independently
  verified. Verify before changing code; sub-agents do get things wrong, and several of these turn on
  reading an acceptance criterion or ADR a particular way rather than on a code fact.
- Line numbers are as of `f8764de` / `af32154`. Re-grep rather than trusting them if the branch moved.
- The two axes are kept separate on purpose (Standards `S*`, Spec `P*`). Both independently flagged
  `tests/verify_artifact_coverage_floor.cmake` (S-J7 and P3) from different directions — that overlap
  is the single strongest signal in the report, but the findings are left un-merged.

---

## 3. Standards axis — hard violations

### S1 — `src/lib.rs` doc comments contradict the code they document — **CONFIRMED**
`src/lib.rs:320` (`al_begin_dataentry_action`): "Mirrors … exactly and **forwards unchanged**" — it
latches the HLI DD version (`resolve.rs:654 hli_version::resolve_for_open()`), can refuse, and
registers the pulse in the context registry.
`src/lib.rs:342-344` (`al_begin_global_action`): "**this ticket forwards them verbatim, DD path
translation is future work**" — `resolve.rs:704` translates `datapath`, discovers the version stamp,
and registers a root conversion record (ADR 0002).
`src/lib.rs:447` (`al_end_action`): "forwards unchanged" — it removes its own context's record.
Also `src/lib.rs:8`.
**Why it matters:** these are the first thing a reader of the public ABI sees, and they now describe a
shim that no longer exists. CLAUDE.md's own rule: keep the *why* next to what it explains "rather than
restating it in prose that can drift" — this is exactly that drift, inside the code.
**Fix:** rewrite the three (four) doc comments to state current behaviour, citing the ADR rather than
a ticket number. Sweep the whole file for other "forwards unchanged" claims on seams that no longer do.
**Verify:** read `src/resolve.rs` for each seam named in a `lib.rs` doc comment; no test covers prose.

### S2 — literal `\\n` (backslash-n) in the copied `CHECK` macro and message strings — **CONFIRMED**
`tests/read_path_test.c:23` `fprintf(stderr, "check failed at %s:%d: %s\\n", …)` — a *literal*
backslash followed by `n`, not a newline.
Occurrence counts: `read_path_test.c` 33, `write_delete_conversion_test.c` 18,
`nested_context_read_test.c` 11, `arraystruct_path_test.c` 9, `scoped_passthrough_test.c` **0** (that
one is correct). Present both in the macro and in ordinary `printf` progress/usage strings.
**Why it matters:** every failure message and progress line in four suites prints a trailing `\n` as
text and never breaks the line — worst exactly when a test fails and someone is reading output. It
propagated because the harness was copy-pasted (see S-J1).
**Fix:** `\\n` → `\n` across those four files. Purely mechanical; no behaviour change. Ideally fold
into the shared-header cleanup of S-J1 so it can't recur.
**Verify:** `grep -rn '\\\\n' tests/*.c` returns nothing; run the suites and eyeball one failure
message (temporarily break an assertion).

### S3 — the issue-#69 datatype-ordinal sweep is incomplete — **CONFIRMED**
`tests/runtime_binding_test.c:661` and `:670` still pass `3` as the `datatype` argument to
`al_plugin_read_data` / `al_plugin_write_data` (and assert `first_int() == 3`), two lines below
`:421`/`:468` where the same branch corrected `3` → `52`. CLAUDE.md's issue-#69 paragraph claims seven
sites were fixed and that `CHAR/INTEGER/DOUBLE/COMPLEX_DATA` are 50/51/52/53.
**Why it matters:** small, but it falsifies a claim CLAUDE.md now makes in writing, and these are the
oracles for the ABI-forwarding proof. The stub doesn't care about the value, so the tests stay green —
which is precisely why it slipped.
**Fix:** `3` → `52` at both call sites and their matching `first_int()` assertions.
**Verify:** re-run the runtime-binding suite; then re-read CLAUDE.md's #69 paragraph and correct the
count if it names a number.

### S4 — `Fidelity::Lossy` breaks CONTEXT.md's own "Avoid" rule — **REPORTED**
`src/conversion_map.rs:302` `Fidelity::Lossy`, surfaced to callers as `lib.rs:56
IMAS_MVDD_FIDELITY_LOSSY`. CONTEXT.md lists as something to avoid: using "lossy" without saying
whether the loss is *potential* or *certain* — the distinction ADR 0008 exists to draw.
**Why it matters:** the caller-visible enum is the one place the vocabulary must be unambiguous, and
the C constant is an ABI commitment that gets harder to rename later.
**Nuance to check before touching:** the tests and ADR 0008 may already use `LOSSY` to mean the
"certainly lossy" bucket, with `POTENTIALLY_LOSSY` as the other; if so this is a naming/doc fix
(`CertainlyLossy`, or a doc comment stating which bucket it is), not a semantics change. **Confirm
against ADR 0008 before renaming an exported constant.**

### S5 — stale caller-visible refusal messages naming closed tickets — **CONFIRMED**
`src/resolve.rs:1958` `"resolving a merged/split path is not yet implemented (issue #57)"`
`src/resolve.rs:1965` `"value-transform execution is not yet implemented (issue #59)"`
Both #57 and #59 are implemented and closed. The *refusal itself* is real (per CLAUDE.md, AOS
container path translation is still limited to concrete, untransformed paths) — the stated **reason**
is false.
**Why it matters:** this text goes into `al_status_t.message`, i.e. straight to the HLI user, telling
them a feature is unimplemented when the actual limit is narrower and different. Worst class of stale
comment: the user-facing kind.
**Fix:** reword to state the real limit (an AOS container path that would need a merged/split plan, or
a value transform, cannot be translated) without a ticket number. Check whether any test asserts the
old string — several suites assert exact messages, so this may need matching test updates.

### S6 — `tests/equilibrium_read_test.c:287` assertion cannot fail — **CONFIRMED (code fact)**
`int count = -1; CHECK_OK(imas_mvdd_context_loss_count(op_ctx, &count)); CHECK(count >= 1);` followed
by a scan for the expected entry. The `>= 1` was loosened (commit `af32154` "Make merged-read loss
assertion platform-stable") with the justification that "Core/backend combinations may retain field
and timebase outcomes as separate entries".
**Why it matters:** the sub-agent argues that justification is impossible — the read passes `""` as
`timebase`, and ADR 0012 §2 / ADR 0003 make the loss log wholly shim-owned, so the entry count is
deterministic and platform-independent; the loosening hides a real difference rather than absorbing
platform noise. The scan that follows does carry the real assertion, so the suite still proves
something — but `count` itself is now unpinned.
**Before fixing:** this is the one finding where the *author's* reasoning and the reviewer's disagree
about a real-Core observation. Read `af32154` and `e18287a` ("Diagnose real-Core merged loss count")
first — there may have been an observed count difference that motivated it. If the log really is
shim-owned, pin the exact count; if a real-Core run genuinely produced a different count, that
divergence is itself a bug worth a ticket.

---

## 4. Standards axis — judgement calls (all REPORTED; each is a labelled heuristic, not a rule breach)

- **S-J1 — Duplicated Code / Shotgun Surgery across the C tests (~550 lines).** `CHECK` defined 12×;
  `string_from_stub`/`int_from_stub` 8×; the same stub-opener body under three different names
  (`open_stub_for_introspection`, `open_stub`, `stub_handle` — **Mysterious Name**);
  `open_mismatched_equilibrium` 5× with *divergent* contracts (`read_path_test.c:61`, 7 lines, vs
  `scoped_passthrough_test.c:111`, 24 lines). No shared header, even though `tests/real_core_abi_contract.h`
  shows the repo already accepts one. **This is the vehicle that carried S2 into four files.** Highest
  leverage cleanup: one `tests/shim_test_support.h`.
- **S-J2 — `CMakeLists.txt:763, 864, 896, 1339`:** four near-identical `add_*_test` wrappers (two
  byte-identical bar the executable name), each carrying the comment "Keep that shared seam setup in
  one place"; 113 `set_tests_properties` calls still inline the env string, 48 of them the same
  literal. This is the bulk of the file's +991 lines.
- **S-J3 — `src/resolve.rs` internal duplication:** `:1802-1824` vs `:1912-1927` duplicate the
  anchor/strip/`CString` block with identical messages; twin enums at `:1671`/`:1685` feed twin
  resolvers at `:1779`/`:1832`; `:1065-1092` repeats one forward block twice and is the only seam not
  routed through a `*_impl` helper. Note CLAUDE.md **explicitly endorses** twin duplication at the
  four-line scale (`write_data`/`plugin_write_data`) — the repo overrides the smell at that size, so
  only the larger blocks are in play.
- **S-J4 — `src/resolve.rs:35-58` `READ_POLICY_STATE: Cell<(u32, Option<usize>)>` / `ReadPolicyGuard`:**
  **CONFIRMED as described** (thread-local depth counter + a pointer stored as `usize`, no doc comment,
  no ADR). It is the mechanism enforcing ADR 0010's "cannot apply a sign change twice", i.e. load-bearing
  policy with the least explanation in the file. Mysterious Name + undocumented invariant. See **P8**,
  which argues the same code is also a correctness risk — the two axes reached it independently.
- **S-J5 — `src/version_stamp.rs:30` `CHAR_DATA_ID = 50` "Mirrors `resolve::CHAR_DATA_ID`" (`:437`)** —
  a hand-maintained duplicate constant, the same drift class issue #69 was cleaning up.
  **`version_stamp.rs:84`** is the shim's first `free()` seam; ADR 0012 rejected taking on a free
  contract, and this is justified only in a module comment.
- **S-J6 — Divergent Change / Speculative Generality.** `src/resolve.rs` is 2,459 lines, still headed
  "Runtime resolution of IMAS-Core", and now owns ADRs 0001, 0002, 0010 and 0012.
  `src/context_registry.rs:52 #![allow(dead_code)]` hides `pulse_ctx_id()` (`:271`), used only by its
  own tests.
- **S-J7 — Repeated Switches:** ~814 lines of hand-rolled `argv[1]` scenario dispatch across the C
  suites, each scenario name spelled three times (dispatch, function, CMake test registration) — a
  rename must hit three places. Also **`tests/verify_artifact_coverage_floor.cmake:29-35`** asserts on
  substrings that end in `=`, so `supported=0` would pass — close to what ADR 0013 rejects. **Same file
  as P3**, from the ADR-conformance direction rather than the acceptance-criteria one.

---

## 5. Spec axis — missing or partial

### P1 — #50 AC1's completeness gate is structurally unreachable — **CONFIRMED (mechanism)**
Criterion: "Every DD path from both … inventories is claimed by a rule."
`docs/3.39.0--4.1.1.xml:35` declares `<default rel="identical"/>`, so `resolve` always matches
(`src/conversion_map.rs:891-897`) and `UnclaimedInventoryPath` can never be raised for the shipped
artifact. Only `DefaultAssumesMissingCounterpart` retains any force. Additionally both inventories omit
`ids_properties/**` and `code/**` entirely (`docs/inventory/equilibrium-3.39.0.txt`) — including
`ids_properties/version_put/data_dictionary`, the one path the shim itself reads at every open.
**Why it matters:** this is the branch's central completeness claim, and the check that is supposed to
back it cannot fail. Everything downstream (the coverage numbers, "supported" counts) inherits that.
**Judgement needed, not just a patch:** either the document-level identity default is wrong for a
completeness gate (make unclaimed paths an error and enumerate identity explicitly / by scope), or the
criterion means something narrower and #50's wording should be corrected on the issue. Decide with the
user before editing the artifact — it is described as hand-authored per ADR 0004.

### P2 — #50's artifact-validation command is not wired into the test runner — **REPORTED**
Criterion: "artifact-validation command invoked through the project test runner."
`check_completeness` has no call site outside `#[cfg(test)]`, and `src/bin/validate_equilibrium_coverage.rs`
never calls it — only #51's floor check is gated as a command. So the completeness half runs as a Rust
unit test, not as the gate the ticket describes. (Interacts with P1: even wired up, it can't fail today.)

### P3 — #51's coverage floor pins no number — **CONFIRMED**
`tests/verify_artifact_coverage_floor.cmake:27-39` asserts only that the report *contains* the
substrings `"shim 3.39.0 -> 4.1.1: supported="`, `"deliberate refusal="`, `"absent stored source="` —
each ending at the `=`, so `supported=341` collapsing to `supported=5` still passes. The negative
fixture removes *all* rules (0/49), so it proves rejection far from the boundary, never near it. Lines
32-33 hardcode `49/49` instead of deriving from `baseline.len()`.
**Why it matters:** this is the regression gate for conversion coverage; as written it detects only
"the report is still shaped like a report". **Same file as S-J7.**
**Fix:** parse the integers out and compare against a pinned floor; add a fixture that drops exactly
one rule below the floor and assert it fails; derive `49` from the baseline length.

### P4 — tier-2 (real-IMAS-Core) matrix cells are stub-only — **REPORTED**
Spec #43's real-Core obligations (c)/(e)/(g): write-against-mismatch refusal, unstamped-occurrence
read, and non-LIFO close / recycled context ID / `al_close_pulse` / AoS iteration are covered only
against the recording stub (`tests/write_delete_conversion_test.c`, `tests/context_lifecycle_test.c`).
"unstamped" appears in no real-Core test. Cross-check against CLAUDE.md's #63 paragraph, which argues
some of these are observable only indirectly — the question is whether "indirectly observable" was
accepted as discharging the *real-Core* obligation or only the ABI-seam one.

### P5 — registry lock taken on every read/write/delete even with the HLI version unset — **CONFIRMED (code fact)**
`src/resolve.rs:1305` (`read_data_impl`, shared by `read_data` `:1213` and `plugin_read_data` `:1241`),
plus `:2045` and `:2064`, call `REGISTRY.lookup(ctx_id)` unconditionally, and
`context_registry.rs:260-265` takes `self.state.lock()`. The `begin_*` seams do short-circuit on
`hli_version::current()` (`:756`, `:1011`, `:1065`); the data-path seams do not.
Against #56 AC5 / user story 32 ("bypass registry lookup") and ADR 0003's "no lookup cost".
**Why it matters:** a mutex acquisition per `al_read_data` on the conversion-disabled path — the path
every non-converting HLI takes for every field it reads. Whether ADR 0003's "no lookup cost" is a
promise about *this* path is worth confirming with the user; the code fact is settled.
**Fix if accepted:** an early `hli_version::current()?`-style guard before the lookup, matching the
`begin_*` seams. Cheap and local.

### P6 — #58 AC3's refusal-message contract is honoured only at the read seam — **REPORTED**
Criterion: the refusal message names the path *and both* DD versions. Read-seam refusals comply
(proven in `tests/equilibrium_read_test.c` on exact message text). Arraystruct
(`src/resolve.rs:1164`, `:1168`), write (`:2046`), delete (`:2065`) and plugin write (`:2315`) emit
the reason only. Note #64 deliberately made write/delete refusal a blanket context-keyed check that
never resolves a path through the map — so for those seams there may be *no* path/version pair
available to name, which would make this a spec-wording problem rather than a code gap. Establish
which before "fixing" it.

### P7 — #49's rank/shape dispatch is rel-keyed, not rank-keyed — **REPORTED**
`src/conversion_map.rs:875` dispatches on the rule's `rel`; `shape="int_1d:struct_array"` is never
parsed. Consequence: the glob stage and `RefusalReason::Unmappable` are unreachable with the shipped
artifact. Relates to P11 (the retype whose second occurrence is served as identity).

---

## 6. Spec axis — behaviour not asked for (scope creep)

### P8 — address-keyed transform dedup — **CONFIRMED as present; risk REPORTED**
`src/resolve.rs:35-58` + `:1387-1398` (thread-local `(depth, Option<usize>)`, the pointer as `usize`),
added by commits `0593f91` "Avoid double conversion on reentrant ordinary reads" and `7664494` "Apply
read transformations once across reentrant calls". No spec basis in #43 or #59/#68.
**The risk as stated:** keying "already transformed" on a raw allocation *address* means that if the
allocator hands back the same address for a different buffer inside a nested read, a required sign
flip is skipped — silent wrong-sign data, the exact failure mode the COCOS work exists to prevent.
**Why it exists (real problem it solves):** `version_stamp::discover` and the plugin reentry path both
re-enter the shim's own `read_data`, so without some guard one buffer could be flipped twice. So do
**not** just delete it — it is load-bearing. The question is whether the identity key should be
address-based, and whether ADR 0010 should record the mechanism at all (S-J4 notes it has no ADR and
no doc comment).
**Suggested handling:** treat as a design question for the user (grilling / ADR), not a quick patch.

### P9 — `translate_down` invents a third `datapath` policy — **REPORTED**
`src/resolve.rs:929-931`: forwards `datapath` unchanged on `Refusal` / `NoSource`, where the documented
policy is two-valued (forward unchanged on first use; translate once a prior open cached a mismatch).
Low practical impact given `datapath` is near-inert on 5 of 6 backends (see CLAUDE.md), but it is an
undocumented third branch in a seam ADR 0002 speaks to.

---

## 7. Spec axis — implemented but looks wrong

### P10 — `docs/inventory/equilibrium-4.1.1.txt` lists 23 paths DD 4.1.1 removed — **REPORTED, high value**
It contains 23 `time_slice/ggd/grid/*` paths, contradicting the artifact's own `drop-timeslice-ggd-grid`
rule (`docs/3.39.0--4.1.1.xml:296-302` — whose note also miscounts, saying 37 where the reviewer counts
23). Because of the identity default (P1) they fall through as *supported*, **overstating reverse
coverage by 23 paths**. No gate cross-checks that a `left_only` side's declared absence matches the
inventory.
**Why it matters:** the inventories are the oracle for every coverage number the branch reports, and
this is a self-contradiction *inside the shipped data* — an artifact rule says these are dropped while
the inventory says they exist. Verify against the `imas-dd` MCP server
(`get_dd_version_context` / `check_dd_paths` for 4.1.1) — CLAUDE.md is explicit that the MCP server,
not memory, is the authority on DD content.
**Fix shape:** correct the inventory, correct the note's count, and add a gate asserting a `left_only`
rule's paths are absent from the side that declares them absent.

### P11 — `time_slice/ggd/grid/space/coordinates_type` has no rule — **REPORTED**
Present in both inventories; the `retyped` rule at `docs/3.39.0--4.1.1.xml:185` covers only
`grids_ggd/…`. So story 29's rank-changing retype (int array → array of identifier structures) is
served as *identity* at its second occurrence — i.e. the one case the branch documents as
"cannot reshape, must refuse" silently passes through at that path. Overlaps P7 and P10 (the same
`time_slice/ggd/grid` subtree). Check with `imas-dd` whether the path exists in 4.1.1 at all.

### P12 — `resolve_merged`/`resolve_split` never honour a declared `unmappable` fidelity — **REPORTED**
`src/conversion_map.rs:947-1045`. A `merged`/`split` rule declared `unmappable` would still produce a
candidate read plan instead of refusing. Compare the refusal path, which does honour it
(`RefusalReason::UnservableRetype` / the `redefine`-declared `unmappable` proven in
`tests/equilibrium_read_test.c`).

### P13 — `src/version_stamp.rs` doc claim is false — **CONFIRMED**
`version_stamp.rs:53-54`: "Reads and classifies the DD-version stamp … **via the real IMAS-Core
`al_read_data` binding**". `:62` actually calls `crate::resolve::read_data` — the shim's own
*converting* wrapper.
**Why it matters:** it reads as reentrancy into the conversion layer from inside version discovery.
In practice the record for that context is not registered yet, so `REGISTRY.lookup` misses and it
forwards — but that is an accident of ordering, not a stated invariant, and it is precisely the
reentrancy P8's guard exists to police. Two coherent fixes: call the raw binding (matching the doc, and
removing the reentrancy), or keep the call and rewrite the doc to state *why* going through the shim is
safe here. Prefer the former unless there's a reason.

### P14 — an unclaimed path is forwarded to Core rather than refused — **REPORTED**
`src/resolve.rs:1841-1846`: `resolve_read_path` forwards a path no rule claims, which the reviewer
reads as contradicting user story 47. Also: ADR 0013 decision 4's motivating case (a bare anchor
absent from the data) does not exist in the artifact. Tightly coupled to P1 — if the identity default
goes, this branch changes meaning. Decide P1 first.

---

## 8. Suggested batches for a fixing agent

Ordered by "safe and mechanical" → "needs a decision". Do **not** bundle across batches.

1. **Mechanical, zero-risk:** S2 (`\\n`), S3 (datatype `3`→`52`). No behaviour change; suites must
   stay green.
2. **Prose that lies:** S1 (`lib.rs` doc comments), S5 (stale refusal strings — check for tests
   asserting exact messages), P13 (either the call or the doc). Ends with a CLAUDE.md/AGENTS.md
   consistency pass — **they must stay byte-identical**, per CLAUDE.md's own rule.
3. **The coverage gate:** P3 + S-J7 (pin real numbers, near-boundary fixture, derive `49`), then P2
   (wire the completeness command into the runner). Cheapest way to stop the gate silently weakening.
4. **The artifact/inventory data:** P10, P11, P7, P12 — verify every DD claim against the `imas-dd`
   MCP server first, never from memory. P10 is the one that changes a reported number.
5. **Needs a decision from the user before code changes:** P1 (identity default vs completeness),
   P8 + S-J4 (address-keyed dedup: correctness + missing ADR), S6 (is the loss count deterministic?),
   P5 (is ADR 0003's "no lookup cost" a promise about the data path?), P6 and P4 (spec wording vs code
   gap), S4 (renaming an exported C constant).
6. **Refactors, only if the user wants them:** S-J1 (one shared test header — this is what would have
   prevented S2), S-J2 (CMake wrappers), S-J3, S-J5, S-J6.

**Working rules for the fixing agent**
- `feat/path-conversion` and `main` are shared branches: branch off, don't commit to either, never
  force-push or merge.
- Before the first edit, isolate with `EnterWorktree` (the repo's convention on this work; every
  ticket above was done in a `worktree-issue-NN-*` branch).
- Green baseline to preserve: `cargo fmt --check`, `cargo test --lib` (149 tests), and the CMake/ctest
  suites. Note the stub-only profile (`IMAS_MVDD_REAL_CORE_TESTS=OFF`) is the only path that needs no
  IMAS-Core.
- Two memory items apply: **local `cmake` is 4.x while CI pins 3.31**, so a green local `ctest` proves
  nothing about the `cmake -P` scripts under `tests/` (relevant to batch 3); and the **local
  IMAS-Core / IMAS-Fortran checkouts are forks ahead of the pin**, so don't cite them as upstream
  guarantees.
- When a finding turns on reading an acceptance criterion, quote the criterion from the issue in the
  fix's commit message or PR body — several findings here are arguable readings, and the next reviewer
  will need to see which reading was chosen.

---

## 9. Suggested skills

- **`/grilling`** — for batch 5. P1, P8 and S6 are decisions, not defects; stress-test them with the
  user before any code moves.
- **`/tdd`** — for batches 3 and 4. Every one of those findings is "a gate that can't fail" or "an
  oracle that's wrong"; the fix must start from a test that fails for the stated reason. The repo's
  own convention (see the #69 paragraph in CLAUDE.md) is to confirm each new assertion fails under a
  deliberately mutated expectation before leaving it green — keep that.
- **`/diagnosing-bugs`** — for P8 specifically (can the allocator actually return a colliding address
  inside a nested read?) and S6 (was there a real observed count divergence in `e18287a`?).
- **`/domain-modeling`** — for S4 and P8: an ADR is missing for the reentrancy guard, and the
  `Lossy`/`PotentiallyLossy` vocabulary needs to land in `CONTEXT.md` consistently with ADR 0008.
- **`/code-review`** — re-run after a batch lands, with the fixed point set to the branch tip before
  the batch, to confirm nothing regressed.
- **`graphify query "<question>"`** — a repo hook enforces running this before grepping raw files
  (`graphify-out/graph.json` exists); use it to orient, and `graphify update .` after code changes.
- **`imas-dd` MCP server** — mandatory for batch 4. `check_dd_paths` / `get_dd_version_context` /
  `get_dd_cocos_fields`. CLAUDE.md forbids reasoning about DD content from memory.

## 10. Not done / explicitly out of scope of this review

- No code was changed; nothing was committed or pushed.
- Neither axis re-checked anything `cargo fmt`/`cargo clippy` enforces.
- The full CMake/ctest suite (including real-IMAS-Core seams) was **not** run — only
  `cargo test --lib`. A fixing agent should run the full suite before and after.
- The two axes were not cross-ranked, so there is no single "worst finding overall" here by design.
  Worst within Standards: **S2**. Worst within Spec: **P1**.
