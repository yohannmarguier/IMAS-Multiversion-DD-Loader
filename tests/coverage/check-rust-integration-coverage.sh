#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <source-dir>" >&2
    exit 2
fi

source_dir=$(cd -- "$1" && pwd)
checker="$source_dir/scripts/check-rust-integration-coverage.py"
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/imas-mvdd-integration-coverage.XXXXXX")
trap 'rm -rf "$work_dir"' EXIT

mkdir -p "$work_dir/profiles" "$work_dir/lib"
: >"$work_dir/profiles/101-abc.profraw"
: >"$work_dir/lib/libimas_mvdd_loader.so"
printf '%s\n' "$work_dir/lib/libimas_mvdd_loader.so" >"$work_dir/loaded-library.txt"

cat >"$work_dir/report.lcov" <<'LCOV'
SF:src/core/core_binding.rs
DA:10,0
DA:11,3
end_of_record
SF:src/interpose/read.rs
DA:20,7
DA:21,0
end_of_record
LCOV
cat >"$work_dir/baseline.lcov" <<'LCOV'
SF:src/core/core_binding.rs
DA:10,0
DA:11,0
end_of_record
LCOV
cat >"$work_dir/probe.lcov" <<'LCOV'
SF:src/core/core_binding.rs
DA:10,1
DA:11,2
end_of_record
LCOV
cat >"$work_dir/occurrence-xml.lcov" <<'LCOV'
SF:src/interpose/occurrence.rs
DA:20,1
DA:21,0
DA:22,3
end_of_record
SF:src/conversion/seam_policy.rs
DA:98,2
DA:104,0
end_of_record
LCOV
cat >"$work_dir/occurrence-graph.lcov" <<'LCOV'
SF:src/interpose/occurrence.rs
DA:20,0
DA:21,4
DA:22,0
end_of_record
SF:src/conversion/seam_policy.rs
DA:98,0
DA:104,3
end_of_record
LCOV

python3 "$checker" \
    --profile recording-stub \
    --artifact private-xml-fixture \
    --revision 0123456789abcdef \
    --configuration 'CMAKE_BUILD_TYPE=Debug;IMAS_MVDD_REAL_CORE_TESTS=OFF' \
    --profile-dir "$work_dir/profiles" \
    --lcov "$work_dir/report.lcov" \
    --library "$work_dir/lib/libimas_mvdd_loader.so" \
    --loaded-library-file "$work_dir/loaded-library.txt" \
    --baseline-lcov "$work_dir/baseline.lcov" \
    --probe-lcov "$work_dir/probe.lcov" \
    --probe-source src/core/core_binding.rs \
    --component-lcov "$work_dir/occurrence-xml.lcov" \
    --component-lcov "$work_dir/occurrence-graph.lcov" \
    --component-profile-dir "$work_dir/profiles" \
    --component-scope "$source_dir/coverage/rust-occurrence-adapter-coverage-scope.json" \
    --source src/interpose/read.rs \
    --metadata "$work_dir/metadata.json"

python3 - "$work_dir/metadata.json" <<'PY'
import json
import sys

value = json.load(open(sys.argv[1]))
assert value["schema_version"] == 1
assert value["profile"] == "recording-stub"
assert value["artifact"] == "private-xml-fixture"
assert value["revision"] == "0123456789abcdef"
assert value["profile_count"] == 1
assert value["probe"]["baseline_covered_lines"] == 0
assert value["probe"]["scenario_covered_lines"] == 2
assert value["component"]["adapter"] == {
    "source": "src/interpose/occurrence.rs",
    "covered_lines": 3,
    "measured_lines": 3,
    "uncovered_lines": [],
}
assert value["component"]["profile_count"] == 1
assert len(value["component"]["lcov"]) == 2
assert value["component"]["lcov"][0].endswith("/occurrence-xml.lcov")
assert value["component"]["lcov"][1].endswith("/occurrence-graph.lcov")
assert value["component"]["policy"] == {
    "source": "src/conversion/seam_policy.rs",
    "start_line": 98,
    "end_line": 134,
    "covered_lines": 2,
    "measured_lines": 2,
    "uncovered_lines": [],
}
assert value["sources"] == {
    "src/interpose/read.rs": {"covered_lines": 1, "measured_lines": 2}
}
PY

if python3 "$checker" \
    --profile recording-stub --artifact private-xml-fixture --revision deadbeef \
    --configuration Debug --profile-dir "$work_dir/profiles" \
    --lcov "$work_dir/report.lcov" --library "$work_dir/lib/libimas_mvdd_loader.so" \
    --loaded-library-file "$work_dir/loaded-library.txt" \
    --component-lcov "$work_dir/occurrence-xml.lcov" \
    --component-profile-dir "$work_dir/profiles" \
    --component-scope "$source_dir/coverage/rust-occurrence-adapter-coverage-scope.json" \
    --metadata "$work_dir/one-component.json" >"$work_dir/one-component.txt" 2>&1; then
    echo "a single occurrence component report must fail" >&2
    exit 1
