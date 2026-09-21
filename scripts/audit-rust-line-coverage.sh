#!/usr/bin/env bash

set -euo pipefail

root_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
scope="$root_dir/coverage/rust-line-coverage-scope.json"
report="$root_dir/target/rust-line-coverage.lcov"

expected_version=$(python3 - "$scope" <<'PY'
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

cd -- "$root_dir"
cargo llvm-cov --all-features --lcov --output-path "$report"
python3 scripts/check-rust-line-coverage.py --root "$root_dir" --scope "$scope" --lcov "$report"
