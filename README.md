# IMAS-Multiversion-DD-Loader

Shim between the IMAS HLIs and IMAS-Core for path conversion across DD versions, with explicitly lossy semantics.

The library is Rust. The C ABI artefacts — shared library, generated header,
pkg-config file — are produced by [cargo-c]; CMake drives cargo-c rather than
compiling anything itself, so consumers depend on this project the way they
depend on IMAS-Core.

**Status: runtime binding proven on all 37 linkable IMAS-Core C exports;
read-path DD conversion implemented for one IDS and one version pair.**
`al_context_info`, six utility/version accessors, thirteen
data-entry/action-lifecycle/data-operation functions, and seventeen
plugin-management/reentry functions use the runtime-binding architecture
(`src/resolve.rs`, `src/dl.rs`): the shim resolves IMAS-Core lazily via
`dlopen`/`dlsym`, version-checks it, and forwards each call unchanged.
`al_plugin_begin_timerange_action` is deliberately absent because IMAS-Core's
public declaration is unlinkable upstream; `al_begin_array_struct_action` is
not an IMAS-Core export. The signatures and exported symbol list are checked
mechanically against IMAS-Core, and the forwarding seams are exercised against
both a recording stub and a real Core.

On top of that, reads of a stored equilibrium occurrence are converted between
DD 3.39.0 and DD 4.1.1 in both directions: the shim discovers the stored DD
version from the occurrence's own `ids_properties/version_put/data_dictionary`
stamp, translates `al_read_data`'s `field` and `timebase` (including beneath
nested arraystruct contexts), applies COCOS sign flips, refuses paths the
conversion map declares unservable, and reports non-exact reads through a loss
log the caller drains from the root context. Writes and deletes against a
mismatched occurrence refuse rather than convert. Read the limitations below
before drawing conclusions from that list.

## Scope and limitations

These are deliberate boundaries, not gaps awaiting a patch. The first, fifth and
sixth are pinned by a named test, so they cannot quietly stop being true. The
other three are scoping decisions no test can express — which is itself worth
knowing when reading a green suite.

- **One DD version per process.** The calling HLI's DD version latches once, on
  the first `imas_mvdd_set_hli_dd_version()` call or from
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
- **A green suite is not a deployment mechanism.** The tests call this library
  directly. They do not place it in front of a real HLI, do not substitute it
  for IMAS-Core in any HLI's link line or runtime search path, and so do not
  demonstrate that any HLI can be made to load it. How an HLI comes to resolve
  `libal`'s symbols to this shim in a real deployment is a separate, unsolved
  question, and no amount of green here answers it.
- **Conversion coverage is one IDS and one version pair.** equilibrium
  3.39.0 ⇄ 4.1.1, served from the single conversion-map artifact embedded in
  `src/known_artifacts.rs` (`docs/3.39.0--4.1.1.xml`). Any other IDS, or any
  other version pair, is forwarded unconverted — as is an occurrence whose
  stamp matches the HLI or is absent
  (`docs/adr/0007-unstamped-ids-occurrences-match-hli.md`).
- **Three conversion-relevant seams are deliberately not translated.**
  `al_list_filled_paths` still returns paths in the *stored* version's
  spelling, and `al_bind_plugin` / `al_unbind_plugin` still take a `fieldPath`
  in it. CLAUDE.md lists all three as seams that will eventually need
  translation; until they get it, `scoped-passthrough-*` pins the current
  behaviour so it cannot change by accident in either direction.

## Toolchain

On the ITER cluster:

```console
$ source scripts/iter-env.sh     # Rust/1.88.0-GCCcore-14.3.0 + cargo-c/0.10.15-GCCcore-14.3.0 + IMAS-Core/5.7.1
```

Elsewhere: Rust ≥ 1.88, `cargo install cargo-c`, CMake ≥ 3.21, a C and C++
compiler, and IMAS-Core itself — see the acquisition options below.

CMake fails at configure time with the module names above if either tool is
missing, so a wrong environment is caught immediately rather than mid-build.

## Build, test, install

Real IMAS-Core is required by the default configure profile. Installed-package
lookup (`find_package(al-core CONFIG)`) is the default; a missing IMAS-Core
fails configure immediately with all three acquisition options and the cluster
module-load hint. CI's explicit `IMAS_MVDD_REAL_CORE_TESTS=OFF` profile is the
only stub-only path: it registers the recording-stub seams and does not pretend
to cover the drift or real-Core checks. See `CMakeLists.txt`'s IMAS-Core
acquisition section for the full rationale.

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

## Layout

```
CMakeLists.txt          drives cargo-c; owns install, package config and tests
.github/actions/setup-toolchain/action.yml  shared pinned CI toolchain setup
Cargo.toml              crate-type + [package.metadata.capi]
IMAS_CORE_VERSION       supported IMAS-Core release used by the runtime compatibility gate
cbindgen.toml           generated-header settings
cmake/imas-mvdd-loaderConfig.cmake.in  find_package template, hand-authored
src/lib.rs              the mirrored C ABI
src/resolve.rs          runtime resolution of IMAS-Core: path/version checks and mirrored symbols
src/dl.rs               minimal dlopen/dlsym/dlerror bindings
tests/abi_smoke.c       links C against the generated header
tests/real_core_abi_*_check.c  compares generated declarations with IMAS-Core's real header
tests/runtime_binding_test.c  drives forwarding against the recording stub and the basic ABI seam against real IMAS-Core
tests/check_exports.cmake     mechanically compares the shim's exported C ABI with IMAS-Core's
tests/check_ci_workflow.cmake guards the fast/full CI responsibilities and pinned toolchains
tests/check-installed-package.sh  consumes an installed tree through pkg-config and find_package
tests/real_core_forwarding_test.c  required legal HDF5 forwarding coverage against real IMAS-Core
tests/real_core_test_plugin.cpp  loadable fixture for real-Core plugin seam tests
tests/stub/             recording stub standing in for IMAS-Core in the runtime-binding test
tests/consumer/         throwaway downstream project proving find_package on the installed tree
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
platform-specific multiarch directory.

cargo-c produces the library, header and `.pc` file directly; the CMake
package config (`cmake/imas-mvdd-loaderConfig.cmake.in`) is authored by hand
— see that file and `CMakeLists.txt` for why. Its version file declares
`SameMajorVersion` compatibility.

A downstream CMake project consumes the installed package the same way it
would IMAS-Core:

```cmake
find_package(imas-mvdd-loader REQUIRED)
target_link_libraries(my_target PRIVATE imas-mvdd-loader::imas-mvdd-loader)
```

Non-CMake consumers use the installed `.pc` file instead:

```console
$ pkg-config --cflags --libs imas-mvdd-loader
```

`tests/consumer/` is a throwaway project exercising the `find_package` path
against only the installed tree; CI builds and runs it after every install,
next to the equivalent `pkg-config` check.

## Tests

- `rust-unit` — `cargo test` over the crate.
- `ci-workflow` — guards the fast/full job split, unrestricted push trigger,
  shared pinned-toolchain setup, explicit test profiles, install checks, and
  `--no-tests=error` coverage gate; its rejection test proves comments or later
  jobs cannot satisfy another job's responsibilities.
- `abi-smoke` — compiles and runs `tests/abi_smoke.c` against the generated
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
- `tests/consumer/` isn't registered with ctest — it needs an installed tree
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