fi
grep -Fq 'requires exactly two LCOV reports' "$work_dir/one-component.txt"

if python3 "$checker" \
    --profile recording-stub --artifact private-xml-fixture --revision deadbeef \
    --configuration Debug --profile-dir "$work_dir/profiles" \
    --lcov "$work_dir/report.lcov" --library "$work_dir/lib/libimas_mvdd_loader.so" \
    --loaded-library-file "$work_dir/loaded-library.txt" \
    --source src/interpose/missing.rs \
    --metadata "$work_dir/missing-source.json" >"$work_dir/missing-source.txt" 2>&1; then
    echo "a requested source absent from LCOV must fail" >&2
    exit 1
fi
grep -Fq 'LCOV report has no measurement data for src/interpose/missing.rs' \
    "$work_dir/missing-source.txt"

mkdir -p "$work_dir/empty-profiles"
if python3 "$checker" \
    --profile recording-stub --artifact private-xml-fixture --revision deadbeef \
    --configuration Debug --profile-dir "$work_dir/empty-profiles" \
    --lcov "$work_dir/report.lcov" --library "$work_dir/lib/libimas_mvdd_loader.so" \
    --loaded-library-file "$work_dir/loaded-library.txt" \
    --metadata "$work_dir/empty.json" >"$work_dir/empty.txt" 2>&1; then
    echo "an empty raw-profile directory must fail" >&2
    exit 1
fi
grep -Fq 'contains no .profraw files' "$work_dir/empty.txt"

if python3 "$checker" \
    --profile recording-stub --artifact private-xml-fixture --revision deadbeef \
    --configuration Debug --profile-dir "$work_dir/profiles" \
    --lcov "$work_dir/report.lcov" --library "$work_dir/lib/missing.so" \
    --loaded-library-file "$work_dir/loaded-library.txt" \
    --metadata "$work_dir/missing.json" >"$work_dir/missing.txt" 2>&1; then
    echo "an absent instrumented library must fail" >&2
    exit 1
fi
grep -Fq 'instrumented library does not exist' "$work_dir/missing.txt"

: >"$work_dir/lib/other.so"
printf '%s\n' "$work_dir/lib/other.so" >"$work_dir/wrong-library.txt"
if python3 "$checker" \
    --profile recording-stub --artifact private-xml-fixture --revision deadbeef \
    --configuration Debug --profile-dir "$work_dir/profiles" \
    --lcov "$work_dir/report.lcov" --library "$work_dir/lib/libimas_mvdd_loader.so" \
    --loaded-library-file "$work_dir/wrong-library.txt" \
    --metadata "$work_dir/wrong.json" >"$work_dir/wrong.txt" 2>&1; then
    echo "a C executable loading another shim must fail" >&2
    exit 1
fi
grep -Fq 'loaded shim does not match the instrumented library' "$work_dir/wrong.txt"

if python3 "$checker" \
    --profile recording-stub --artifact private-xml-fixture --revision deadbeef \
    --configuration Debug --profile-dir "$work_dir/profiles" \
    --lcov "$work_dir/report.lcov" --library "$work_dir/lib/libimas_mvdd_loader.so" \
    --loaded-library-file "$work_dir/loaded-library.txt" \
    --baseline-lcov "$work_dir/baseline.lcov" \
    --probe-lcov "$work_dir/baseline.lcov" \
    --probe-source src/core/core_binding.rs \
    --metadata "$work_dir/no-hit.json" >"$work_dir/no-hit.txt" 2>&1; then
    echo "a C-only probe that adds no Rust hits must fail" >&2
    exit 1
fi
grep -Fq 'scenario leaves src/core/core_binding.rs at zero covered lines' "$work_dir/no-hit.txt"

# Exercise cargo-llvm-cov's strict merge path with a genuinely malformed raw
# profile. This is behavioral coverage of the incompatibility refusal, not a
# source-text assertion that the option happens to be present.
mkdir -p "$work_dir/incompatible-target"
printf 'not-an-llvm-profile\n' >"$work_dir/incompatible-target/incompatible.profraw"
host_target=$(rustc -vV | sed -n 's/^host: //p')
if CARGO_TARGET_DIR="$work_dir/incompatible-target" \
    CARGO_LLVM_COV_TARGET_DIR="$work_dir/incompatible-target" \
    CARGO_LLVM_COV_BUILD_DIR="$work_dir/incompatible-target" \
    CARGO_LLVM_COV=1 CARGO_LLVM_COV_SHOW_ENV=1 \
    cargo llvm-cov report --manifest-path "$source_dir/Cargo.toml" \
        --target "$host_target" --profile dev \
        --failure-mode any --lcov \
        --output-path "$work_dir/incompatible.lcov" \
        >"$work_dir/incompatible.txt" 2>&1; then
    echo "an incompatible raw profile must fail" >&2
    exit 1
