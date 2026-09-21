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
test "$(find "$mock_state/containers" -type f | wc -l | tr -d ' ')" = 1 \
    || fail 'setup did not create exactly one task-owned service'
test "$(wc -l < "$temp/oras.log" | tr -d ' ')" = 1 || fail 'clean setup did not acquire exactly once'

IMAS_MVDD_GRAPH_HOME="$clean_state" "$script" stop

clean_service=$(IMAS_MVDD_GRAPH_HOME="$clean_state" "$script" inspect \
    | sed -n 's/^service: //p')
clean_service_file="$mock_state/containers/$clean_service"

# Even a resource with the expected home-derived name is not adopted when its
# ownership or immutable-selection labels disagree.
cp "$clean_service_file" "$temp/clean-service.labels"
sed -i.bak 's/^label:imas.mvdd.dd-graph.owner=.*/label:imas.mvdd.dd-graph.owner=another-task/' \
    "$clean_service_file"
rm -f "$clean_service_file.bak"
before_start_count=$(grep -c '^start ' "$temp/docker.log" || true)
if IMAS_MVDD_GRAPH_HOME="$clean_state" "$script" start >"$temp/owner-mismatch.out" 2>&1; then
    fail 'start adopted a service with a mismatched owner label'
fi
grep -F 'refusing to adopt or mutate it' "$temp/owner-mismatch.out" >/dev/null \
    || fail 'owner mismatch failure is not actionable'
test "$(grep -c '^start ' "$temp/docker.log" || true)" = "$before_start_count" \
    || fail 'owner mismatch issued a mutating Docker command'
mv "$temp/clean-service.labels" "$clean_service_file"

# A selected-service identity mismatch is rejected before a content query.
cp "$clean_service_file" "$temp/clean-service.labels"
sed -i.bak 's/^label:imas.mvdd.dd-graph.commit=.*/label:imas.mvdd.dd-graph.commit=wrong/' \
    "$clean_service_file"
rm -f "$clean_service_file.bak"
before_exec_count=$(grep -c '^exec ' "$temp/docker.log" || true)
if IMAS_MVDD_GRAPH_HOME="$clean_state" IMAS_MVDD_GRAPH_PASSWORD=test-password \
    "$script" query >"$temp/identity-mismatch.out" 2>&1; then
    fail 'query accepted a service whose graph identity mismatched its selection'
fi
grep -F 'imas.mvdd.dd-graph.commit=wrong' "$temp/identity-mismatch.out" >/dev/null \
    || fail 'selected-service identity mismatch is not explicit'
test "$(grep -c '^exec ' "$temp/docker.log" || true)" = "$before_exec_count" \
    || fail 'identity mismatch queried unverified graph contents'
mv "$temp/clean-service.labels" "$clean_service_file"

# Two homes selecting one immutable archive own distinct mutable services.
# Home B must not discover or mutate home A's service merely because the
# manifest digest is the same.
other_state="$temp/other-state"
IMAS_MVDD_GRAPH_HOME="$other_state" "$script" select --selection "$runtime_selection"
before_stop_count=$(grep -c '^stop ' "$temp/docker.log" || true)
if IMAS_MVDD_GRAPH_HOME="$other_state" "$script" stop >"$temp/cross-home-stop.out" 2>&1; then
    fail 'an unprovisioned home stopped another home service'
fi
grep -F 'no task-owned service' "$temp/cross-home-stop.out" >/dev/null \
    || fail 'cross-home stop failure is not actionable'
test "$(grep -c '^stop ' "$temp/docker.log" || true)" = "$before_stop_count" \
    || fail 'cross-home stop issued a mutating Docker command'
if IMAS_MVDD_GRAPH_HOME="$other_state" "$script" stop --container "$clean_service" \
    >"$temp/cross-home-explicit-stop.out" 2>&1; then
    fail 'an explicit container name bypassed cross-home ownership'
