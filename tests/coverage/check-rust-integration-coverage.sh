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
assert value["sources"] == {
    "src/interpose/read.rs": {"covered_lines": 1, "measured_lines": 2}
}
PY

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
    cargo llvm-cov report --target "$host_target" --profile dev \
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
for source in \
    src/interpose/read.rs \
    src/interpose/write.rs \
    src/interpose/delete.rs \
    src/conversion/seam_policy.rs; do
    grep -Fq -- "--source $source" "$collector"
done
