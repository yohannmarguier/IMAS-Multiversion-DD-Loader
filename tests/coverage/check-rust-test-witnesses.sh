#!/usr/bin/env bash

set -euo pipefail

root_dir=$1
checker="$root_dir/scripts/check-rust-test-witnesses.py"
manifest="$root_dir/coverage/rust-production-line-coverage-tests.json"
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/imas-rust-test-witnesses.XXXXXX")
trap 'rm -rf -- "$work_dir"' EXIT

python3 - "$manifest" >"$work_dir/discovered.txt" <<'PY'
import json
import sys

for test in json.load(open(sys.argv[1]))["witnesses"].values():
    print(f"{test}: test")
PY
: >"$work_dir/ignored.txt"

python3 "$checker" --manifest "$manifest" \
    --discovered "$work_dir/discovered.txt" --ignored "$work_dir/ignored.txt"

head -n 1 "$work_dir/discovered.txt" >"$work_dir/ignored.txt"
if python3 "$checker" --manifest "$manifest" \
    --discovered "$work_dir/discovered.txt" --ignored "$work_dir/ignored.txt" \
    >"$work_dir/ignored-output.txt" 2>&1; then
    echo "an ignored production witness must fail" >&2
    exit 1
fi
grep -Fq 'production runtime-map test witnesses are ignored:' \
    "$work_dir/ignored-output.txt"

tail -n +2 "$work_dir/discovered.txt" >"$work_dir/missing.txt"
: >"$work_dir/ignored.txt"
if python3 "$checker" --manifest "$manifest" \
    --discovered "$work_dir/missing.txt" --ignored "$work_dir/ignored.txt" \
    >"$work_dir/missing-output.txt" 2>&1; then
    echo "a missing production witness must fail" >&2
    exit 1
fi
grep -Fq 'production runtime-map test witnesses were not discovered:' \
    "$work_dir/missing-output.txt"