fi
grep -F 'expected' "$temp/cross-home-explicit-stop.out" >/dev/null \
    || fail 'explicit cross-home ownership mismatch is not actionable'
test "$(grep -c '^stop ' "$temp/docker.log" || true)" = "$before_stop_count" \
    || fail 'explicit cross-home stop issued a mutating Docker command'

# Authentication-only and empty/wrong-schema query responses cannot satisfy
# the content smoke contract.
for smoke_case in empty wrong_schema query_error; do
    case "$smoke_case" in
        empty) query_value=imas_mvdd_smoke_empty; query_error= ;;
        wrong_schema) query_value=imas_mvdd_smoke_wrong_schema; query_error= ;;
        query_error) query_value=imas_mvdd_smoke_ok; query_error=yes ;;
    esac
    if TEST_DOCKER_QUERY_VALUE="$query_value" TEST_DOCKER_QUERY_ERROR="$query_error" \
        IMAS_MVDD_GRAPH_HOME="$clean_state" IMAS_MVDD_GRAPH_PASSWORD=test-password \
        "$script" query >"$temp/smoke-$smoke_case.out" 2>&1; then
        fail "$smoke_case graph passed the meaningful smoke gate"
    fi
done
IMAS_MVDD_GRAPH_HOME="$clean_state" "$script" start
IMAS_MVDD_GRAPH_HOME="$clean_state" IMAS_MVDD_GRAPH_PASSWORD=test-password "$script" query \
    | grep -F 'imas_mvdd_smoke_ok' >/dev/null || fail 'valid graph content failed smoke'
IMAS_MVDD_GRAPH_HOME="$clean_state" "$script" stop

# A host-port conflict is reported before loading a database or creating a
# service; it never causes an existing container to be stopped or adopted.
port_conflict_state="$temp/port-conflict-state"
port_conflict_archive="$port_conflict_state/archives/dc90975cb9fa/imas-codex-graph-dd-v5.3.0.tar.gz"
mkdir -p "$(dirname "$port_conflict_archive")"
cp "$clean_state/archives/dc90975cb9fa/imas-codex-graph-dd-v5.3.0.tar.gz" "$port_conflict_archive"
IMAS_MVDD_GRAPH_HOME="$port_conflict_state" "$script" select --selection "$runtime_selection"
printf 'running=true\npublish=127.0.0.1:17687:7687\n' > "$mock_state/containers/unrelated-service"
if TEST_ORAS_FAIL=yes IMAS_MVDD_GRAPH_HOME="$port_conflict_state" \
    IMAS_MVDD_GRAPH_PASSWORD=test-password "$script" setup >"$temp/port-conflict.out" 2>&1; then
    fail 'setup accepted a host Bolt port conflict'
fi
grep -F 'host Bolt port 17687 is already published' "$temp/port-conflict.out" >/dev/null \
    || fail 'port conflict failure is not actionable'
test ! -d "$port_conflict_state/databases/dc90975cb9fa" \
    || fail 'port-conflicted setup loaded mutable database storage'
rm "$mock_state/containers/unrelated-service"

# Simulate a fresh CI runner restoring only the immutable archive cache. Its
# selection and database state are new, so setup must neither download again
# nor reuse the earlier runner's database/container identity.
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
    | grep -F 'imas_mvdd_smoke_ok' >/dev/null || fail 'cache-hit setup did not start a queryable service'
IMAS_MVDD_GRAPH_HOME="$cache_hit_state" "$script" stop

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
test "$(find "$mock_state/containers" -type f | wc -l | tr -d ' ')" = 2 \
    || fail 'failed service start left a service behind'

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
printf '%s\n' "$query" | grep -F 'imas_mvdd_smoke_ok' >/dev/null || fail 'query did not reach the started service'
test "$(wc -l < "$temp/oras.log" | tr -d ' ')" = 2 || fail 'ordinary restart attempted a new acquisition'
