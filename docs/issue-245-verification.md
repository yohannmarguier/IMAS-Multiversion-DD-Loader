# Issue #245 verification and handoff

This corrective stream started from reviewed baseline `feb843e` and owns graph
lifecycle/setup plus measurement orchestration. It does not change production
map acquisition, conversion policy, the five-second default, Core/HLI behavior,
or the companion #244 deadline/publication corrections. Issue #246 owns the
final combined CI/HLI acceptance run.

## Four-finding checklist

| Finding | Original red evidence | Corrected contract and regression |
| --- | --- | --- |
| F2 — cross-home ownership | On `feb843e`, provisioning home A and calling `stop` from unprovisioned home B selected the same manifest-derived container and the new regression stopped with `an unprovisioned home stopped another home service`. | Service names combine stable home and manifest identities. Owner/home plus release, manifest, archive, exporter and Neo4j labels are checked immediately before setup reuse, start, stop, query and explicit-container operations. `dd-graph-setup` covers two homes, mismatched/legacy ownership, same-home restart, distinct storage and an occupied host port without issuing a foreign mutation. |
| F3 — authentication-only smoke | The old mock returned `node_count / 1`, which passed because `RETURN count(*)` produces one row even for an empty graph. The new empty/wrong-schema fixtures therefore failed the old behavior. | Local setup and CI both run `config/dd-graph-smoke.cypher`. Success requires one DD 4.1.1 release, the equilibrium DD-version-stamp node, and at least one change with exactly one `FOR_IMAS_PATH` owner and one `IN_VERSION` release. The command validates the returned status and reports the counts; empty, incomplete, identity-mismatched and query-error cases fail. |
| F10 — unrelated provenance | On `feb843e`, the report read `config/dd-graph-release.env` even after a custom-home selection/update, so the custom-selection regression reported repository defaults. A labelled container could also differ from the service reached by `NEO4J_URI`. | Reports use the recorded home selection only after the actual container passes the complete label contract, and the URI must equal that container's published Bolt endpoint. Default, custom/updated selection, matching explicit override, mismatched override, wrong endpoint, dirty-state and credential-exclusion cases are deterministic regressions. Evidence records graph release, manifest/archive digests, exporter revision, Neo4j version/digest, service, code revision/dirty state, and a patch SHA-256 rather than patch contents. |
| F11 — second direction already warm | The old wrapper restarted once, while its Rust invocation emitted two directions; the recording Cargo double rejected the missing single-direction selector before the fix. | The Rust helper accepts exactly one pair and direction. The wrapper performs `stop → start → sleep-only readiness → acquire(direction)` six times per run, once for each directional key. `runtime-map-measurement` asserts the exact orchestrator event log and separately labels the warm batch. |

## Effective operator contract

The graph home owns mutable database and service identity. Immutable archives
may be copied between homes after digest verification; database directories and
containers may not. Legacy or unlabelled resources are never adopted or
removed by the script. An operator must inspect and retire those resources
manually, then run `setup` to create a home-owned replacement. A host-port
conflict fails before database loading and names
`IMAS_MVDD_GRAPH_BOLT_PORT` as the remedy.

`query` is the shared local/CI content gate. `verify-service` performs only the
non-warming identity check. Measurement revalidates identity before every
restart operation, performs no content query between a restart and its claimed
service-cold acquisition, excludes startup timing, and records that OS/VM/disk
caches were not flushed. `IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_DEADLINE_SECONDS`
is a test-only diagnostic budget when explicitly set; it never changes the
five-second production default.

## Executed evidence

Deterministic red/green checks:

```console
bash tests/scripts/dd_graph_setup_test.sh
bash tests/scripts/measure_runtime_map_test.sh
cargo test --lib
cargo clippy --all-targets --all-features -- -D warnings
```

The live smoke used the task-owned home
`/private/tmp/imas-mvdd-issue-245-graph`, service
`imas-mvdd-dd-graph-bd621bb0990c-dc90975cb9fa`, and isolated port 17690. It
returned `release_count=1`, `stamp_count=1`, and `change_count=94158` for the
pinned v5.3.0 graph. No other task's service or database was changed.

The corrected one-run evidence set is
[`KG_RUNTIME_MAP_MEASUREMENTS.json`](KG_RUNTIME_MAP_MEASUREMENTS.json), with its
rendered report in
[`KG_RUNTIME_MAP_MEASUREMENTS.md`](KG_RUNTIME_MAP_MEASUREMENTS.md). It retains
all 12 observations: the three required pairs, both directions, warm and an
independent service restart per direction. This is reproducible evidence, not
a latency threshold or a cold-storage claim. Equilibrium still reaches the
unchanged five-second deadline during rule construction; `pulse_schedule`
completes and retains memory/resolver/cache measurements. The earlier
2026-09-20 #232 table is superseded because its service identity could be
unrelated and its second direction was not service-cold.
