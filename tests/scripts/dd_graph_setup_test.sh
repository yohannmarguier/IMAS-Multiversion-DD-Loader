#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
script="$repo_root/scripts/dd-graph.sh"
selection="$repo_root/config/dd-graph-release.env"
temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT

fail() {
    printf 'dd-graph setup test: %s\n' "$*" >&2
    exit 1
}

test -x "$script" || fail 'setup script is executable'
bash -n "$script"

state="$temp/state"
IMAS_MVDD_GRAPH_HOME="$state" "$script" select --selection "$selection"
test -f "$state/selection.env" || fail 'select records the selection outside the repository'
cmp -s "$selection" "$state/selection.env" || fail 'recorded selection differs from requested selection'

inspect=$(IMAS_MVDD_GRAPH_HOME="$state" "$script" inspect)
printf '%s\n' "$inspect" | grep -F 'release: v5.3.0' >/dev/null || fail 'inspect reports the recorded release'
printf '%s\n' "$inspect" | grep -F 'manifest: sha256:dc90975cb9fa0c7b08e9e4809640d01e41d927b4162200c13eec5076e030329b' >/dev/null || fail 'inspect reports the recorded manifest'
printf '%s\n' "$inspect" | grep -F 'neo4j: neo4j:2026.01.4-community@sha256:657e0b601f09da7ef1bd51eebfe3758c3123eb362249767d8476500c95ea810e' >/dev/null || fail 'inspect reports the pinned Neo4j image'

bad_selection="$temp/bad.env"
cp "$selection" "$bad_selection"
printf '%s\n' 'GRAPH_ARCHIVE=../escape.tar.gz' >> "$bad_selection"
if IMAS_MVDD_GRAPH_HOME="$temp/bad-state" "$script" select --selection "$bad_selection" >"$temp/bad.out" 2>&1; then
    fail 'unsafe archive name was accepted'
fi
grep -F 'safe filename' "$temp/bad.out" >/dev/null || fail 'unsafe archive failure is explicit'

if IMAS_MVDD_GRAPH_HOME="$temp/unselected-state" "$script" setup >"$temp/unselected.out" 2>&1; then
    fail 'setup accepted an implicit default selection'
fi
grep -F 'run select first' "$temp/unselected.out" >/dev/null || fail 'missing selection failure is explicit'

archive_content="$repo_root/tests/fixtures/dd_graph_setup_archive"
archive="$temp/imas-codex-graph-dd-v5.3.0.tar.gz"
tar -C "$(dirname "$archive_content")" -czf "$archive" "$(basename "$archive_content")"
archive_digest=$(shasum -a 256 "$archive" | awk '{print $1}')
runtime_selection="$temp/runtime-selection.env"
sed "s|^GRAPH_ARCHIVE_DIGEST=.*|GRAPH_ARCHIVE_DIGEST=sha256:$archive_digest|" \
    "$selection" > "$runtime_selection"

mock_bin="$repo_root/tests/fixtures/dd_graph_setup_mocks"
mock_state="$temp/mock-state"
mkdir "$mock_state"
export TEST_GRAPH_ARCHIVE="$archive"
export TEST_ORAS_LOG="$temp/oras.log"
export TEST_DOCKER_LOG="$temp/docker.log"
export TEST_DOCKER_STATE="$mock_state"
export PATH="$mock_bin:$PATH"

clean_state="$temp/clean-state"
IMAS_MVDD_GRAPH_HOME="$clean_state" "$script" select --selection "$runtime_selection"
IMAS_MVDD_GRAPH_HOME="$clean_state" IMAS_MVDD_GRAPH_PASSWORD=test-password "$script" setup
test -f "$clean_state/archives/dc90975cb9fa/imas-codex-graph-dd-v5.3.0.tar.gz" \
    || fail 'setup did not retain its verified archive outside the repository'
test -f "$mock_state/container" || fail 'setup did not create its task-owned service'
test "$(wc -l < "$temp/oras.log" | tr -d ' ')" = 1 || fail 'clean setup did not acquire exactly once'

IMAS_MVDD_GRAPH_HOME="$clean_state" "$script" stop

# Simulate a fresh CI runner restoring only the immutable archive cache. Its
# selection and database state are new, so setup must neither download again
# nor reuse the earlier runner's database/container identity.
rm "$mock_state/container"
cache_hit_state="$temp/cache-hit-state"
cache_hit_archive="$cache_hit_state/archives/dc90975cb9fa/imas-codex-graph-dd-v5.3.0.tar.gz"
mkdir -p "$(dirname "$cache_hit_archive")"
cp "$clean_state/archives/dc90975cb9fa/imas-codex-graph-dd-v5.3.0.tar.gz" "$cache_hit_archive"
IMAS_MVDD_GRAPH_HOME="$cache_hit_state" "$script" select --selection "$runtime_selection"
TEST_ORAS_FAIL=yes IMAS_MVDD_GRAPH_HOME="$cache_hit_state" IMAS_MVDD_GRAPH_PASSWORD=test-password \
    "$script" setup
