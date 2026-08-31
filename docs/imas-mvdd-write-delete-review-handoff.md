# Handoff — code review findings, `feat/delete-write` (spec #122 / tickets #123–#134)

**Written:** 2026-08-26. **Repo:** `/Users/yohann/Documents/Dev/ITER/IMAS-Multiversion-DD-Loader`
**Purpose:** a fresh agent should be able to (a) explain any finding below to the user, or (b) fix one
or a batch of them, without re-running the review.

---

## 1. What was reviewed

Two-axis review (`/code-review` skill: Standards + Spec, run as two independent parallel sub-agents,
deliberately **not** merged or cross-ranked). Every finding below was then re-checked by hand against
the source before being written down; the verdicts in section 2 record how far that check went.

- **Fixed point:** merge-base with `develop` = `931cf2c` *"Record the write/delete path decisions as
  ADRs 0016-0019"* — the ADR commit immediately preceding the series implementation.
- **Head:** `feat/delete-write` = `6fbe7ea` (merge of PR #140).
- **Diff:** `git diff develop...HEAD` — 34 files, +4,860 / −359, 54 commits.

**Baseline state at review time (verified, do not re-derive):** `cargo fmt --check` clean;
`cargo test --lib` = **183 passed / 0 failed** (matches CLAUDE.md's claimed count). Anything
`cargo fmt` / `cargo clippy` enforces was deliberately out of scope for both axes.

**Scope note:** the diff range also contains issue **#136** (ADR 0020, the stamp probe) and its PR
#140, which are *not* part of the #122 series. #136 was given to the Spec axis as context only, so
none of its work is reported as scope creep. Issues **#138** and **#139** were filed by that work and
remain open; #138 in particular invalidates one of #133's claims — see P4.

### Sources — reference, don't duplicate

| What | Where |
|---|---|
| Parent spec | GitHub issue #122 `Best-effort write and delete across a DD-version mismatch` |
| Implementation tickets + acceptance criteria | issues #123 … #134 |
| Context-only (in range, not in series) | issue #136; follow-ups #138, #139 |
| Design decisions | `docs/adr/0016` … `docs/adr/0020` (plus `0002`, `0011`, `0012`, `0013`, `0014`, `0015`) |
| Repo standards | `CLAUDE.md` / `AGENTS.md`, `CONTEXT.md`, `README.md`, `tests/README.md` |
| Conversion artifact under test | `docs/3.39.0--4.1.1.xml`, `docs/inventory/equilibrium-{3.39.0,4.1.1}.txt` |

The review's fetched copies of the issue text live in an **ephemeral job dir** and should be assumed
gone. Re-fetch with `gh issue view <n> --json number,title,state,body,comments`. (Plain
`gh issue view <n> --comments` has been observed returning empty intermittently on this machine; the
`--json` form is reliable.)

---

## 2. How to read the finding table

- **CONFIRMED** — independently re-checked against the code; the claim holds as stated. Evidence noted.
- **REPORTED** — a review sub-agent's claim, plausible and specific, but **not** independently
  verified. Verify before changing code.
- **DISPUTED** — re-checked and the claim does **not** hold as stated, or its stated consequence is
  backwards. Recorded so nobody re-raises it. Read the reasoning before acting.
- Line numbers are as of `6fbe7ea`. Re-grep rather than trusting them if the branch moved.
- The two axes are kept separate on purpose (Standards `S*`, Spec `P*`). Two pairs overlap across the
  axes — **S-J3 ↔ P5** (write/delete guard order) and **S-J2 ↔ P1** (`unwritten_candidates`). Those
  overlaps are the strongest signal in the report, but the findings are left un-merged.

---

## 3. Standards axis — hard violations

### S1 — duplicate `EMPTY_DOUBLE` sentinel, in the series that centralised it — **CONFIRMED**

`src/conversion/read_outcome.rs:52-58` documents the sentinel explicitly:

> "Two seams need it and for opposite reasons … so it is **defined once, here**, alongside the outcome
> it decides."

`src/interpose.rs:1367-1368`, inside `is_empty_scalar`, re-declares it locally and adds an unshared
integer companion:

```rust
const EMPTY_INT: c_int = -999_999_999;
const EMPTY_DOUBLE: f64 = -9e40;
```

This is the same series that deliberately moved `EMPTY_DOUBLE` out of `seam_policy` into
`read_outcome` *for sharing*.

**Correction to the sub-agent's report:** it called this "a third definition". It is the **second** —
`grep -rn "EMPTY_DOUBLE" src/ | grep const` returns exactly `interpose.rs:1368` and
`read_outcome.rs:58`. The `seam_policy` one was moved, not left behind. The violation stands; the count
was overstated.

**Fix:** delete both local constants; `pub(crate) use` the `read_outcome` one, and give `EMPTY_INT` the
same single home next to it (`is_empty_scalar` is the only consumer today, but that is exactly the
argument `read_outcome.rs:52-58` already makes for the double).

### S2 — stale path in a new doc comment — **CONFIRMED**

`src/lib.rs:366`: ``/// Shim-owned export (ADR 0012) — listed on `tests/owned_exports.def` ``.
The file is `tests/abi/owned_exports.def` (CLAUDE.md, "Current path map": C tests are grouped under
`tests/abi/`, …). One-line fix; check whether sibling doc comments on the other shim-owned exports
carry the same stale path.

### S3 — `contextual_refusal`'s doc comment describes a call site that no longer exists — **CONFIRMED**

`src/interpose.rs:1589-1595` documents the helper as serving

> "the delete seam, **whose refusal remains a blanket context-keyed check**, and arraystruct opens…"

`grep -n "contextual_refusal" src/interpose.rs` gives exactly three lines: the definition at `:1604`
and two call sites, `:1011` and `:1015` — **both arraystruct opens**. `delete_data` no longer calls it,
because #129/#131 replaced the blanket check with real path resolution. The doc now asserts the
pre-#122 behaviour the whole series exists to remove.

Same class as finding S1/S5 in the previous review (`docs/imas-mvdd-read-path-review-handoff.md`):
doc comments that outlived the code. Worth a sweep, not just a one-line edit.

### S4 — C test helpers re-copied, one with a divergent contract — **CONFIRMED**

`tests/support/shim_test_support.h:6-16` exists precisely because these helpers had been copied "with
contracts that had begun to diverge — one definition per thing is what stops the next such defect from
being copied", and CLAUDE.md's "Adding a C ABI test" section closes with "a new copy starts that over".

Current state — `static int loss_count(int ctx_id)` is defined **five** times:

| File | Line | |
|---|---|---|
| `tests/shim/read_path_test.c` | 40 | |
| `tests/shim/nested_context_read_test.c` | 46 | |
| `tests/shim/plugin_reentry_policy_test.c` | 55 | |
| `tests/shim/reentry_guard_test.c` | 26 | **← new in range** |
| `tests/shim/write_delete_conversion_test.c` | 21 | **← new in range** |

`check_loss_at` is defined **four** times, and the newest copy has a **different signature**:

- 4 params (`ctx_id, index, expected_path, expected_verdict`) — `read_path_test.c:51`,
  `nested_context_read_test.c:57`, `plugin_reentry_policy_test.c:62`
- **5 params** — `write_delete_conversion_test.c:58` (adds the expected loss *operation*, from #124)

That fifth parameter is a legitimate new need (#124 exposed the operation on a loss entry); copying
rather than extending the shared header is what makes it a violation. **Fix:** promote both helpers
into `tests/support/shim_test_support.h`, with the operation parameter, and update the four other
call sites — this is the *documented* remedy, not a judgement call.

---

## 4. Standards axis — judgement calls

All **judgement calls**; each is a labelled heuristic, not a rule breach. A documented repo standard
overrides any of them.

### S-J1 — Speculative Generality: `TransformationDirection` has no reader — **CONFIRMED (code fact)**

`src/conversion/conversion_map.rs:313-324` introduces the enum, `ValueTransformation::SignFlip` gains a
`direction` field (`:333`), and `ValueTransformation::inverse` flips it (`:353`).

Verified: production code constructs only `TransformationDirection::ToHli` (`conversion_map.rs:1436`),
and **no production site ever reads or matches the field** — every consumer matches
`ValueTransformation::SignFlip { .. }` and ignores it (`seam_policy.rs:366`, `:502`, `:508`, `:525`).

**Refinement of the sub-agent's report:** it implied `ValueTransformation::inverse()` itself is dead.
It is not — `seam_policy.rs:258` consumes its `Option`, and the `None` arm (a sign flip between
identical conventions) is load-bearing for a write refusal. What has no consumer is specifically the
`TransformationDirection` **enum, the field, and `TransformationDirection::inverse`**.

The doc comment argues the field is there so "a write must explicitly request the inverse rather than
assuming a transformation happens to be an involution (ADR 0016)" — a real concern from #122. But
nothing enforces it: the write path gets its correctness from calling `inverse()`, not from reading
`direction`. Either make a consumer assert on it (e.g. `copy_value_transformation` refusing a
`ToHli`-directed transform on the write path) or delete the enum and keep `inverse()`.

### S-J2 — Mysterious Name: `unwritten_candidates` means two different things — **CONFIRMED**

`WriteVerdict.unwritten_candidates: Vec<&'a str>` is documented "Every stored candidate that
deliberately remains unwritten", while the same identifier on `ResolvedWriteArgument`
(`seam_policy.rs:299`) is a `usize` count. The `Vec` is built from the count, not from paths — see
**P1**, which is the same defect seen from the spec side and is the more actionable write-up.

### S-J3 — write and delete resolvers disagree on their own stated guard order — **CONFIRMED** *(overlaps P5)*

`resolve_write_path` (`src/conversion/path_conversion.rs:345-363`) hoists the shared refusal check
**above** the precedence guard, with a comment explaining why:

> "Keep the shared guard ahead of the write-specific precedence guard: a rule that cannot be served at
> all must not appear to be merely a collision risk."

`resolve_delete_path` (`:447-460`) does the **opposite** — precedence guard first, `Outcome::Refusal`
reached only in the `match` below at `:462` — while carrying a comment that describes the *write's*
order as though it were its own:

> "`Outcome::Refusal` is the shared pre-resolution guard: it is computed by ConversionMap before any
> consumer narrows a path. A non-primary source is the delete-specific guard that follows it."

**Observable consequence:** for a path that is *both* a non-primary source *and* an `Outcome::Refusal`,
`al_write_data` reports the rule's refusal reason while `al_delete_data` reports "this path is a
non-primary source…". Two different messages for the same rule, and the delete comment is false about
its own code. Two ~60-line near-identical resolvers (Duplicated Code) is the underlying smell; the
false comment is the part worth fixing regardless of whether they are ever unified.

### S-J4 — Feature Envy: `is_equilibrium_leaf` embeds artifact data in the wrong module — **CONFIRMED (placement), deliberate**

`src/conversion/path_conversion.rs:516-525` `include_str!`s both equilibrium inventories into the
module whose own header says it "knows nothing about seams"; `known_artifacts.rs` is where embedded
artifact data otherwise lives. **However** the doc comment at `:507-515` already records the exception
and cites `ADR 0013 decision 6` for it. Treat as a placement preference, not a defect. See **P6**,
where the spec axis drew a stronger — and wrong — conclusion from the same code.

### S-J5 — Middle Man: `seam_policy::run_delete` — **REPORTED (not verified)**

Claimed to be a 1:1 `DeletePath` → `DeleteVerdict` remap carrying no decision. Not independently
checked. If true, weigh against the repo's documented tolerance for thin twins (CLAUDE.md's
`write_data`/`plugin_write_data` note) before collapsing it.

### S-J6 — magic int in a new test — **REPORTED (not verified)**

`tests/shim/write_delete_conversion_test.c:645`: `data_event_kind_at(...) == 1 /* READ */` — exactly
the "bare small integer under a comment naming a constant" shape CLAUDE.md's issue-#69 note tells you
to grep for (`&data, 3,`). That note's own history is three successive sweeps each claiming
completeness; this would be a fourth site. Cheap to fix, cheap to verify.

---

## 5. Spec axis — missing or partial

### P1 — #128: the loss log names the caller's own path, not the unwritten stored candidates — **CONFIRMED. Strongest finding in the report.**

Spec (#128): *"The unwritten candidates reach the root context's loss log as `POTENTIALLY_LOSSY` …
**naming the paths**"*. User story 22: *"so that I can see … the other spelling."*

`src/conversion/seam_policy.rs:306-315`:

```rust
fn unwritten_candidate_paths<'a>(
    field: &ResolvedWriteArgument<'a>,
    timebase: &ResolvedWriteArgument<'a>,
) -> Vec<&'a str> {
    let mut paths = Vec::with_capacity(field.unwritten_candidates + timebase.unwritten_candidates);
    paths.extend((0..field.unwritten_candidates).map(|_| field.dd_path));
    paths.extend((0..timebase.unwritten_candidates).map(|_| timebase.dd_path));
```

It pushes `field.dd_path` — the **caller's own HLI path** — N times. `ResolvedWriteArgument` (`:299`)
keeps only a `usize` count; the stored `WriteCandidate.path`s are discarded at `:333`.

The test enshrines the defect rather than catching it —
`tests/shim/write_delete_conversion_test.c:215-217` asserts two entries **both** reading
`time_slice/profiles_2d/b_field_phi`, the HLI spelling, at indices 0 and 1.

**Net effect:** the one thing story 22 asks the log to reveal — *which stored spelling now holds a
stale value* — is exactly what it cannot show. A caller draining the log sees their own path repeated.

**Fix shape:** carry `Vec<&'a str>` of stored candidate paths through `ResolvedWriteArgument` instead
of a count, then update the two assertions at `write_delete_conversion_test.c:215-217` and `:289-291`
to the stored spellings. Note this changes caller-visible loss entries, so check `tests/README.md`
counts and any real-Core assertions on loss content.

### P2 — #128: the `Fidelity::Lossy`-unreachable assertion does not exist — **CONFIRMED**

Spec (#128): *"a test that fails telling the reader to add real coverage if a future artifact makes it
reachable"* — the ADR 0011 shape ("silence is earned by mechanism coverage").

Verified: the write-side verdict is hardcoded at both sites —
`src/interpose.rs:1806` (`Fidelity::PotentiallyLossy`) and `:1823` (`Fidelity::Unmappable`) — so
`Fidelity::Lossy` is unreachable on the write path. No test asserts that. The `Fidelity::Lossy`
occurrences in `seam_policy.rs:970`/`:1005` are inside `#[cfg(test)]` (module starts at `:722`) and
belong to a **read** test (`run_read`), not a write-side reachability proof.

Compare `docs/adr/0011` and the existing unreachability tests written for the read path's
`RefusalReason::Unmappable` and the glob stage (review finding P7 of the previous round) — copy that
shape.

### P3 — #134: the passthrough half of the acceptance criteria is entirely absent — **CONFIRMED**

Spec (#134): *"With a mismatched occurrence open and a write demonstrably converting,
`al_get_occurrences`, `al_list_filled_paths` and the plugin bind/unbind family all forward
unchanged."*

Verified: `git diff develop...HEAD --stat -- tests/shim/scoped_passthrough_test.c` is **empty** (file
untouched in range), and it contains **zero** `al_write_data`/`al_delete_data` calls;
`write_delete_conversion_test.c` contains no `al_get_occurrences`.

This is the direct write-side analogue of what issue #69 did for the read path, and the file to extend
already exists. Straightforward to close.

### P4 — #133: the delete half of on-disk claim 4 is unproven, and ADR 0016 decision 1 is currently false — **CONFIRMED**

Spec (#133): *"the precedence-2 candidate … removed by a delete."*

`tests/real_core/write_delete_oracle_test.c:542` is
`scenario_reverse_delete_fan_out_does_not_reach_disk` — a **regression marker for open issue #138**,
not the claim. It asserts `deletion.code == 0` having deleted nothing, and says so in its own comment:

> "Reporting `code == 0` for that is still a defect — ADR 0016 decision 1 forbids exactly it — but it
> is a different defect from the one it is hiding."

So `ADR 0016 decision 1` ("data silently discarded with `code == 0` cannot happen") is **not** honoured
for delete on real IMAS-Core today. The in-code documentation is exemplary and the marker is the right
call; what matters for the handoff is that **#133's acceptance criteria are not met**, and closing them
depends on **#138** (candidate probes run through the caller's write-mode context) and **#139**
(`HDF5Writer::deleteData` ignores its `path` and destroys the whole IDS file).

Do not attempt to close #133 without #138 first. Note the interaction the scenario documents: #138's
silence is currently the *only* thing preventing #139 from destroying an occurrence.

### P5 — #126: the guard order the test claims to pin is not pinned — **CONFIRMED** *(overlaps S-J3)*

Spec (#126): *"pinned by test rather than left to arm order."*

`src/conversion/path_conversion.rs:810-868`,
`write_pre_resolution_refusals_keep_the_shared_guard_ahead_of_rule_specific_ones`, builds a fixture
artifact with three rules and asserts three refusals on paths `shape`, `impossible`, `missing`.

**Each path triggers exactly one guard.** No path is simultaneously a non-primary-precedence source
*and* an `Outcome::Refusal`, which is the only configuration in which the order is observable — so
swapping the two guards in `resolve_write_path` leaves this test green. The non-invertible-transform
guard is not in the fixture at all.

**Fix:** add a fourth rule that is both (a `merged` rule whose `fidelity` is `unmappable` on the
direction under test, giving a non-1 precedence *and* an `Outcome::Refusal`), assert the refusal
message is the rule's reason rather than "non-primary source", and mutate the arm order to confirm the
new assertion goes red. While there, settle S-J3 — decide which order `resolve_delete_path` should
have and make its comment true.

---

## 6. Spec axis — implemented but wrong

### P6 — `is_equilibrium_leaf` is IDS-blind — **DISPUTED. Do not act on the stated consequence.**

The sub-agent reported: *"A future artifact for another IDS would classify its structures as leaves,
skipping the escaping-rule check #131 exists for."*

Re-checked, and **the failure direction is backwards**. `is_equilibrium_leaf`
(`src/conversion/path_conversion.rs:516-525`) selects an inventory by `record.direction_to_stored`
only and asks `inventory.lines().any(|leaf| leaf == hli_path)`. A path from a *different* IDS is not in
the equilibrium inventory, so the function returns **`false`**. At the one call site (`:472`) the guard
reads:

```rust
} if !is_equilibrium_leaf(record, &hli_absolute)
    && !record.map.subtree_delete_is_trivial(...) =>
{
    DeletePath::Refusal { reason: "this subtree delete would leave data at a stored path outside the requested subtree" ... }
```

`false` therefore drives the path **into** the refusal branch, not past it. The behaviour is
**fail-closed**: a foreign-IDS delete is refused, which is the safe answer. Furthermore the doc comment
at `:507-515` already states the limitation and cites `ADR 0013 decision 6` for the narrow exception,
so #122's *"do not write version-specific policy code"* is knowingly bounded rather than breached.

**What survives:** the placement objection only (see S-J4), plus a note that when a generated artifact
for a second IDS lands, this function must gain an IDS check *or* the inventories must move to
`known_artifacts.rs` alongside the artifact they describe. Neither is urgent, and neither is a
correctness defect today.

### P7 — the crate-level doc describes the #125/#126 intermediate state — **CONFIRMED**

`src/lib.rs:37-46` still reads:

> "…`al_write_data` and `al_plugin_write_data` independently resolve identity, `renamed`, and `moved`
> field/timebase paths to one stored spelling before IMAS-Core is called. **Candidate plans, declared
> value transformations**, and a write to the DD-version stamp **refuse**…"

Both clauses were true after #125/#126 and false after #127 (write-side value transformation on a
copy, ADR 0018) and #128 (candidate plans write the precedence-1 slot). The delete sentence at `:42-46`
*has* been updated for #129/#131 and reads correctly, which makes the write sentence's staleness a
clear oversight rather than a deliberate lag.

This is the same defect class as S2/S3 and as S1 in the previous review round. Against #134's
traceability acceptance criterion.

### P8 — minor: `delete_candidates` probes every candidate as a rank-0 `DOUBLE_DATA` read — **REPORTED (not verified)**

`src/interpose.rs:1892-1904`. Claimed to probe array-valued merged candidates such as
`profiles_2d/b_field_phi` with a scalar double read. Currently unobservable against real Core because
**#138** stops the probe reaching it at all — so this becomes testable only once #138 is fixed, and
should be re-checked as part of that work rather than now.

---

## 7. Suggested batches for a fixing agent

Ordered by value-per-risk. Batches A–E are independent of each other.

**Batch A — caller-visible correctness (do first).**
P1 alone. It changes what a caller reads out of the loss log, touches `seam_policy.rs`,
`interpose.rs` and two test files, and is the only finding in the report where the shipped behaviour
contradicts a user story. Expect to update `tests/README.md` counts.

**Batch B — the guard-order pair.**
S-J3 + P5 together; they are the same code read from two directions. Decide the intended order for
`resolve_delete_path`, make both comments true, and extend the #126 fixture with a path that triggers
both guards so the order is actually pinned. Confirm the new assertion is red under a mutated arm
order before leaving it green (this repo's convention — see CLAUDE.md's #69 and #133 notes).

**Batch C — doc/comment truth sweep.**
S2, S3, P7 in one pass, plus a grep for sibling instances: doc comments naming closed tickets, stale
`tests/*.def` paths, and any remaining prose describing the pre-#127 write policy. The previous review
round produced the same batch (its S1/S5), which suggests a standing check is worth more than another
one-off sweep.

**Batch D — test-infrastructure debt.**
S4 (promote `loss_count` and the 5-param `check_loss_at` into `tests/support/shim_test_support.h`,
update five and four call sites respectively) and S-J6 (the magic `1 /* READ */`). Mechanical, but
touches five test files, so run the full `ctest` profile rather than `cargo test`.

**Batch E — earned silence.**
S-J1 (decide whether `TransformationDirection` gets a consumer or gets deleted) and P2 (add the
`Fidelity::Lossy`-unreachable assertion in the ADR 0011 shape). Both are about making an unproven claim
either provable or absent.

**Batch F — partly blocked.**
P3 is *not* blocked and is easy — extend `tests/shim/scoped_passthrough_test.c` with the write/delete
cases #134 asks for. But **P4 is blocked on #138**, and #138 interacts with #139 in a way that can
destroy a fixture occurrence (`HDF5Writer::deleteData` ignores its `path`). Read
`write_delete_oracle_test.c:520-560` in full before touching either.

---

## 8. Not done / explicitly out of scope of this review

- Anything `cargo fmt` or `cargo clippy --all-targets` enforces.
- Issue **#136** / ADR 0020 (the stamp probe) and PR #140 — in the diff range, reviewed only as
  context. Its own six stub-suite scenarios were not audited.
- Issues **#138** and **#139** as *defects* — they are treated here only where they invalidate a #122
  acceptance criterion (P4).
- The real-Core ctest profile was **not run** during this review; only `cargo fmt --check` and
  `cargo test --lib` (183 passed). A fixing agent touching test infrastructure (Batch D) or loss-log
  content (Batch A) must run the full profile, not the unit tests.
- No cross-axis reranking. The Standards and Spec sections are deliberately independent; the two
  overlaps are flagged in section 2 rather than merged.
