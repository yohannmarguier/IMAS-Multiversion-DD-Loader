#!/usr/bin/env bash
# Functional verification uses an explicit 120-second attempt in the test;
# the library's default remains five seconds. Reject stale zero-test filters.
set -euo pipefail
log=$(mktemp)
trap 'rm -f "$log"' EXIT
report=${IMAS_MVDD_LIVE_FIDELITY_REPORT:-target/runtime-map-fidelity/live.json}
scripts/dd-graph.sh verify-service >/dev/null
IMAS_MVDD_LIVE_FIDELITY_REPORT="$report" \
    cargo test --release pinned_graph_returns_complete_reference_scopes --lib -- --ignored --nocapture 2>&1 | tee "$log"
grep -q 'test result: ok. 1 passed;' "$log"
test -s "$report"
jq -e '
    .schema == "imas-mvdd-live-fidelity-v1" and
    .scope == "sampled-pinned-graph" and
    (.source_identity.manifest | startswith("sha256:")) and
    (.requests | length == 6) and
    (.checks | length > 0) and
    ([.checks[] | select(.expected != .observed)] | length == 0) and
    (.mismatches | length == 0)
' "$report" >/dev/null
