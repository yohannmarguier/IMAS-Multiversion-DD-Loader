# tests/ — what is covered, and where

**198 ctest tests** (18 labelled `real-core`; the `IMAS_MVDD_REAL_CORE_TESTS=OFF`
stub-only profile registers 176). None of the C sources here is registered by
itself: every test is declared in `cmake/tests/{Common,Abi,Shim,RealCore}.cmake`,
**one ctest process per scenario**, because both the HLI DD version latch
(ADR 0005) and the context registry (ADR 0003) are process-wide state that
settles once. A scenario name maps to `<executable> <scenario-argument>`.

```console
$ ctest --test-dir build --output-on-failure    # everything
$ ctest --test-dir build -L real-core           # the 18 real-IMAS-Core ones
$ ctest --test-dir build -R read-path           # one group
$ ./build/read_path_test identity-rule-returns-data   # one scenario, directly
```

## Directory map

| Path | What lives there |
|---|---|
| `support/` | `shim_test_support.h` — the one shared C harness: `CHECK`/`CHECK_OK`/`CHECK_REFUSAL_MESSAGE`, IMAS-Core's four data-type codes, `open_recording_stub` plus the `{string,int,double,double_at,pointer}_from_stub` accessors, `open_mismatched_occurrence`, and the `{name, function}` scenario table `RUN_NAMED_SCENARIO` dispatches `argv[1]` through. Include this instead of writing a prologue. |
| `stub/` | `recording_stub.c` — a fake `libal` exporting the whole runtime-bound surface and recording what it received, including snapshots of write payloads whose shim-owned buffers are freed on return, so assertions are made on what crossed the boundary rather than inferred from a data round trip. ~23 `RECORDING_STUB_*` env knobs drive fixtures and failures (stamp version, not-found, sign-flip values, per-seam `*_FAIL` knobs, filled-paths CSV, reentrant reads and writes). |
| `shim/` | 11 C suites driving the public ABI against that stub — 169 tests. |
| `real_core/` | 3 C suites + a loadable C++ plugin fixture, against genuine CMake-acquired IMAS-Core and the checked-in equilibrium HDF5 fixture pair. |
| `abi/` | The linkage smoke test and three `.def` manifests that are the single source of truth for the mirrored surface: `abi_symbols.def` (37 mirrored symbols + expected fn-pointer types), `owned_exports.def` (the 4 `imas_mvdd_*` exports the shim owns), `abi_fallback_constants.def` (the id/name tables `core_binding.rs` hand-transcribes from `al_const.h`). |
| `cmake/` | `cmake -P` checks of the build/CI configuration itself, each with a guard-the-guard companion that proves it rejects what it claims. |
| `scripts/` | Install/packaging shell checks. **CI-only — not in ctest.** |
| `package/` | A downstream `find_package()` consumer project, used by `scripts/check-installed-package.sh`. |
| `fixtures/` | A deliberately reduced conversion-map artifact — the negative fixture for the coverage-floor gate. |

## Groups, in rough dependency order

### `runtime-binding-*` — 10 stub + 2 real · `shim/runtime_binding_test.c`

ADR 0001: the shim `dlopen`/`dlsym`'s IMAS-Core with local symbol visibility
instead of linking it. Covers successful resolution, tolerated minor drift,
refused major mismatch, a null `getALVersion`, a missing library, resolution by
bare soname through the loader's own search path, and verbatim forwarding of
the data, plugin and utility/version families. The `real-core` variants prove
the same against the genuine library.

> The stub is deliberately never linked into the test binary — that would give
> the linker two definitions of `al_context_info` to choose between, exactly
> the ambiguity runtime binding exists to avoid. The test `dlopen`s it itself,
> purely to read back recorded state.

### `hli-dd-version-*` — 10 · `shim/hli_dd_version_test.c`

