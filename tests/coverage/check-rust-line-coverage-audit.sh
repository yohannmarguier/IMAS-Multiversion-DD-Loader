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
revision=$(git -C "$source_dir" rev-parse HEAD)

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

# The gate owns a deliberately narrower denominator than cargo-llvm-cov's
# unfiltered report. Prove one fresh report explains both without turning the
# lower diagnostic percentage into a failure.
report="$work_dir/report.md"
printf 'stale report\n' >"$report"
python3 "$checker" \
    --initialize-report \
    --report-output "$report"
grep -Fq '# Rust line-coverage audit' "$report"
grep -Fq 'Overall audit result: **INCOMPLETE**' "$report"
if grep -Fq 'stale report' "$report"; then
    echo "report initialization must replace stale evidence" >&2
    exit 1
fi
output=$(python3 "$checker" \
    --root "$source_dir" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_scoped_pass_full_below.lcov" \
    --configuration 'production (--no-default-features)' \
    --revision "$revision" \
    --rustc-version 'rustc 1.88.0 (fixture)' \
    --cargo-llvm-cov-version 'cargo-llvm-cov 0.9.1' \
    --report-output "$report")
grep -Fq 'aggregate: 54/60 lines (90.0%) PASS' <<<"$output"
grep -Fq '# Rust line-coverage audit' "$report"
grep -Fq "Revision: \`$revision\`" "$report"
grep -Fq 'Rust toolchain: `rustc 1.88.0 (fixture)`' "$report"
grep -Fq 'Coverage tool: `cargo-llvm-cov 0.9.1`' "$report"
grep -Fq '## production (--no-default-features)' "$report"
grep -Fq 'Scoped aggregate | 54/60 | 90.0% | PASS (minimum 90.0%)' "$report"
grep -Fq 'Full unfiltered report | 54/80 | 67.5% | diagnostic only' "$report"
grep -Fq 'Excluded production | 0/20 | 0.0% | outside the scoped gate' "$report"
grep -Fq 'Inline test bodies in full metric | 0/0 | absent | outside the scoped gate' "$report"
grep -Fq 'The pass condition is the scoped aggregate and every scoped group; the full unfiltered percentage is diagnostic.' "$report"
grep -Fq 'src/interpose/read.rs — raw-pointer result adaptation' "$report"

python3 - "$fixture_dir/rust_line_coverage_scope.json" "$work_dir/inline-scope.json" <<'PY'
import json
import sys

scope = json.load(open(sys.argv[1]))
scope["groups"][0]["sources"].append(
    {
        "path": "tests/fixtures/rust_line_coverage_inline_source.rs",
        "start_line": 1,
        "end_line": 2,
    }
)
json.dump(scope, open(sys.argv[2], "w"), indent=2)
PY
cp "$fixture_dir/rust_line_coverage_scoped_pass_full_below.lcov" "$work_dir/inline.lcov"
printf 'SF:tests/fixtures/rust_line_coverage_inline_source.rs\nDA:1,1\nDA:5,1\nend_of_record\n' \
    >>"$work_dir/inline.lcov"
python3 "$checker" \
    --root "$source_dir" \
    --scope "$work_dir/inline-scope.json" \
    --lcov "$work_dir/inline.lcov" \
    --configuration 'inline-test accounting fixture' \
    --revision "$revision" \
    --rustc-version 'rustc 1.88.0 (fixture)' \
    --cargo-llvm-cov-version 'cargo-llvm-cov 0.9.1' \
    --report-output "$work_dir/inline-report.md" >/dev/null
grep -Fq 'Inline test bodies in full metric | 1/1 | present | outside the scoped gate' \
    "$work_dir/inline-report.md"

python3 - "$work_dir/inline-scope.json" "$work_dir/inline-overreach.json" <<'PY'
import json
import sys

scope = json.load(open(sys.argv[1]))
scope["groups"][0]["sources"][-1]["end_line"] = 5
json.dump(scope, open(sys.argv[2], "w"), indent=2)
PY
if python3 "$checker" \
    --root "$source_dir" \
    --scope "$work_dir/inline-overreach.json" \
    --lcov "$work_dir/inline.lcov" >"$work_dir/inline-overreach.txt" 2>&1; then
    echo "a scoped range reaching into an inline test module must fail" >&2
    exit 1
fi
grep -Fq 'tests/fixtures/rust_line_coverage_inline_source.rs:3-5 belongs to an inline test module, not the production scope' \
    "$work_dir/inline-overreach.txt"

python3 "$checker" \
    --root "$source_dir" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_group_boundary.lcov" \
    --configuration 'private XML fixture (--no-default-features --features xml-fixture-source)' \
    --revision "$revision" \
    --rustc-version 'rustc 1.88.0 (fixture)' \
    --cargo-llvm-cov-version 'cargo-llvm-cov 0.9.1' \
    --report-output "$report" \
    --append-report >/dev/null
grep -Fq '## private XML fixture (--no-default-features --features xml-fixture-source)' "$report"
[[ $(grep -c '^## ' "$report") -eq 2 ]]

if python3 "$checker" \
    --root "$source_dir" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_all_pass.lcov" \
    --configuration production \
    --revision deadbeef \
    --rustc-version 'rustc 1.88.0 (fixture)' \
    --cargo-llvm-cov-version 'cargo-llvm-cov 0.9.1' \
    --report-output "$work_dir/stale.md" >"$work_dir/stale.txt" 2>&1; then
    echo "stale revision metadata must fail" >&2
    exit 1
fi
grep -Fq "coverage audit input error: recorded revision deadbeef does not match HEAD $revision" \
    "$work_dir/stale.txt"

if python3 "$checker" \
    --root "$source_dir" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_group_failure.lcov" \
    --configuration production \
    --revision "$revision" \
    --rustc-version 'rustc 1.88.0 (fixture)' \
    --cargo-llvm-cov-version 'cargo-llvm-cov 0.9.1' \
    --report-output "$work_dir/failed-report.md" >/dev/null 2>&1; then
    echo "the weak scoped group must still fail" >&2
    exit 1
fi
grep -Fq 'Overall scoped verdict: **FAIL**' "$work_dir/failed-report.md"

# Inclusive ranges may meet at adjacent lines, but assigning even one line to
# two groups would count one production decision twice.
python3 - "$fixture_dir/rust_line_coverage_scope.json" "$work_dir/overlap.json" <<'PY'
import json
import sys

scope = json.load(open(sys.argv[1]))
scope["groups"][0]["sources"] = [
    {"path": "src/shared.rs", "start_line": 1, "end_line": 5}
]
scope["groups"][1]["sources"] = [
    {"path": "src/shared.rs", "start_line": 5, "end_line": 10}
]
json.dump(scope, open(sys.argv[2], "w"), indent=2)
PY
if python3 "$checker" \
    --scope "$work_dir/overlap.json" \
    --lcov "$fixture_dir/rust_line_coverage_all_pass.lcov" >"$work_dir/overlap.txt" 2>&1; then
    echo "overlapping group assignments must fail" >&2
    exit 1
fi
grep -Fq 'source src/shared.rs:5-10 is assigned to both conversion and dd-version' \
    "$work_dir/overlap.txt"

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
    --lcov "$fixture_dir/rust_line_coverage_aggregate_failure.lcov" >"$work_dir/aggregate-failure.txt" 2>&1; then
    echo "groups at 80% with an aggregate below 90% must fail" >&2
    exit 1
fi
grep -Fq 'conversion: 8/10 lines (80.0%) PASS' "$work_dir/aggregate-failure.txt"
grep -Fq 'aggregate: 48/60 lines (80.0%) below 90.0%' "$work_dir/aggregate-failure.txt"

if python3 "$checker" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_empty.lcov" >"$work_dir/empty.txt" 2>&1; then
    echo "an incomplete coverage report must fail" >&2
    exit 1
fi
grep -Fq 'required source src/fixture/version.rs has no measurement data' "$work_dir/empty.txt"
grep -Fq 'required source src/fixture/conversion.rs has empty measurement data' "$work_dir/empty.txt"

printf 'stale report\n' >"$work_dir/missing-report.md"
if python3 "$checker" \
    --root "$source_dir" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$work_dir/missing.lcov" \
    --configuration production \
    --revision "$revision" \
    --rustc-version 'rustc 1.88.0 (fixture)' \
    --cargo-llvm-cov-version 'cargo-llvm-cov 0.9.1' \
    --report-output "$work_dir/missing-report.md" >"$work_dir/missing.txt" 2>&1; then
    echo "a missing coverage report must fail" >&2
    exit 1
fi
grep -Fq 'coverage audit input error: cannot read LCOV report' "$work_dir/missing.txt"
grep -Fq '# Rust line-coverage audit' "$work_dir/missing-report.md"
grep -Fq '## production' "$work_dir/missing-report.md"
grep -Fq 'Overall audit result: **INPUT ERROR**' "$work_dir/missing-report.md"
grep -Fq 'cannot read LCOV report' "$work_dir/missing-report.md"
if grep -Fq 'stale report' "$work_dir/missing-report.md"; then
    echo "an input failure must replace a stale report" >&2
    exit 1
fi
python3 "$checker" \
    --root "$source_dir" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_group_boundary.lcov" \
    --configuration 'private XML fixture' \
    --revision "$revision" \
    --rustc-version 'rustc 1.88.0 (fixture)' \
    --cargo-llvm-cov-version 'cargo-llvm-cov 0.9.1' \
    --report-output "$work_dir/missing-report.md" \
    --append-report >/dev/null
[[ $(grep -c '^# Rust line-coverage audit$' "$work_dir/missing-report.md") -eq 1 ]]
[[ $(grep -c '^## ' "$work_dir/missing-report.md") -eq 2 ]]

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

# Simulate the real regression: a previously complete range becomes stale
# when a production decision is inserted immediately before the test module.
python3 - "$tree/src/example.rs" <<'PY'
import sys

path = sys.argv[1]
source = open(path).read()
source = source.replace(
    "#[cfg(test)]",
    "fn newly_inserted_decision(value: bool) -> i32 {\n    if value { 1 } else { 2 }\n}\n\n#[cfg(test)]",
)
open(path, "w").write(source)
PY
if python3 "$checker" --root "$tree" \
    --scope "$tree/declared.json" \
    --lcov "$tree/example.lcov" >"$work_dir/inserted.txt" 2>&1; then
    echo "production inserted before a test module must invalidate the stale scope" >&2
    exit 1
fi
grep -Fq 'src/example.rs: production lines 9-12 belong to no group and no exclusion' \
    "$work_dir/inserted.txt"

if python3 "$checker" \
    --scope "$fixture_dir/rust_line_coverage_scope.json" \
    --lcov "$fixture_dir/rust_line_coverage_invalid.lcov" >"$work_dir/invalid.txt" 2>&1; then
    echo "a malformed LCOV record must fail" >&2
    exit 1
fi
grep -Fq 'coverage audit input error: unrecognised LCOV record: not-an-lcov-record' "$work_dir/invalid.txt"
