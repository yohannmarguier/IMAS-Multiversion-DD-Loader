# Runtime conversion-map measurements

Issue #232 measures the selected Rust Bolt path, from one map-acquisition
attempt through validated `ConversionMap` publication. It does not measure
build time, graph download, Neo4j start-up, peak/RSS memory, or an HLI/Core
operation. Correctness remains the deterministic lifecycle and C ABI suite;
these are observations, not timing thresholds.

## Conditions

The measurements ran on 2026-09-20 on macOS (`Darwin arm64`) against the
committed DD-only graph selection: IMAS-Codex graph `v5.3.0`, manifest
`sha256:dc90975cb9fa0c7b08e9e4809640d01e41d927b4162200c13eec5076e030329b`,
producer `ba2c50cf4ce0df1fc4adc58f6957ce99b5c2db7e`, Neo4j
`2026.01.4-community`, and Rust `1.97.1`. The source baseline was
`a5e3a8d8555f3d9f35a3c15e3f39be28b7dad9fa`; the exact measurement harness
and this report are committed together in the issue #232 change. Every future
JSON evidence file records both that `HEAD` identity and its complete
`git diff HEAD --binary` patch, so a deliberate dirty-tree run remains
reconstructable.

The graph ran in a uniquely named task-owned local container. Its archive and
database were outside Git under `/private/tmp`; the existing default
digest-named container belonged to another task, and its data and credentials
were neither used nor changed. The warm run first issued the setup script's
harmless count query.
For the restarted-service run, the isolated service was stopped, started, then
given 30 seconds to initialise without a Cypher query; the following test
process acquired its one selected reference pair. A restart is not a
cold-storage claim: OS and VM caches were not flushed and startup was excluded.

The runnable harness is:

```console
$ IMAS_MVDD_GRAPH_HOME=/private/path \
  IMAS_MVDD_GRAPH_PASSWORD=... \
  NEO4J_URI=bolt://127.0.0.1:17687 \
  NEO4J_USERNAME=neo4j NEO4J_PASSWORD=... \
  scripts/measure-runtime-map.sh \
    --output docs/KG_RUNTIME_MAP_MEASUREMENTS.md \
    --evidence /private/path/runtime-map-measurements.json --runs 5
```

It records every stage timestamp in the JSON evidence. `Connection` is driver
construction; the driver may establish its TCP session lazily on the first
query, so connection handshake time is reported as part of query retrieval.
The harness defaults to the unchanged five-second deadline. Its test-only
deadline override was used once, separately, to observe successful retained
maps after the default measurements had established their outcome; it is not a
runtime configuration change or a revised default.

## Five-second results

One observation of every directional key was collected in each condition.
`equilibrium` safely timed out in `RuleConstruction` in both conditions. No
partial map was published, retained, or used for lookup measurement.

| Condition | Pair | Stored → HLI | Result | Total ms |
| --- | --- | --- | --- | ---: |
| Warm | equilibrium 3.39.0 ↔ 4.1.1 | 3.39.0 → 4.1.1 | timed out: rule construction | 5000.76 |
| Warm | equilibrium 3.39.0 ↔ 4.1.1 | 4.1.1 → 3.39.0 | timed out: rule construction | 5000.96 |
| Warm | equilibrium 3.42.0 ↔ 4.1.1 | 3.42.0 → 4.1.1 | timed out: rule construction | 5000.46 |
| Warm | equilibrium 3.42.0 ↔ 4.1.1 | 4.1.1 → 3.42.0 | timed out: rule construction | 5000.39 |
| Warm | pulse_schedule 3.25.0 ↔ 3.30.0 | 3.25.0 → 3.30.0 | complete map | 2261.14 |
| Warm | pulse_schedule 3.25.0 ↔ 3.30.0 | 3.30.0 → 3.25.0 | complete map | 2289.19 |
| Service restarted | equilibrium 3.39.0 ↔ 4.1.1 | 3.39.0 → 4.1.1 | timed out: rule construction | 5000.98 |
| Service restarted | equilibrium 3.39.0 ↔ 4.1.1 | 4.1.1 → 3.39.0 | timed out: rule construction | 5000.76 |
| Service restarted | equilibrium 3.42.0 ↔ 4.1.1 | 3.42.0 → 4.1.1 | timed out: rule construction | 5001.52 |
| Service restarted | equilibrium 3.42.0 ↔ 4.1.1 | 4.1.1 → 3.42.0 | timed out: rule construction | 5001.19 |
| Service restarted | pulse_schedule 3.25.0 ↔ 3.30.0 | 3.25.0 → 3.30.0 | complete map | 2400.22 |
| Service restarted | pulse_schedule 3.25.0 ↔ 3.30.0 | 3.30.0 → 3.25.0 | complete map | 2276.17 |

