#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "usage: $0 <build-dir> <install-prefix> <imas-core-library>" >&2
    exit 2
fi

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
source_dir=$(cd -- "$script_dir/.." && pwd)
build_dir=$(cd -- "$1" && pwd)
install_prefix=$(cd -- "$2" && pwd)
core_library=$(cd -- "$(dirname -- "$3")" && pwd)/$(basename -- "$3")

test -f "$core_library"
test -f "$install_prefix/include/imas_mvdd_loader.h"

pc=$(find "$install_prefix" -name imas-mvdd-loader.pc | head -1)
test -n "$pc" || { echo "no pkg-config file installed" >&2; exit 1; }
pc_dir=$(dirname -- "$pc")
read -r -a pkg_flags <<< "$(PKG_CONFIG_PATH="$pc_dir" \
    pkg-config --print-errors --cflags --libs imas-mvdd-loader)"
cc "$source_dir/tests/consumer/main.c" -o "$build_dir/pkg-config-consumer" \
    "${pkg_flags[@]}"

libdir=$(PKG_CONFIG_PATH="$pc_dir" \
    pkg-config --variable=libdir imas-mvdd-loader)

run_consumer() {
    if [[ $(uname -s) == Darwin ]]; then
        DYLD_LIBRARY_PATH="$libdir${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}" \
            IMAS_CORE_LIBRARY="$core_library" "$@"
    else
        LD_LIBRARY_PATH="$libdir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
            IMAS_CORE_LIBRARY="$core_library" "$@"
    fi
}

run_consumer "$build_dir/pkg-config-consumer"

consumer_build="$build_dir/installed-consumer"
cmake -S "$source_dir/tests/consumer" -B "$consumer_build" \
    -DCMAKE_PREFIX_PATH="$install_prefix"
cmake --build "$consumer_build"
run_consumer "$consumer_build/consumer_smoke"
