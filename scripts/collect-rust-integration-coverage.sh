#!/usr/bin/env bash

set -euo pipefail

root_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output_root="$root_dir/target/rust-integration-coverage"
build_root="$root_dir/target/rust-integration-coverage-build"

usage() {
    echo "usage: $0 <recording-stub|real-core|all>" >&2
}

if [[ $# -ne 1 ]]; then
    usage
    exit 2
fi
profile=$1
if [[ "$profile" == all ]]; then
    "$0" recording-stub
    "$0" real-core
    exit 0
fi
if [[ "$profile" != recording-stub && "$profile" != real-core ]]; then
    usage
    exit 2
fi

# Clear this profile's task-owned evidence before any tool preflight can fail,
# so a failed invocation can never leave an earlier report looking current.
run_dir="$output_root/$profile"
build_dir="$build_root/$profile"
case "$run_dir" in
    "$root_dir"/target/rust-integration-coverage/*) ;;
    *) echo "refusing unsafe integration-coverage output path: $run_dir" >&2; exit 2 ;;
esac
case "$build_dir" in
    "$root_dir"/target/rust-integration-coverage-build/*) ;;
    *) echo "refusing unsafe integration-coverage build path: $build_dir" >&2; exit 2 ;;
esac
rm -rf -- "$run_dir" "$build_dir"
mkdir -p "$run_dir" "$build_dir"

expected_version=$(python3 - "$root_dir/coverage/rust-line-coverage-scope.json" <<'PY'
import json
import sys

print(json.load(open(sys.argv[1]))["tool"]["cargo_llvm_cov_version"])
PY
)
actual_version=$(cargo llvm-cov --version)
if [[ "$actual_version" != "cargo-llvm-cov $expected_version" ]]; then
    echo "cargo-llvm-cov $expected_version is required; found $actual_version" >&2
    exit 2
fi

host_target=$(rustc -vV | sed -n 's/^host: //p')
if [[ -z "$host_target" ]]; then
    echo "could not determine the rustc host target" >&2
    exit 2
fi

cmake_args=(-S "$root_dir" -B "$build_dir" -DCMAKE_BUILD_TYPE=Debug)
if [[ "$profile" == recording-stub ]]; then
    artifact=private-xml-fixture
    cargo_target="$build_dir/xml-fixture-cargo"
    staged_library_dir="$build_dir/xml-fixture-stage/lib"
    loader_probe="$build_dir/rust_integration_private_loader_probe"
    cmake_args+=(-DIMAS_MVDD_REAL_CORE_TESTS=OFF)
    configuration='CMAKE_BUILD_TYPE=Debug;IMAS_MVDD_REAL_CORE_TESTS=OFF;IMAS_MVDD_GRAPH_TEST_SOURCE=controlled'
else
    artifact=production
    cargo_target="$build_dir/cargo"
    staged_library_dir="$build_dir/stage/lib"
    loader_probe="$build_dir/rust_integration_production_loader_probe"
    cmake_args+=(-DIMAS_MVDD_GRAPH_TEST_SOURCE=controlled)
    configuration='CMAKE_BUILD_TYPE=Debug;IMAS_MVDD_REAL_CORE_TESTS=ON;IMAS_MVDD_GRAPH_TEST_SOURCE=controlled'
    if [[ -n "${IMAS_MVDD_INTEGRATION_CMAKE_PREFIX_PATH:-}" ]]; then
        cmake_args+=("-DCMAKE_PREFIX_PATH=${IMAS_MVDD_INTEGRATION_CMAKE_PREFIX_PATH}")
        configuration+=";CMAKE_PREFIX_PATH=${IMAS_MVDD_INTEGRATION_CMAKE_PREFIX_PATH}"
    elif [[ "${IMAS_MVDD_INTEGRATION_DOWNLOAD_CORE:-0}" == 1 ]]; then
        cmake_args+=(-DIMAS_CORE_DOWNLOAD_DEPENDENCIES=ON)
        configuration+=';IMAS_CORE_DOWNLOAD_DEPENDENCIES=ON'
    fi
fi

# show-env supplies the rustc wrapper and matching instrumentation flags. The
# target triple is explicit because cargo-c puts even native artifacts beneath
# a target-triple directory, unlike an ordinary cargo build.
export CARGO_TARGET_DIR="$cargo_target"
eval "$(cargo llvm-cov show-env --sh --target "$host_target" 2>/dev/null)"

cmake "${cmake_args[@]}" 2>&1 | tee "$run_dir/configure.log"
cmake --build "$build_dir" 2>&1 | tee "$run_dir/build.log"

case "$(uname -s)" in
    Darwin) library_name=libimas_mvdd_loader.dylib ;;
    Linux) library_name=libimas_mvdd_loader.so ;;
    *) echo "unsupported platform for shared-shim coverage: $(uname -s)" >&2; exit 2 ;;
esac
instrumented_library="$staged_library_dir/$library_name"
if [[ ! -f "$instrumented_library" ]]; then
    echo "instrumented library does not exist after build: $instrumented_library" >&2
    exit 2
fi
if [[ ! -x "$loader_probe" ]]; then
    echo "loaded-library probe does not exist after build: $loader_probe" >&2
    exit 2
fi

clear_raw_profiles() {
    find "$cargo_target" -maxdepth 1 -type f -name '*.profraw' -delete
}

generate_report() {
    local report=$1
    cargo llvm-cov report \
        --target "$host_target" \
        --profile dev \
        --failure-mode any \
        --lcov \
        --output-path "$report"
}

archive_profiles() {
    local destination=$1
    mkdir -p "$destination"
    local found=0
    while IFS= read -r -d '' raw_profile; do
        found=1
        mv -- "$raw_profile" "$destination/"
    done < <(find "$cargo_target" -maxdepth 1 -type f -name '*.profraw' -print0)
    if [[ $found -eq 0 ]]; then
        echo "raw-profile directory $cargo_target contains no .profraw files" >&2
        return 2
    fi
}

# The recording-stub profile carries an executable, source-level A/B proof:
# loading the instrumented library alone covers no core-binding decisions, then
# the existing runtime-binding-success C scenario makes those Rust hits appear.
if [[ "$profile" == recording-stub ]]; then
    clear_raw_profiles
    export LLVM_PROFILE_FILE="$cargo_target/%p-%m.profraw"
    "$loader_probe" "$instrumented_library" >"$run_dir/baseline-loaded-library.txt"
    generate_report "$run_dir/baseline.lcov"
    archive_profiles "$run_dir/baseline-profiles"

    clear_raw_profiles
    export LLVM_PROFILE_FILE="$cargo_target/%p-%m.profraw"
    ctest --test-dir "$build_dir" -R '^runtime-binding-success$' \
        --output-on-failure --no-tests=error >"$run_dir/probe-ctest.log" 2>&1
    generate_report "$run_dir/probe.lcov"
    archive_profiles "$run_dir/probe-profiles"
fi

# Start the report-bearing run with no stale raw data. %p keeps subprocesses
# distinct and %m keeps multiple instrumented binaries distinct under parallel
# CTest execution.
clear_raw_profiles
export LLVM_PROFILE_FILE="$cargo_target/%p-%m.profraw"
"$loader_probe" "$instrumented_library" >"$run_dir/loaded-library.txt"

set +e
if [[ "$profile" == recording-stub ]]; then
    ctest --test-dir "$build_dir" \
        -L recording-stub -LE graph-runtime-map \
        -j "${CTEST_PARALLEL_LEVEL:-2}" \
        --output-on-failure --no-tests=error 2>&1 | tee "$run_dir/ctest.log"
else
    ctest --test-dir "$build_dir" \
        -L production-shim \
        -j "${CTEST_PARALLEL_LEVEL:-2}" \
        --output-on-failure --no-tests=error 2>&1 | tee "$run_dir/ctest.log"
fi
ctest_status=${PIPESTATUS[0]}
set -e

report_status=0
generate_report "$run_dir/coverage.lcov" || report_status=$?
archive_profiles "$run_dir/profiles" || report_status=$?

revision=$(git -C "$root_dir" rev-parse HEAD)
checker_args=(
    --profile "$profile"
    --artifact "$artifact"
    --revision "$revision"
    --configuration "$configuration"
    --profile-dir "$run_dir/profiles"
    --lcov "$run_dir/coverage.lcov"
    --library "$instrumented_library"
    --loaded-library-file "$run_dir/loaded-library.txt"
    --metadata "$run_dir/metadata.json"
)
if [[ "$profile" == recording-stub ]]; then
    checker_args+=(
        --baseline-lcov "$run_dir/baseline.lcov"
        --probe-lcov "$run_dir/probe.lcov"
        --probe-source src/core/core_binding.rs
    )
fi
if [[ $report_status -eq 0 ]]; then
    python3 "$root_dir/scripts/check-rust-integration-coverage.py" "${checker_args[@]}" \
        || report_status=$?
fi

if [[ $ctest_status -ne 0 ]]; then
    echo "$profile C scenarios failed with status $ctest_status; coverage evidence was retained in $run_dir" >&2
    exit "$ctest_status"
fi
if [[ $report_status -ne 0 ]]; then
    echo "$profile coverage collection failed with status $report_status; partial evidence was retained in $run_dir" >&2
    exit "$report_status"
fi

echo "$profile integration coverage written to $run_dir/coverage.lcov"
