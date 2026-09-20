#!/usr/bin/env bash
# Record live complete-map acquisition observations without changing runtime
# policy. The Rust test supplies the production Bolt source and five-second
# default; this wrapper only establishes warm and restarted-service conditions.
set -euo pipefail

readonly script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
readonly repo_root=$(cd "$script_dir/.." && pwd)
readonly dd_graph="$script_dir/dd-graph.sh"

usage() {
    cat <<'EOF'
Usage: scripts/measure-runtime-map.sh --output REPORT.md --evidence SAMPLES.json [--runs N]

Records the three live reference pairs in both directions through the current
Rust Bolt source. `--runs` is the number of observations per condition and
pair (default: 5). The report distinguishes a warmed running service from a
service restarted immediately before each observation; neither condition
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

measurement_container=${IMAS_MVDD_MEASUREMENT_CONTAINER:-}
if test -n "$measurement_container"; then
    case "$measurement_container" in
        imas-mvdd-*) ;;
        *) die 'IMAS_MVDD_MEASUREMENT_CONTAINER must name an imas-mvdd-* container' ;;
    esac
    command -v docker >/dev/null || die 'docker is required for a custom measurement container'
    test "$(docker inspect --format '{{ index .Config.Labels \"imas.mvdd.dd-graph\" }}' "$measurement_container")" = true \
        || die 'custom measurement container is not labelled imas.mvdd.dd-graph=true'
    test "$(docker inspect --format '{{ index .Config.Labels \"imas.mvdd.dd-graph.home\" }}' "$measurement_container")" = "$IMAS_MVDD_GRAPH_HOME" \
        || die 'custom measurement container does not belong to IMAS_MVDD_GRAPH_HOME'
fi

graph_start() {
    if test -n "$measurement_container"; then
        docker start "$measurement_container" >/dev/null
    else
        "$dd_graph" start >/dev/null
    fi
}

graph_stop() {
    if test -n "$measurement_container"; then
        docker stop "$measurement_container" >/dev/null
    else
        "$dd_graph" stop >/dev/null
    fi
}

graph_query() {
    if test -n "$measurement_container"; then
        docker exec "$measurement_container" cypher-shell --non-interactive \
            -u "$NEO4J_USERNAME" -p "$NEO4J_PASSWORD" 'RETURN count(*) AS node_count;'
    else
        "$dd_graph" query
    fi
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
    local run=$3
    local sample="$sample_dir/${condition}-${pair}-${run}.json"
    IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_OUTPUT="$sample" \
        IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_PAIR="$pair" \
        cargo test --release measure_pinned_runtime_map_acquisition --lib -- \
        --ignored --nocapture
    jq -e '.schema == 1 and (.records | length == 2)' "$sample" >/dev/null \
        || die "measurement test did not emit two directional records for $pair"
    jq --arg condition "$condition" --argjson run "$run" \
        '.records[] + {condition: $condition, run: $run}' "$sample" >>"$sample_dir/records.jsonl"
}

cd "$repo_root"
graph_query >/dev/null
for run in $(seq 1 "$runs"); do
    for pair in "${pairs[@]}"; do
        measure_one warm "$pair" "$run"
    done
done

# A service restart invalidates Neo4j's in-process caches. It deliberately
# does not claim a cold disk: this command cannot flush host OS or VM caches.
for run in $(seq 1 "$runs"); do
    for pair in "${pairs[@]}"; do
        graph_stop
        graph_start
        # Sleeping waits for service initialization without a Cypher request,
        # which would warm the graph and invalidate this condition's label.
        sleep "$restart_settle_seconds"
        measure_one service-restarted "$pair" "$run"
    done
done

release=$(awk -F= '$1 == "GRAPH_RELEASE" { print $2 }' config/dd-graph-release.env)
manifest=$(awk -F= '$1 == "GRAPH_MANIFEST_DIGEST" { print $2 }' config/dd-graph-release.env)
graph_commit=$(awk -F= '$1 == "GRAPH_COMMIT" { print $2 }' config/dd-graph-release.env)
neo4j=$(awk -F= '$1 == "GRAPH_NEO4J_VERSION" { print $2 }' config/dd-graph-release.env)
code_commit=$(git rev-parse HEAD)
code_diff=$(git diff HEAD --binary --no-ext-diff)
rust_version=$(rustc --version)
machine=$(uname -sm)

