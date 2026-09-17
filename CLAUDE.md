# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.
If it has to been modified, apply the same changes to AGENTS.md.

## Current path map

Current source ownership is `src/core/`, `src/conversion/`, `src/loss.rs`,
`src/loss_file.rs`, `src/registry/`, and `src/version/`; C ABI adaptation lives
under `src/interpose/`. `src/loss_file.rs` owns the append-only process loss-log
file and its written-key set; it receives copied occurrence facts and never
holds a registry lock during filesystem I/O.

The read, write and delete **loops** live in `src/conversion/seam_policy.rs`,
not in the interposition layer: `run_read`, `run_write`, `run_delete`, the
`ReadAttempt` type, the `impl TranslatedReadPath` block that produces
attempts, and `validate_value_transformation` /
`apply_value_transformation` are all there, and none of them reaches
IMAS-Core or process-global state (ADR 0015).
`src/conversion/path_conversion.rs` answers *which stored path does this HLI
argument mean, and at what fidelity* and knows about neither seams nor
IMAS-Core.

`src/interpose.rs` itself holds **nothing but module declarations and
`pub(crate) use` re-exports** — the surface `lib.rs` reaches through
`use interpose as resolve;` (issue #153). One module per seam family under
`src/interpose/`:

| Module | Owns |
|---|---|
| `occurrence.rs` | `begin_dataentry_action`, the global/slice/timerange/arraystruct opening seams and their plugin twins, `end_action`, plus stamp discovery (`discover_stamp`, `probe_stamp_through_a_read_context`), registration (`apply_discovery_decision`, `apply_occurrence_cache_effect`), the conversion-map cache (`resolve_conversion_map`, `map_cache_key`, `load_artifact`) and `resolve_arraystruct_argument` |
| `read.rs` | `read_data` / `plugin_read_data` and their shared `read_data_impl` |
| `write.rs` | `write_data` / `plugin_write_data` and their shared impl |
| `delete.rs` | `delete_data` and `candidate_failure` |
| `loss.rs` | the shim-owned `imas_mvdd_context_loss_*` exports |
| `passthrough.rs` | the ADR 0002 untranslated seams and the verbatim forwards, including `close_pulse` |
| `dispatch.rs` | `CallFamily` and its ABI-symbol dispatch |
| `reentry.rs` | the ADR 0014 depth gate |
| `refusal.rs` | the one refusal formatter — `context_path_refusal`, `contextual_refusal` and `live_conversion_record` — plus the raw-argument marshalling they need (`c_str_ref`, `c_str_or_none`, `joined_argument_path`, `read_argument_path`) |

C tests are
grouped under `tests/abi/`, `tests/shim/`, `tests/real_core/`, and
`tests/package/`, with shared test infrastructure in `tests/support/` (the
C harness), `tests/stub/` (the recording stub), `tests/fixtures/` (the
reduced conversion-map fixture), `tests/cmake/` (`cmake -P` script checks),
and `tests/scripts/` (install/package shell checks plus the hermetic
DD-graph setup lifecycle check). The historical
per-issue entries under `docs/history/` — and the ADRs under `docs/adr/`,
which are dated records of a decision rather than navigation aids — retain the
paths used when they were written; use this map for current navigation.

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
| `al_begin_global_action` (+ `al_plugin_*` twin) | `seam_policy::decide_occurrence_registration` decides stored-version discovery and registration, while its sibling `decide_datapath_translation` decides the pre-forward translation from a cached mismatch; both occurrence-opening policy functions live in `src/conversion/seam_policy.rs`. A root conversion record is registered **only** when a present, valid stamp names a stored version that differs from the latched HLI version *and* has an embedded artifact to serve it (`src/conversion/known_artifacts.rs`). A matching or absent stamp registers nothing (ADR 0007); a malformed present stamp refuses and ends the just-opened context rather than leaking it (ADR 0009). `datapath` is translated only once a prior open of the same occurrence cached a mismatch. When the caller's `rwmode != READ_OP`, the stamp is read through a shim-owned `READ_OP` probe context of its own (ADR 0020) |
| `al_begin_slice_action`, `al_begin_timerange_action` | same discovery/registration rule; no `datapath` argument, so only the discovery half applies |
| `al_begin_arraystruct_action` (+ plugin twin) | resolves `path` and `timebase` before Core is called; on success registers the returned context as a child record inheriting the shared map, root identity, stored direction, and `opened_read_op`. A `renamed`/`moved` anchor translates to one stored spelling; a `merged`/`split` anchor whose candidates carry no value transformation is a candidate plan the seam itself decides how to use (issue #178, ADR 0025) — under `READ_OP` it tries each stored candidate against Core in declared precedence order, keeping the first that reports a populated array (or the last, if every candidate comes back empty, since a wholly empty subtree is not a refusal); any other access mode takes the declared primary without trying the rest, mirroring `al_write_data`'s own ambiguous-plan policy. Whichever candidate actually opens is remembered as the child's own `stored_path`, since a merged anchor has no single map-derivable stored spelling the way a renamed one does — a relative argument under it filters out any sibling candidate that does not lie beneath that fixed anchor rather than refusing the whole read. A refusal here retains an `UNMAPPABLE` read loss, closing the one gap left after write and delete refusals already did |
| `al_read_data` / `al_plugin_read_data` | one shared `read_data_impl`: identity, `renamed`, `moved`, and `merged`/`split` candidate plans tried in declared precedence order, COCOS sign flip applied in place, three-way read-outcome classification (ADR 0012), every non-exact success retained in the root's loss log. A `dim == 0` candidate is classified through the EMPTY sentinel in the caller's own buffer rather than the data pointer, since a scalar read has no null-pointer channel; an exhausted scalar plan returns that sentinel where an array plan reports not-found. A shim-decided not-found — the artifact says the path has no stored source, so IMAS-Core is never called — leaves the caller's buffers as `Lowlevel::setDefaultValue` would: for `dim > 0` a null `*data` and every returned extent zeroed, for `dim == 0` the datatype's EMPTY sentinel written into the caller's own scalar, which is absence's only channel at rank zero |
| `al_write_data` / `al_plugin_write_data` | resolves `field` and `timebase` independently to one stored spelling, keeping relative/absolute child-context semantics and the caller's own `data`/`size`. An ambiguous plan writes **only** precedence 1 and records each skipped candidate's *stored* path as `POTENTIALLY_LOSSY` after Core succeeds; a non-primary source, an unservable rule, or a path with no stored slot refuses before Core is called. A value transformation executes on a shim-owned copy (ADR 0018) and leaves an unset rank-0 scalar alone, since `EMPTY_DOUBLE` is negative and flipping it would store a fabricated measurement with `code == 0` |
| `al_delete_data` | translates identity, `renamed` and `moved` leaves; fans a candidate plan out in declared order and calls Core for **every** candidate, with no presence probe (ADR 0017 — a write asserts a value, a delete asserts an absence, so where a write must not fan out a delete must; decision 2 records why the probe that used to precede each candidate is gone: it read through the *caller's* context, so a write-mode open reported every candidate absent). The first nonzero status is retained while later candidates are still attempted, so an absent candidate can look like a backend failure — the honest limitation of an ABI with no not-found outcome. A refusal retains an `UNMAPPABLE` loss naming the caller path; a fan-out retains `POTENTIALLY_LOSSY` delete losses naming every stored candidate after all are attempted. Admits a *trivial* structure delete but refuses one with an escaping rule nested underneath it (decision 4); an empty path is the caller's explicit whole-DATAOBJECT migration route |
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
  structures. The four `redefine` entries over
  `constraints/{strike_point,x_point}/chi_squared_{r,z}` were removed after
  review: those paths forward verbatim and the shim corrects no units.
  Coverage floors are pinned in `cmake/tests/Common.cmake` (346 forward / 339
  reverse supported, each split `by rule` + `by identity default`) and gated by
  `tests/cmake/verify_artifact_coverage_floor.cmake` against real inventories
  (ADR 0013) with near-boundary fixtures generated inside the script.
- **Loss reaches the caller by a context log** (ADR 0012), drained without
  allocating through the shim-owned `imas_mvdd_context_loss_*` exports
  (`tests/abi/owned_exports.def`). A query on a child context resolves to its
  root; an untracked context reports zero rather than a refusal. The two entry
  kinds differ deliberately: a read loss and a refused write name *your* path, a
  successful write's leftovers name the *stored* ones. A refused context open
  logs as a read loss too (issue #178, ADR 0025) — it was the one shim-decided
  refusal that used to reach neither the log nor the loss log file, while a
  refused write and a refused delete already did both.
- **Every refusal names reason, DD path, HLI version and stored version**, from
  one formatter, asserted as a single exact string via `CHECK_REFUSAL_MESSAGE`.
- **ADR 0011 — silence is earned by mechanism coverage.** Don't invent a rule for
  a case the shipped artifact cannot reach; an invented rule is uncovered code.
  `RefusalReason::Unmappable`, `RefusalReason::UnitRedefinition` and the glob
  match stage are all unreachable from the approved artifact, and tests assert
  that rather than assume it, failing with instructions to add real coverage if a
  future artifact makes one reachable. `UnitRedefinition` joined that list when
  the four chi_squared `redefine` entries were removed, so its only coverage is
  now synthetic (`a_redefine_entry_refuses_a_default_matched_path` and its
  `_an_explicitly_matched_path` sibling, one per call site).
- **ADR 0015 — seam policy never reaches global state.** See "Current path map"
  above: `src/conversion/` and `src/core/` know nothing about IMAS-Core or
  process-global state; only `src/interpose/` is C-facing.
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

- **#139 — corrected by the pinned Core fork.** `IMAS_CORE_REF` now includes
  IMAS-Core #64's path-aware HDF5 delete fix. The real-Core delete oracle
  verifies both stored candidates disappear while unrelated data and the
  stamp survive. Older Core builds, including upstream 5.7.2, still delete
  the whole occurrence; ABI version compatibility does not guarantee the fix.
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
code lives today. The decisions of record are `docs/adr/0001`–`0026`.

## Build, toolchain and tests

Single crate at the repo root. Keep it that way until `imas-core-sys` lands — cargo allows only one package per `links` value, so the crate binding `libal` must be separate, and that is the moment to add `[workspace]` to `Cargo.toml` plus a `crates/` directory. Nothing moves when that happens.

**Language: Rust.** The C ABI artefacts (shared library, cbindgen-generated header, pkg-config file) are produced by [cargo-c]; CMake drives cargo-c rather than compiling anything itself. Toolchain on the ITER cluster comes from modules `Rust/1.88.0-GCCcore-14.3.0`, `cargo-c/0.10.15-GCCcore-14.3.0` and `IMAS-Core/5.7.1` — `source scripts/iter-env.sh`.

Real IMAS-Core is required by the default configure profile. CMake acquires it
in one of three modes (installed package lookup by default, development layout,
or download-and-build). CI's explicit `IMAS_MVDD_REAL_CORE_TESTS=OFF` profile
is the only stub-only path; it registers the recording-stub seams without
silently reducing the real-Core-gated set. See CMakeLists.txt's IMAS-Core
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
on pull requests and `main` pushes that downloads and caches the IMAS-Core fork
at the committed `IMAS_CORE_REF` before the drift and real-Core seams. It is the
only thing keeping the CMake path honest — `cargo test` alone never re-runs
cargo-c, never regenerates the header, and never compiles the C smoke test.

A third workflow, `.github/workflows/hli-validation.yml`, runs real
HLIs through the shim — one job each for Fortran, C++, MATLAB and Java, pinned
in `IMAS_FORTRAN_REF`, `IMAS_CPP_REF`, `IMAS_MATLAB_REF` and `IMAS_JAVA_REF`.
Its Fortran job builds the IMAS-Fortran fork pinned in
`IMAS_FORTRAN_REF` with `AL_USE_MULTIVERSION_SHIM=ON` against the *installed*
shim and runs that HLI's own suite — 83 per-IDS round-trips over memory, ASCII
and HDF5 for passthrough, plus `play_eq_two_dd-cross` for conversion. It runs on
pull requests based on `develop`/`main` (fail-safe `paths-ignore`) and on
`workflow_dispatch`. Three facts about it are easy to get wrong: it acquires
the same IMAS-Core fork and committed `IMAS_CORE_REF` as the `full` CI job,
`DD_VERSION` is **pinned to 4.1.1** because `src/known_artifacts.rs` embeds one
artifact, and 20 of the HLI's `examples/` tests can *never* run in a shim build,
so the workflow asserts the disabled count as well as the total. See
`docs/adr/0026-pin-imas-core-until-upstream-corrects-delete.md` for why Core is
pinned rather than floated.

Its C++ job builds `yohannmarguier/IMAS-Cpp` at `IMAS_CPP_REF` against the
installed shim and the same Core fork pin, with DD 4.1.1. The generated C++
suite only implements MDSplus, so this job installs the MDSplus runtime,
development and Java packages, builds the DD models, and enables MDSplus and
HDF5 in Core. It checks that tests are enabled and select the shim's runtime
Core, checks HLI linkage, and runs the existing suite and examples serially.
MDSplus package versions and CTest diagnostics are retained with the run.

Since `IMAS_CPP_REF` moved to `38b9460` that fork also carries a **Tier-1 shim
conformance suite** under `tests/shim/`, registered only when
`AL_USE_MULTIVERSION_SHIM=ON`: eighteen catalogue scenarios in six families,
thirteen of them contract assertions held red while the shim disagrees rather
than inverted or quarantined. A DD 4.1.1 HLI reads and writes a checked-in DD
3.39.0 pulse through the shim and is compared against the same HLI reading the
DD 4.1.1 pulse of the same equilibrium, which makes this **the only HLI job
that asserts on what conversion returns** rather than only that the HLI builds,
links and runs. One direction only: the reverse needs a second `al-cpp` built
against DD 3.39.0. The asserted count is 65 — the generated suite, 21 examples,
two generator refusal-policy tests, and 41 from that suite. Five of its
contract assertions register only when `imas-python-fixtures/.venv` can import
h5py, so the job provisions that venv before configuring; it deliberately stops
short of the fixtures' full requirements, which would also register the
fixture-provenance check and make a green run depend on whatever Data
Dictionary pip resolved that morning.

Its MATLAB job builds `yohannmarguier/IMAS-MATLAB` at `IMAS_MATLAB_REF` the same
way, adding `matlab-actions/setup-matlab`. MDSplus is **not** optional here:
`tests/imas_unit_tests.m` parameterises its class setup over
`struct('MDSplus',12,'HDF5',13)` unconditionally, and seven of the eight
examples are MDSplus-only, so dropping it would halve `al-mex-test` rather than
skip it. The job requires 12 enabled tests and none disabled, and leans on the
two linkage tests the fork registers itself (`al-mex-shim-linkage`,
`mex-imas_open-shim-linkage`) instead of running `readelf`; those two are
excluded from the per-test environment assertion because they inspect a file
and never open a data entry.

**`setup-matlab` installs MATLAB but does not license it**, and that shapes the
whole job. On a public project only MathWorks' own Run MATLAB
Command/Tests/Build actions license MATLAB automatically, and each licenses the
single process it starts; there is no documented job-wide token a
CTest-spawned `matlab -batch` would pick up. A direct `matlab -batch` fails
with `License checkout failed. License Manager Error -1`. Compiling MEX files
needs the installation and not a licence, so **only the two linkage tests
actually run**: the job proves IMAS-MATLAB configures against the installed
shim, that every MEX target compiles against it, and that the inspected ones
link `libimas_mvdd_loader` rather than `libal`. It does *not* prove MATLAB code
round-trips through the shim.

That limit is **measured, not assumed**. Run 34852296651 drove the nine that
existed at the time through `matlab-actions/run-command`, the supported
auto-licensed entry point, and **0 of 9 passed** — every one died on
`Licensing error: -1,359`, because run-command licenses the single MATLAB it
starts and that licence does not reach the `matlab -batch` processes CTest
starts underneath it. `al-utils-unit-test`, which `a7905ab` added since, is a
tenth `matlab -batch` test and inherits that limit without having been measured
under it. The probe was removed once it had answered; re-add it only if
MathWorks documents a job-wide batch licence. Note that the IMAS-MATLAB fork's own CI does not contradict
this — it tolerates the same failure with `continue-on-error: true` and
`|| echo "MATLAB batch mode failed"`, so it never ran MATLAB either.

Its Java job builds `yohannmarguier/IMAS-Java` at `IMAS_JAVA_REF`, also with
MDSplus and the DD models, because the fork's own `ci/build_and_test.sh`
defaults to that backend and `examples/CMakeLists.txt` asks `al-mdsplus-model`
for its model directory. IMAS-Java adds no `tests/` subdirectory to its CMake
graph, so the whole suite is the 25 example programs; the job requires all 25
enabled and checks `lib/libal-java-binding.so` with `readelf`, since this fork
registers no linkage test of its own. It is the one job needing the **full**
`openjdk-21-jdk`: it calls `find_package(JNI)`, which wants the AWT native
libraries `-headless` omits, and fails at configure with
`Could NOT find JNI (missing: AWT)` without them. The C++ and MATLAB jobs stay
on `-headless` because they only need Java for MDSplus CompileTree. Expect a
harmless `Failed to determine VERSION from git tags` warning: the fork carries
no tags and `ALDetermineVersion.cmake` falls back to `0.0.0`. That is the
HLI's own version, not IMAS-Core's, so the Core version tags this repo depends
on are unaffected.

The MATLAB and Java counts were first read off the pinned forks' CMake and have
since been **confirmed on Linux** — 11 for MATLAB and 21 for Java at the
previous pins by run 34852296651, and **12 and 25 at the current pins by run
35205740907**, which reported them by failing the old assertions. None are
disabled in either. Like the Fortran and C++ counts they are assertions about
the pinned fork rather than guesses, so a mismatch is a report about a moved
pin — as it was here: `a7905ab` added `al-utils-unit-test` and `30ea5f1` added
that fork's four shim-tolerance examples.

The C++ count of 65 was derived the same way — from the fork's CMake and the
suite's own README — and **confirmed on Linux by run 35238451263**, which
registered 65 and passed all of them, 41 of those being `tests/shim`. That run
also corrected an assertion of this repository's rather than of the fork's:
`cpp-test-shim-version-unset` carries `IMAS_CORE_LIBRARY` and deliberately no
`IMAS_MVDD_HLI_DD_VERSION`, because the shim latches that version once per
process and F2.1 asserts what happens when it was never declared. A rule
demanding the full environment of every test said that scenario was
misconfigured; it is the scenario.

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
- `docs/ARCHITECTURE.md` — UML class, sequence and state diagrams (Mermaid) of the layering, type model and the read/write/delete pipelines. Generalized on purpose: it draws the shapes, not the 37 individual seams. Start here for orientation; "Where each seam stands" above stays the authority on per-seam policy.
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
