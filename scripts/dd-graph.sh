#!/usr/bin/env bash
# Manage a locally loaded, pinned IMAS-Codex DD-only graph.
#
# This script deliberately has no build-system entry point.  It operates only
# when requested by an operator and keeps its archive, database and selection
# in IMAS_MVDD_GRAPH_HOME (outside this repository by default).
set -euo pipefail

readonly script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
readonly repo_root=$(cd "$script_dir/.." && pwd)
readonly default_selection="$repo_root/config/dd-graph-release.env"
readonly smoke_query="$repo_root/config/dd-graph-smoke.cypher"
readonly state_root="${IMAS_MVDD_GRAPH_HOME:-${XDG_STATE_HOME:-$HOME/.local/state}/imas-mvdd-loader/dd-graph}"
readonly selection_record="$state_root/selection.env"
readonly bolt_port="${IMAS_MVDD_GRAPH_BOLT_PORT:-17687}"

die() {
    printf 'dd-graph: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Usage: scripts/dd-graph.sh <command> [--selection FILE]

Commands:
  select   record an immutable release selection outside the repository
  setup    acquire, verify, load, and start a new selection
  update   select, acquire, verify, load, and start a replacement selection
  start    start the recorded task-owned service without network access
  stop     stop the recorded task-owned service without deleting it
  query    verify service identity and required DD graph contents
  inspect  print the recorded immutable selection and service identity
  verify-service  verify the actual service ownership and selected identity

`--selection FILE` is accepted by select and update. Without it, select uses
config/dd-graph-release.env and setup reuses an existing recorded selection.
`--container NAME` explicitly selects an alternate service for start, stop,
query or verify-service; its complete ownership and selection identity must
still match the configured graph home before the command acts.
EOF
}

need_command() {
    command -v "$1" >/dev/null 2>&1 || die "requires '$1' on PATH"
}

