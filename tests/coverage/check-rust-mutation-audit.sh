#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <source-dir>" >&2
    exit 2
fi

source_dir=$(cd -- "$1" && pwd)
checker="$source_dir/scripts/check-rust-mutation-audit.py"
fixture_dir="$source_dir/tests/fixtures"
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/imas-mvdd-mutation-audit.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT

run_fixture() {
    local fixture=$1
    local fixture_output="$work_dir/$fixture"
    python3 "$fixture_dir/create-rust-mutation-fixture.py" "$fixture" "$fixture_output"
    python3 "$checker" \
        --line-scope "$fixture_dir/rust_line_coverage_scope.json" \
        --audit "$fixture_dir/rust_mutation_audit.json" \
        --selected "$fixture_output/selected.json" \
        --mutants "$fixture_output/mutants.json" \
        --outcomes "$fixture_output/outcomes.json" \
        --dispositions "$fixture_output/dispositions.json"
}

python3 "$fixture_dir/create-rust-mutation-fixture.py" all_pass "$work_dir/selection"
python3 - "$work_dir/selection/selected.json" <<'PY'
import json
import sys

path = sys.argv[1]
mutants = json.load(open(path))
mutants.append({
    "name": "src/interpose/read.rs:1:1: replace adapter with none",
    "file": "src/interpose/read.rs",
    "span": {"start": {"line": 1}, "end": {"line": 1}},
})
json.dump(mutants, open(path, "w"), indent=2)
PY
python3 "$checker" \
    --line-scope "$fixture_dir/rust_line_coverage_scope.json" \
    --candidate-mutants "$work_dir/selection/selected.json" \
    --write-selection "$work_dir/selection/scoped.json" \
    --write-cargo-mutants-config "$work_dir/selection/mutants.toml"
grep -Fq 'src/conversion.rs:1:1: replace conversion outcome 1' "$work_dir/selection/scoped.json"
if grep -Fq 'src/interpose/read.rs' "$work_dir/selection/scoped.json"; then
    echo "a mutant outside the line-audit scope must not be selected" >&2
    exit 1
fi
grep -Fq '^src/conversion\\.rs:1:1: replace conversion outcome 1$' "$work_dir/selection/mutants.toml"

output=$(run_fixture all_pass)
grep -Fq 'conversion: caught=2 missed=0 timed-out=0 unviable=1 excluded=1 score=100.0% PASS' <<<"$output"
grep -Fq 'aggregate: caught=7 missed=0 timed-out=0 unviable=1 excluded=1 score=100.0% PASS' <<<"$output"
grep -Fq 'src/conversion.rs:4:1: replace branch with false | observable behavior: the documented equivalent branch is unchanged | classification: equivalent | disposition: excluded: zero and positive values take the same branch' <<<"$output"

output=$(run_fixture integration_only)
grep -Fq 'classification: integration-only | disposition: excluded: the observable backend behavior is covered by a real-Core integration test' <<<"$output"

output=$(run_fixture group_boundary)
grep -Fq 'loss: caught=3 missed=1 timed-out=0 unviable=0 excluded=0 score=75.0% PASS' <<<"$output"
grep -Fq 'aggregate: caught=23 missed=1 timed-out=0 unviable=0 excluded=0 score=95.8% PASS' <<<"$output"

if run_fixture aggregate_failure >"$work_dir/aggregate-failure.txt" 2>&1; then
    echo "a below-floor aggregate must fail even when every mutation group passes" >&2
    exit 1
fi
grep -Fq 'aggregate: caught=19 missed=5 timed-out=0 unviable=0 excluded=0 score=79.2% below 85.0%' "$work_dir/aggregate-failure.txt"

if run_fixture group_failure >"$work_dir/group-failure.txt" 2>&1; then
    echo "an aggregate pass with a weak mutation group must fail" >&2
    exit 1
fi
grep -Fq 'loss: caught=2 missed=2 timed-out=0 unviable=0 excluded=0 score=50.0% below 75.0%' "$work_dir/group-failure.txt"
grep -Fq 'aggregate: caught=22 missed=2 timed-out=0 unviable=0 excluded=0 score=91.7% PASS' "$work_dir/group-failure.txt"

if run_fixture timeout >"$work_dir/timeout.txt" 2>&1; then
    echo "a timed-out mutant must fail even above the numeric floor" >&2
    exit 1
fi
grep -Fq 'loss: caught=1 missed=0 timed-out=1 unviable=0 excluded=0 score=50.0% below 75.0% timed-out' "$work_dir/timeout.txt"

output=$(run_fixture unviable)
grep -Fq 'conversion: caught=1 missed=0 timed-out=0 unviable=1 excluded=0 score=100.0% PASS' <<<"$output"

if run_fixture unscoreable >"$work_dir/unscoreable.txt" 2>&1; then
    echo "a group with no scoreable mutant must fail as incomplete" >&2
    exit 1
fi
grep -Fq 'mutation audit input error: group conversion has no scoreable mutants' "$work_dir/unscoreable.txt"

if run_fixture output_mismatch >"$work_dir/output-mismatch.txt" 2>&1; then
    echo "a mutated output record must not pass by retaining its identity" >&2
    exit 1
fi
grep -Fq 'mutation audit input error: cargo-mutants mutant list does not match the selected scoped mutants' "$work_dir/output-mismatch.txt"

if run_fixture incomplete >"$work_dir/incomplete.txt" 2>&1; then
    echo "an incomplete mutation run must fail" >&2
    exit 1
fi
grep -Fq 'mutation audit input error: selected mutant src/loss.rs:1:1: replace loss with none has no outcome' "$work_dir/incomplete.txt"
