# Building

The library is Rust; the C ABI artefacts (shared library, generated header,
pkg-config file) are produced by [cargo-c]. CMake drives cargo-c rather than
compiling anything itself, so consumers can depend on this project the same way
they depend on IMAS-Core.

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
selects `--release`.

### Options

| Option | Default | Meaning |
|---|---|---|
| `IMAS_MVDD_BUILD_TESTS` | `ON` | Build the C smoke test and register both test suites |
| `IMAS_MVDD_CARGO_OFFLINE` | `OFF` | Pass `--offline` to cargo, for build nodes without network |

Only single-config generators are supported (Ninja, Unix Makefiles) — cargo has
one target directory per configuration, so a multi-config generator has nothing
coherent to map onto.

## Layout

```
CMakeLists.txt                        drives cargo-c; owns install and tests
Cargo.toml                            crate-type + [package.metadata.capi]
cbindgen.toml                         generated-header settings
src/lib.rs                            the mirrored C ABI
tests/abi_smoke.c                     links C against the generated header
scripts/iter-env.sh                   ITER cluster module loads
```

A single crate at the root, deliberately. `imas-core-sys` will have to be a
separate crate when it lands — cargo permits only one package per `links`
value, so a `-sys` crate binding `libal` cannot live here — and that is when
`[workspace]` earns its place. Adding it later means adding a section to this
`Cargo.toml` and a `crates/` directory; nothing moves.

Build outputs land in `build/`:

- `build/cargo/` — cargo's target directory
- `build/stage/` — install-shaped staging tree (`lib/`, `include/`,
  `lib/pkgconfig/`) that the C smoke test and in-tree consumers link against

`cmake --install` re-runs cargo-c against the real prefix instead of copying
the staged tree, because the generated `.pc` file embeds the prefix and has to
reflect where the artefacts actually land. `DESTDIR` is honoured.

## Tests

- `rust-unit` — `cargo test` over the workspace.
- `abi-smoke` — compiles and runs `tests/abi_smoke.c` against the generated
  header and the built shared library. This is the test that proves the ABI
  pipeline is intact end to end: cbindgen emitted a usable header, cargo-c
  produced a linkable library, and the struct layouts agree on both sides.

[cargo-c]: https://github.com/lu-zero/cargo-c
