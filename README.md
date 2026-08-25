# IMAS-Multiversion-DD-Loader

A shim between an IMAS HLI (Fortran, C++, …) and IMAS-Core that translates
Data Dictionary paths across DD versions, so an HLI compiled against one DD
version can read a pulse stored under another. Conversion is explicitly
lossy where it has to be, and surfaces that loss rather than hiding it.

```
HLI (imas-Fortran, imas-CPP; compiled against DD version V)
        │
        ▼
IMAS-Multiversion-DD-Loader   ← this project: re-exports IMAS-Core's C ABI verbatim,
        │                        translates DD paths V ⇄ W in between
        ▼
IMAS-Core (libal)             ← stores the IDS under DD version W
```

**Jump to:** [Status](#status) · [Toolchain](#toolchain) ·
[Build, test, install](#build-test-install) ·
[Using it with an HLI](#using-it-with-an-hli) ·
[Scope and limitations](#scope-and-limitations) · [Layout](#layout) ·
[Installed layout](#installed-layout-and-consuming-the-package) ·
[Tests](#tests)

## Status

**Runtime binding is proven on all 37 linkable IMAS-Core C exports.
Read-path DD conversion is implemented for one IDS and one version pair.**

- **Verbatim forwarding.** `al_context_info`, six utility/version accessors,
  thirteen data-entry/action-lifecycle/data-operation functions, and
  seventeen plugin-management/reentry functions all resolve IMAS-Core
  lazily via `dlopen`/`dlsym` (`src/core/core_binding.rs`, `src/core/dl.rs`),
  version-check it, and forward each call unchanged.
  `al_plugin_begin_timerange_action` is deliberately absent — its public
  declaration is unlinkable upstream — and `al_begin_array_struct_action`
  is not an IMAS-Core export at all. The exported symbol list and every
  signature are checked mechanically against IMAS-Core, and the forwarding
  seams are exercised against both a recording stub and a real Core.
- **Read-path conversion.** Reads of a stored **equilibrium** occurrence
  convert between DD **3.39.0** and DD **4.1.1**, in both directions. The
  shim discovers the stored DD version from the occurrence's own
  `ids_properties/version_put/data_dictionary` stamp, translates
  `al_read_data`'s `field` and `timebase` (including beneath nested
  arraystruct contexts), applies COCOS sign flips, refuses paths the
  conversion map declares unservable, and reports non-exact reads through a
  loss log the caller drains from the root context (see [Draining the loss
  log](#draining-the-loss-log) below).
- **Write/delete conversion.** Safe mismatched writes translate one
  unambiguous path and send IMAS-Core a shim-owned, COCOS-flipped copy where
  the rule calls for one, leaving the caller's own storage untouched; a write
  the map cannot serve refuses before Core and is retained on the loss log.
  Deletes translate identity, renamed, and moved leaf paths
  to their stored spelling; deleting the whole DATAOBJECT is the explicit
  migration route, while a DD-version stamp, unsafe source, no-source path,
  or non-primary source refuses. A candidate-plan write reaches only
  precedence 1 and records every skipped candidate as potentially lossy (apart
  from ADR 0018's unset rank-zero scalar, which stores no value and earns no
  loss);
  candidate-plan delete behavior remains deferred. `al_list_filled_paths` and `al_bind_plugin`/`al_unbind_plugin` are
  deliberately not translated.

Read [Scope and limitations](#scope-and-limitations) before drawing
conclusions from that list — several of the boundaries below are permanent
design decisions, not gaps waiting on the next PR.

## Toolchain

On the ITER cluster:

```console
$ source scripts/iter-env.sh     # Rust/1.88.0-GCCcore-14.3.0 + cargo-c/0.10.15-GCCcore-14.3.0 + IMAS-Core/5.7.1
```

Elsewhere: Rust ≥ 1.88, `cargo install cargo-c`, CMake ≥ 3.21, a C and C++
compiler, and IMAS-Core itself — see the acquisition options in [Build,
test, install](#build-test-install).

CMake fails at configure time with the module names above if either tool is
missing, so a wrong environment is caught immediately rather than mid-build.

## Build, test, install

Real IMAS-Core is required by the default configure profile. Installed-package
lookup (`find_package(al-core CONFIG)`) is the default; a missing IMAS-Core
fails configure immediately with all three acquisition options and the
cluster module-load hint. CI's explicit `IMAS_MVDD_REAL_CORE_TESTS=OFF` profile
is the only stub-only path: it registers the recording-stub seams and does
not pretend to cover the drift or real-Core checks. See `CMakeLists.txt`'s
IMAS-Core acquisition section for the full rationale.

```console
$ cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
$ cmake --build build
$ ctest --test-dir build --output-on-failure
$ cmake --install build --prefix /path/to/prefix
```

`CMAKE_BUILD_TYPE=Debug` selects cargo's `dev` profile; every other build type
selects `release`. `DESTDIR` is honoured on install.

Day to day, `cargo test` is the faster loop — the CMake path is what CI and
packaging use.

| Option | Default | Meaning |
|---|---|---|
| `IMAS_MVDD_BUILD_TESTS` | `ON` | Build the C test suites and register them with ctest |
| `IMAS_MVDD_CARGO_OFFLINE` | `OFF` | Pass `--offline` to cargo, for build nodes without network |
| `IMAS_MVDD_REAL_CORE_TESTS` | `ON` | Acquire IMAS-Core and register the drift and real-Core seam tests; `OFF` is the explicit recording-stub-only CI profile |
| `IMAS_CORE_DOWNLOAD_DEPENDENCIES` | `OFF` | Fetch and build IMAS-Core at `IMAS_CORE_GIT_TAG` instead of finding an installed one |
| `IMAS_CORE_DEVELOPMENT_LAYOUT` | `OFF` | Build IMAS-Core from a sibling checkout at `../IMAS-Core` instead of finding an installed one |
| `IMAS_CORE_GIT_REPOSITORY` / `IMAS_CORE_GIT_TAG` | upstream repo / the `IMAS_CORE_VERSION` pin | Where `IMAS_CORE_DOWNLOAD_DEPENDENCIES` fetches from |

Use a single-config generator (Ninja, Unix Makefiles) and set
`CMAKE_BUILD_TYPE`; multi-config generators are rejected at configure time.

## Using it with an HLI

The shim mirrors IMAS-Core's C ABI symbol-for-symbol, but its build output
is named differently — `libimas_mvdd_loader.{a,so,dylib}` /
`imas_mvdd_loader.h`, not `libal.so` / `al_lowlevel.h`. That is what is
*proven*: a client links or `dlopen`s the shim exactly as it would
IMAS-Core, under the shim's own name, and gets converting reads. What
*isn't* proven is getting an already-built HLI binary — one that already
links `-lal` and includes `al_lowlevel.h` — to resolve to this shim with
zero changes to that binary. See [Scope and limitations](#scope-and-limitations)
for exactly where that line sits.

### The three things a client does

Whatever the calling language, a client of this shim does these three
things, in order (`docs/adr/0005-hli-dd-version-entry-point.md`):

**1. Link against the shim exactly as you would against IMAS-Core.**

```cmake
find_package(imas-mvdd-loader REQUIRED)
target_link_libraries(my_hli_target PRIVATE imas-mvdd-loader::imas-mvdd-loader)
```

or, for a non-CMake build:

```console
$ pkg-config --cflags --libs imas-mvdd-loader
```

Every mirrored function (`al_begin_global_action`, `al_read_data`, …) is
called exactly as it would be against IMAS-Core. The shim resolves the real
IMAS-Core underneath, itself, via `dlopen` (see [Locating real
IMAS-Core](#locating-real-imas-core) below) — nothing in the calling code
needs to know it's talking to a shim rather than IMAS-Core directly.

**2. Report the HLI's own DD version before the first open.** The shim
cannot ask IMAS-Core what DD version a pulse was written under
(`getDDVersion()` is deliberately dead upstream), so the caller has to say
what version *it itself* was built against, once, up front. Two routes:

```c
al_status_t status = imas_mvdd_set_hli_dd_version("4.1.1");
```

or, when the calling binary has no hook to call a setter from — true of
today's IMAS-Fortran/IMAS-CPP, investigated and confirmed: neither has an
initialiser hook that runs on its own without patching upstream:

```console
$ export IMAS_MVDD_HLI_DD_VERSION=4.1.1
```

The value **latches for the life of the process**: it's a compile-time
constant of the calling binary, not a per-pulse or per-thread setting. An
identical repeat is accepted; a conflicting later report is refused with
both versions named in the error message. The setter always wins over the
environment variable, so prefer it wherever the binary can call one. Leave
both unset and the shim reads no version stamp at all and forwards every
call unchanged — the zero-cost passthrough path.

**Do not** set `IMAS_MVDD_HLI_DD_VERSION` in a process that also runs an
HLI performing its own DD conversion (imas-python is the known example —
see [Scope and limitations](#scope-and-limitations)): the shim cannot tell
the two callers apart, and would silently convert the self-converting one's
reads too.

**3. Just read.** Open a pulse and call `al_read_data` as usual. If the
occurrence's stored DD version matches the HLI's latched version, every
call forwards unchanged. If it doesn't — and the shim has a conversion map
for that IDS and version pair (today: **equilibrium**, 3.39.0 ⇄ 4.1.1) — the
path is translated, sign flips are applied, and the outcome is classified
as an exact read, a lossy-but-served read, or a refusal, before IMAS-Core is
called.

### Locating real IMAS-Core

The shim never links against IMAS-Core; it opens it at runtime via
`dlopen`/`dlsym` with a handle-scoped symbol lookup, specifically so the
shim's own exports can't shadow IMAS-Core's (ADR 0001). By default it looks
for the bare soname `libal.so` / `libal.dylib` — IMAS-Core's own — through
the dynamic loader's normal search path. Two ways to control what it finds:

```console
$ export LD_LIBRARY_PATH=/path/to/real/imas-core/lib:$LD_LIBRARY_PATH   # bare-soname search order
$ export IMAS_CORE_LIBRARY=/opt/iter/lib/libal.so                       # or pin an exact path
```

`IMAS_CORE_LIBRARY` wins when set.

### A drop-in placement is possible in principle, not validated here

Because the fallback lookup is the literal soname `libal.so`, and because
`RTLD_LOCAL` handle-scoped resolution means the shim's own `al_read_data`
export never shadows the one it calls into, the runtime-binding design
*permits* deploying the shim as `libal.so` itself, ahead of the real
IMAS-Core on the search path, with `IMAS_CORE_LIBRARY` pointing at the real
one's actual file so the shim's own lookup doesn't just find itself again.
That would let an **unmodified** HLI binary — one that was never rebuilt or
relinked — pick up the shim transparently.

No test, script, or CI job in this repository does this, and ADR 0001
rejected `LD_PRELOAD`-style interposition as fragile and invisible in a
normal build for the shim's own IMAS-Core dispatch — the same fragility
argument applies to using a renamed artifact as a transparent swap
underneath an HLI. Treat the paragraph above as "the architecture doesn't
forbid it," not as a supported deployment recipe.

### Draining the loss log

A non-exact read is logged, not silently accepted. Three shim-owned exports
let a caller inspect it without allocating:

```c
int count = 0;
imas_mvdd_context_loss_count(ctx_id, &count);   /* entries on ctx_id's root context */

for (int i = 0; i < count; ++i) {
    char path[256];
    int verdict = 0;
    int operation = 0;
    imas_mvdd_context_loss_at(ctx_id, i, path, sizeof(path), &verdict);
    imas_mvdd_context_loss_operation_at(ctx_id, i, &operation);
    /* verdict is IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY, _LOSSY, or _UNMAPPABLE */
    /* operation is IMAS_MVDD_LOSS_OPERATION_READ or _WRITE */
}
```

A query on a child context (e.g. one opened by `al_begin_arraystruct_action`)
resolves to the same log as its root; an untracked context reports `0`
rather than a refusal.

### Environment variables at a glance

| Variable | Read by | Purpose |
|---|---|---|
| `IMAS_MVDD_HLI_DD_VERSION` | the shim, at first open | Fallback for `imas_mvdd_set_hli_dd_version()` — the calling HLI's own DD version |
| `IMAS_CORE_LIBRARY` | the shim, at first IMAS-Core call | Absolute path to the real IMAS-Core shared library, overriding the bare-soname search |

These are the only two environment variables the shim itself reads.

## Scope and limitations

These are deliberate boundaries, not gaps awaiting a patch. The first,
fifth and sixth are pinned by a named test, so they cannot quietly stop
being true. The others are scoping decisions no test can express — which
is itself worth knowing when reading a green suite.

- **How an unmodified HLI binary comes to load this shim instead of
  IMAS-Core is still open.** Everything under [Using it with an
  HLI](#using-it-with-an-hli) is proven at this project's own C ABI and at
  `tests/package/find_package/`, which links against the shim as a normal
  library dependency the way a *newly built* consumer would — it never
  renames the shim to `libal.so` or makes an HLI-shaped binary resolve to
  it in place of IMAS-Core. A green suite is not a deployment mechanism:
  the tests do not place this library in front of a real HLI binary, and no
  amount of green here answers that question on its own.
- **One DD version per process.** The calling HLI's DD version latches once,
  on the first `imas_mvdd_set_hli_dd_version()` call or from
  `IMAS_MVDD_HLI_DD_VERSION` at the first open, and never changes afterwards
  (`docs/adr/0005-hli-dd-version-entry-point.md`). It cannot vary per pulse,
  per `DBEntry`-equivalent, or per thread. Reading two different HLI DD
  versions therefore takes two processes, and because the fallback is an
  environment variable, the version is a property of how the process was
  launched. Every conversion test in the suite is registered as its own CTest
  process for exactly this reason.
- **Self-converting clients are excluded.** imas-python is not a client: it
  converts DD versions itself and holds one DD version per `DBEntry` rather
  than one per process, so stacking this shim beneath it would convert twice.
  The criterion is the client's shape — one DD version for the life of the
  process, no conversion of its own — not the language it is written in.
- **Validation is IMAS-Fortran-first.** The conversion behaviour is proven at
  this project's own C ABI, which is the ABI imas-Fortran consumes. imas-CPP is
  expected to fit the same client shape but has not been validated here;
  imas-Matlab and imas-Java have not been judged at all.
- **Conversion coverage is one IDS and one version pair.** equilibrium
  3.39.0 ⇄ 4.1.1, served from the single conversion-map artifact embedded in
  `src/conversion/known_artifacts.rs` (`docs/3.39.0--4.1.1.xml`). Any other IDS, or any
  other version pair, is forwarded unconverted — as is an occurrence whose
  stamp matches the HLI or is absent
  (`docs/adr/0007-unstamped-ids-occurrences-match-hli.md`).
- **The completeness proof's oracle is two inventories, not the DD.** The
  artifact is proven complete against `docs/inventory/equilibrium-{3.39.0,4.1.1}.txt`
  — the imas-dd path sets for those versions, which exclude the
  `ids_properties/**` and `code/**` metadata subtrees wholesale, plus
  `ids_properties/version_put/data_dictionary`, added by hand because the shim
  reads it at every open. Nothing proves either inventory complete against its
  own DD version, and the artifact's `<default rel="identical"/>` means the
  proof's content is *not* "a rule claims every path": it is "every path a rule
  does not claim exists by the same spelling on the other side". The coverage
  report prints that split (`by rule=` versus `by identity default=`) so the
  weight each carries is visible rather than implied.
- **Three conversion-relevant seams are deliberately not translated.**
  `al_list_filled_paths` still returns paths in the *stored* version's
  spelling, and `al_bind_plugin` / `al_unbind_plugin` still take a `fieldPath`
  in it. CLAUDE.md lists all three as seams that will eventually need
  translation; until they get it, `scoped-passthrough-*` pins the current
  behaviour so it cannot change by accident in either direction.

## Layout

```
CMakeLists.txt          drives cargo-c; owns install, package config and tests
.github/actions/setup-toolchain/action.yml  shared pinned CI toolchain setup
Cargo.toml              crate-type + [package.metadata.capi]
IMAS_CORE_VERSION       supported IMAS-Core release used by the runtime compatibility gate
cbindgen.toml           generated-header settings
cmake/imas-mvdd-loaderConfig.cmake.in  find_package template, hand-authored
src/lib.rs              the mirrored C ABI
src/core/               runtime binding and dlopen/dlsym adapter
src/conversion/         map resolution, path policy, outcomes, and embedded artifacts
src/registry/           live conversion-context registry
src/version/            DD versions, HLI latch, and occurrence stamp discovery
src/interpose.rs        C-facing seam adapter over those modules
tests/abi/              generated-header smoke test and ABI manifests
tests/shim/             recording-stub seam tests
tests/real_core/        HDF5 and real-IMAS-Core checks and plugin fixture
tests/package/          installed-package consumer fixture
tests/support/          shared C test harness
tests/cmake/            CMake-script checks
tests/scripts/          install and package checks
tests/stub/             recording stub standing in for IMAS-Core
tests/fixtures/         reduced conversion-map fixture for the coverage-floor test
scripts/iter-env.sh     ITER cluster module loads
docs/                   reference material — read the inventory before designing anything
```

Build outputs land in `build/`: `build/cargo/` is cargo's target directory,
`build/stage/` an install-shaped tree (`lib/`, `include/`, `lib/pkgconfig/`)
that the C smoke test and in-tree consumers link against.

## Installed layout and consuming the package

`cmake --install` produces, mirroring IMAS-Core's own layout:

```
<prefix>/include/imas_mvdd_loader.h
<prefix>/<libdir>/libimas_mvdd_loader.{a,so,dylib}
<prefix>/<libdir>/pkgconfig/imas-mvdd-loader.pc
<prefix>/<libdir>/cmake/imas-mvdd-loader/imas-mvdd-loaderConfig.cmake
<prefix>/<libdir>/cmake/imas-mvdd-loader/imas-mvdd-loaderConfigVersion.cmake
```

`<libdir>` is selected by `GNUInstallDirs` and may be `lib`, `lib64` or a
platform-specific multiarch directory. A relative `--prefix` produces the same
layout as an absolute one, resolved — as CMake resolves it — against the
working directory of the `cmake --install` run, not the source tree
(`tests/scripts/check-relative-prefix-install.sh`).

cargo-c produces the library, header and `.pc` file directly; the CMake
package config (`cmake/imas-mvdd-loaderConfig.cmake.in`) is authored by hand
— see that file and `CMakeLists.txt` for why. Its version file declares
`SameMajorVersion` compatibility.

A downstream CMake project consumes the installed package the same way it
would IMAS-Core — see [Using it with an HLI](#using-it-with-an-hli) for the
full sequence (link, set the HLI DD version, read):

```cmake
find_package(imas-mvdd-loader REQUIRED)
target_link_libraries(my_target PRIVATE imas-mvdd-loader::imas-mvdd-loader)
```

Non-CMake consumers use the installed `.pc` file instead:

```console
$ pkg-config --cflags --libs imas-mvdd-loader
```

`tests/package/find_package/` is a throwaway project exercising the `find_package` path
against only the installed tree; CI builds and runs it after every install,
next to the equivalent `pkg-config` check.

## Tests

- `rust-unit` — `cargo test` over the crate.
- `ci-workflow` — guards the fast/full job split, unrestricted push trigger,
  shared pinned-toolchain setup, explicit test profiles, install checks, and
  `--no-tests=error` coverage gate; its rejection test proves comments or later
  jobs cannot satisfy another job's responsibilities.
- `abi-smoke` — compiles and runs `tests/abi/abi_smoke.c` against the generated
  header and built shared library. It forwards to the recording stub in the
  fast profile and CMake-acquired IMAS-Core in the full profile.
- `real-core-export-list` — mechanically compares the filtered public C
  exports of IMAS-Core and the shim with `nm`.
- `real-core-abi` — compiles the generated header and IMAS-Core's real
  `al_lowlevel.h` in separate C translation units against one shared contract.
  It checks every mirrored parameter list plus `al_status_t` layout and the shared ABI constants;
  `real-core-abi-rejects-mismatch` proves a modified shim header is rejected.
- `runtime-binding-*` — default scenarios drive the shim through its exported
  C ABI. The recording-stub scenarios include `utility-forwarding`, which
  verifies the backend-id accessor, legacy URI builder, both string helpers,
  and both version accessors; `real-core` checks `al_context_info` against the
  CMake-acquired IMAS-Core. `verbatim-forwarding` exercises all thirteen
  data-entry, action-lifecycle and data-operation symbols and verifies that
  arguments and results cross the boundary unchanged; `plugin-forwarding`
  does the same for all seventeen callable plugin symbols, while
  `plugin-timerange-omitted` pins the deliberately missing export.
- `runtime-binding-real-core-forwarding` — drives the utility/version,
  thirteen data, and all seventeen callable plugin seams through a legal
  temporary HDF5 lifecycle against a real IMAS-Core. Its loadable fixture
  verifies plugin registration, binding and parameter values end to end.
- `hli-dd-version-*`, `version-discovery-*`, `read-path-*`,
  `write-delete-*`, `arraystruct-path-*`, `nested-context-read-*`,
  `context-lifecycle-*` and `plugin-reentry-policy-*` — the conversion seams
  against the recording stub, one CTest process per scenario because both the
  HLI DD version latch and the context registry are process-wide.
- `scoped-passthrough-*` — the other half of that claim: with a mismatched
  equilibrium occurrence open and converting, `al_get_occurrences`,
  `al_list_filled_paths`, `al_bind_plugin`/`al_unbind_plugin` and every
  remaining non-seam export must still forward unchanged. The path arguments
  are ones the loaded artifact has rules for, so a shim that started rewriting
  them would fail rather than pass by coincidence.
- `equilibrium-read-*` — the same conversion behaviour end to end against the
  checked-in equilibrium HDF5 fixture pair and a real IMAS-Core, in both
  directions: renames, merged and split paths, COCOS sign flips, refusals, and
  the matching-version and conversion-disabled cases that must stay untouched.
  "forward" names an HLI declaring 4.1.1 reading the 3.39.0 fixture, "reverse"
  the other way round.
- `equilibrium-artifact-coverage-floor` — runs the artifact's
  autoconvert-equivalence floor check, including its deliberately reduced
  fixture, so an apparent identity-only map is rejected.
- `tests/package/find_package/` isn't registered with ctest — it needs an installed tree
  to configure against, so CI drives it directly after the install step.

The recording-stub and real-Core cases complement each other: the stub exposes
what arrived at the boundary, while the real-Core case proves that the shim's
calls form a valid lifecycle accepted by the actual implementation. See
`docs/adr/0001-runtime-binding-not-linking.md`.

CI (`.github/workflows/ci.yml`) splits early feedback from the expensive real
dependency. The `fast` job runs fmt, clippy, both CMake build configurations,
all recording-stub seams, install, and both installed-package consumers. The
`full` job runs for pull requests and `main` pushes; it downloads and caches the
pinned IMAS-Core build, then runs the ABI drift and real-Core seam suites before
performing the same install and consumer checks. Every CTest invocation uses
`--no-tests=error`; both jobs stay pinned to the cluster's Rust and cargo-c
module versions.

[cargo-c]: https://github.com/lu-zero/cargo-c
