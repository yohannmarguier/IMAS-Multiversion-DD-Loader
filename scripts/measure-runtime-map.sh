#!/usr/bin/env bash
# Record live complete-map acquisition observations without changing runtime
# policy. The Rust test supplies the production Bolt source and five-second
# default; this wrapper only establishes warm and restarted-service conditions.
set -euo pipefail

readonly script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
readonly repo_root=$(cd "$script_dir/.." && pwd)
readonly dd_graph="$script_dir/dd-graph.sh"
readonly smoke_query="$repo_root/config/dd-graph-smoke.cypher"

usage() {
    cat <<'EOF'
Usage: scripts/measure-runtime-map.sh --output REPORT.md --evidence SAMPLES.json [--runs N]

Records the three live reference pairs in both directions through the current
Rust Bolt source. `--runs` is the number of observations per condition and
pair (default: 5). The report distinguishes a warmed running service from a
service restarted immediately before each directional observation; neither condition
flushes OS or VM caches, and service startup is intentionally not timed.

The selected task-owned graph must already exist and be running. Set
IMAS_MVDD_GRAPH_HOME, IMAS_MVDD_GRAPH_PASSWORD, NEO4J_URI, NEO4J_USERNAME and
NEO4J_PASSWORD as described in README.md. The command writes the two output
paths and temporary files beside the evidence output, as well as normal Cargo
build artifacts. It stops and starts the selected graph service for the
restarted-service observations.

When the digest-derived default container name is occupied by another task,
set IMAS_MVDD_MEASUREMENT_CONTAINER to a separately provisioned container
whose name begins `imas-mvdd-`, is labelled `imas.mvdd.dd-graph=true`, and
has `imas.mvdd.dd-graph.home=$IMAS_MVDD_GRAPH_HOME`. The wrapper verifies
those labels before controlling that explicit container for restart observations.
EOF
}

die() {
    printf 'measure-runtime-map: %s\n' "$*" >&2
    exit 1
}

need_value() {
    test $# -ge 2 || die "$1 needs a value"
}

runs=5
report=
evidence=
restart_settle_seconds=${IMAS_MVDD_GRAPH_RESTART_SETTLE_SECONDS:-30}
while test $# -gt 0; do
    case "$1" in
        --output)
            need_value "$@"
            report=$2
            shift 2
            ;;
        --evidence)
            need_value "$@"
            evidence=$2
            shift 2
            ;;
        --runs)
            need_value "$@"
            runs=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *) die "unknown argument: $1" ;;
    esac
done

test -n "$report" || die '--output is required'
test -n "$evidence" || die '--evidence is required'
case "$runs" in
    ''|*[!0-9]*) die '--runs must be a positive integer' ;;
esac
test "$runs" -gt 0 || die '--runs must be a positive integer'
case "$restart_settle_seconds" in
    ''|*[!0-9]*) die 'IMAS_MVDD_GRAPH_RESTART_SETTLE_SECONDS must be a nonnegative integer' ;;
esac
for value in IMAS_MVDD_GRAPH_HOME IMAS_MVDD_GRAPH_PASSWORD NEO4J_URI NEO4J_USERNAME NEO4J_PASSWORD; do
    test -n "${!value:-}" || die "set $value before measuring"
done
command -v cargo >/dev/null || die 'cargo is required'
command -v jq >/dev/null || die 'jq is required'
test -x "$dd_graph" || die "missing graph lifecycle script: $dd_graph"
test -f "$smoke_query" || die "missing required DD graph smoke query: $smoke_query"

recorded_identity=$("$dd_graph" inspect)
identity_value() {
    local name=$1
    printf '%s\n' "$recorded_identity" | sed -n "s/^${name}: //p"
}

expected_service=$(identity_value service)
expected_owner=$(identity_value owner)
graph_release=$(identity_value release)
graph_manifest=$(identity_value manifest)
graph_archive=$(identity_value archive)
graph_commit=$(identity_value commit)
neo4j_version=$(identity_value neo4j-version)
neo4j_digest=$(identity_value neo4j-digest)
for value in expected_service expected_owner graph_release graph_manifest graph_archive graph_commit neo4j_version neo4j_digest; do
    test -n "${!value:-}" || die "recorded graph identity omits $value"
done

measurement_container=${IMAS_MVDD_MEASUREMENT_CONTAINER:-$expected_service}
if test -n "${IMAS_MVDD_MEASUREMENT_CONTAINER:-}"; then
    case "$measurement_container" in
        imas-mvdd-*) ;;
        *) die 'IMAS_MVDD_MEASUREMENT_CONTAINER must name an imas-mvdd-* container' ;;
    esac
fi
command -v docker >/dev/null || die 'docker is required for graph measurement'

container_label() {
    docker inspect --format "{{ index .Config.Labels \"$1\" }}" "$measurement_container"
}

