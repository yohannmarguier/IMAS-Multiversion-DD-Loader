# IMAS-Multiversion-DD-Loader

Middleware between IMAS HLI and IMAS-Core providing a path conversion to load from the DD version of a HLI, all the version of the DD with potential losses

The library is Rust. The C ABI artefacts — shared library, generated header,
pkg-config file — are produced by [cargo-c]; CMake drives cargo-c rather than
compiling anything itself, so consumers depend on this project the way they
depend on IMAS-Core.

**Status: runtime binding proven on thirty-one callable symbols; no conversion
logic yet.** `al_context_info`, thirteen data-entry/action-lifecycle/data-operation
functions, and seventeen plugin-management/reentry functions use the
runtime-binding architecture (`src/resolve.rs`, `src/dl.rs`): the shim resolves IMAS-Core lazily via
`dlopen`/`dlsym`, version-checks it, and forwards each call unchanged.
`al_plugin_begin_timerange_action` is deliberately absent because IMAS-Core's
public declaration is unlinkable upstream. The signatures are checked against
IMAS-Core's real header, and the forwarding seams are exercised against both a
recording stub and a real Core. Every other mirrored ABI entry point, and all
DD path/version conversion, is still unimplemented.

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

IMAS-Core is required to configure — there is no skip-if-missing path.
Installed-package lookup (`find_package(al-core CONFIG)`) is the default; a
missing IMAS-Core fails configure immediately with all three acquisition
options and the cluster module-load hint. See `CMakeLists.txt`'s IMAS-Core
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
| `IMAS_CORE_DOWNLOAD_DEPENDENCIES` | `OFF` | Fetch and build IMAS-Core at `IMAS_CORE_GIT_TAG` instead of finding an installed one |
| `IMAS_CORE_DEVELOPMENT_LAYOUT` | `OFF` | Build IMAS-Core from a sibling checkout at `../IMAS-Core` instead of finding an installed one |
| `IMAS_CORE_GIT_REPOSITORY` / `IMAS_CORE_GIT_TAG` | upstream repo / the `IMAS_CORE_VERSION` pin | Where `IMAS_CORE_DOWNLOAD_DEPENDENCIES` fetches from |
| `IMAS_MVDD_REAL_CORE_LIBRARY` | empty | Optional path to a real IMAS-Core `libal` for the forwarding integration test |
| `IMAS_MVDD_REAL_CORE_INCLUDE_DIR` | empty | Directory containing that Core's `al_const.h` and `al_defs.h`; set together with the library |

Use a single-config generator (Ninja, Unix Makefiles) and set
`CMAKE_BUILD_TYPE`; multi-config generators are rejected at configure time.

## Layout

```
CMakeLists.txt          drives cargo-c; owns install, package config and tests
Cargo.toml              crate-type + [package.metadata.capi]
IMAS_CORE_VERSION       supported IMAS-Core release used by the runtime compatibility gate
cbindgen.toml           generated-header settings
cmake/imas-mvdd-loaderConfig.cmake.in  find_package template, hand-authored
src/lib.rs              the mirrored C ABI
src/resolve.rs          runtime resolution of IMAS-Core: path/version checks and mirrored symbols
src/dl.rs               minimal dlopen/dlsym/dlerror bindings
tests/abi_smoke.c       links C against the generated header
tests/real_core_abi_check.cpp  compares generated declarations with IMAS-Core's real header
tests/runtime_binding_test.c  drives forwarding against the recording stub and al_context_info against real IMAS-Core
tests/real_core_forwarding_test.c  optional legal HDF5 lifecycle against real IMAS-Core
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
- `abi-smoke` — compiles and runs `tests/abi_smoke.c` against the generated
  header and the built shared library. This is the one that proves the ABI
  pipeline is intact end to end: cbindgen emitted a usable header, cargo-c
  produced a linkable library, and the struct layouts agree on both sides.
- `real-core-abi` — compiles the generated header and IMAS-Core's real
  `al_lowlevel.h` into one C++ translation unit. It checks every mirrored
  parameter list plus `al_status_t` layout and the shared ABI constants.
- `runtime-binding-*` — ten default scenarios (`success`,
  `version-drift-tolerated`, `version-mismatch`, `null-version`,
  `missing-library`, `verbatim-forwarding`, `plugin-forwarding`,
  `plugin-timerange-omitted`, `bare-soname`, `real-core`) drive the shim through
  its exported C ABI. Seven run against a recording stub (`tests/stub/`);
  `verbatim-forwarding` exercises all thirteen
  data-entry, action-lifecycle and data-operation symbols and verifies that
  arguments and results cross the boundary unchanged; `plugin-forwarding`
  does the same for all seventeen callable plugin symbols, while
  `plugin-timerange-omitted` pins the deliberately missing export. `real-core`
  checks `al_context_info` against the CMake-acquired IMAS-Core.
- `runtime-binding-real-core-forwarding` — optional; drives the thirteen data
  seams and all seventeen callable plugin seams through a legal temporary HDF5
  lifecycle against a real IMAS-Core. Its loadable fixture verifies plugin
  registration, binding and parameter values end to end. Enable it with both
  `IMAS_MVDD_REAL_CORE_LIBRARY=/path/to/libal` and
  `IMAS_MVDD_REAL_CORE_INCLUDE_DIR=/path/to/include`.
- `tests/consumer/` isn't registered with ctest — it needs an installed tree
  to configure against, so CI drives it directly after the install step.

The recording-stub and real-Core cases complement each other: the stub exposes
what arrived at the boundary, while the real-Core case proves that the shim's
calls form a valid lifecycle accepted by the actual implementation. See
`docs/adr/0001-runtime-binding-not-linking.md`.

CI (`.github/workflows/ci.yml`) runs fmt, clippy and the whole CMake path —
build, `ctest`, install, then `pkg-config` and `find_package` consumer builds
against the installed tree — for both `Debug` and `Release`, pinned to the
cluster's Rust and cargo-c versions so it guards the MSRV too. It downloads
and builds IMAS-Core to exercise the same acquisition path as a real
configure would.

[cargo-c]: https://github.com/lu-zero/cargo-c
