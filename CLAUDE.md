# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.
If it has to been modified, apply the same changes to AGENTS.md.

## Current path map

Current source ownership is `src/core/`, `src/conversion/`, `src/registry/`,
and `src/version/`; C ABI adaptation remains in `src/interpose.rs`.

The read, write and delete **loops** live in `src/conversion/seam_policy.rs`,
not in the interposition layer: `run_read`, `run_write`, `run_delete`, the
`ReadAttempt` type, the `impl TranslatedReadPath` block that produces
attempts, and `validate_value_transformation` /
`apply_value_transformation` are all there, and none of them reaches
IMAS-Core or process-global state (ADR 0015). `src/interpose.rs` keeps only
what is C-facing: `read_data_impl` and its siblings, the `CallFamily`
dispatch that chooses an ABI symbol, `resolve_arraystruct_argument`,
`contextual_refusal`, `joined_argument_path` and `live_conversion_record`.
`src/conversion/path_conversion.rs` answers *which stored path does this HLI
argument mean, and at what fidelity* and knows about neither seams nor
IMAS-Core.

C tests are
grouped under `tests/abi/`, `tests/shim/`, `tests/real_core/`, and
`tests/package/`, with shared test infrastructure in `tests/support/` (the
C harness), `tests/stub/` (the recording stub), `tests/fixtures/` (the
reduced conversion-map fixture), `tests/cmake/` (`cmake -P` script checks),
and `tests/scripts/` (install/package shell checks). The historical
per-issue entries under `docs/history/` retain the paths used when their
changes landed; use this map for current navigation.

## Repository state

