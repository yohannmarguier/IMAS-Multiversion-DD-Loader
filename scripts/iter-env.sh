#!/usr/bin/env bash
# Load the ITER cluster modules this project builds against.
#
#   source scripts/iter-env.sh
#
# Kept as a source-able script rather than a CMake toolchain file because the
# modules must be in the environment before CMake runs its toolchain discovery.

module load Rust/1.88.0-GCCcore-14.3.0
module load cargo-c/0.10.15-GCCcore-14.3.0

# CMake and a C compiler are also required. If they are not already in the
# environment on your login/build node, load the site modules for them here.

command -v cargo >/dev/null || echo "warning: cargo not on PATH after module load" >&2
cargo capi --version >/dev/null 2>&1 || echo "warning: cargo-c not available after module load" >&2