test -d "$cache_hit_state/databases/dc90975cb9fa" || fail 'cache-hit setup did not load a fresh database'
test "$(wc -l < "$temp/oras.log" | tr -d ' ')" = 1 || fail 'cache-hit setup downloaded instead of restoring its archive'
IMAS_MVDD_GRAPH_HOME="$cache_hit_state" IMAS_MVDD_GRAPH_PASSWORD=test-password "$script" query \
    | grep -F 'node_count' >/dev/null || fail 'cache-hit setup did not start a queryable service'
IMAS_MVDD_GRAPH_HOME="$cache_hit_state" "$script" stop
rm "$mock_state/container"

# A restored archive is still verified before use; a corrupt cache makes setup
# fail instead of bypassing graph-required work.
corrupt_state="$temp/corrupt-state"
corrupt_archive="$corrupt_state/archives/dc90975cb9fa/imas-codex-graph-dd-v5.3.0.tar.gz"
mkdir -p "$(dirname "$corrupt_archive")"
printf 'corrupt archive cache entry\n' > "$corrupt_archive"
IMAS_MVDD_GRAPH_HOME="$corrupt_state" "$script" select --selection "$runtime_selection"
if IMAS_MVDD_GRAPH_HOME="$corrupt_state" IMAS_MVDD_GRAPH_PASSWORD=test-password \
    "$script" setup >"$temp/corrupt.out" 2>&1; then
    fail 'corrupt archive cache entry was accepted'
fi
grep -F 'archive digest mismatch' "$temp/corrupt.out" >/dev/null \
    || fail 'corrupt archive cache failure is explicit'

# A service-start failure is an error from setup, so graph-required CI cannot
# silently continue after archive acquisition and database loading succeeded.
start_failure_state="$temp/start-failure-state"
start_failure_archive="$start_failure_state/archives/dc90975cb9fa/imas-codex-graph-dd-v5.3.0.tar.gz"
mkdir -p "$(dirname "$start_failure_archive")"
cp "$clean_state/archives/dc90975cb9fa/imas-codex-graph-dd-v5.3.0.tar.gz" "$start_failure_archive"
IMAS_MVDD_GRAPH_HOME="$start_failure_state" "$script" select --selection "$runtime_selection"
if TEST_DOCKER_FAIL_START=yes IMAS_MVDD_GRAPH_HOME="$start_failure_state" \
    IMAS_MVDD_GRAPH_PASSWORD=test-password "$script" setup >"$temp/start-failure.out" 2>&1; then
    fail 'service-start failure was accepted'
fi
test ! -e "$mock_state/container" || fail 'failed service start left a running service'
# The original clean-state service remains available but stopped. The mock is
# process-local rather than state-directory-aware, so restore that fact before
# verifying an ordinary restart below.
touch "$mock_state/container"

failing_selection="$temp/failing-selection.env"
sed -e 's|^GRAPH_REFERENCE=.*|GRAPH_REFERENCE=ghcr.io/iterorganization/not-a-graph|' \
    -e 's|^GRAPH_MANIFEST_DIGEST=.*|GRAPH_MANIFEST_DIGEST=sha256:0000000000000000000000000000000000000000000000000000000000000000|' \
    "$runtime_selection" > "$failing_selection"
if TEST_ORAS_FAIL=yes IMAS_MVDD_GRAPH_HOME="$clean_state" IMAS_MVDD_GRAPH_PASSWORD=test-password \
    "$script" update --selection "$failing_selection" >"$temp/update.out" 2>&1; then
    fail 'failed update was accepted'
fi
grep -F 'ghcr.io/iterorganization/not-a-graph@sha256:0000000000000000000000000000000000000000000000000000000000000000' \
    "$temp/oras.log" >/dev/null || fail 'failed update did not attempt archive acquisition'
cmp -s "$runtime_selection" "$clean_state/selection.env" \
    || fail 'failed update replaced the active selection record'
IMAS_MVDD_GRAPH_HOME="$clean_state" "$script" start
query=$(IMAS_MVDD_GRAPH_HOME="$clean_state" IMAS_MVDD_GRAPH_PASSWORD=test-password "$script" query)
printf '%s\n' "$query" | grep -F 'node_count' >/dev/null || fail 'query did not reach the started service'
test "$(wc -l < "$temp/oras.log" | tr -d ' ')" = 2 || fail 'ordinary restart attempted a new acquisition'
