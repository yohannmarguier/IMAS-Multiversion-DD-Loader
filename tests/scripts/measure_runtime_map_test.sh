#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
measure="$repo_root/scripts/measure-runtime-map.sh"
graph="$repo_root/scripts/dd-graph.sh"
selection="$repo_root/config/dd-graph-release.env"
temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT

fail() {
    printf 'runtime-map measurement test: %s\n' "$*" >&2
    exit 1
}

bash -n "$measure"
mock_bin="$temp/bin"
mkdir -p "$mock_bin" "$temp/docker/containers"
ln -s "$repo_root/tests/fixtures/dd_graph_setup_mocks/docker" "$mock_bin/docker"
ln -s "$repo_root/tests/fixtures/runtime_map_measurement_mocks/cargo" "$mock_bin/cargo"
export PATH="$mock_bin:$PATH"
export TEST_DOCKER_STATE="$temp/docker"
export TEST_DOCKER_LOG="$temp/docker.log"
export TEST_ORCHESTRATOR_LOG="$temp/orchestrator.log"
export IMAS_MVDD_GRAPH_HOME="$temp/custom-home"
export IMAS_MVDD_GRAPH_PASSWORD=graph-secret-not-for-report
export NEO4J_URI=bolt://127.0.0.1:17687
export NEO4J_USERNAME=neo4j
export NEO4J_PASSWORD=bolt-secret-not-for-report
export IMAS_MVDD_GRAPH_RESTART_SETTLE_SECONDS=0

custom_selection="$temp/updated-selection.env"
manifest=sha256:1111111111111111111111111111111111111111111111111111111111111111
archive=sha256:2222222222222222222222222222222222222222222222222222222222222222
neo4j_digest=sha256:3333333333333333333333333333333333333333333333333333333333333333
sed \
    -e 's/^GRAPH_RELEASE=.*/GRAPH_RELEASE=v9.9.9/' \
    -e "s/^GRAPH_MANIFEST_DIGEST=.*/GRAPH_MANIFEST_DIGEST=$manifest/" \
    -e "s/^GRAPH_ARCHIVE_DIGEST=.*/GRAPH_ARCHIVE_DIGEST=$archive/" \
    -e 's/^GRAPH_COMMIT=.*/GRAPH_COMMIT=custom-exporter-revision/' \
    -e 's/^GRAPH_NEO4J_VERSION=.*/GRAPH_NEO4J_VERSION=9.9.9-community/' \
    -e "s/^GRAPH_NEO4J_DIGEST=.*/GRAPH_NEO4J_DIGEST=$neo4j_digest/" \
    "$selection" > "$custom_selection"
"$graph" select --selection "$custom_selection"
service=$("$graph" inspect | sed -n 's/^service: //p')
owner=$("$graph" inspect | sed -n 's/^owner: //p')
service_file="$TEST_DOCKER_STATE/containers/$service"
{
    printf 'running=true\n'
    printf 'label:imas.mvdd.dd-graph=true\n'
    printf 'label:imas.mvdd.dd-graph.owner=%s\n' "$owner"
    printf 'label:imas.mvdd.dd-graph.home=%s\n' "$IMAS_MVDD_GRAPH_HOME"
    printf 'label:imas.mvdd.dd-graph.release=v9.9.9\n'
    printf 'label:imas.mvdd.dd-graph.manifest=%s\n' "$manifest"
    printf 'label:imas.mvdd.dd-graph.archive=%s\n' "$archive"
    printf 'label:imas.mvdd.dd-graph.commit=custom-exporter-revision\n'
    printf 'label:imas.mvdd.dd-graph.neo4j-version=9.9.9-community\n'
    printf 'label:imas.mvdd.dd-graph.neo4j-digest=%s\n' "$neo4j_digest"
    printf 'publish=127.0.0.1:17687:7687\n'
} > "$service_file"

report="$temp/report.md"
evidence="$temp/evidence.json"
"$measure" --output "$report" --evidence "$evidence" --runs 1

jq -e '
    .schema == 2 and
    .identities.graph_release == "v9.9.9" and
    .identities.graph_manifest == "sha256:1111111111111111111111111111111111111111111111111111111111111111" and
    .identities.graph_archive == "sha256:2222222222222222222222222222222222222222222222222222222222222222" and
    .identities.graph_exporter_revision == "custom-exporter-revision" and
    .identities.neo4j_version == "9.9.9-community" and
    .identities.neo4j_digest == "sha256:3333333333333333333333333333333333333333333333333333333333333333" and
    .identities.service == $service and
    (.identities.code_dirty | type) == "boolean" and
    .conditions.service_restarted.readiness_probe == "sleep-only; no Cypher content query" and
    .conditions.service_restarted.os_vm_caches_flushed == false and
    (.samples | length) == 12 and
    ([.samples[] | select(.condition == "service-restarted")] | length) == 6 and
    ([.samples[] | select(.condition == "service-restarted") | .direction] | sort) == ["forward", "forward", "forward", "reverse", "reverse", "reverse"]
' --arg service "$service" "$evidence" >/dev/null \
    || fail 'evidence did not report actual service identities and directional conditions'
if grep -F 'secret-not-for-report' "$evidence" "$report" >/dev/null; then
    fail 'measurement outputs exposed a credential'
fi

expected="$temp/expected-order"
{
    printf 'docker exec %s\n' "$service"
    for pair in equilibrium-3.39.0-4.1.1 equilibrium-3.42.0-4.1.1 pulse-schedule-3.25.0-3.30.0; do
        printf 'acquire %s forward\n' "$pair"
        printf 'acquire %s reverse\n' "$pair"
    done
    for pair in equilibrium-3.39.0-4.1.1 equilibrium-3.42.0-4.1.1 pulse-schedule-3.25.0-3.30.0; do
        for direction in forward reverse; do
            printf 'docker stop %s\n' "$service"
            printf 'docker start %s\n' "$service"
            printf 'acquire %s %s\n' "$pair" "$direction"
        done
    done
} > "$expected"
cmp -s "$expected" "$TEST_ORCHESTRATOR_LOG" \
    || fail 'each service-restarted direction was not the first workload after its own restart'

# An explicitly named service is accepted only when all provenance labels
# match the selected home. The verified override identity is then reported.
override=imas-mvdd-explicit-measurement
cp "$service_file" "$TEST_DOCKER_STATE/containers/$override"
: > "$TEST_ORCHESTRATOR_LOG"
IMAS_MVDD_MEASUREMENT_CONTAINER="$override" "$measure" \
    --output "$temp/override.md" --evidence "$temp/override.json" --runs 1
jq -e '.identities.service == $service' --arg service "$override" \
    "$temp/override.json" >/dev/null || fail 'explicit service identity was not reported'

sed -i.bak 's/^label:imas.mvdd.dd-graph.commit=.*/label:imas.mvdd.dd-graph.commit=wrong/' \
    "$TEST_DOCKER_STATE/containers/$override"
rm -f "$TEST_DOCKER_STATE/containers/$override.bak"
if IMAS_MVDD_MEASUREMENT_CONTAINER="$override" "$measure" \
    --output "$temp/wrong.md" --evidence "$temp/wrong.json" --runs 1 \
    >"$temp/wrong.out" 2>&1; then
    fail 'measurement accepted an explicit container with mismatched provenance'
fi
grep -F 'does not match the recorded graph selection' "$temp/wrong.out" >/dev/null \
    || fail 'explicit-container provenance mismatch is not actionable'