The shim mirrors IMAS-Core's public C ABI, binds IMAS-Core at runtime rather than
linking it (ADR 0001), discovers the stored DD version from
`ids_properties/version_put/data_dictionary` at every occurrence open, and
translates read, write and delete paths *and values* across one hand-authored
equilibrium 3.39.0 ⇄ 4.1.1 conversion-map artifact. All 37 linkable IMAS-Core C
exports are forwarded; the 38th public header declaration,
`al_plugin_begin_timerange_action`, is deliberately absent because it is
unlinkable upstream, and `al_begin_array_struct_action` is not an IMAS-Core
symbol at all (the real name is `al_begin_arraystruct_action`). The export list
is compared mechanically with IMAS-Core's. Both conversion efforts — read (#43)
and write/delete (#122) — are implemented and validated against the recording
stub and against real, CMake-acquired IMAS-Core.

**A green suite is not a deployment mechanism:** nothing here places the shim in
front of a real HLI or in any HLI's runtime search path. See README.md's "Scope
and limitations".

### Where each seam stands

| Seam | Policy |
|---|---|
| `al_begin_dataentry_action` | registers its pulse in the context registry (ADR 0003) on success |
| `al_begin_global_action` (+ `al_plugin_*` twin) | discovers the stored version, then registers a root conversion record **only** when a present, valid stamp names a stored version that differs from the latched HLI version *and* has an embedded artifact to serve it (`src/conversion/known_artifacts.rs`). A matching or absent stamp registers nothing (ADR 0007); a malformed present stamp refuses and ends the just-opened context rather than leaking it (ADR 0009). `datapath` is translated only once a prior open of the same occurrence cached a mismatch. When the caller's `rwmode != READ_OP`, the stamp is read through a shim-owned `READ_OP` probe context of its own (ADR 0020) |
| `al_begin_slice_action`, `al_begin_timerange_action` | same discovery/registration rule; no `datapath` argument, so only the discovery half applies |
| `al_begin_arraystruct_action` (+ plugin twin) | resolves `path` and `timebase` before Core is called; on success registers the returned context as a child record inheriting the shared map, root identity and stored direction |
| `al_read_data` / `al_plugin_read_data` | one shared `read_data_impl`: identity, `renamed`, `moved`, and `merged`/`split` candidate plans tried in declared precedence order, COCOS sign flip applied in place, three-way read-outcome classification (ADR 0012), every non-exact success retained in the root's loss log |
| `al_write_data` / `al_plugin_write_data` | resolves `field` and `timebase` independently to one stored spelling, keeping relative/absolute child-context semantics and the caller's own `data`/`size`. An ambiguous plan writes **only** precedence 1 and records each skipped candidate's *stored* path as `POTENTIALLY_LOSSY` after Core succeeds; a non-primary source, an unservable rule, or a path with no stored slot refuses before Core is called. A value transformation executes on a shim-owned copy (ADR 0018) and leaves an unset rank-0 scalar alone, since `EMPTY_DOUBLE` is negative and flipping it would store a fabricated measurement with `code == 0` |
| `al_delete_data` | translates identity, `renamed` and `moved` leaves; fans a candidate plan out in declared order and calls Core for **every** candidate, with no presence probe (ADR 0017 — a write asserts a value, a delete asserts an absence, so where a write must not fan out a delete must; decision 2 records why the probe that used to precede each candidate is gone: it read through the *caller's* context, so a write-mode open reported every candidate absent). The first nonzero status is retained while later candidates are still attempted, so an absent candidate can look like a backend failure — the honest limitation of an ABI with no not-found outcome. Admits a *trivial* structure delete but refuses one with an escaping rule nested underneath it (decision 4); an empty path is the caller's explicit whole-DATAOBJECT migration route; never retains a loss entry |
| `al_end_action` / `al_plugin_end_action` | removes only its own context's record, only on success. Non-LIFO close and recycled context IDs are proven safe |
| `al_iterate_over_arraystruct`, `al_close_pulse` | plain forwards; neither touches the registry |
| `al_get_occurrences`, `al_list_filled_paths`, `al_bind_plugin`/`al_unbind_plugin` | deliberately **untranslated** (ADR 0002), proven to hold their passthrough contract while a read *and* a write convert |
| utility/version accessors, plugin registration/metadata/readback, parameter setters | verbatim forwards. `getDDVersion()` keeps returning Core's `"!!DEPRECATED!!"` sentinel even when the shim has just discovered that occurrence's stored version |
| any seam re-entered from underneath IMAS-Core | forwarded exactly as received — one thread-local counter across every seam Core can call back through, data-path family and plugin-manager entry points alike (ADR 0014). By then the path in flight is already a *stored* path, so resolving it again would translate it twice |

### Standing facts

- **Conversion is gated on the latched HLI DD version** (ADR 0005) — a `OnceLock`
  that never falls back, so `live_conversion_record` can short-circuit every
  data-path seam ahead of the registry lock when conversion is impossible. The
  *matching* and *unknown* cases still cost the one lookup ADR 0003 budgets,
  since neither is knowable without asking.
- **One artifact:** `docs/3.39.0--4.1.1.xml`, equilibrium 3.39.0 ⇄ 4.1.1,
  hand-authored (ADR 0004). `moved` and `retyped` resolve; `retyped` refuses
  unconditionally as `UnservableRetype` even where it declares itself *exact*,
  because the shim cannot reshape an int array into an array of identifier
  structures. Coverage floors are pinned in `cmake/tests/Common.cmake` (342 forward / 335
  reverse supported, each split `by rule` + `by identity default`) and gated by
  `tests/cmake/verify_artifact_coverage_floor.cmake` against real inventories
  (ADR 0013) with near-boundary fixtures generated inside the script.
- **Loss reaches the caller by a context log** (ADR 0012), drained without
  allocating through the shim-owned `imas_mvdd_context_loss_*` exports
  (`tests/abi/owned_exports.def`). A query on a child context resolves to its
  root; an untracked context reports zero rather than a refusal. The two entry
  kinds differ deliberately: a read loss and a refused write name *your* path, a
  successful write's leftovers name the *stored* ones.
- **Every refusal names reason, DD path, HLI version and stored version**, from
  one formatter, asserted as a single exact string via `CHECK_REFUSAL_MESSAGE`.
- **ADR 0011 — silence is earned by mechanism coverage.** Don't invent a rule for
  a case the shipped artifact cannot reach; an invented rule is uncovered code.
  `RefusalReason::Unmappable` and the glob match stage are both unreachable from
  the approved artifact, and tests assert that rather than assume it, failing with
  instructions to add real coverage if a future artifact makes either reachable.
- **ADR 0015 — seam policy never reaches global state.** See "Current path map"
  above: `src/conversion/` and `src/core/` know nothing about IMAS-Core or
  process-global state; only `src/interpose.rs` is C-facing.
- **Mutation-test with the test binary deleted first.** A stale build makes a red
  assertion look green, lagging by exactly one iteration.
- **Doc comments decay.** Any comment naming a ticket, a file under `tests/`, or
  another module's responsibilities is a claim with a shelf life, and two
  consecutive review rounds produced the same corrective sweep. The cheapest time
  to fix one is the PR that makes it false.
- **A compaction restates history as the present tense.** This file was condensed
  from a 93KB chronological `Update (issue #NNN)` log, and summarising that log
  faithfully reproduced claims the code had already falsified: a buried "left to
  #138" became a standalone **Open exposures** bullet, and a per-issue count
  became the **Counts** section, both asserting a world six commits out of date.
  Summarising is fidelity to the *old text*, not to the code. When a section here
  is rewritten, re-derive each claim from `src/`, `ctest -N` and the ADRs — the
  old wording is a draft, not a source.

### Open exposures

- **#139** — real IMAS-Core's `HDF5Writer::deleteData` ignores its `path`
  argument entirely and deletes the whole IDS pulse file plus its master-file
  link, so ADR 0017's per-path fan-out has no per-path effect on the only backend
  that implements delete at all. Nothing masks this any more: #138 removed the
  probe whose silence used to stop the fan-out before Core was reached, so a
  converted candidate-plan delete now destroys the occurrence, and
  `reverse-delete-fan-out-reaches-disk` pins that as today's behaviour rather
  than asserting it is desirable. Stated for users in README.md's "Scope and
  limitations".
- **`timebase` inherits the read path wholesale** (ADR 0016 decision 10) — it
  resolves independently of `field`, either one refusing refuses the write, and
  both feed the fidelity verdict. The named hazard — a write whose timebase
  resolved to a *different* candidate than the neighbours already in the
  occurrence, attaching its value to a different time basis — is unreachable in
  this artifact, where `time` is identity and no rule touches a timebase path.
  **The first conversion-map artifact that touches one must reopen the question
  rather than read this silence as a decision that it is safe.**
- **A refused write tears the time slice** (ADR 0019 decisions 4 and 5, filed as
  `yohannmarguier/IMAS-Fortran#61`, stated for users in README.md) —
  IMAS-Fortran's generated `put`/`put_slice` routines have no refusal-tolerance
  branch, and a shim refusal aborts the put where it stands with no rollback. On
  disk that leaves every leaf dataset unchanged and the `time_slice` container
  one element longer, because the caller's own `al_begin_arraystruct_action`
  widens it before any leaf write is attempted and Core commits that shape at
  end-action time regardless. A documented limitation of this shim, not a defect.
- **`rwmode` is not a policy input** (ADR 0016 decision 11) — the stamp decides
  whether conversion applies, never the access mode. ADR 0020 makes `rwmode` an
  input to *which context the stamp is read through*, and nothing more. This is
  sound only while scope stays append-only, so a write-mode open inherits a
  mismatch and never creates one.
- **Test-suite debt:** seven bare `52`-for-`DOUBLE_DATA` literals remain in
  `tests/shim/nested_context_read_test.c` (six) and
  `tests/shim/arraystruct_path_test.c` (one) although `tests/README.md` already
  mandates the `IMAS_*_DATA` macros — the grep shape is a small integer in an
  `al_read_data` datatype argument, e.g. `&data, 52,`. A half-finished migration
  whose earlier passes each claimed to be complete; verify by grep before
  claiming it again.

### Counts

`cargo test` 185 unit tests; real-Core ctest profile 226 tests, 30 of them
`real_core`-labelled; stub-only profile 192. Take these from `ctest -N` and
`cargo test`, never from a previous prose statement of them.

### History

The per-issue narrative that used to live in this section is preserved verbatim,
in landing order, under `docs/history/`:

| File | Covers |
|---|---|
| `docs/history/read-conversion-43.md` | runtime-binding baseline and read conversion — #54–#69, ADR 0014, and two rounds of `feat/path-conversion` review fixes |
| `docs/history/module-split-101.md` | the ADR 0015 module split — #105, #106, #109 |
| `docs/history/write-delete-122.md` | write and delete conversion — #123–#134, #136, ADRs 0016–0020, the on-disk oracle, and the `feat/delete-write` review fixes |

Each entry describes the tree as it was when it was written and several name
paths that have since moved; "Current path map" above is the authority on where
code lives today. The decisions of record are `docs/adr/0001`–`0020`.

## Build, toolchain and tests

Single crate at the repo root. Keep it that way until `imas-core-sys` lands — cargo allows only one package per `links` value, so the crate binding `libal` must be separate, and that is the moment to add `[workspace]` to `Cargo.toml` plus a `crates/` directory. Nothing moves when that happens.

**Language: Rust.** The C ABI artefacts (shared library, cbindgen-generated header, pkg-config file) are produced by [cargo-c]; CMake drives cargo-c rather than compiling anything itself. Toolchain on the ITER cluster comes from modules `Rust/1.88.0-GCCcore-14.3.0`, `cargo-c/0.10.15-GCCcore-14.3.0` and `IMAS-Core/5.7.1` — `source scripts/iter-env.sh`.

Real IMAS-Core is required by the default configure profile. CMake acquires it
in one of three modes (installed package lookup by default, development layout,
or download-and-build). CI's explicit `IMAS_MVDD_REAL_CORE_TESTS=OFF` profile
is the only stub-only path; it registers the recording-stub seams without
silently reducing the real-Core suite. See CMakeLists.txt's IMAS-Core
acquisition section for the option names and `IMAS_CORE_LIBRARY`-free test
wiring.

```console
$ cmake -S . -B build -DCMAKE_BUILD_TYPE=Release   # Debug → cargo `dev` profile
$ cmake --build build
$ ctest --test-dir build --output-on-failure       # rust-unit + abi-smoke + tracer (stub and real IMAS-Core)
$ cmake --install build --prefix /path/to/prefix
$ cargo fmt && cargo clippy --all-targets          # lint, no CMake wrapper
```

CI (`.github/workflows/ci.yml`) has a fast recording-stub job for fmt, clippy,
both CMake configurations, install and downstream consumption, plus a full job
on pull requests and `main` pushes that downloads and caches the pinned
IMAS-Core build before the drift and real-Core seams. It is the only thing
keeping the CMake path honest — `cargo test` alone never re-runs cargo-c, never
regenerates the header, and never compiles the C smoke test.

A third workflow, `.github/workflows/hli-validation.yml`, is the only place a
real HLI calls the shim: it builds the IMAS-Fortran fork pinned in
`IMAS_FORTRAN_REF` with `AL_USE_MULTIVERSION_SHIM=ON` against the *installed*
shim and runs that HLI's own suite — 83 per-IDS round-trips over memory, ASCII
and HDF5 for passthrough, plus `play_eq_two_dd-cross` for conversion. It runs on
pull requests based on `develop`/`main` (fail-safe `paths-ignore`) and on
`workflow_dispatch`. Two facts about it are easy to get wrong: IMAS-Core
deliberately **floats** (the HLI picks it; the shim's gate is major-only) while
`DD_VERSION` is **pinned to 4.1.1** because `src/known_artifacts.rs` embeds one
artifact, and 20 of the HLI's `examples/` tests can *never* run in a shim build,
so the workflow asserts the disabled count as well as the total. See
`docs/adr/0020-hli-validation-floats-core-and-pins-the-dd.md`.

`README.md` carries the build options and layout. The *why* behind the build
lives in comments next to what it explains — `CMakeLists.txt` for the staging
tree, the install path, the multi-config refusal and the IMAS-Core
acquisition modes, `Cargo.toml` for the `capi` feature and the workspace
question. Keep it there rather than restating it in prose that can drift.

[cargo-c]: https://github.com/lu-zero/cargo-c

Adding a C ABI test: include `tests/support/shim_test_support.h` rather than
writing a prologue. It owns `CHECK`/`CHECK_OK`, IMAS-Core's four data-type codes, the
recording-stub accessors (`string_from_stub`, `int_from_stub`,
`double_from_stub`, `pointer_from_stub`, over one `open_recording_stub`),
`open_mismatched_occurrence`, and the `{name, function}` scenario table that
`RUN_NAMED_SCENARIO` dispatches `argv[1]` through. Register the scenario with
`add_stub_test(<ctest-name> <executable> <scenario> [HLI_DD_VERSION v]
[STAMP_VERSION v] [ENV "KNOB=value"...])`, which owns the shared environment.
Twelve copies of that prologue and a hundred inlined environment strings is
where they came from, and one of those copies printed `\n` as text in four
suites' failure messages for months; a new copy starts that over.


Reference documents:
- `docs/IMAS-CORE_FUNCTIONALITY_INVENTORY.md` — the primary technical reference (938 lines). A per-capability, code-verified inventory of the IMAS-Core surface this project must mirror. Read this before designing anything.
- `docs/PROTOTYPE_CRITIC.md` — critique of the earlier `dd-maps/` + `middleware/` prototype: which of its choices were load-bearing and which should not be inherited without a decision.
- `CODE_OF_CONDUCT.md` — ITER's Contributor Covenant; contact `imas-administration@iter.org`.

`IMAS-CORE_FUNCTIONALITY_INVENTORY.md` cross-references `NORTH_STAR.md`, `CONTEXT.md`, `CLAUDE.md`, `docs/adr/0001-*.md`, `CMakeLists.txt` and `src/**` paths. **Those live in the separate IMAS-Core repository, not here.** Every `src/...:NNN` and `include/...` citation in that document is a pointer into IMAS-Core's tree. Don't try to resolve them locally, and don't treat their absence as a gap in this repo.

## What this project is

A **shim between the IMAS HLIs and IMAS-Core**:

```
HLI (imas-Fortran, imas-CPP; imas-Matlab and imas-Java not yet judged)
        │  compiled/configured against DD version V
        ▼
IMAS-Multiversion-DD-Loader   ← this project: mirrors IMAS-Core's public C ABI
        │  translates DD paths V ⇄ W
        ▼
IMAS-Core (libal)             ← low-level access layer, stores IDSs written under DD version W
```

**imas-python is not a client** — it converts DD versions itself and holds one DD version per `DBEntry` rather than one per process, so the shim's version latch does not apply to it and stacking the two would convert twice. The criterion is the client's shape, not its language: any caller of the C ABI holding one DD version for the life of the process and not converting on its own is a client, whatever it is written in. See `docs/adr/0005-hli-dd-version-entry-point.md`.

The core idea: **this project re-exports IMAS-Core's public C ABI verbatim** — same function names, same signatures, same `al_status_t` contract — and interposes between the mirrored functions. An HLI set up for DD 4.1.1 can then read an IDS stored under an earlier DD by having its path arguments rewritten on the way down and results rewritten on the way back up.

Conversion is **best-effort and explicitly lossy**. Fields that were removed, renamed with changed semantics, or sign-flipped (COCOS) between versions cannot always round-trip. Loss must be surfaced, not silently swallowed — but note the ABI leaves little room for it: `al_status_t` carries a single `int code` plus a `char message[256]`, and `code == 0` means success. Deciding how partial/lossy conversions are reported through that narrow channel is a core design question, not an implementation detail.

## Architecture: where the conversion seams are

Derived from the inventory — these are the ABI entry points that carry DD paths or IDS names and therefore need translation. Everything else can pass straight through.

**Down-converted (HLI's DD version → stored DD version):**

| Function | Path-bearing arguments |
|---|---|
| `al_begin_global_action` | `dataobjectname` (IDS name), `datapath` |
| `al_begin_slice_action` | `dataobjectname` |
| `al_begin_timerange_action` | `dataobjectname` |
| `al_begin_arraystruct_action` | `path`, `timebase` |
| `al_read_data` / `al_write_data` | `field`, `timebase` |
| `al_delete_data` | `path` |
| `al_get_occurrences` | `ids_name` |
| `al_list_filled_paths` | `dataobjectname` |
| `al_bind_plugin` / `al_unbind_plugin` | `fieldPath` |
| `al_plugin_*` reentry family | same path arguments as their non-`al_plugin_` twins |

**Up-converted (stored → HLI's DD version):** `al_list_filled_paths`'s returned `path_list` is the main one — it hands back DD paths that were written under the stored version and must be presented in the caller's version. Note the caller owns and must free both the list and every string in it; a shim that rewrites those strings takes on that ownership contract too.

Also relevant to path handling: `field`/`path` arguments are **relative to `ctxID` unless prefixed with `/`**, in which case they're absolute. A converter must handle both forms, and must know the enclosing context's path to resolve the relative case — meaning the shim has to track context state (`dectxID`/`octxID`/`actxID` → resolved DD path), not just rewrite strings statelessly. AOS iteration via `al_iterate_over_arraystruct` mutates that state.

## Constraints inherited from the mirrored ABI

Read the inventory for the full picture; these are the ones that most directly shape this project's design.

- **`getDDVersion()` is deliberately dead in IMAS-Core** — it returns the sentinel `"!!DEPRECATED!!"`, and an upstream test asserts it stays that way. This project **cannot** ask IMAS-Core which DD version a pulse was written under via that call. Determining the stored version is an open problem and a prerequisite for conversion.
- **`datapath` on `al_begin_global_action` is near-inert.** HDF5, MDSplus, Memory, ASCII, and Flexbuffers all ignore it. Only UDA in remote mode with `cache_mode=ids` actually honors it. Don't build a partial-get strategy on it.
- **`al_list_filled_paths` hard-fails on 4 of 6 backends** (MDSplus, Memory, ASCII, Flexbuffers throw unconditionally; only HDF5 has a real implementation, UDA delegates). If the conversion logic wants to discover what's actually stored, that discovery path only works against HDF5/UDA. Plan a fallback.
- **Two conflicting meanings of `0` — but not at the shim.** In `al_status_t.code`, `0` = success. In `Backend::readData` (`al_backend.h:138`) / plugin `read_data`'s `int` return, `0` = *not found* and `1` = success. The shim never sees the second convention: all 37 mirrored symbols return `al_status_t`, both `int`-returning layers sit below the C ABI, and `al_register_plugin` takes a plugin *name* rather than callbacks, so the shim cannot become a plugin. What the shim does have to get right is the three-way read outcome — failure (`code != 0`), not-found (`code == 0` with a null data pointer), and data — which ADR 0012 confines to a single classifier function.
- **`MAXDIM = 7`** (max array rank), **`MAX_ERR_MSG_LEN = 256`** (`al_status_t.message`).
- **Data types: `CHAR_DATA`, `INTEGER_DATA`, `DOUBLE_DATA`, `COMPLEX_DATA` only** — no boolean, no single-precision float. DD type changes across versions must land in one of these four.
- **Time-range and slice support is not universal.** Only HDF5 supports `supportsTimeRangeOperation()` unconditionally; MDSplus supports interpolation but *not* time range; UDA's support is gated on the remote server plugin version (`> 1.4.0`); Memory/ASCII/Flexbuffers support neither.
- Several upstream behaviors are outright bugs or silent degradations the inventory documents in detail (e.g. `al_plugin_begin_timerange_action` has a header/impl signature mismatch and is unlinkable; `al_setvalue_*_parameter_plugin` null-derefs on an unregistered plugin name; `al_unregister_plugin` only destroys plugins that were bound). When mirroring the ABI, decide *deliberately* per case whether to reproduce the upstream behavior or fix it at the shim — and record the decision.

## Working with DD versions

Use the **`imas-dd` MCP server** as the authority on DD content and inter-version differences — do not guess at path renames or reason from memory about what changed between versions.

- 35 versions in the chain, `3.22.0` … `4.1.1`; `4.1.0` is flagged as current. The 3.x → 4.0.0 boundary is the big breaking one.
- `get_dd_migration_guide(from_version, to_version)` — breaking changes, COCOS sign-flip tables, path renames, unit changes. This is the closest thing to a specification of what the conversion layer must implement. Use `summary_only=true` / `ids_filter` to keep responses manageable; unfiltered full-DD guides are large.
- `get_dd_changelog` — ranks paths by volatility across versions; useful for finding where conversion will hurt most.
- `check_dd_paths` / `search_dd_paths` / `get_dd_version_context` — validate that a specific path exists in a specific version.
- `get_dd_cocos_fields` — COCOS-sensitive fields, i.e. the ones where conversion is a sign transformation, not a rename.

A rename table alone is insufficient for correctness: unit changes and COCOS sign flips are *value* transformations that have to happen on the data buffers in `al_read_data`/`al_write_data`, not on the path strings.

## Agent skills

### Issue tracker

Issues are tracked on GitHub (yohannmarguier/IMAS-Multiversion-DD-Loader), via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default label vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout — `CONTEXT.md` + `docs/adr/` at the repo root, created lazily as terms/decisions get resolved. See `docs/agents/domain.md`.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
