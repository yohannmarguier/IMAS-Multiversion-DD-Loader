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

# A group range that stops short of its file's inline test module takes
# production code out of the denominator without saying so. Prove the checker
# refuses that, and accepts the same range once the remainder is a declared
# exclusion. This needs a real source file, so build a throwaway one.
tree="$work_dir/tree"
mkdir -p "$tree/src"
cat >"$tree/src/example.rs" <<'RUST'
fn one() -> i32 {
    1
}

fn two() -> i32 {
    2
}

#[cfg(test)]
mod tests {
    #[test]
    fn works() {}
}
RUST
cp "$fixture_dir/rust_line_coverage_all_pass.lcov" "$tree/example.lcov"
printf 'SF:src/example.rs\nDA:2,1\nDA:6,1\nend_of_record\n' >>"$tree/example.lcov"

partial_scope() {
    python3 - "$fixture_dir/rust_line_coverage_scope.json" "$1" "$2" <<'PY'
import json
import sys

scope = json.load(open(sys.argv[1]))
for group in scope["groups"]:
    if group["name"] == "conversion":
        group["sources"].append({"path": "src/example.rs", "start_line": 1, "end_line": 3})
if sys.argv[3] == "declared":
    scope["exclusions"].append(
        {"path": "src/example.rs", "start_line": 4, "end_line": 8, "reason": "throwaway fixture"}
    )
json.dump(scope, open(sys.argv[2], "w"), indent=2)
PY
}

partial_scope "$tree/truncated.json" undeclared
if python3 "$checker" --root "$tree" \
    --scope "$tree/truncated.json" \
    --lcov "$tree/example.lcov" >"$work_dir/truncated.txt" 2>&1; then
    echo "a range that stops short of the test module must fail" >&2
    exit 1
fi
grep -Fq 'src/example.rs: production lines 4-8 belong to no group and no exclusion' \
    "$work_dir/truncated.txt"

partial_scope "$tree/declared.json" declared
python3 "$checker" --root "$tree" \
    --scope "$tree/declared.json" \
    --lcov "$tree/example.lcov" >/dev/null

if python3 "$checker" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_invalid.lcov" >"$work_dir/invalid.txt" 2>&1; then
    echo "a malformed LCOV record must fail" >&2
    exit 1
fi
grep -Fq 'coverage audit input error: unrecognised LCOV record: not-an-lcov-record' "$work_dir/invalid.txt"
