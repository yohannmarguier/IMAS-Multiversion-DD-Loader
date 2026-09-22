#!/usr/bin/env bash

set -euo pipefail

root_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
scope="$root_dir/coverage/rust-line-coverage-scope.json"
xml_scope="$root_dir/coverage/rust-xml-line-coverage-scope.json"
production_test_witnesses="$root_dir/coverage/rust-production-line-coverage-tests.json"
production_report="$root_dir/target/rust-line-coverage-production.lcov"
xml_report="$root_dir/target/rust-line-coverage-xml.lcov"
production_tests=$(mktemp "${TMPDIR:-/tmp}/imas-mvdd-production-tests.XXXXXX")
ignored_production_tests=$(mktemp "${TMPDIR:-/tmp}/imas-mvdd-ignored-production-tests.XXXXXX")
trap 'rm -f -- "$production_tests" "$ignored_production_tests"' EXIT

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
cargo test --no-default-features --lib -- --list >"$production_tests"
cargo test --no-default-features --lib -- --ignored --list >"$ignored_production_tests"
python3 scripts/check-rust-test-witnesses.py --manifest "$production_test_witnesses" --discovered "$production_tests" --ignored "$ignored_production_tests"
cargo llvm-cov --no-default-features --lib --lcov --output-path "$production_report"
echo "production Rust line coverage"
python3 scripts/check-rust-line-coverage.py --root "$root_dir" --scope "$scope" --lcov "$production_report"

cargo llvm-cov --no-default-features --features xml-fixture-source --lib --lcov --output-path "$xml_report"
echo "XML fixture Rust line coverage"
python3 scripts/check-rust-line-coverage.py --root "$root_dir" --scope "$xml_scope" --lcov "$xml_report"