ADR 0005: the process-wide DD-version latch. `imas_mvdd_set_hli_dd_version`
accepting a valid version and an identical repeat, rejecting a conflicting
repeat / invalid / null; concurrent identical setters; the
`IMAS_MVDD_HLI_DD_VERSION` environment fallback and its precedence rules; an
invalid environment value failing the first open; a setter refused after an
unset first open has already latched.

### `version-discovery-*` — 22 · `shim/version_discovery_test.c`

Stamp discovery and registration at `al_begin_{dataentry,global,slice,timerange}_action`
(ADR 0002/0007/0009/0012, issues #53 and #55). Per seam: unstamped and matching
stamps forward unchanged and register nothing; a mismatch registers the
occurrence; a **malformed stamp refuses *and* ends the just-opened context**
rather than leaking it; a Core failure forwards its status unchanged; an unset
HLI version is a plain forward.

> There is no C-level registry introspection, so "the mismatch was registered"
> is proven the only way it is externally observable: a *second* open of the
> same occurrence translates `datapath` before Core is called.

### `read-path-*` — 39 · `shim/read_path_test.c`

`al_read_data`, the main conversion seam (issues #56 and #65, ADR 0014).

- **Translation** — `field` and `timebase` resolved independently, identity and
  `renamed` rules, relative vs absolute arguments, both directions.
- **Candidate plans** — `merged`/`split` falling through to the next candidate,
  stopping at the first with data, all-absent → not-found, and no-source
  returning null without ever calling Core.
- **Value transformation** — COCOS sign flip on a double array (empty array
  preserved), rank/shape validation, `MAXDIM`, not-found skipping the flip,
  refusal without touching the caller's buffer.
- **Refusals before Core** — rank-changing retype, unit redefinition,
  unsupported sign-flip data types.
- **Reentrancy** — a read arriving beneath an in-flight read is forwarded
  untouched and does not re-apply a sign flip.
- **Bypass** — matching, unknown, unstamped and conversion-disabled contexts.
- **Loss log** — lossy `merged`/`moved` reads retained, log destroyed with its
  context, plus the ten safety refusals of the `imas_mvdd_context_loss_*` query
  exports (null output, negative / out-of-range index, short buffer).

### `arraystruct-path-*` — 8 · `shim/arraystruct_path_test.c`

`al_begin_arraystruct_action` (issue #61): renamed container `path` and
`timebase` translated before Core is called, absolute/relative mixes, a
no-source refusal, a failed open leaving no child record, and the four
forwarding cases (matching / unstamped / unknown / conversion disabled).

### `nested-context-read-*` — 8 · `shim/nested_context_read_test.c`

`al_read_data` through a live AOS child (issues #62 and #66): a relative
argument resolving beneath a child whose *own* anchor is itself renamed,
absolute resolution from the IDS root regardless of that anchor, no-source,
refusal, sign flip — and the #66 fix: a nested non-exact read retains the
**complete joined DD path** on its root's loss log, queryable from either the
child or the root, surviving a non-LIFO close.

### `context-lifecycle-*` — 7 · `shim/context_lifecycle_test.c`

`al_end_action` / `al_iterate_over_arraystruct` / `al_close_pulse` (issue #63):
ending a child or a root removes only its own record, a refused close keeps the
record intact, a recycled context ID never exposes a stale record, and the
latter two seams forward unchanged without touching the registry. Observed
indirectly, via whether a later read still translates.

### `write-delete-*` — 33 · `shim/write_delete_conversion_test.c`

Issue #125's safe write slice: `al_write_data` and `al_plugin_write_data`
independently resolve identity, `renamed`, and `moved` field/timebase paths to
one stored spelling, preserving relative/absolute child-context semantics and
caller-owned `data`/`size`. Issue #127 adds COCOS writes: the policy sends Core
a sign-flipped, shim-owned copy (including rank 7) while preserving caller
storage; an unset rank-0 sentinel forwards unchanged so Core keeps its own
skip behaviour. Issue #128 writes only an ambiguous plan's precedence-1
candidate, records every skipped candidate as a `POTENTIALLY_LOSSY` `WRITE`
loss after Core succeeds, and refuses a non-primary source even where its
artifact entry is not deprecated; a child keeps those losses at its root under
the complete HLI path. Candidate deletes and the DD-version stamp still refuse
before Core; the stamp's access-layer siblings still forward. Matching,
unstamped, unknown and conversion-disabled contexts forward unchanged.

Issue #126 adds the impossible-write proof: both fixture directions refuse a
field with no stored slot, and a retyped field refuses before Core. Each refusal
keeps caller storage untouched, records an `UNMAPPABLE` `WRITE` loss, and a
child-context refusal reaches its root under the complete joined DD path.

Issue #129 translates identity, `renamed`, and `moved` leaf deletes to one
stored spelling in both directions; it refuses the DD-version stamp and
containing subtrees, non-primary aliases, no-source and unservable paths, and
candidate plans. An empty delete forwards as the caller's explicit whole-
DATAOBJECT migration route, and delete never retains a loss-log entry.

### `plugin-reentry-policy-*` — 22 · `shim/plugin_reentry_policy_test.c`

The `al_plugin_*` reentry twins carry the same policy as their ordinary
counterparts (issues #67 and #68): discovery / registration / translation /
malformed-stamp refusal for plugin global and slice actions, translation and
refusal for the plugin arraystruct action, record removal-on-success and
retention-on-failure for plugin end action, and the full read policy for
`al_plugin_read_data` (translation, refusal, no-source, merged fallthrough,
sign flip, loss retention through a child context).

### `scoped-passthrough-*` — 4 · `shim/scoped_passthrough_test.c`

The outside edge of the seam list (issue #69). With a mismatched occurrence open
and demonstrably converting, `al_get_occurrences`, `al_list_filled_paths` (both
directions) and `al_bind_plugin`/`al_unbind_plugin` must still forward
unchanged, as must every remaining non-seam export. Their arguments are
deliberately the two spellings of a real rename rule, so a shim that started
rewriting them would produce a visibly different string rather than pass by
coincidence. Also pins `getDDVersion()`'s `"!!DEPRECATED!!"` sentinel.

### `equilibrium-read-*` — 17, `real-core` · `real_core/equilibrium_read_test.c`

The same conversion policy against genuine IMAS-Core and the checked-in
equilibrium HDF5 fixture pair, in **both** fixture directions: a renamed scalar
read through the HLI's own spelling, renamed and sign-flipped fields nested
under `time_slice`, `merged` and `split` read plans, refusals for an unmappable
`redefine` and for the artifact's one `retyped` rule (lossless in principle,
unavailable in practice), the remaining mismatched delete refusal across a
real boundary, a real context lifecycle, and the two no-op cases (same
version, conversion disabled). Safe writes are asserted at the recording-stub
boundary, where their translated Core arguments are directly observable.
Scenarios sharing a fixture directory hold a ctest
`RESOURCE_LOCK`, because of HDF5's own file locking. One harness scenario uses
an isolated temporary copy instead: it reads the copied DD-version stamp and a
numeric dataset through raw HDF5, then re-proves a translated read against that
copy, leaving the checked-in pair untouched.

### `runtime-binding-real-core-forwarding` — 1, `real-core` · `real_core/real_core_forwarding_test.c`

Every mirrored symbol driven through a legal HDF5 lifecycle against real
IMAS-Core: slice and time-range reads, arraystruct reads, utility/version
accessors, plugin registration/binding/parameters/reentry (via the
`real_core_test_plugin.cpp` fixture), a plugin-seam read across a real version
mismatch, malformed-stamp refusals seeded by writing bad DD metadata with raw
HDF5, and an unstamped read (the stamp is deleted from a copied fixture) proving
the stored spelling reaches the value while the HLI's own spelling is *not*
rewritten into it.

### `abi-smoke`, `real-core-abi`, `real-core-abi-rejects-mismatch`, `real-core-export-list`

The ABI contract itself.

- `abi_smoke` links a plain C translation unit against the cargo-c output using
  only the generated header.
- `real_core_abi_check` compiles the expected signatures in two separate TUs —
  one against IMAS-Core's headers, one against the shim's generated header —
  since runtime `dlsym` cannot type-check hand-written signatures. It also
  verifies every hand-transcribed fallback constant against the real macro, and
  every fallback string against the real `const2str`/`err2str`.
- `-rejects-mismatch` guards that guard: it rebuilds the checker against a
  header with a deliberately wrong constant and requires the compiler to reject
  it.
- `real-core-export-list` compares shim and Core exports mechanically with `nm`,
  so a new upstream symbol or leftover shim-only scaffolding cannot hide.

### `rust-unit`

`cargo test` — the crate's own unit tests: conversion-map resolution,
read-outcome classification, registry behaviour, path joining, and the branches
no C-ABI test can reach.

### `equilibrium-artifact-coverage-floor`

Runs the `validate_equilibrium_coverage` binary **as a command**, not as an
internal helper: parses per-direction supported/total counts, checks them
against the inventory files and the floors pinned in the top-level
`CMakeLists.txt`, checks that `by rule` + `by identity default` accounts for
`supported`, derives the IMAS-Python rename baseline from the TSV, runs the
completeness check, and requires rejection of two near-boundary fixtures
generated inside the script (the approved artifact minus exactly one rule) so
the gate cannot pass by matching a substring.

### `ci-workflow`, `script-policy-versions` (+ their two guards)

Configuration-as-tested.

- `check_ci_workflow.cmake` parses `.github/workflows/ci.yml` and the toolchain
  action to assert the fast job never acquires real IMAS-Core, that the required
  steps sit in the right jobs, and that branch pushes cannot bypass the fast job.
- `check_script_policies.cmake` asserts every `cmake -P` script here pins its
  own `cmake_minimum_required` — script mode inherits no policies, and CMake 4.x
  defaults them to NEW while CI's 3.31 does not, which once let an unpinned
  `IN_LIST` pass locally and fail only on CI.
- Each has a `verify_*_guard` companion feeding it throwaway mutated fixtures,
  to prove it rejects them.

### `scripts/` — CI only, not in ctest

| Script | Property under test |
|---|---|
| `check-installed-package.sh` | The installed prefix is consumable downstream via `find_package` (`package/find_package/`), and carries nothing it should not. |
| `check-staged-install.sh` | `DESTDIR` staging: the generated `.pc` must name the final prefix, not the staging directory. |
| `check-relative-prefix-install.sh` | `--prefix <relative>` run from a third directory — CMake resolves it against the cwd while cargo-c joins it onto the source tree, which used to split the install silently. |

## Adding a test

Include `support/shim_test_support.h`, add a `{name, function}` row to the
scenario table, and register it in `cmake/tests/Shim.cmake`:

```cmake
add_stub_test(<ctest-name> <executable> <scenario>
    [HLI_DD_VERSION v] [STAMP_VERSION v] [ENV "KNOB=value"...])
```

That function owns the shared environment (`IMAS_CORE_LIBRARY`, the latched HLI
version, the stub's stamp version). Do not copy a prologue: twelve copies of one
is where the shared harness came from, and one of those copies printed a literal
`\n` in four suites' failure messages for months.

Two standing cautions:

1. **A green local run proves less than it looks.** The `cmake -P` scripts
   behave differently under a local CMake 4.x than under CI's 3.31 pin, and the
   stub-only profile silently skips 21 tests.
2. **Never pass a small ordinal as a data type.** The stub-only profile has no
   `al_const.h` to include, so `support/shim_test_support.h` defines
   `IMAS_CHAR_DATA` / `IMAS_INTEGER_DATA` / `IMAS_DOUBLE_DATA` /
   `IMAS_COMPLEX_DATA` as 50/51/52/53. Use those — twenty-one call sites once
   passed 2/3/4 under a comment naming the right constant.
