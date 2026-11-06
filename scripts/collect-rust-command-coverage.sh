#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
run_dir="$root_dir/target/rust-integration-coverage/artifact-command"
build_dir="$root_dir/target/rust-integration-coverage-build/artifact-command"
# Both paths are fixed, task-owned children of target; every run starts fresh.
rm -rf -- "$run_dir" "$build_dir"
mkdir -p "$run_dir" "$build_dir"
cd -- "$root_dir"
expected_version=$(python3 -c 'import json; print(json.load(open("coverage/rust-line-coverage-scope.json"))["tool"]["cargo_llvm_cov_version"])')
[[ "$(cargo llvm-cov --version)" == "cargo-llvm-cov $expected_version" ]] || {
    echo "cargo-llvm-cov $expected_version is required" >&2; exit 2;
}
host_target=$(rustc -vV | sed -n 's/^host: //p')
export CARGO_TARGET_DIR="$build_dir"
eval "$(cargo llvm-cov show-env --sh --target "$host_target")"
cargo build --no-default-features --target "$host_target" --bin validate_equilibrium_coverage \
    2>&1 | tee "$run_dir/build.log"
binary="$build_dir/$host_target/debug/validate_equilibrium_coverage"
find "$build_dir" -maxdepth 1 -name '*.profraw' -delete
export LLVM_PROFILE_FILE="$build_dir/%p-%m.profraw"
scenario_status=0
python3 tests/coverage/check-artifact-command.py --executable "$binary" --root "$root_dir" \
    --report "$run_dir/scenarios.json" 2>&1 | tee "$run_dir/scenarios.log" || scenario_status=$?
report_status=0
CARGO_LLVM_COV_TARGET_DIR="$build_dir" CARGO_LLVM_COV_BUILD_DIR="$build_dir" \
    cargo llvm-cov report --target "$host_target" --profile dev --failure-mode any \
    --lcov --output-path "$run_dir/coverage.lcov" || report_status=$?
mkdir -p "$run_dir/profiles"
find "$build_dir" -maxdepth 1 -name '*.profraw' -exec mv '{}' "$run_dir/profiles/" \;
python3 scripts/check-rust-integration-coverage.py \
    --profile artifact-command --artifact production-executable \
    --revision "$(git rev-parse HEAD)" \
    --configuration "$(rustc --version);cargo-llvm-cov $expected_version;--no-default-features;dev" \
    --profile-dir "$run_dir/profiles" --lcov "$run_dir/coverage.lcov" \
    --executable "$binary" --detail-source src/bin/validate_equilibrium_coverage.rs \
    --metadata "$run_dir/metadata.json" || report_status=$?
if [[ $scenario_status -ne 0 ]]; then exit "$scenario_status"; fi
exit "$report_status"
