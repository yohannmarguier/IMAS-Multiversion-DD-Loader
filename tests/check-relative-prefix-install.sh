#!/usr/bin/env bash

# `cmake --install build --prefix <relative-path>` -- the form a user typing a
# prefix by hand reaches for, and the one CI never used, because every install
# step here passes "$PWD/dist".
#
# Two rules collide in that form. CMake resolves a relative install prefix
# against the working directory of the install run. cargo-c, which produces the
# header, the libraries and the pkg-config file (CMakeLists.txt's install(CODE)
# block), instead joins a *relative* --libdir onto --prefix and runs with its
# working directory set to the source tree. Passing the prefix through
# unresolved therefore did not fail the install -- it silently split it in two,
# CMake's package config landing under <cwd>/<prefix>/lib/cmake and cargo-c's
# artifacts under <source-dir>/<prefix>/<prefix>/{include,lib}. The install
# reported success either way, so the breakage only surfaced downstream, as an
# HLI build asking for <prefix>/lib/libimas_mvdd_loader.dylib and finding
# nothing there.
#
# The install is therefore run from a scratch directory that is neither the
# source tree nor the build tree: that is what distinguishes "resolved against
# the caller's working directory", which is CMake's documented rule, from
# "resolved against wherever cargo happened to run".

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <build-dir>" >&2
    exit 2
fi

build_dir=$(cd -- "$1" && pwd)
scratch_dir="$build_dir/relative-prefix-cwd"
relative_prefix=relative-prefix-install
prefix="$scratch_dir/$relative_prefix"

rm -rf "$scratch_dir"
mkdir -p "$scratch_dir"

(cd "$scratch_dir" && cmake --install "$build_dir" --prefix "$relative_prefix")

for required in \
    "$prefix/include/imas_mvdd_loader.h" \
    "$prefix/lib/pkgconfig/imas-mvdd-loader.pc" \
    "$prefix/lib/cmake/imas-mvdd-loader/imas-mvdd-loaderConfig.cmake" \
    "$prefix/lib/cmake/imas-mvdd-loader/imas-mvdd-loaderConfigVersion.cmake"; do
    test -f "$required" || {
        echo "install under a relative prefix is missing $required" >&2
        exit 1
    }
done

installed_library=$(find "$prefix/lib" -maxdepth 1 -type f \
    \( -name 'libimas_mvdd_loader.so*' -o -name 'libimas_mvdd_loader*.dylib' \) |
    head -1)
test -n "$installed_library" || {
    echo "install under a relative prefix has no shared library under" >&2
    echo "  $prefix/lib" >&2
    exit 1
}

# The specific shape the bug produced: the prefix applied twice. Named
# explicitly so a regression reads as itself rather than as a missing file.
if [[ -e "$prefix/$relative_prefix" ]]; then
    echo "the install prefix was applied twice:" >&2
    echo "  $prefix/$relative_prefix" >&2
    exit 1
fi

# The other half of the same mistake: cargo-c resolving the relative prefix
# against its own working directory, the source tree, instead of the caller's.
source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
for stray in "$source_dir/$relative_prefix" "$build_dir/$relative_prefix"; do
    if [[ -e $stray ]]; then
        echo "a relative install prefix was resolved against the wrong" >&2
        echo "directory, leaving artifacts at $stray" >&2
        exit 1
    fi
done

# A .pc file naming a relative prefix describes nothing a consumer can use, so
# the resolved absolute path is the only correct content here.
pc_prefix=$(sed -n 's/^prefix=//p' \
    "$prefix/lib/pkgconfig/imas-mvdd-loader.pc")
if [[ $pc_prefix != "$prefix" ]]; then
    echo "pkg-config file does not name the resolved install prefix" >&2
    echo "  expected: $prefix" >&2
    echo "  found:    $pc_prefix" >&2
    exit 1
fi

echo "relative-prefix install check passed: $prefix"