load_selection() {
    local file=$1
    test -f "$file" || die "selection file does not exist: $file"

    unset GRAPH_RELEASE GRAPH_REFERENCE GRAPH_MANIFEST_DIGEST GRAPH_ARCHIVE
    unset GRAPH_ARCHIVE_DIGEST GRAPH_COMMIT GRAPH_NEO4J_IMAGE
    unset GRAPH_NEO4J_VERSION GRAPH_NEO4J_DIGEST
    # Selection files are operator-provided configuration, not credentials.
    # Shell syntax makes a compact, inspectable format for this local tool.
    # shellcheck source=/dev/null
    source "$file"

    local name
    for name in GRAPH_RELEASE GRAPH_REFERENCE GRAPH_MANIFEST_DIGEST GRAPH_ARCHIVE \
        GRAPH_ARCHIVE_DIGEST GRAPH_COMMIT GRAPH_NEO4J_IMAGE GRAPH_NEO4J_VERSION \
        GRAPH_NEO4J_DIGEST; do
        test -n "${!name:-}" || die "selection omits $name"
    done

    [[ "$GRAPH_MANIFEST_DIGEST" =~ ^sha256:[[:xdigit:]]{64}$ ]] || die 'manifest digest must be sha256:<64 hex characters>'
    [[ "$GRAPH_ARCHIVE_DIGEST" =~ ^sha256:[[:xdigit:]]{64}$ ]] || die 'archive digest must be sha256:<64 hex characters>'
    [[ "$GRAPH_NEO4J_DIGEST" =~ ^sha256:[[:xdigit:]]{64}$ ]] || die 'Neo4j digest must be sha256:<64 hex characters>'
    case "$GRAPH_ARCHIVE" in ''|*/*|*\\*) die 'archive must be a safe filename' ;; esac
    case "$GRAPH_ARCHIVE" in *[!A-Za-z0-9._-]*) die 'archive must be a safe filename' ;; esac
    case "$GRAPH_RELEASE" in *[!A-Za-z0-9._-]*) die 'release must contain only letters, digits, dot, underscore, or hyphen' ;; esac
}

record_selection() {
    local source_file=$1
    load_selection "$source_file"
    umask 077
    mkdir -p "$state_root"
    cp "$source_file" "$selection_record"
    printf 'Recorded %s at %s\n' "$GRAPH_RELEASE" "$selection_record"
}

stage_selection() {
    local source_file=$1 staged
    load_selection "$source_file"
    umask 077
    mkdir -p "$state_root"
    staged=$(mktemp "$state_root/.selection.XXXXXX")
    cp "$source_file" "$staged"
    printf '%s\n' "$staged"
}

require_recorded_selection() {
    load_selection "$selection_record"
}

selection_id() {
    printf '%s' "${GRAPH_MANIFEST_DIGEST#sha256:}" | cut -c1-12
}

home_id() {
    printf '%s' "$state_root" | shasum -a 256 | awk '{print substr($1, 1, 12)}'
}

container_name() {
    printf 'imas-mvdd-dd-graph-%s-%s' "$(home_id)" "$(selection_id)"
}

archive_path() {
    printf '%s/archives/%s/%s' "$state_root" "$(selection_id)" "$GRAPH_ARCHIVE"
}

database_path() {
    printf '%s/databases/%s' "$state_root" "$(selection_id)"
}

image_ref() {
    printf '%s:%s@%s' "$GRAPH_NEO4J_IMAGE" "$GRAPH_NEO4J_VERSION" "$GRAPH_NEO4J_DIGEST"
}

sha256_of() {
    shasum -a 256 "$1" | awk '{print $1}'
}

verify_archive() {
    local archive=$1
    test -f "$archive" || die "archive is missing: $archive"
    local actual
    actual=$(sha256_of "$archive")
    test "sha256:$actual" = "$GRAPH_ARCHIVE_DIGEST" || die "archive digest mismatch for $archive"
}

acquire_archive() {
    local archive
    archive=$(archive_path)
    mkdir -p "$(dirname "$archive")"
    if test -f "$archive"; then
        verify_archive "$archive"
        printf 'Verified cached archive %s\n' "$archive"
        return
    fi

    need_command oras
    local download_dir
    download_dir=$(mktemp -d "$state_root/.download.XXXXXX")
    # A large archive can be interrupted. Never leave a partial archive that
    # could later be mistaken for a verified selection.
    trap 'rm -rf "$download_dir"' EXIT INT TERM
    oras pull --output "$download_dir" "$GRAPH_REFERENCE@$GRAPH_MANIFEST_DIGEST"
    test -f "$download_dir/$GRAPH_ARCHIVE" || die "OCI artifact did not contain $GRAPH_ARCHIVE"
    verify_archive "$download_dir/$GRAPH_ARCHIVE"
    mv "$download_dir/$GRAPH_ARCHIVE" "$archive"
    rm -rf "$download_dir"
    trap - EXIT INT TERM
    printf 'Acquired and verified archive %s\n' "$archive"
}

verify_archive_manifest() {
    local archive=$1 temporary extracted manifest
    temporary=$(mktemp -d "$state_root/.extract.XXXXXX")
    trap 'rm -rf "$temporary"' RETURN
    # The published archive has one top-level directory. Strip it so
    # neo4j-admin sees graph.dump directly in the mounted /archives path.
    tar -xzf "$archive" -C "$temporary" --strip-components=1
    extracted=$(find "$temporary" -mindepth 1 -maxdepth 1 -type f -name graph.dump -print -quit)
    manifest=$(find "$temporary" -mindepth 1 -maxdepth 1 -type f -name manifest.json -print -quit)
    test -n "$extracted" || die 'archive does not contain graph.dump'
    test -n "$manifest" || die 'archive does not contain manifest.json'
    need_command jq
    # The immutable release archive records both its producer's development
    # version and the release tag.  The tag, rather than a rendered dev
    # version such as `5.2.1.dev0+g…`, is the configured selection identity.
    jq -e --arg release "$GRAPH_RELEASE" --arg commit "$GRAPH_COMMIT" \
        '(.version == $release or .git_tag == $release) and .git_commit == $commit' "$manifest" >/dev/null \
        || die 'archive manifest does not match the recorded release and commit'
    # `neo4j-admin database load neo4j --from-path` selects `neo4j.dump` by
    # database name. The immutable archive deliberately calls the payload
    # `graph.dump`, so normalize the disposable extracted copy only.
    mv "$extracted" "$temporary/neo4j.dump"
    printf '%s\n' "$temporary"
    trap - RETURN
}

ensure_password() {
    test -n "${IMAS_MVDD_GRAPH_PASSWORD:-}" || die 'set IMAS_MVDD_GRAPH_PASSWORD before setup or query'
}

ensure_free_database() {
    local data=$1
    if test -e "$data" && test -n "$(find "$data" -mindepth 1 -maxdepth 1 -print -quit)"; then
        die "database storage already exists: $data; refusing to overwrite it"
    fi
}

service_label() {
    local container=$1 label=$2
    docker inspect --format "{{ index .Config.Labels \"$label\" }}" "$container"
}

validate_service_identity() {
    local container=$1 actual
    actual=$(service_label "$container" imas.mvdd.dd-graph)
    test "$actual" = true \
        || die "service $container is not an IMAS-MVDD DD graph; refusing to mutate it"
    for label_and_expected in \
        "imas.mvdd.dd-graph.owner=$(home_id)" \
        "imas.mvdd.dd-graph.home=$state_root" \
        "imas.mvdd.dd-graph.release=$GRAPH_RELEASE" \
        "imas.mvdd.dd-graph.manifest=$GRAPH_MANIFEST_DIGEST" \
        "imas.mvdd.dd-graph.archive=$GRAPH_ARCHIVE_DIGEST" \
        "imas.mvdd.dd-graph.commit=$GRAPH_COMMIT" \
        "imas.mvdd.dd-graph.neo4j-version=$GRAPH_NEO4J_VERSION" \
        "imas.mvdd.dd-graph.neo4j-digest=$GRAPH_NEO4J_DIGEST"; do
        local label=${label_and_expected%%=*}
        local expected=${label_and_expected#*=}
        actual=$(service_label "$container" "$label")
        test "$actual" = "$expected" \
            || die "service $container has $label=${actual:-<missing>}, expected $expected; refusing to adopt or mutate it"
    done
}

ensure_bolt_port_available() {
    local occupants
    occupants=$(docker ps --quiet --filter "publish=$bolt_port")
    test -z "$occupants" \
        || die "host Bolt port $bolt_port is already published by another running container; choose IMAS_MVDD_GRAPH_BOLT_PORT"
}

load_and_start() {
    local archive data extracted container image
    archive=$(archive_path)
    data=$(database_path)
    container=$(container_name)
    image=$(image_ref)

    need_command docker
    ensure_password
    if docker container inspect "$container" >/dev/null 2>&1; then
        validate_service_identity "$container"
        die "task-owned service already exists: $container; use start, stop, or inspect"
    fi
    ensure_bolt_port_available
    ensure_free_database "$data"
    mkdir -p "$data"
    extracted=$(verify_archive_manifest "$archive")
    docker pull "$image"
    docker run --rm --name "$container-loader" --entrypoint neo4j-admin \
        --mount "type=bind,source=$data,target=/data" \
        --mount "type=bind,source=$extracted,target=/archives,readonly" \
        "$image" database load neo4j --from-path=/archives --overwrite-destination=false
    rm -rf "$extracted"
    docker run --detach --name "$container" \
        --label "imas.mvdd.dd-graph=true" \
        --label "imas.mvdd.dd-graph.owner=$(home_id)" \
        --label "imas.mvdd.dd-graph.home=$state_root" \
        --label "imas.mvdd.dd-graph.release=$GRAPH_RELEASE" \
        --label "imas.mvdd.dd-graph.manifest=$GRAPH_MANIFEST_DIGEST" \
        --label "imas.mvdd.dd-graph.archive=$GRAPH_ARCHIVE_DIGEST" \
        --label "imas.mvdd.dd-graph.commit=$GRAPH_COMMIT" \
        --label "imas.mvdd.dd-graph.neo4j-version=$GRAPH_NEO4J_VERSION" \
        --label "imas.mvdd.dd-graph.neo4j-digest=$GRAPH_NEO4J_DIGEST" \
        --env "NEO4J_AUTH=neo4j/$IMAS_MVDD_GRAPH_PASSWORD" \
        --publish "127.0.0.1:$bolt_port:7687" \
        --mount "type=bind,source=$data,target=/data" \
        "$image"
    printf 'Started %s at bolt://127.0.0.1:%s\n' "$container" "$bolt_port"
}

ensure_no_running_task_service() {
    need_command docker
    local running
    running=$(docker ps --quiet --filter "label=imas.mvdd.dd-graph.home=$state_root")
    test -z "$running" || die 'stop the currently selected task-owned graph before updating it'
}

start() {
    local container=${1:-}
    require_recorded_selection
    test -n "$container" || container=$(container_name)
    need_command docker
    docker container inspect "$container" >/dev/null 2>&1 \
        || die "no task-owned service for recorded selection; run setup instead"
    validate_service_identity "$container"
    docker start "$container" >/dev/null
    printf 'Started %s at bolt://127.0.0.1:%s\n' "$container" "$bolt_port"
}

stop() {
    local container=${1:-}
    require_recorded_selection
    test -n "$container" || container=$(container_name)
    need_command docker
    docker container inspect "$container" >/dev/null 2>&1 \
        || die "no task-owned service for recorded selection"
    validate_service_identity "$container"
    docker stop "$container" >/dev/null
    printf 'Stopped %s; archive and database remain at %s\n' "$container" "$state_root"
}

query() {
    local container=${1:-} result status
    require_recorded_selection
    ensure_password
    test -n "$container" || container=$(container_name)
    need_command docker
    test -f "$smoke_query" || die "required DD graph smoke query is missing: $smoke_query"
    docker container inspect "$container" >/dev/null 2>&1 \
        || die "no task-owned service for recorded selection"
    validate_service_identity "$container"
    result=$(docker exec "$container" cypher-shell --format plain --non-interactive \
        -u "${IMAS_MVDD_GRAPH_USERNAME:-neo4j}" \
        -p "$IMAS_MVDD_GRAPH_PASSWORD" \
        "$(<"$smoke_query")")
    status=$(printf '%s\n' "$result" | tail -n 1 \
        | awk -F', ' '{ gsub(/"|\r/, "", $1); print $1 }')
    test "$status" = imas_mvdd_smoke_ok \
        || die "selected service failed required DD graph content smoke (status: ${status:-empty result})"
    printf '%s\n' "$result"
}

verify_service() {
    local container=${1:-}
    require_recorded_selection
    test -n "$container" || container=$(container_name)
    need_command docker
    docker container inspect "$container" >/dev/null 2>&1 \
        || die "no task-owned service for recorded selection"
    validate_service_identity "$container"
    printf 'service: %s\n' "$container"
    printf 'owner: %s\n' "$(home_id)"
    printf 'release: %s\n' "$GRAPH_RELEASE"
    printf 'manifest: %s\n' "$GRAPH_MANIFEST_DIGEST"
    printf 'archive: %s\n' "$GRAPH_ARCHIVE_DIGEST"
    printf 'commit: %s\n' "$GRAPH_COMMIT"
    printf 'neo4j-version: %s\n' "$GRAPH_NEO4J_VERSION"
    printf 'neo4j-digest: %s\n' "$GRAPH_NEO4J_DIGEST"
}

inspect() {
    require_recorded_selection
    printf 'release: %s\n' "$GRAPH_RELEASE"
    printf 'manifest: %s\n' "$GRAPH_MANIFEST_DIGEST"
    printf 'archive: %s\n' "$GRAPH_ARCHIVE_DIGEST"
    printf 'commit: %s\n' "$GRAPH_COMMIT"
    printf 'neo4j: %s\n' "$(image_ref)"
    printf 'neo4j-version: %s\n' "$GRAPH_NEO4J_VERSION"
    printf 'neo4j-digest: %s\n' "$GRAPH_NEO4J_DIGEST"
    printf 'archive path: %s\n' "$(archive_path)"
    printf 'database path: %s\n' "$(database_path)"
    printf 'service: %s\n' "$(container_name)"
    printf 'owner: %s\n' "$(home_id)"
    printf 'bolt: bolt://127.0.0.1:%s\n' "$bolt_port"
}

main() {
    local command=${1:-}
    local chosen_selection=
    local chosen_container=
    shift || true
    while test $# -gt 0; do
        case "$1" in
            --selection)
                test $# -ge 2 || die '--selection needs a file'
                chosen_selection=$2
                shift 2
                ;;
            --container)
                test $# -ge 2 || die '--container needs a name'
                chosen_container=$2
                shift 2
                ;;
            -h|--help)
                usage
                return
                ;;
            *) die "unknown argument: $1" ;;
        esac
    done

    case "$command" in
        select)
            test -z "$chosen_container" || die 'select does not accept --container'
            record_selection "${chosen_selection:-$default_selection}"
            ;;
        setup)
            test -z "$chosen_container" || die 'setup does not accept --container'
            test -z "$chosen_selection" || die 'setup does not accept --selection; use select or update first'
            test -f "$selection_record" || die 'no recorded selection; run select first'
            require_recorded_selection
            acquire_archive
            load_and_start
            ;;
        update)
            local staged
            test -n "$chosen_selection" || die 'update requires --selection FILE'
            test -z "$chosen_container" || die 'update does not accept --container'
            ensure_no_running_task_service
            staged=$(stage_selection "$chosen_selection")
            load_selection "$staged"
            acquire_archive
            load_and_start
            mv "$staged" "$selection_record"
            printf 'Updated recorded selection at %s\n' "$selection_record"
            ;;
        start)
            test -z "$chosen_selection" || die 'start does not accept --selection'
            start "$chosen_container"
            ;;
        stop)
            test -z "$chosen_selection" || die 'stop does not accept --selection'
            stop "$chosen_container"
            ;;
        query)
            test -z "$chosen_selection" || die 'query does not accept --selection'
            query "$chosen_container"
            ;;
        inspect)
            test -z "$chosen_selection" || die 'inspect does not accept --selection'
            test -z "$chosen_container" || die 'inspect does not accept --container'
            inspect
            ;;
        verify-service)
            test -z "$chosen_selection" || die 'verify-service does not accept --selection'
            verify_service "$chosen_container"
            ;;
        ''|-h|--help)
            usage
            ;;
        *) die "unknown command: $command" ;;
    esac
}

main "$@"
