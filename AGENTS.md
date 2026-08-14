# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.
If it has to been modified, apply the same changes to AGENTS.md.

## Repository state

**Runtime binding proven, all 37 linkable IMAS-Core C exports forwarded; the data-entry, global-action, slice-action and time-range-action seams discover and register, and ordinary root reads now translate field/timebase pairs.** The build system is verified end to end, and `src/resolve.rs` / `src/dl.rs` prove the runtime-binding architecture (see `docs/adr/0001-runtime-binding-not-linking.md`): the shim opens IMAS-Core with local symbol visibility via hand-rolled `dlopen`/`dlsym` bindings, checks `getALVersion()` against the version it was built against, and forwards the call — verified by `tests/runtime_binding_test.c` against a recording stub (`tests/stub/`) standing in for IMAS-Core and against a real, CMake-acquired IMAS-Core. `al_context_info`, the utility/version accessors, the thirteen data-entry/action-lifecycle/data-operation symbols, and the seventeen plugin registration, binding, metadata, parameter-setter and reentry symbols forward unchanged this way. The 38th public header declaration, `al_plugin_begin_timerange_action`, is deliberately absent because it is unlinkable upstream; `al_begin_array_struct_action` is not an IMAS-Core symbol (the real name is `al_begin_arraystruct_action`). The export list is compared mechanically with IMAS-Core's. `src/conversion_map.rs` parses a caller-supplied hand-authored equilibrium 3.39.0 ⇄ 4.1.1 conversion-map artifact (`docs/3.39.0--4.1.1.xml`, ADR 0004) and resolves the document-level identity default, `renamed` path-level rules, and `merged`/`split` rules (the latter two as an ordered `CandidatePath` read plan on their ambiguous side, per ADR 0006) to a `RuleExplanation`; `moved`, `retyped`, `left_only` and `right_only` parse structurally but are not yet matched. `src/read_outcome.rs` is the one classifier turning `al_status_t` plus a returned data pointer into failure/not-found/data (ADR 0012); `src/version_stamp.rs` uses it to read and decode `ids_properties/version_put/data_dictionary` (CHAR_DATA, `dim == 1`, sized rather than NUL-scanned) immediately after `al_begin_global_action` opens. `al_begin_dataentry_action` registers its pulse in the context registry (`src/context_registry.rs`, ADR 0003) on success; `al_begin_global_action`, `al_begin_slice_action` and `al_begin_timerange_action` all register a root conversion record through the one shared `discover_and_register_occurrence` rule (`src/resolve.rs`, issue #55) only when a present, valid stamp names a stored DD version that both differs from the latched HLI DD version (ADR 0005) and has an embedded artifact to serve it (`src/known_artifacts.rs`, the one equilibrium 3.39.0⇄4.1.1 artifact until a generator exists) — a matching or absent stamp registers nothing (ADR 0007), and a malformed present stamp refuses and ends the just-opened context rather than leaking it (ADR 0009). The IDS name is forwarded unchanged at every one of these seams. `datapath` on `al_begin_global_action` is translated only once a prior open of the same occurrence already cached a mismatch; on first use it forwards unchanged, since the version that would justify translating it is not yet known at the point IMAS-Core must be called — slice and time-range actions carry no `datapath` argument, so only the discovery/registration half of the rule applies to them. `al_end_action` removes only its own context's record on success. `al_begin_arraystruct_action` resolves its `path` and `timebase` through its parent record before IMAS-Core is called and, on success, registers the returned context ID as a child conversion record inheriting the shared map, root identity, and stored-direction (issues #54 and #61); `al_read_data` resolves each nonempty `field` and `timebase` independently through that map before IMAS-Core is called (issue #56). Explicit identity and renamed rules reach their stored spelling, no-source returns success with a null data pointer without a Core call, and relative/absolute arguments resolve from the context/IDS roots respectively; matching, unknown, unstamped, and conversion-disabled contexts remain plain forwards. The scalar tracer uses HLI-owned storage as IMAS-Core's scalar ABI requires; the shim preserves that pointer unchanged. Still unimplemented: reading a `merged`/`split` candidate plan (issue #57), executing a value transformation such as a COCOS sign flip (issue #59).

**Update:** `al_read_data` now executes `merged`/`split` read plans in declared candidate order in either stored-direction, classifies each result through the shared read-outcome rule, and applies the selected COCOS sign flip in place while retaining its loss record. AOS container path translation remains limited to concrete, untransformed paths.

**Update (issue #65):** every non-exact successful read — a single translated `renamed`/`moved` path as well as a `merged`/`split` candidate plan — now reaches its root context's loss log (`src/context_registry.rs`, ADR 0012); a fast-path short-circuit that previously skipped logging for the single-path case is gone. Two new shim-owned exports, `imas_mvdd_context_loss_count` and `imas_mvdd_context_loss_at` (`tests/owned_exports.def`), let a caller drain that log without allocating: a query on a child context resolves to its root, an untracked context reports zero rather than a refusal, and `loss_at` copies one path and its fidelity verdict into caller storage, refusing safely on a null output, a negative index/length, an out-of-range index, or a buffer too small for the path.

**Update (issue #62):** `al_read_data` through a live arraystruct context — opened one or more levels deep by `al_begin_arraystruct_action` — was already generic over root and child contexts (`resolve::resolve_read_path` anchors every relative argument on `ConversionRecord::resolved_path`, whichever context that record belongs to, and `resolve::stored_anchor` translates a *renamed* child anchor before stripping it back off a resolved stored path); this issue proves it rather than changing it. `tests/nested_context_read_test.c` covers the one path no earlier issue exercised — a relative field/timebase resolving beneath a child whose own anchor is itself a `renamed` rule, not just an identically-spelled one like `time_slice` — plus anchor-independent absolute resolution, no-source, refusal, and a COCOS sign flip, all addressed relative to a live child context. `tests/equilibrium_read_test.c` adds the matching real-Core fixture proof: `constraints/bpol_probe`/`constraints/b_field_pol_probe` (renamed, literal `0.42`) and `constraints/flux_loop` (identically spelled, COCOS-flipped `measured`, literal `1.15`/`-1.15`), both nested beneath `time_slice`. The remaining two acceptance criteria are architectural rather than ABI-observable and are discharged by inspection, not a new test: the registry lock is already proven to release before any conversion work in `src/context_registry.rs`'s `child_lookup_releases_the_lock_before_returning` (issue #60), and `ContextRegistry`'s public interface has no ancestry-walking method at all — a child record resolves its root identity, pulse context ID, and shared conversion map from a live parent *snapshot* taken once at `al_begin_arraystruct_action` time, never at read time.

**Update (issue #63):** `al_end_action`, `al_iterate_over_arraystruct`, and `al_close_pulse` already had the registry behavior these acceptance criteria describe — non-LIFO close, recycled context IDs, and plain forwarding for the latter two were never a distinct code path, only unproven at the public ABI seam. `tests/context_lifecycle_test.c` (new) proves each one the same indirect way `version_discovery_test.c` and `nested_context_read_test.c` already do — there is no C-level registry introspection, so a later `al_read_data` through a still-live context either translates (record intact) or forwards unchanged (record gone/never existed): ending a child leaves its parent's record live and vice versa, a refused close (`RECORDING_STUB_END_ACTION_FAIL`, new stub knob) leaves the record intact, a raw context ID recycled onto a matching-version reopen never exposes the ended occurrence's stale record, `al_iterate_over_arraystruct` forwards `aosctx`/`step` unchanged without touching the registry, and `al_close_pulse` forwards `pulseCtx`/`mode` unchanged without clearing the pulse's occurrence-version cache. Concurrent close/read safety on one IMAS-Core context was already out of scope by design (ADR 0003).
**Update (issue #64):** `al_write_data`, `al_delete_data`, and the matching plugin reentry `al_plugin_write_data` now refuse before IMAS-Core is called whenever `ctx_id` carries a live conversion record — a known mismatched root or a child context that inherited one — per `docs/adr/0002-read-path-seam-policy.md`'s stated rule ("If known versions differ, return failure without calling IMAS-Core"). The refusal is a blanket, context-keyed check (`ContextRegistry::lookup`) that never resolves `field`/`timebase`/`path` through the conversion map, so write-path translation remains unintroduced. Matching, unknown, unstamped, and conversion-disabled contexts carry no record and forward unchanged, as before. `tests/write_delete_conversion_test.c` covers direct write/delete refusal under a known mismatched root and through a nested arraystruct child, the plugin-write refusal, plus the four forwarding cases, and confirms the caller's `data`/`size` storage is left untouched by a refusal.

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

`README.md` carries the build options and layout. The *why* behind the build
lives in comments next to what it explains — `CMakeLists.txt` for the staging
tree, the install path, the multi-config refusal and the IMAS-Core
acquisition modes, `Cargo.toml` for the `capi` feature and the workspace
question. Keep it there rather than restating it in prose that can drift.

[cargo-c]: https://github.com/lu-zero/cargo-c

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
