# IMAS-Multiversion-DD-Loader

Middleware between IMAS HLI and IMAS-Core providing a path conversion to load from the DD version of a HLI, all the version of the DD with potential losses

The library is Rust. The C ABI artefacts — shared library, generated header,
pkg-config file — are produced by [cargo-c]; CMake drives cargo-c rather than
compiling anything itself, so consumers depend on this project the way they
depend on IMAS-Core.

**Status: runtime binding proven on one symbol, now against real IMAS-Core
too.** The build is complete and verified end to end. `al_context_info`
proves the runtime-binding architecture (`src/resolve.rs`, `src/dl.rs`) — the
shim resolves IMAS-Core lazily via `dlopen`/`dlsym`, version-checks it, and
forwards the call — but every other mirrored ABI entry point, and all DD
path/version conversion, is still unimplemented.

## Toolchain

On the ITER cluster:

```console
$ source scripts/iter-env.sh     # Rust/1.88.0-GCCcore-14.3.0 + cargo-c/0.10.15-GCCcore-14.3.0 + IMAS-Core/5.7.1
```

Elsewhere: Rust ≥ 1.88, `cargo install cargo-c`, CMake ≥ 3.20, a C and C++
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

Use a single-config generator (Ninja, Unix Makefiles) and set
`CMAKE_BUILD_TYPE`; multi-config generators are rejected at configure time.

## Layout

```
CMakeLists.txt          drives cargo-c; owns install and tests
Cargo.toml              crate-type + [package.metadata.capi]
IMAS_CORE_VERSION       supported IMAS-Core release used by the runtime compatibility gate
cbindgen.toml           generated-header settings
src/lib.rs              the mirrored C ABI
src/resolve.rs          runtime resolution of IMAS-Core: path/version checks, al_context_info
src/dl.rs               minimal dlopen/dlsym/dlerror bindings
tests/abi_smoke.c       links C against the generated header
tests/runtime_binding_test.c  drives al_context_info against the recording stub and real IMAS-Core
tests/stub/             recording stub standing in for IMAS-Core in the runtime-binding test
scripts/iter-env.sh     ITER cluster module loads
docs/                   reference material — read the inventory before designing anything
```

Build outputs land in `build/`: `build/cargo/` is cargo's target directory,
`build/stage/` an install-shaped tree (`lib/`, `include/`, `lib/pkgconfig/`)
that the C smoke test and in-tree consumers link against.

## Tests

- `rust-unit` — `cargo test` over the crate.
- `abi-smoke` — compiles and runs `tests/abi_smoke.c` against the generated
  header and the built shared library. This is the one that proves the ABI
  pipeline is intact end to end: cbindgen emitted a usable header, cargo-c
  produced a linkable library, and the struct layouts agree on both sides.
- `runtime-binding-*` — seven scenarios (`success`, `version-drift-tolerated`,
  `version-mismatch`, `null-version`, `missing-library`, `bare-soname`,
  `real-core`) drive the shim's exported `al_context_info`, proving the
  runtime-binding architecture end to end (see
  `docs/adr/0001-runtime-binding-not-linking.md`). The first six run against
  a recording stub (`tests/stub/`) standing in for IMAS-Core; `real-core`
  runs the same kind of assertion against the IMAS-Core CMake acquired for
  this build (see CMakeLists.txt), so nothing here can pass against the stub
  and fail for real.

CI (`.github/workflows/ci.yml`) runs fmt, clippy and the whole CMake path —
build, `ctest`, install, then a `pkg-config` query against the installed tree —
for both `Debug` and `Release`, pinned to the cluster's Rust and cargo-c
versions so it guards the MSRV too, and downloads and builds IMAS-Core to
exercise the same acquisition path as a real configure would.

[cargo-c]: https://github.com/lu-zero/cargo-c
