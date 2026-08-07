# IMAS-Multiversion-DD-Loader

Middleware between IMAS HLI and IMAS-Core providing a path conversion to load from the DD version of a HLI, all the version of the DD with potential losses

The library is Rust. The C ABI artefacts — shared library, generated header,
pkg-config file — are produced by [cargo-c]; CMake drives cargo-c rather than
compiling anything itself, so consumers depend on this project the way they
depend on IMAS-Core.

**Status: runtime binding proven on fourteen symbols; no conversion logic
yet.** `al_context_info` and thirteen data-entry, action-lifecycle and
data-operation functions use the runtime-binding architecture
(`src/resolve.rs`, `src/dl.rs`): the shim resolves IMAS-Core lazily via
`dlopen`/`dlsym`, version-checks it, and forwards each call unchanged. Every
other mirrored ABI entry point, and all DD path/version conversion, is still
unimplemented.

## Toolchain

On the ITER cluster:

```console
$ source scripts/iter-env.sh     # Rust/1.88.0-GCCcore-14.3.0 + cargo-c/0.10.15-GCCcore-14.3.0
```

Elsewhere: Rust ≥ 1.88, `cargo install cargo-c`, CMake ≥ 3.20, a C compiler.

CMake fails at configure time with the module names above if either tool is
missing, so a wrong environment is caught immediately rather than mid-build.

## Build, test, install

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
| `IMAS_MVDD_REAL_CORE_LIBRARY` | empty | Optional path to a real IMAS-Core `libal` for the forwarding integration test |
| `IMAS_MVDD_REAL_CORE_INCLUDE_DIR` | empty | Directory containing that Core's `al_const.h` and `al_defs.h`; set together with the library |

Use a single-config generator (Ninja, Unix Makefiles) and set
`CMAKE_BUILD_TYPE`; multi-config generators are rejected at configure time.

## Layout

```
CMakeLists.txt          drives cargo-c; owns install and tests
Cargo.toml              crate-type + [package.metadata.capi]
IMAS_CORE_VERSION       supported IMAS-Core release used by the runtime compatibility gate
cbindgen.toml           generated-header settings
src/lib.rs              the mirrored C ABI
src/resolve.rs          runtime resolution of IMAS-Core: path/version checks and mirrored symbols
src/dl.rs               minimal dlopen/dlsym/dlerror bindings
tests/abi_smoke.c       links C against the generated header
tests/runtime_binding_test.c  drives forwarding against the recording stub
tests/real_core_forwarding_test.c  optional legal HDF5 lifecycle against real IMAS-Core
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
- `runtime-binding-*` — seven default scenarios (`success`,
  `version-drift-tolerated`, `version-mismatch`, `null-version`,
  `missing-library`, `verbatim-forwarding`, `bare-soname`) drive the shim
  against a recording stub (`tests/stub/`). The forwarding scenario exercises
  all thirteen data-entry, action-lifecycle and data-operation symbols and
  verifies that arguments and results cross the boundary unchanged.
- `runtime-binding-real-core-forwarding` — optional; drives those same
  thirteen symbols through a legal temporary HDF5 lifecycle against a real
  IMAS-Core. Enable it by configuring with both
  `IMAS_MVDD_REAL_CORE_LIBRARY=/path/to/libal` and
  `IMAS_MVDD_REAL_CORE_INCLUDE_DIR=/path/to/include`.

The recording-stub and real-Core cases complement each other: the stub exposes
what arrived at the boundary, while the real-Core case proves that the shim's
calls form a valid lifecycle accepted by the actual implementation. See
`docs/adr/0001-runtime-binding-not-linking.md`.

CI (`.github/workflows/ci.yml`) runs fmt, clippy and the whole CMake path —
build, `ctest`, install, then a `pkg-config` query against the installed tree —
for both `Debug` and `Release`, pinned to the cluster's Rust and cargo-c
versions so it guards the MSRV too.

[cargo-c]: https://github.com/lu-zero/cargo-c