The warm stage trace reached the end of complete query retrieval in
approximately 0.11–0.20 seconds and completed decoding/scope validation by
approximately 0.12–0.20 seconds. The successful `pulse_schedule` maps spent
about 2.14–2.17 seconds in rule construction. The four equilibrium keys had
reached rule construction in about 0.19–0.20 seconds but did not finish before
the same five-second attempt expired. This is a safely reported deadline
outcome, not permission to extend the default or accept a partial map.

## Retained maps and lookups

The successful default `pulse_schedule` maps retained an estimated 440,790
and 440,770 bytes (430.46 and 430.44 KiB) for the two direction-specific keys.
The estimate counts map-owned inline storage, vector/hash-table capacities and
owned string capacities. It excludes allocator overhead, `Arc` and coordinator
cache objects, transient construction data, process RSS and peak memory.

Resolving the existing `ids_properties/homogeneous_time` path 20,000 times
after acquisition took 2,267 and 2,283 ns/call in the warm run; process-life
coordinator hits for those two retained keys took 58 and 57 ns/call. The
restarted run recorded 2,251/2,269 ns/call and 57/56 ns/call respectively.
Those cache-hit measurements reuse the same `Arc` after construction, so they
perform no graph work. The harness first drops its caller-owned `Arc`, then
requires the coordinator to return the same retained `Arc` before timing its
hits; the evidence records `retained_cache_hit_after_caller_release: true` for
every successful key. That is a direct post-release cache result, and the
coordinator's immediate cache path has no graph call.

For a diagnostic comparison only, the same complete requests were allowed a
non-default 120-second bound. All six completed: equilibrium 3.39.0 ↔ 4.1.1
took 6764.61/6560.40 ms and retained 1093933/1094691 bytes; equilibrium
3.42.0 ↔ 4.1.1 took 6766.36/6760.44 ms and retained 1131109/1132649 bytes;
pulse_schedule took 2265.27/2280.45 ms and retained 440790/440770 bytes.
Resolver lookup costs were 6569/9537, 6246/10358 and 2259/2264 ns/call,
respectively; cache hits were 54–58 ns/call. These successful diagnostic maps
show retained storage and lookup cost; they do not make the five-second
equilibrium timeouts acceptable.

The cost is dominated by local rule construction, not the measured complete
retrieval. Any optimisation decision needs its own behavior-preserving scoped
work; this issue neither changes the deadline nor proposes selector
compression.

## Publication-boundary handoff

`AcquisitionStage::Publication` currently marks completion of map construction,
not completion of the coordinator's cache insertion. The evidence therefore
records the successful `coordinator.acquire` return time as
`publication_completed_ns`; it is the end-to-end completion measure, whereas
the stage timeline's `publication` entry is only a start marker. The final
deadline check also precedes the cache insertion. This is a concrete
deadline/publication observability defect in the #205 prerequisite, not a
reason to change the five-second default here. It was returned to its owner in
[the #205 handoff](https://github.com/yohannmarguier/IMAS-Multiversion-DD-Loader/issues/205#issuecomment-5752064958): add a post-insertion completion boundary and terminal deadline check.