verify_measurement_service() {
    local label expected actual
    for label_and_expected in \
        'imas.mvdd.dd-graph=true' \
        "imas.mvdd.dd-graph.owner=$expected_owner" \
        "imas.mvdd.dd-graph.home=$IMAS_MVDD_GRAPH_HOME" \
        "imas.mvdd.dd-graph.release=$graph_release" \
        "imas.mvdd.dd-graph.manifest=$graph_manifest" \
        "imas.mvdd.dd-graph.archive=$graph_archive" \
        "imas.mvdd.dd-graph.commit=$graph_commit" \
        "imas.mvdd.dd-graph.neo4j-version=$neo4j_version" \
        "imas.mvdd.dd-graph.neo4j-digest=$neo4j_digest"; do
        label=${label_and_expected%%=*}
        expected=${label_and_expected#*=}
        actual=$(container_label "$label")
        test "$actual" = "$expected" \
            || die "measurement container $measurement_container does not match the recorded graph selection: $label=${actual:-<missing>}, expected $expected"
    done
}
verify_measurement_service

# Capture source provenance before creating measurement outputs or temporary
# sample files, so the harness does not make its own checkout look dirty.
code_commit=$(git -C "$repo_root" rev-parse HEAD)
code_diff=$(git -C "$repo_root" diff HEAD --binary --no-ext-diff)
code_status=$(git -C "$repo_root" status --porcelain=v1)
if test -n "$code_status"; then code_dirty=true; else code_dirty=false; fi
rust_version=$(rustc --version)
machine=$(uname -sm)

graph_start() {
    docker start "$measurement_container" >/dev/null
}

graph_stop() {
    docker stop "$measurement_container" >/dev/null
}

graph_query() {
    local result status
    result=$(docker exec "$measurement_container" cypher-shell --format plain --non-interactive \
        -u "$NEO4J_USERNAME" -p "$NEO4J_PASSWORD" \
        "$(<"$smoke_query")")
    status=$(printf '%s\n' "$result" | tail -n 1 | tr -d '"\r')
    test "$status" = imas_mvdd_smoke_ok \
        || die "measurement service failed required DD graph content smoke (status: ${status:-empty result})"
}

mkdir -p "$(dirname "$report")" "$(dirname "$evidence")"
sample_dir=$(mktemp -d "$(dirname "$evidence")/.runtime-map-samples.XXXXXX")
trap 'rm -rf "$sample_dir"' EXIT

readonly -a pairs=(
    equilibrium-3.39.0-4.1.1
    equilibrium-3.42.0-4.1.1
    pulse-schedule-3.25.0-3.30.0
)

measure_one() {
    local condition=$1
    local pair=$2
    local direction=$3
    local run=$4
    local sample="$sample_dir/${condition}-${pair}-${direction}-${run}.json"
    IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_OUTPUT="$sample" \
        IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_PAIR="$pair" \
        IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_DIRECTION="$direction" \
        cargo test --release measure_pinned_runtime_map_acquisition --lib -- \
        --ignored --nocapture
    jq -e --arg direction "$direction" \
        '.schema == 1 and (.records | length == 1) and .records[0].direction == $direction' \
        "$sample" >/dev/null \
        || die "measurement test did not emit the selected $direction record for $pair"
    jq --arg condition "$condition" --arg direction "$direction" --argjson run "$run" \
        '.records[] + {condition: $condition, direction: $direction, run: $run}' \
        "$sample" >>"$sample_dir/records.jsonl"
}

cd "$repo_root"
graph_query >/dev/null
for run in $(seq 1 "$runs"); do
    for pair in "${pairs[@]}"; do
        for direction in forward reverse; do
            measure_one warm "$pair" "$direction" "$run"
        done
    done
done

# A service restart invalidates Neo4j's in-process caches. It deliberately
# does not claim a cold disk: this command cannot flush host OS or VM caches.
for run in $(seq 1 "$runs"); do
    for pair in "${pairs[@]}"; do
        for direction in forward reverse; do
            graph_stop
            graph_start
            # Sleeping waits for service initialization without a Cypher request,
            # which would warm the graph and invalidate this condition's label.
            sleep "$restart_settle_seconds"
            measure_one service-restarted "$pair" "$direction" "$run"
        done
    done
done

jq -s \
    --arg release "$graph_release" \
    --arg manifest "$graph_manifest" \
    --arg archive "$graph_archive" \
    --arg graph_commit "$graph_commit" \
    --arg neo4j_version "$neo4j_version" \
    --arg neo4j_digest "$neo4j_digest" \
    --arg service "$measurement_container" \
    --arg graph_home "$IMAS_MVDD_GRAPH_HOME" \
    --arg neo4j_uri "$NEO4J_URI" \
    --arg code_commit "$code_commit" \
    --arg code_diff "$code_diff" \
    --arg code_status "$code_status" \
    --argjson code_dirty "$code_dirty" \
    --arg rust_version "$rust_version" \
    --arg machine "$machine" \
    --argjson restart_settle_seconds "$restart_settle_seconds" \
    --arg command "scripts/measure-runtime-map.sh --output $report --evidence $evidence --runs $runs" \
    '{
       schema: 2,
       conditions: {
         warm: {
           service_state: "running",
           readiness_probe: "meaningful DD content query before the warm batch",
           os_vm_caches_flushed: false
         },
         service_restarted: {
           service_state: "stopped and started immediately before every directional acquisition",
           readiness_probe: "sleep-only; no Cypher content query",
           startup_settle_seconds: $restart_settle_seconds,
           startup_timed: false,
           os_vm_caches_flushed: false
         }
       },
       identities: {
         graph_release: $release,
         graph_manifest: $manifest,
         graph_archive: $archive,
         graph_exporter_revision: $graph_commit,
         neo4j_version: $neo4j_version,
         neo4j_digest: $neo4j_digest,
         service: $service,
         code_commit: $code_commit,
         code_dirty: $code_dirty,
         rust: $rust_version,
         machine: $machine
       },
       reproducibility: {
         graph_home: $graph_home,
         neo4j_uri: $neo4j_uri,
         code_status: $code_status,
         code_diff_from_head: $code_diff
       },
       command: $command,
       samples: .
     }' "$sample_dir/records.jsonl" >"$evidence"

