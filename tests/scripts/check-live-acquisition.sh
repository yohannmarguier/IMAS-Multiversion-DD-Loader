#!/usr/bin/env bash
# Functional verification uses an explicit 120-second attempt in the test;
# the library's default remains five seconds. Reject stale zero-test filters.
set -euo pipefail
log=$(mktemp)
trap 'rm -f "$log"' EXIT
cargo test --release pinned_graph_returns_complete_reference_scopes --lib -- --ignored --nocapture 2>&1 | tee "$log"
grep -q 'test result: ok. 1 passed;' "$log"
