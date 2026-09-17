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
failing_selection="$temp/failing-selection.env"
sed 's|^GRAPH_REFERENCE=.*|GRAPH_REFERENCE=ghcr.io/iterorganization/not-a-graph|' \
    "$runtime_selection" > "$failing_selection"
if TEST_ORAS_FAIL=yes IMAS_MVDD_GRAPH_HOME="$clean_state" IMAS_MVDD_GRAPH_PASSWORD=test-password \
    "$script" update --selection "$failing_selection" >"$temp/update.out" 2>&1; then
    fail 'failed update was accepted'
fi
cmp -s "$runtime_selection" "$clean_state/selection.env" \
    || fail 'failed update replaced the active selection record'
IMAS_MVDD_GRAPH_HOME="$clean_state" "$script" start
query=$(IMAS_MVDD_GRAPH_HOME="$clean_state" IMAS_MVDD_GRAPH_PASSWORD=test-password "$script" query)
printf '%s\n' "$query" | grep -F 'node_count' >/dev/null || fail 'query did not reach the started service'
test "$(wc -l < "$temp/oras.log" | tr -d ' ')" = 1 || fail 'ordinary restart attempted a new acquisition'
