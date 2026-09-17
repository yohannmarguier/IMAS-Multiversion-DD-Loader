#!/usr/bin/env bash

set -euo pipefail

root_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
audit="$root_dir/coverage/rust-mutation-audit.json"
line_scope="$root_dir/coverage/rust-line-coverage-scope.json"
dispositions="$root_dir/coverage/rust-mutation-dispositions.json"

if [[ -n $(git -C "$root_dir" status --porcelain) ]]; then
    echo "mutation audit requires a clean worktree; commit, stash, or discard unrelated changes first" >&2
    exit 2
fi

expected_version=$(python3 - "$audit" <<'PY'
import json
import sys

print(json.load(open(sys.argv[1]))["tool"]["cargo_mutants_version"])
PY
)
minimum_rust_version=$(python3 - "$audit" <<'PY'
import json
import sys

print(json.load(open(sys.argv[1]))["tool"]["minimum_rust_version"])
PY
)
actual_version=$(cargo mutants --version)
if [[ "$actual_version" != "cargo-mutants $expected_version" ]]; then
    echo "cargo-mutants $expected_version is required; found $actual_version" >&2
    exit 2
fi
actual_rust_version=$(rustc --version | awk '{print $2}')
if ! python3 - "$minimum_rust_version" "$actual_rust_version" <<'PY'
import sys

def version(value):
    try:
        return tuple(int(part) for part in value.split(".")[:3])
    except ValueError:
        raise SystemExit(2)

raise SystemExit(0 if version(sys.argv[2]) >= version(sys.argv[1]) else 1)
PY
then
    echo "Rust $minimum_rust_version or newer is required; found $actual_rust_version" >&2
    exit 2
fi

mkdir -p "$root_dir/target"
audit_dir=$(mktemp -d "$root_dir/target/rust-mutation-audit.XXXXXX")
candidate_mutants="$audit_dir/candidates.json"
selected_mutants="$audit_dir/selected-mutants.json"
cargo_mutants_config="$audit_dir/cargo-mutants.toml"

cd -- "$root_dir"
started=$SECONDS
cargo mutants --no-config --all-features --list --json >"$candidate_mutants"
python3 scripts/check-rust-mutation-audit.py \
    --line-scope "$line_scope" \
    --candidate-mutants "$candidate_mutants" \
    --write-selection "$selected_mutants" \
    --write-cargo-mutants-config "$cargo_mutants_config"

if cargo mutants --config "$cargo_mutants_config" --all-features --output "$audit_dir" -- --lib; then
    cargo_mutants_status=0
else
    cargo_mutants_status=$?
fi

if python3 scripts/check-rust-mutation-audit.py \
    --line-scope "$line_scope" \
    --audit "$audit" \
    --selected "$selected_mutants" \
    --mutants "$audit_dir/mutants.out/mutants.json" \
    --outcomes "$audit_dir/mutants.out/outcomes.json" \
    --dispositions "$dispositions"; then
    checker_status=0
else
    checker_status=$?
fi

echo "mutation audit report: $audit_dir (elapsed ${SECONDS}s)"
if (( checker_status != 0 )); then
    exit "$checker_status"
fi
exit "$cargo_mutants_status"
