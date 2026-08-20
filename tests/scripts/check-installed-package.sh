#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "usage: $0 <build-dir> <install-prefix> <imas-core-library>" >&2
    exit 2
fi

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
source_dir=$(cd -- "$script_dir/../.." && pwd)
build_dir=$(cd -- "$1" && pwd)
install_prefix=$(cd -- "$2" && pwd)
core_library=$(cd -- "$(dirname -- "$3")" && pwd)/$(basename -- "$3")
core_dir=$(dirname -- "$core_library")

test -f "$core_library"
test -f "$install_prefix/include/imas_mvdd_loader.h"

# --- What the installed artifact must not carry -----------------------------
#
# Issue #3 requires that no absolute path from the build machine is baked into
# the artifact, and issue #4 that the installed shim carries none — it must
# resolve IMAS-Core through the environment, because a path captured at build
# time describes the build machine and would silently outrank the IMAS-Core the
# user's `module load` selected. Issue #1 additionally requires that the shim's
# output record no dependency on IMAS-Core at all.
#
# None of that can be checked by running the shim: the build tree deliberately
# *does* carry an RPATH to the acquired Core (that is what lets ctest run with
# no setup), and a leaked copy of it in the installed artifact still passes
# every functional test on the machine that produced it. The regression is only
# visible in the artifact's own load commands, so read those.

# -type f deliberately: the unversioned name is a symlink to the versioned
# real file, and it is the real file whose load commands matter.
installed_shim=$(find "$install_prefix" -type f \
    \( -name 'libimas_mvdd_loader.so*' -o -name 'libimas_mvdd_loader*.dylib' \) |
    head -1)
test -n "$installed_shim" || {
    echo "no installed shim shared library found under $install_prefix" >&2
    exit 1
}

# A missing inspection tool must fail rather than quietly skip the check
# (issue #10: no job may pass while silently reducing coverage).
if [[ $(uname -s) == Darwin ]]; then
    command -v otool > /dev/null || {
        echo "otool is required to inspect the installed artifact" >&2
        exit 1
    }
    embedded_search_paths=$(otool -l "$installed_shim" |
        awk '/LC_RPATH/ {rpath = 1; next}
             rpath && $1 == "path" {print $2; rpath = 0}')
    recorded_dependencies=$(otool -L "$installed_shim" | tail -n +2 |
        awk '{print $1}')
else
    command -v readelf > /dev/null || {
        echo "readelf is required to inspect the installed artifact" >&2
        exit 1
    }
    embedded_search_paths=$(readelf -d "$installed_shim" |
        sed -n 's/.*(R\(UN\)\?PATH)[^[]*\[\([^]]*\)\].*/\2/p' | tr ':' '\n')
    recorded_dependencies=$(readelf -d "$installed_shim" |
        sed -n 's/.*(NEEDED)[^[]*\[\([^]]*\)\].*/\1/p')
fi

while read -r embedded_path; do
    [[ -n $embedded_path ]] || continue
    # $ORIGIN-relative and other relative entries describe the artifact, not
    # the machine that built it, so only absolute paths can leak a build path.
    if [[ $embedded_path != /* ]]; then
        continue
    fi
    if [[ $embedded_path == "$install_prefix"/* || $embedded_path == "$install_prefix" ]]; then
        continue
    fi
    echo "installed shim bakes in an absolute build-machine path:" >&2
    echo "  $embedded_path" >&2
    echo "  (not under the install prefix $install_prefix)" >&2
    exit 1
done <<< "$embedded_search_paths"

# Belt and braces: name the two directories that would matter most if the
# clause above were ever loosened.
while read -r embedded_path; do
    [[ -n $embedded_path ]] || continue
    for build_machine_dir in "$build_dir" "$core_dir"; do
        if [[ $embedded_path == *"$build_machine_dir"* ]]; then
            echo "installed shim bakes in a build-tree path: $embedded_path" >&2
            exit 1
        fi
    done
done <<< "$embedded_search_paths"

while read -r dependency; do
    [[ -n $dependency ]] || continue
    case $(basename -- "$dependency") in
        libal.so* | libal.dylib | libal.*.dylib)
            echo "installed shim records a link-time dependency on IMAS-Core:" >&2
            echo "  $dependency" >&2
            echo "  the shim must bind IMAS-Core at runtime only (issue #1)" >&2
            exit 1
            ;;
    esac
done <<< "$recorded_dependencies"

# --- Consuming the installed package ----------------------------------------

pc=$(find "$install_prefix" -name imas-mvdd-loader.pc | head -1)
test -n "$pc" || {
    echo "no pkg-config file installed" >&2
    exit 1
}
pc_dir=$(dirname -- "$pc")
read -r -a pkg_flags <<< "$(PKG_CONFIG_PATH="$pc_dir" \
    pkg-config --print-errors --cflags --libs imas-mvdd-loader)"
cc "$source_dir/tests/cmake_find_package/main.c" -o "$build_dir/pkg-config-consumer" \
    "${pkg_flags[@]}"

libdir=$(PKG_CONFIG_PATH="$pc_dir" \
    pkg-config --variable=libdir imas-mvdd-loader)

if [[ $(uname -s) == Darwin ]]; then
    search_path_var=DYLD_LIBRARY_PATH
else
    search_path_var=LD_LIBRARY_PATH
fi
inherited_search_path=${!search_path_var:-}

# The consumer asserts getALVersion() returned a non-empty string, so it fails
# if the shim could not reach IMAS-Core. Both documented resolution routes get
# their own run.

# 1. The explicit override.
run_consumer_with_override() {
    env "$search_path_var=$libdir${inherited_search_path:+:$inherited_search_path}" \
        IMAS_CORE_LIBRARY="$core_library" "$@"
}

# 2. The bare soname through the loader's normal search path, with no override
#    set — the route an installed shim actually takes on the cluster, where
#    `module load IMAS-Core` is the only setup step. Running this at all is the
#    point: with IMAS_CORE_LIBRARY always set, the highest-precedence branch
#    answers every time and a shim that could only ever resolve Core through a
#    baked-in build path would pass unnoticed.
run_consumer_via_search_path() {
    env -u IMAS_CORE_LIBRARY \
        "$search_path_var=$libdir:$core_dir${inherited_search_path:+:$inherited_search_path}" \
        "$@"
}

run_consumer_with_override "$build_dir/pkg-config-consumer"
run_consumer_via_search_path "$build_dir/pkg-config-consumer"

consumer_build="$build_dir/installed-consumer"
cmake -S "$source_dir/tests/cmake_find_package" -B "$consumer_build" \
    -DCMAKE_PREFIX_PATH="$install_prefix"
cmake --build "$consumer_build"
run_consumer_with_override "$consumer_build/consumer_smoke"
run_consumer_via_search_path "$consumer_build/consumer_smoke"