fi
grep -Eq 'truncated profile data|malformed instrumentation profile data|no profile can be merged' \
    "$work_dir/incompatible.txt"

# Protect the collection contract that cannot be represented by an LCOV
# fixture: fresh task-owned output, collision-free process/module names,
# and preservation of CTest's own exit status.
collector="$source_dir/scripts/collect-rust-integration-coverage.sh"
grep -Fq 'rm -rf -- "$run_dir" "$build_dir"' "$collector"
grep -Fq 'LLVM_PROFILE_FILE="$cargo_target/%p-%m.profraw"' "$collector"
grep -Fq 'ctest_status=${PIPESTATUS[0]}' "$collector"
grep -Fq 'exit "$ctest_status"' "$collector"
grep -Fq -- '-L occurrence-adapter' "$collector" || {
    echo "collector does not execute the occurrence-adapter scenario ledger" >&2
    exit 1
}
grep -Fq 'occurrence-adapter-xml.lcov' "$collector" || {
    echo "collector does not retain the XML occurrence adapter report" >&2
    exit 1
}
grep -Fq 'occurrence-adapter-graph.lcov' "$collector" || {
    echo "collector does not retain the graph occurrence adapter report" >&2
    exit 1
}
grep -Fq -- '--component-scope "$root_dir/coverage/rust-occurrence-adapter-coverage-scope.json"' "$collector" || exit 1
for source in \
    src/interpose/read.rs \
    src/interpose/write.rs \
    src/interpose/delete.rs \
    src/conversion/seam_policy.rs; do
    grep -Fq -- "--source $source" "$collector"
done

# A helper-only report must not satisfy the executable or loss-adapter claim.
cat >"$work_dir/command.lcov" <<'LCOV'
SF:src/bin/validate_equilibrium_coverage.rs
DA:20,1
DA:21,0
end_of_record
SF:src/artifact_validation.rs
DA:30,5
end_of_record
LCOV
command_args=(--profile artifact-command --artifact production-executable
    --revision deadbeef --configuration production --profile-dir "$work_dir/profiles"
    --executable "$work_dir/lib/libimas_mvdd_loader.so"
    --detail-source src/bin/validate_equilibrium_coverage.rs)
python3 "$checker" "${command_args[@]}" --lcov "$work_dir/command.lcov" \
    --metadata "$work_dir/command.json"
python3 - "$work_dir/command.json" <<'PY'
import json, sys
value = json.load(open(sys.argv[1]))
assert value['details'] == [{
    'source': 'src/bin/validate_equilibrium_coverage.rs',
    'covered_lines': 1, 'measured_lines': 2, 'uncovered_lines': [21]}]
assert 'executable' in value and 'library' not in value
PY
sed 's/DA:20,1/DA:20,0/' "$work_dir/command.lcov" >"$work_dir/helper-only.lcov"
if python3 "$checker" "${command_args[@]}" --lcov "$work_dir/helper-only.lcov" \
    --metadata "$work_dir/helper-only.json" >"$work_dir/helper-only.txt" 2>&1; then
    echo "helper-only hits must not satisfy executable measurement" >&2
    exit 1
fi
grep -Fq 'required source src/bin/validate_equilibrium_coverage.rs has no executed lines' \
    "$work_dir/helper-only.txt"
if python3 "$checker" "${command_args[@]}" --lcov "$work_dir/report.lcov" \
    --metadata "$work_dir/absent-command.json" >"$work_dir/absent-command.txt" 2>&1; then
    echo "a report without the command source must fail" >&2
    exit 1
fi
grep -Fq 'no measurement data for src/bin/validate_equilibrium_coverage.rs' "$work_dir/absent-command.txt"
grep -Fq -- '-L loss-adapter' "$collector"
grep -Fq -- '--detail-source src/interpose/loss.rs --detail-source src/loss_file.rs' "$collector"
grep -Fq 'exit "$loss_status"' "$collector"
command_collector="$source_dir/scripts/collect-rust-command-coverage.sh"
grep -Fq 'tests/coverage/check-artifact-command.py --executable "$binary"' "$command_collector"
grep -Fq -- '--detail-source src/bin/validate_equilibrium_coverage.rs' "$command_collector"
grep -Fq 'exit "$scenario_status"' "$command_collector"