{
    printf '# Runtime conversion-map measurement\n\n'
    printf 'Generated by `%s`. It measures the selected production Bolt transport and complete Rust map path; it does not include service startup, build time, graph download, OS/VM cache flushing, peak memory, or process RSS.\n\n' \
        "scripts/measure-runtime-map.sh --output $(basename "$report") --evidence $(basename "$evidence") --runs $runs"
    printf '## Identities and conditions\n\n'
    printf '| Item | Value |\n| --- | --- |\n'
    jq -r '.identities | to_entries[] | "| \(.key) | `\(.value)` |"' "$evidence"
    printf '| warm | running service preflighted by the required DD-content query; startup excluded; OS/VM caches not flushed |\n'
    printf '| service-restarted | stopped and restarted before every directional observation; sleep-only readiness (no Cypher probe); startup excluded; OS/VM caches not flushed |\n\n'
    printf 'The unchanged default is **5 seconds** per complete attempt. An observation is successful only when the Rust coordinator returns a complete validated map before that deadline; the harness preserves failures rather than retrying, extending an individual attempt, or converting a partial map into success.\n\n'
    printf '## Median observations\n\n'
    printf 'Each row aggregates `%s` independent samples of one map key. Retained bytes are a capacity-based estimate of map-owned data only; they exclude allocator overhead, the `Arc`/coordinator cache, transient construction allocations, process RSS and peak memory. Resolver and cache figures are average nanoseconds per call over 20,000 in-process repetitions, not a latency SLO.\n\n' "$runs"
    printf '| Condition | Pair | Direction | Stored → HLI | Outcome | Total ms | Retained estimate KiB | Resolver ns/call | Cache-hit ns/call | Failure |\n| --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | --- |\n'
    jq -r '
      def median($key): sort_by(.[$key]) | .[length / 2 | floor][$key];
      .samples
      | group_by([.condition, .pair, .direction, .stored_dd, .hli_dd])[]
      | . as $group
      | ($group | median("total_ns")) as $total
      | ([ $group[].outcome ] | unique) as $outcomes
      | (if $outcomes == ["success"] then "success" else ($outcomes | join(", ")) end) as $outcome
      | (if $outcome == "success" then ($group | median("retained_map_estimate_bytes")) else null end) as $retained
      | (if $outcome == "success" then ($group | median("resolver_lookup_ns_per_call")) else null end) as $lookup
      | (if $outcome == "success" then ($group | median("cache_hit_ns_per_call")) else null end) as $cache
      | ([ $group[].failure | select(. != null) ] | unique | join("; ")) as $failure
      | "| \($group[0].condition) | \($group[0].pair) | \($group[0].direction) | \($group[0].stored_dd) → \($group[0].hli_dd) | \($outcome) | \(($total / 1000000 * 100 | round) / 100) | \(if $retained == null then "n/a" else (($retained / 1024 * 100 | round) / 100 | tostring) end) | \(if $lookup == null then "n/a" else ($lookup | tostring) end) | \(if $cache == null then "n/a" else ($cache | tostring) end) | \($failure) |"' \
        "$evidence"
    printf '\n## Stage samples\n\n'
    printf 'Per-sample stage timestamps (connection setup, complete query retrieval, decoding, scope validation, rule construction, map validation and publication) are retained in the companion JSON evidence. `Connection` records construction of the configured Bolt driver; the driver may establish its TCP session lazily with the first query, so transport handshake time is included in query retrieval rather than overstated as an eagerly verified connection time. Timed-out rows retain the terminal failure and their stage timestamps but have no retained-map or lookup figures, because failed maps are never published or cached.\n'
} >"$report"

printf 'Wrote %s and %s\n' "$report" "$evidence"
