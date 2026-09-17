#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <source-dir>" >&2
    exit 2
fi

source_dir=$(cd -- "$1" && pwd)
checker="$source_dir/scripts/check-rust-line-coverage.py"
fixture_dir="$source_dir/tests/fixtures"
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/imas-mvdd-coverage-audit.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT

output=$(python3 "$checker" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_all_pass.lcov")
grep -Fq 'aggregate: 54/60 lines (90.0%)' <<<"$output"
grep -Fq 'conversion: 9/10 lines (90.0%) PASS' <<<"$output"

output=$(python3 "$checker" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_group_boundary.lcov")
grep -Fq 'loss: 8/10 lines (80.0%) PASS' <<<"$output"
grep -Fq 'aggregate: 58/60 lines (96.7%) PASS' <<<"$output"

if python3 "$checker" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_group_failure.lcov" >"$work_dir/group-failure.txt" 2>&1; then
    echo "an aggregate pass with a weak group must fail" >&2
    exit 1
fi
grep -Fq 'loss: 6/10 lines (60.0%) below 80.0%' "$work_dir/group-failure.txt"
grep -Fq 'aggregate: 56/60 lines (93.3%)' "$work_dir/group-failure.txt"

if python3 "$checker" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_empty.lcov" >"$work_dir/empty.txt" 2>&1; then
    echo "an incomplete coverage report must fail" >&2
    exit 1
fi
grep -Fq 'required source src/version.rs has no measurement data' "$work_dir/empty.txt"
grep -Fq 'required source src/conversion.rs has empty measurement data' "$work_dir/empty.txt"

cp "$fixture_dir/rust_line_coverage_all_pass.lcov" "$work_dir/unmapped.lcov"
printf 'SF:src/unmapped.rs\nDA:1,1\nend_of_record\n' >>"$work_dir/unmapped.lcov"
if python3 "$checker" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$work_dir/unmapped.lcov" >"$work_dir/unmapped.txt" 2>&1; then
    echo "an unmapped first-party source must fail" >&2
    exit 1
fi
grep -Fq 'source src/unmapped.rs is unmapped; assign it to a group or an exclusion' "$work_dir/unmapped.txt"

if python3 "$checker" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_invalid.lcov" >"$work_dir/invalid.txt" 2>&1; then
    echo "a malformed LCOV record must fail" >&2
    exit 1
fi
grep -Fq 'coverage audit input error: unrecognised LCOV record: not-an-lcov-record' "$work_dir/invalid.txt"