jq -s \
    --arg release "$release" \
    --arg manifest "$manifest" \
    --arg graph_commit "$graph_commit" \
    --arg neo4j "$neo4j" \
    --arg code_commit "$code_commit" \
    --arg code_diff "$code_diff" \
    --arg rust_version "$rust_version" \
    --arg machine "$machine" \
    --arg command "scripts/measure-runtime-map.sh --output $report --evidence $evidence --runs $runs" \
    '{
       schema: 1,
       conditions: {
         warm: "running service preflighted with dd-graph query; service startup excluded",
         service_restarted: "service stopped and started immediately before every observation; startup excluded; OS and VM caches were not flushed"
       },
       identities: {
         graph_release: $release,
         graph_manifest: $manifest,
         graph_commit: $graph_commit,
         neo4j: $neo4j,
         code_commit: $code_commit,
         code_diff_from_head: $code_diff,
         rust: $rust_version,
         machine: $machine
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
    printf '| warm | running service preflighted by a harmless count query; startup excluded |\n'
    printf '| service-restarted | stopped and restarted before every observation; startup excluded; OS and VM caches not flushed |\n\n'
    printf 'The unchanged default is **5 seconds** per complete attempt. An observation is successful only when the Rust coordinator returns a complete validated map before that deadline; the harness preserves failures rather than retrying, extending an individual attempt, or converting a partial map into success.\n\n'
    printf '## Median observations\n\n'
    printf 'Each row aggregates `%s` independent samples of one map key. Retained bytes are a capacity-based estimate of map-owned data only; they exclude allocator overhead, the `Arc`/coordinator cache, transient construction allocations, process RSS and peak memory. Resolver and cache figures are average nanoseconds per call over 20,000 in-process repetitions, not a latency SLO.\n\n' "$runs"
    printf '| Condition | Pair | Stored → HLI | Outcome | Total ms | Retained estimate KiB | Resolver ns/call | Cache-hit ns/call | Failure |\n| --- | --- | --- | --- | ---: | ---: | ---: | ---: | --- |\n'
    jq -r '
      def median($key): sort_by(.[$key]) | .[length / 2 | floor][$key];
      .samples
      | group_by([.condition, .pair, .stored_dd, .hli_dd])[]
      | . as $group
      | ($group | median("total_ns")) as $total
      | ([ $group[].outcome ] | unique) as $outcomes
      | (if $outcomes == ["success"] then "success" else ($outcomes | join(", ")) end) as $outcome
      | (if $outcome == "success" then ($group | median("retained_map_estimate_bytes")) else null end) as $retained
      | (if $outcome == "success" then ($group | median("resolver_lookup_ns_per_call")) else null end) as $lookup
      | (if $outcome == "success" then ($group | median("cache_hit_ns_per_call")) else null end) as $cache
      | ([ $group[].failure | select(. != null) ] | unique | join("; ")) as $failure
      | "| \($group[0].condition) | \($group[0].pair) | \($group[0].stored_dd) → \($group[0].hli_dd) | \($outcome) | \(($total / 1000000 * 100 | round) / 100) | \(if $retained == null then "n/a" else (($retained / 1024 * 100 | round) / 100 | tostring) end) | \(if $lookup == null then "n/a" else ($lookup | tostring) end) | \(if $cache == null then "n/a" else ($cache | tostring) end) | \($failure) |"' \
        "$evidence"
    printf '\n## Stage samples\n\n'
    printf 'Per-sample stage timestamps (connection setup, complete query retrieval, decoding, scope validation, rule construction, map validation and publication) are retained in the companion JSON evidence. `Connection` records construction of the configured Bolt driver; the driver may establish its TCP session lazily with the first query, so transport handshake time is included in query retrieval rather than overstated as an eagerly verified connection time. Timed-out rows retain the terminal failure and their stage timestamps but have no retained-map or lookup figures, because failed maps are never published or cached.\n'
} >"$report"

printf 'Wrote %s and %s\n' "$report" "$evidence"
