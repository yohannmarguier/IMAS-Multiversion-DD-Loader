#!/usr/bin/env bash

# Issue #5: "Installing works with a staged destination directory as well as a
# plain prefix." That is the mode every distribution packager and every
# EasyBuild-style module build uses, so it is not an exotic path -- but it is
# one the plain `--install --prefix` runs elsewhere never touch, because the
# artifact is produced by cargo-c rather than by CMake's own install rules and
# the DESTDIR passthrough is hand-written (CMakeLists.txt's install(CODE)
# block). Nothing exercised it until this script.
#
# The property that actually breaks under DESTDIR is subtle: the staging
# directory is a packaging detail, so the generated pkg-config file must
# describe where the artifact will end up, not where it was staged. A .pc
# naming the staging path produces a package that only works on the machine
# that built it.

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <build-dir>" >&2
    exit 2
fi

build_dir=$(cd -- "$1" && pwd)
staging_dir="$build_dir/destdir-staging"
# Deliberately never created before the install: a DESTDIR install must not
# write to the final prefix at all, so its absence afterwards is an assertion.
final_prefix="$build_dir/destdir-final-prefix"

rm -rf "$staging_dir" "$final_prefix"

DESTDIR="$staging_dir" cmake --install "$build_dir" --prefix "$final_prefix"

# How DESTDIR composes: the absolute prefix is appended to the staging root.
staged_root="$staging_dir$final_prefix"

for required in \
    "$staged_root/include/imas_mvdd_loader.h" \
    "$staged_root/lib/pkgconfig/imas-mvdd-loader.pc" \
    "$staged_root/lib/cmake/imas-mvdd-loader/imas-mvdd-loaderConfig.cmake" \
    "$staged_root/lib/cmake/imas-mvdd-loader/imas-mvdd-loaderConfigVersion.cmake"; do
    test -f "$required" || {
        echo "staged install is missing $required" >&2
        exit 1
    }
done

staged_library=$(find "$staged_root/lib" -maxdepth 1 -type f \
    \( -name 'libimas_mvdd_loader.so*' -o -name 'libimas_mvdd_loader*.dylib' \) |
    head -1)
test -n "$staged_library" || {
    echo "staged install has no shared library under $staged_root/lib" >&2
    exit 1
}

pc_prefix=$(sed -n 's/^prefix=//p' \
    "$staged_root/lib/pkgconfig/imas-mvdd-loader.pc")
if [[ $pc_prefix != "$final_prefix" ]]; then
    echo "staged pkg-config file names the staging path, not the final prefix" >&2
    echo "  expected: $final_prefix" >&2
    echo "  found:    $pc_prefix" >&2
    exit 1
fi

if [[ -e $final_prefix ]]; then
    echo "a DESTDIR install must not write to the final prefix: $final_prefix" >&2
    exit 1
fi

echo "staged (DESTDIR) install check passed: $staged_root"
