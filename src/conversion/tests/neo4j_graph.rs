use super::*;
use crate::conversion::runtime_map::{
    AcquisitionAttempt, AcquisitionClock, AcquisitionStage, AttemptObserver,
};
use std::cell::RefCell;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Row = std::collections::BTreeMap<String, GraphValue>;
type Reply = (std::collections::BTreeMap<String, GraphValue>, Vec<Row>);

#[test]
fn raw_scope_acquires_a_supported_scalar_without_inventing_coordinates() {
    use crate::conversion::conversion_map::{Direction, Outcome};
    use crate::conversion::runtime_map::{MapRequest, RuntimeMapAcquirer};
    let executor = scalar_executor();
    let source = Neo4jScopeSource::new(executor, 2).unwrap();
    let map = RuntimeMapAcquirer::new(source)
        .acquire(&MapRequest {
            ids: "equilibrium".into(),
            stored_dd: ArtifactDdVersion::new("3.39.0").unwrap(),
            hli_dd: ArtifactDdVersion::new("4.1.1").unwrap(),
        })
        .expect("complete raw evidence reaches the existing map constructor");
    assert!(matches!(
        map.resolve("ids_properties/homogeneous_time", Direction::Forward)
            .unwrap()
            .outcome,
        Outcome::Path { .. }
    ));
}

fn scalar_executor() -> ControlledExecutor {
    ControlledExecutor::default()
        .reply(query_parameters(None, 0), count(2))
        .reply(
            query_parameters(None, 0),
            vec![
                row(&[
                    ("release", GraphValue::String("3.39.0".into())),
                    ("cocos", GraphValue::Null),
                ]),
                row(&[
                    ("release", GraphValue::String("4.1.1".into())),
                    ("cocos", GraphValue::String("17".into())),
                ]),
            ],
        )
        .reply(query_parameters(Some("equilibrium"), 0), count(1))
        .reply(
            query_parameters(Some("equilibrium"), 0),
            vec![row(&[
                ("ids", GraphValue::String("equilibrium".into())),
                (
                    "path",
                    GraphValue::String("equilibrium/ids_properties/homogeneous_time".into()),
                ),
                ("data_type", GraphValue::String("INT_0D".into())),
                ("ndim", GraphValue::Integer(0)),
                ("units", GraphValue::Null),
                ("timebase", GraphValue::Null),
                ("coordinate_relationships", GraphValue::List(vec![])),
                (
                    "introduced",
                    GraphValue::List(vec![GraphValue::String("3.39.0".into())]),
                ),
                ("deprecated", GraphValue::List(vec![])),
                ("change_nbc_version", GraphValue::Null),
                ("change_nbc_description", GraphValue::Null),
                ("change_nbc_previous_name", GraphValue::Null),
                ("change_nbc_previous_type", GraphValue::Null),
                ("cocos_label_transformation", GraphValue::Null),
                ("cocos_transformation_expression", GraphValue::Null),
                ("cocos_label_source", GraphValue::Null),
            ])],
        )
        .reply(query_parameters(Some("equilibrium"), 0), count(0))
        .reply(query_parameters(Some("equilibrium"), 0), count(0))
}

#[derive(Default)]
struct ControlledExecutor {
    replies: RefCell<Vec<Reply>>,
    calls: RefCell<Vec<BoundQuery>>,
}

impl ControlledExecutor {
    fn reply(
        mut self,
        parameters: std::collections::BTreeMap<String, GraphValue>,
        rows: Vec<Row>,
    ) -> Self {
        self.replies.get_mut().push((parameters, rows));
        self
    }
}

impl CypherExecutor for ControlledExecutor {
    fn execute(&self, query: BoundQuery) -> Result<Vec<Row>, GraphSourceError> {
        self.calls.borrow_mut().push(query.clone());
        let position = self.calls.borrow().len() - 1;
        let (expected, rows) = self
            .replies
            .borrow()
            .get(position)
            .cloned()
            .ok_or_else(|| GraphSourceError("unexpected query".to_string()))?;
        if expected != query.parameters {
            return Err(GraphSourceError(
                "query parameters were not bound as expected".to_string(),
            ));
        }
        Ok(rows)
    }
}

#[derive(Default)]
struct ManualClock(Mutex<Duration>);

impl ManualClock {
    fn advance(&self, duration: Duration) {
        *self.0.lock().expect("manual clock mutex is not poisoned") += duration;
    }
}

impl AcquisitionClock for ManualClock {
    fn now(&self) -> Duration {
        *self.0.lock().expect("manual clock mutex is not poisoned")
    }
}

struct NoopObserver;

impl AttemptObserver for NoopObserver {
    fn entered(&self, _stage: AcquisitionStage) {}
}

fn attempt() -> AcquisitionAttempt {
    AcquisitionAttempt::new(
        Duration::from_secs(1),
        Arc::new(ManualClock::default()),
        Arc::new(NoopObserver),
    )
}

struct BlockedExecutor {
    clock: Arc<ManualClock>,
    timeout: RefCell<Option<Duration>>,
}

impl CypherExecutor for BlockedExecutor {
    fn execute(&self, query: BoundQuery) -> Result<Vec<Row>, GraphSourceError> {
        *self.timeout.borrow_mut() = Some(query.timeout);
        self.clock.advance(Duration::from_secs(5));
        Ok(count(0))
    }
}

fn row(entries: &[(&str, GraphValue)]) -> Row {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

fn count(value: i64) -> Vec<Row> {
    vec![row(&[("count", GraphValue::Integer(value))])]
}

fn query_parameters(
    ids: Option<&str>,
    skip: usize,
) -> std::collections::BTreeMap<String, GraphValue> {
    let mut parameters = std::collections::BTreeMap::from([
        ("skip".to_string(), GraphValue::Integer(skip as i64)),
        ("limit".to_string(), GraphValue::Integer(2)),
    ]);
    if let Some(ids) = ids {
        parameters.insert("ids".to_string(), GraphValue::String(ids.to_string()));
    }
    parameters
}

fn complete_executor() -> ControlledExecutor {
    ControlledExecutor::default()
        .reply(query_parameters(None, 0), count(2))
        .reply(
            query_parameters(None, 0),
            vec![
                row(&[
                    ("release", GraphValue::String("4.1.1".to_string())),
                    ("cocos", GraphValue::String("17".to_string())),
                ]),
                row(&[
                    ("release", GraphValue::String("3.39.0".to_string())),
                    ("cocos", GraphValue::Null),
                ]),
            ],
        )
        .reply(query_parameters(Some("equilibrium"), 0), count(2))
        .reply(
            query_parameters(Some("equilibrium"), 0),
            vec![
                row(&[
                    ("ids", GraphValue::String("equilibrium".to_string())),
                    ("path", GraphValue::String("time_slice".to_string())),
                    (
                        "coordinate_relationships",
                        GraphValue::List(vec![GraphValue::List(vec![
                            GraphValue::Integer(1),
                            GraphValue::String("time".to_string()),
                            GraphValue::String("path".into()),
                        ])]),
                    ),
                    ("introduced", GraphValue::List(Vec::new())),
                    ("deprecated", GraphValue::List(Vec::new())),
                ]),
                row(&[
                    ("ids", GraphValue::String("equilibrium".to_string())),
                    (
                        "path",
                        GraphValue::String(
                            "ids_properties/version_put/data_dictionary".to_string(),
                        ),
                    ),
                    ("coordinate_relationships", GraphValue::List(Vec::new())),
                    ("introduced", GraphValue::List(Vec::new())),
                    ("deprecated", GraphValue::List(Vec::new())),
                ]),
            ],
        )
        .reply(query_parameters(Some("equilibrium"), 0), count(1))
        .reply(
            query_parameters(Some("equilibrium"), 0),
            vec![row(&[
                ("id", GraphValue::String("event-1".to_string())),
                ("path", GraphValue::String("time_slice".to_string())),
                ("release", GraphValue::String("4.1.1".to_string())),
                ("kind", GraphValue::String("structure_changed".to_string())),
                (
                    "releases",
                    GraphValue::List(vec![GraphValue::String("4.1.1".into())]),
                ),
                (
                    "owners",
                    GraphValue::List(vec![GraphValue::String("time_slice".to_string())]),
                ),
            ])],
        )
        .reply(query_parameters(Some("equilibrium"), 0), count(1))
        .reply(
            query_parameters(Some("equilibrium"), 0),
            vec![row(&[
                ("from_path", GraphValue::String("time_slice".to_string())),
                (
                    "to_path",
                    GraphValue::String("ids_properties/version_put/data_dictionary".to_string()),
                ),
            ])],
        )
}

#[test]
fn retrieves_all_schema_streams_with_bound_pagination_and_typed_nulls() {
    let executor = complete_executor();
    let source = Neo4jScopeSource::new(executor, 2).unwrap();
    let attempt = attempt();
    let scope = source
        .load_raw_scope("equilibrium", &attempt)
        .expect("complete rows must decode");
    assert_eq!(scope.versions.len(), 2);
    assert_eq!(scope.nodes.len(), 2);
    assert_eq!(scope.events.len(), 1);
    assert_eq!(scope.successors.len(), 1);
    assert_eq!(scope.versions[1]["cocos"], GraphValue::Null);
    assert_eq!(
        scope.nodes[0]["coordinate_relationships"],
        GraphValue::List(vec![GraphValue::List(vec![
            GraphValue::Integer(1),
            GraphValue::String("time".to_string()),
            GraphValue::String("path".into()),
        ])])
    );
}

#[test]
fn node_query_keeps_unversioned_coordinate_relationships_distinct_from_raw_history() {
    assert!(NODES.contains("HAS_COORDINATE"));
    assert!(NODES.contains("coordinate_relationships"));
    assert!(!NODES.contains("n.coordinates"));
}

#[test]
fn a_blocked_transport_receives_the_attempt_remainder_and_times_out_after_returning() {
    let clock = Arc::new(ManualClock::default());
    let executor = BlockedExecutor {
        clock: clock.clone(),
        timeout: RefCell::new(None),
    };
    let source = Neo4jScopeSource::new(executor, 2).unwrap();
    let attempt = AcquisitionAttempt::new(Duration::from_secs(5), clock, Arc::new(NoopObserver));

    assert!(matches!(
        source.load_raw_scope("equilibrium", &attempt),
        Err(GraphSourceError(message)) if message.contains("timed out")
    ));
    assert_eq!(
        source.executor.timeout.borrow().as_ref(),
        Some(&Duration::from_secs(5))
    );
}

#[test]
fn caller_returns_at_its_deadline_while_a_blocked_driver_worker_cannot_publish_later() {
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        release_receiver
            .recv()
            .expect("test releases the blocked worker");
        let _ = result_sender.send(Ok(()));
    });

    assert!(matches!(
        receive_before_deadline(result_receiver, Duration::ZERO),
        Err(GraphSourceError(message)) if message == "Neo4j query exceeded the acquisition deadline"
    ));
    release_sender.send(()).expect("worker is still waiting");
    worker.join().expect("released worker exits normally");
}

#[test]
fn refuses_a_missing_page_instead_of_returning_a_partial_scope() {
    let executor = ControlledExecutor::default()
        .reply(query_parameters(None, 0), count(3))
        .reply(
            query_parameters(None, 0),
            vec![
                row(&[
                    ("release", GraphValue::String("3.39.0".to_string())),
                    ("cocos", GraphValue::Null),
                ]),
                row(&[
                    ("release", GraphValue::String("4.1.1".to_string())),
                    ("cocos", GraphValue::String("17".to_string())),
                ]),
            ],
        )
        .reply(query_parameters(None, 2), Vec::new());
    let source = Neo4jScopeSource::new(executor, 2).unwrap();
    let attempt = attempt();
    assert!(
        matches!(source.load_raw_scope("equilibrium", &attempt), Err(GraphSourceError(message)) if message.contains("pagination ended"))
    );
}

#[test]
fn rejects_duplicate_or_cross_scope_evidence_after_shuffled_pages() {
    let mut executor = complete_executor();
    executor.replies.get_mut()[3].1.swap(0, 1);
    executor.replies.get_mut()[3].1[0].insert(
        "path".to_string(),
        GraphValue::String("time_slice".to_string()),
    );
    let source = Neo4jScopeSource::new(executor, 2).unwrap();
    let attempt = attempt();
    assert!(
        matches!(source.load_raw_scope("equilibrium", &attempt), Err(GraphSourceError(message)) if message == "duplicate node path")
    );
}

#[test]
fn distinguishes_a_typed_null_from_a_missing_required_value() {
    let mut executor = complete_executor();
    executor.replies.get_mut()[5].1[0].insert("kind".to_string(), GraphValue::Null);
    let source = Neo4jScopeSource::new(executor, 2).unwrap();
    let attempt = attempt();
    assert!(
        matches!(source.load_raw_scope("equilibrium", &attempt), Err(GraphSourceError(message)) if message == "kind is typed null")
    );
}

#[test]
fn rejects_duplicate_event_ids_even_when_their_rows_are_otherwise_valid() {
    let mut executor = complete_executor();
    *executor.replies.get_mut()[4].1[0]
        .get_mut("count")
        .expect("controlled count row has count") = GraphValue::Integer(2);
    let duplicate = executor.replies.get_mut()[5].1[0].clone();
    executor.replies.get_mut()[5].1.push(duplicate);
    let source = Neo4jScopeSource::new(executor, 2).unwrap();
    let attempt = attempt();
    assert!(
        matches!(source.load_raw_scope("equilibrium", &attempt), Err(GraphSourceError(message)) if message == "duplicate event ID")
    );
}

#[test]
#[ignore = "requires the pinned Neo4j graph provisioned by CI"]
fn pinned_graph_returns_complete_reference_scopes() {
    let config = live_config();
    use crate::conversion::conversion_map::{Direction, Outcome};
    use crate::conversion::runtime_map::{MapRequest, RuntimeMapCoordinator};
    let coordinator =
        RuntimeMapCoordinator::with_deadline(Neo4jFactsSource(config), Duration::from_secs(120));
    for (ids, first, second) in [
        ("equilibrium", "3.39.0", "4.1.1"),
        ("equilibrium", "3.42.0", "4.1.1"),
        ("pulse_schedule", "3.25.0", "3.30.0"),
    ] {
        for (stored, hli) in [(first, second), (second, first)] {
            let request = MapRequest {
                ids: ids.into(),
                stored_dd: ArtifactDdVersion::new(stored).unwrap(),
                hli_dd: ArtifactDdVersion::new(hli).unwrap(),
            };
            let map = coordinator
                .acquire(&request)
                .unwrap_or_else(|error| panic!("{ids} {stored} -> {hli}: {error:?}"));
            assert!(matches!(
                map.resolve("ids_properties/homogeneous_time", Direction::Forward)
                    .unwrap()
                    .outcome,
                Outcome::Path { .. }
            ));
            if ids == "equilibrium" {
                use crate::conversion::conversion_map::ValueTransformation;
                let psi = map
                    .resolve("time_slice/profiles_1d/psi", Direction::Forward)
                    .unwrap();
                assert!(matches!(
                    psi.outcome,
                    Outcome::Path {
                        value_transformation: ValueTransformation::SignFlip { .. },
                        ..
                    }
                ));
                let j = if hli == "4.1.1" { "j_phi" } else { "j_tor" };
                let field = format!("time_slice/constraints/{j}/measured");
                assert!(matches!(
                    map.resolve(&field, Direction::Forward).unwrap().outcome,
                    Outcome::Path { .. }
                ));
                {
                    let field = format!(
                        "time_slice/global_quantities/magnetic_axis/b_field_{}",
                        if hli == "4.1.1" { "phi" } else { "tor" }
                    );
                    let outcome = map.resolve(&field, Direction::Forward);
                    assert!(outcome.is_some(), "missing endpoint {field}");
                    assert!(
                        matches!(outcome.unwrap().outcome, Outcome::Refusal(_)),
                        "unsupported evidence became a value conversion: {field}"
                    );
                }
                assert!(matches!(
                    map.resolve("time_slice/boundary/outline/r", Direction::Forward)
                        .unwrap()
                        .outcome,
                    Outcome::Path { .. }
                ));
                if hli == "4.1.1" {
                    assert!(matches!(
                        map.resolve("time_slice/boundary/gap", Direction::Forward)
                            .unwrap()
                            .outcome,
                        Outcome::Refusal(_)
                    ));
                    assert!(matches!(
                        map.resolve("grids_ggd/grid/space/coordinates_type", Direction::Forward)
                            .unwrap()
                            .outcome,
                        Outcome::Refusal(_)
                    ));
                }
            } else {
                let (caller, target) = if hli == "3.30.0" {
                    ("launcher", "antenna")
                } else {
                    ("antenna", "launcher")
                };
                for suffix in ["", "/name"] {
                    let field = format!("ec/{caller}{suffix}");
                    let outcome = map.resolve(&field, Direction::Forward).unwrap();
                    assert!(
                        matches!(outcome.outcome, Outcome::Path { resolved_path, .. } if resolved_path == format!("ec/{target}{suffix}"))
                    );
                }
                assert!(
                    map.resolve("ec/beam/name", Direction::Forward).is_none(),
                    "an intermediate witness is not an endpoint"
                );
            }
            eprintln!("live complete acquisition succeeded: {ids} {stored} -> {hli}");
        }
    }
}

#[derive(Default)]
struct StageTimeline {
    started: Mutex<Option<Instant>>,
    entries: Mutex<Vec<(AcquisitionStage, Duration)>>,
}

impl StageTimeline {
    fn start(&self) {
        *self
            .started
            .lock()
            .expect("timeline start mutex is not poisoned") = Some(Instant::now());
        self.entries
            .lock()
            .expect("timeline entries mutex is not poisoned")
            .clear();
    }

    fn entries(&self) -> Vec<(AcquisitionStage, Duration)> {
        self.entries
            .lock()
            .expect("timeline entries mutex is not poisoned")
            .clone()
    }
}

impl AttemptObserver for StageTimeline {
    fn entered(&self, stage: AcquisitionStage) {
        let started = self
            .started
            .lock()
            .expect("timeline start mutex is not poisoned")
            .expect("measurement starts before map acquisition");
        self.entries
            .lock()
            .expect("timeline entries mutex is not poisoned")
            .push((stage, started.elapsed()));
    }
}

fn reference_measurement_pairs() -> [(&'static str, &'static str, &'static str, &'static str); 3] {
    [
        ("equilibrium-3.39.0-4.1.1", "equilibrium", "3.39.0", "4.1.1"),
        ("equilibrium-3.42.0-4.1.1", "equilibrium", "3.42.0", "4.1.1"),
        (
            "pulse-schedule-3.25.0-3.30.0",
            "pulse_schedule",
            "3.25.0",
            "3.30.0",
        ),
    ]
}

fn measurement_stage_name(stage: AcquisitionStage) -> &'static str {
    match stage {
        AcquisitionStage::Connection => "connection",
        AcquisitionStage::Source => "source",
        AcquisitionStage::Query => "query",
        AcquisitionStage::Decoding => "decoding",
        AcquisitionStage::ScopeValidation => "scope_validation",
        AcquisitionStage::RuleConstruction => "rule_construction",
        AcquisitionStage::MapValidation => "map_validation",
        AcquisitionStage::Publication => "publication",
    }
}

fn json_string(value: &str) -> String {
    use std::fmt::Write;

    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('\"');
    for character in value.chars() {
        match character {
            '\"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0c}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control <= '\u{1f}' => write!(escaped, "\\u{:04x}", control as u32)
                .expect("writing JSON escape to String cannot fail"),
            character => escaped.push(character),
        }
    }
    escaped.push('\"');
    escaped
}

fn measurement_deadline() -> Duration {
    match std::env::var("IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_DEADLINE_SECONDS") {
        Ok(value) => Duration::from_secs(
            value
                .parse()
                .ok()
                .filter(|seconds: &u64| *seconds > 0)
                .expect("IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_DEADLINE_SECONDS must be positive"),
        ),
        Err(_) => Duration::from_secs(5),
    }
}

/// Emits one independently reproducible directional sample of the selected
/// live pair. The accompanying script owns service restart and report
/// conditions. This
/// ignored test intentionally has no timing threshold: deterministic
/// lifecycle tests establish correctness, while this records observations.
#[test]
#[ignore = "requires the pinned Neo4j graph and an explicit measurement output path"]
fn measure_pinned_runtime_map_acquisition() {
    use crate::conversion::conversion_map::Direction;
    use crate::conversion::runtime_map::{MapRequest, RuntimeMapCoordinator};

    let output = std::env::var("IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_OUTPUT")
        .expect("set IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_OUTPUT to a new sample path");
    let selected = std::env::var("IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_PAIR")
        .expect("select exactly one IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_PAIR");
    let direction = std::env::var("IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_DIRECTION")
        .expect("select IMAS_MVDD_RUNTIME_MAP_MEASUREMENT_DIRECTION=forward or reverse");
    assert!(
        direction == "forward" || direction == "reverse",
        "measurement direction must be forward or reverse"
    );
    let pairs: Vec<_> = reference_measurement_pairs()
        .into_iter()
        .filter(|(name, _, _, _)| selected == *name)
        .collect();
    assert_eq!(pairs.len(), 1, "unknown measurement pair {selected}");

    let timeline = Arc::new(StageTimeline::default());
    let deadline = measurement_deadline();
    let coordinator = RuntimeMapCoordinator::with_clock_and_observer(
        Neo4jFactsSource(live_config()),
        deadline,
        Arc::new(crate::conversion::runtime_map::SystemClock::new()),
        Arc::clone(&timeline) as Arc<dyn AttemptObserver>,
    );
    let mut records = Vec::new();
    for (name, ids, first, second) in pairs {
        let (stored, hli) = if direction == "forward" {
            (first, second)
        } else {
            (second, first)
        };
        let request = MapRequest {
            ids: ids.to_string(),
            stored_dd: ArtifactDdVersion::new(stored).unwrap(),
            hli_dd: ArtifactDdVersion::new(hli).unwrap(),
        };
        timeline.start();
        let started = Instant::now();
        let acquired = coordinator.acquire(&request);
        let total = started.elapsed();
        let entries = timeline.entries();
        let (outcome, failure, retained, lookup_ns, cached_ns, retained_cache_hit) = match acquired
        {
            Ok(map) => {
                let retained = Arc::downgrade(&map);
                drop(map);
                let map = coordinator.acquire(&request).unwrap();
                let retained_cache_hit = Arc::ptr_eq(
                    &map,
                    &retained
                        .upgrade()
                        .expect("coordinator retains a successful map after caller release"),
                );
                let lookup_path = "ids_properties/homogeneous_time";
                let lookup_count = 20_000_u32;
                let lookup_started = Instant::now();
                for _ in 0..lookup_count {
                    assert!(map.resolve(lookup_path, Direction::Forward).is_some());
                }
                let lookup_ns = lookup_started.elapsed().as_nanos() / u128::from(lookup_count);
                let cached_count = 20_000_u32;
                let cached_started = Instant::now();
                for _ in 0..cached_count {
                    assert!(Arc::ptr_eq(&map, &coordinator.acquire(&request).unwrap()));
                }
                let cached_ns = cached_started.elapsed().as_nanos() / u128::from(cached_count);
                (
                    "success",
                    None,
                    Some(map.estimated_retained_bytes()),
                    Some(lookup_ns),
                    Some(cached_ns),
                    Some(retained_cache_hit),
                )
            }
            Err(error) => (
                "failure",
                Some(format!("{error:?}")),
                None,
                None,
                None,
                None,
            ),
        };
        let stages = entries
            .iter()
            .map(|(stage, elapsed)| {
                format!(
                    "{{\"stage\":{},\"elapsed_ns\":{}}}",
                    json_string(measurement_stage_name(*stage)),
                    elapsed.as_nanos()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        records.push(format!(
                "{{\"pair\":{},\"ids\":{},\"stored_dd\":{},\"hli_dd\":{},\"direction\":{},\"deadline_seconds\":{},\"outcome\":{},\"failure\":{},\"total_ns\":{},\"publication_completed_ns\":{},\"retained_map_estimate_bytes\":{},\"resolver_lookup_ns_per_call\":{},\"cache_hit_ns_per_call\":{},\"retained_cache_hit_after_caller_release\":{},\"stages\":[{}]}}",
                json_string(name),
                json_string(ids),
                json_string(stored),
                json_string(hli),
                json_string(&direction),
                deadline.as_secs(),
                json_string(outcome),
                failure.map_or_else(|| "null".to_string(), |value| json_string(&value)),
                total.as_nanos(),
                if outcome == "success" { total.as_nanos().to_string() } else { "null".to_string() },
                retained.map_or_else(|| "null".to_string(), |value| value.to_string()),
                lookup_ns.map_or_else(|| "null".to_string(), |value| value.to_string()),
                cached_ns.map_or_else(|| "null".to_string(), |value| value.to_string()),
                retained_cache_hit.map_or_else(|| "null".to_string(), |value| value.to_string()),
                stages,
            ));
    }
    std::fs::write(
        &output,
        format!("{{\"schema\":1,\"records\":[{}]}}\n", records.join(",")),
    )
    .unwrap_or_else(|error| panic!("write measurement output {output}: {error}"));
}

#[test]
fn acquisition_rejects_an_event_with_multiple_owners_in_the_same_ids() {
    let executor = complete_executor();
    executor.replies.borrow_mut()[5].1[0].insert(
        "owners".into(),
        GraphValue::List(vec![
            GraphValue::String("time_slice".into()),
            GraphValue::String("ids_properties/version_put/data_dictionary".into()),
        ]),
    );
    let source = Neo4jScopeSource::new(executor, 2).unwrap();
    assert!(source.load_raw_scope("equilibrium", &attempt()).is_err());
}

#[test]
fn raw_array_coordinates_use_complete_one_based_relationships_without_cocos_fallback() {
    use crate::conversion::conversion_map::{Direction, Outcome};
    use crate::conversion::runtime_map::{MapRequest, RuntimeMapAcquirer};
    let executor = scalar_executor();
    {
        let mut replies = executor.replies.borrow_mut();
        let node = &mut replies[3].1[0];
        node.insert("path".into(), GraphValue::String("equilibrium/time".into()));
        node.insert("data_type".into(), GraphValue::String("FLT_1D".into()));
        node.insert("ndim".into(), GraphValue::Integer(1));
        node.insert("timebase".into(), GraphValue::String("time".into()));
        node.insert(
            "coordinate_relationships".into(),
            GraphValue::List(vec![GraphValue::List(vec![
                GraphValue::Integer(1),
                GraphValue::String("1...N".into()),
                GraphValue::String("spec".into()),
            ])]),
        );
    }
    let map = RuntimeMapAcquirer::new(Neo4jScopeSource::new(executor, 2).unwrap())
        .acquire(&MapRequest {
            ids: "equilibrium".into(),
            stored_dd: ArtifactDdVersion::new("3.39.0").unwrap(),
            hli_dd: ArtifactDdVersion::new("4.1.1").unwrap(),
        })
        .unwrap();
    assert!(matches!(
        map.resolve("time", Direction::Forward).unwrap().outcome,
        Outcome::Path { .. }
    ));
}

fn raw_event(field: &str, kind: &str, old: GraphValue, new: GraphValue) -> Row {
    let path = "equilibrium/ids_properties/homogeneous_time";
    row(&[
        ("id", GraphValue::String(format!("{path}:{field}:4.1.1"))),
        ("path", GraphValue::String(path.into())),
        ("release", GraphValue::String("4.1.1".into())),
        (
            "releases",
            GraphValue::List(vec![GraphValue::String("4.1.1".into())]),
        ),
        (
            "owners",
            GraphValue::List(vec![GraphValue::String(path.into())]),
        ),
        ("kind", GraphValue::String(kind.into())),
        ("old_value", old),
        ("new_value", new),
        ("semantic_type", GraphValue::Null),
        ("unit_change_subtype", GraphValue::Null),
    ])
}

fn add_raw_event(executor: &ControlledExecutor, event: Row) {
    let mut replies = executor.replies.borrow_mut();
    replies[4].1 = count(1);
    replies.insert(5, (query_parameters(Some("equilibrium"), 0), vec![event]));
}

fn acquire_raw(
    executor: ControlledExecutor,
) -> Result<
    crate::conversion::conversion_map::ConversionMap,
    crate::conversion::runtime_map::AcquisitionFailure,
> {
    use crate::conversion::runtime_map::{MapRequest, RuntimeMapAcquirer};
    RuntimeMapAcquirer::new(Neo4jScopeSource::new(executor, 2).unwrap()).acquire(&MapRequest {
        ids: "equilibrium".into(),
        stored_dd: ArtifactDdVersion::new("3.39.0").unwrap(),
        hli_dd: ArtifactDdVersion::new("4.1.1").unwrap(),
    })
}

#[test]
fn raw_acquisition_rejects_unknown_required_event_semantics() {
    let executor = scalar_executor();
    add_raw_event(
        &executor,
        raw_event(
            "data_type",
            "unknown_transform",
            GraphValue::String("INT_0D".into()),
            GraphValue::String("INT_0D".into()),
        ),
    );
    assert!(
        acquire_raw(executor).is_err(),
        "an unknown kind must not become identity"
    );
}

#[test]
fn raw_acquisition_replays_a_type_change_from_the_addition_property() {
    use crate::conversion::conversion_map::{Direction, Outcome, RefusalReason};
    let executor = scalar_executor();
    add_raw_event(
        &executor,
        raw_event(
            "data_type",
            "data_type",
            GraphValue::String("INT_0D".into()),
            GraphValue::String("STRUCTURE".into()),
        ),
    );
    let map = acquire_raw(executor).unwrap();
    assert_eq!(
        map.resolve("ids_properties/homogeneous_time", Direction::Forward)
            .unwrap()
            .outcome,
        Outcome::Refusal(RefusalReason::UnservableRetype)
    );
}

#[test]
fn raw_acquisition_rejects_invalid_required_type_rank_and_columns() {
    for (key, value) in [
        ("data_type", GraphValue::String("invented_type".into())),
        ("ndim", GraphValue::Integer(8)),
        ("ndim", GraphValue::Integer(1)),
        ("units", GraphValue::Integer(1)),
        ("introduced", GraphValue::Null),
    ] {
        let executor = scalar_executor();
        executor.replies.borrow_mut()[3].1[0].insert(key.into(), value);
        assert!(acquire_raw(executor).is_err(), "accepted malformed {key}");
    }
    let executor = scalar_executor();
    executor.replies.borrow_mut()[3].1[0].remove("timebase");
    assert!(acquire_raw(executor).is_err());
}

#[test]
fn raw_acquisition_is_invariant_to_release_row_order() {
    use crate::conversion::conversion_map::Direction;
    let ordered = acquire_raw(scalar_executor()).unwrap();
    let shuffled = scalar_executor();
    shuffled.replies.borrow_mut()[1].1.reverse();
    let shuffled = acquire_raw(shuffled).unwrap();
    assert_eq!(
        ordered.resolve("ids_properties/homogeneous_time", Direction::Forward),
        shuffled.resolve("ids_properties/homogeneous_time", Direction::Forward)
    );
}

#[test]
fn raw_scientific_documentation_and_node_type_changes_cannot_become_identity() {
    use crate::conversion::conversion_map::{Direction, Outcome};
    for field in ["documentation", "node_type"] {
        let executor = scalar_executor();
        let mut event = raw_event(
            field,
            field,
            GraphValue::String("old".into()),
            GraphValue::String("new".into()),
        );
        event.insert(
            "semantic_type".into(),
            GraphValue::String("coordinate_convention".into()),
        );
        add_raw_event(&executor, event);
        let map = acquire_raw(executor).unwrap();
        assert!(
            matches!(
                map.resolve("ids_properties/homogeneous_time", Direction::Forward)
                    .unwrap()
                    .outcome,
                Outcome::Refusal(_)
            ),
            "accepted unsupported {field} change"
        );
    }
}

#[test]
#[ignore = "stops and restarts the explicitly named task-owned Neo4j container"]
fn pinned_live_map_survives_graph_shutdown() {
    use crate::conversion::runtime_map::{MapRequest, RuntimeMapCoordinator};
    use std::process::Command;
    let container = std::env::var("IMAS_MVDD_TEST_GRAPH_CONTAINER")
        .expect("name an isolated disposable graph service, never a shared service");
    struct Restart(String);
    impl Drop for Restart {
        fn drop(&mut self) {
            let _ = Command::new("docker").args(["start", &self.0]).status();
        }
    }
    let coordinator = RuntimeMapCoordinator::with_deadline(
        Neo4jFactsSource(live_config()),
        Duration::from_secs(120),
    );
    let request = MapRequest {
        ids: "equilibrium".into(),
        hli_dd: ArtifactDdVersion::new("4.1.1").unwrap(),
        stored_dd: ArtifactDdVersion::new("3.39.0").unwrap(),
    };
    let map = coordinator.acquire(&request).unwrap();
    let retained = Arc::downgrade(&map);
    drop(map);
    let _restart = Restart(container.clone());
    assert!(
        Command::new("docker")
            .args(["stop", &container])
            .status()
            .unwrap()
            .success()
    );
    let cached = coordinator.acquire(&request).unwrap();
    assert!(Arc::ptr_eq(&cached, &retained.upgrade().unwrap()));
    let uncached = MapRequest {
        stored_dd: ArtifactDdVersion::new("3.42.0").unwrap(),
        ..request
    };
    assert!(coordinator.acquire(&uncached).is_err());
    assert!(
        Command::new("docker")
            .args(["start", &container])
            .status()
            .unwrap()
            .success()
    );
    // Service readiness may lag `docker start`; each iteration is an explicit
    // later request, never an automatic retry inside an occurrence open.
    let started = std::time::Instant::now();
    loop {
        if coordinator.acquire(&uncached).is_ok() {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "graph never became ready"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn live_config() -> Neo4jConfig {
    Neo4jConfig {
        uri: std::env::var("NEO4J_URI").expect("graph CI supplies NEO4J_URI"),
        username: std::env::var("NEO4J_USERNAME").expect("graph CI supplies NEO4J_USERNAME"),
        password: std::env::var("NEO4J_PASSWORD").expect("graph CI supplies NEO4J_PASSWORD"),
        database: std::env::var("NEO4J_DATABASE").unwrap_or_else(|_| "neo4j".into()),
        page_size: 256,
        connection_timeout: Duration::from_secs(5),
    }
}

#[test]
fn raw_faults_never_reach_a_complete_map() {
    for fault in [
        "duplicate",
        "short",
        "query",
        "owner",
        "release",
        "event_duplicate",
        "dangling",
    ] {
        let executor = scalar_executor();
        match fault {
            "duplicate" => {
                let mut replies = executor.replies.borrow_mut();
                replies[2].1 = count(2);
                let duplicate = replies[3].1[0].clone();
                replies[3].1.push(duplicate);
            }
            "short" => executor.replies.borrow_mut()[2].1 = count(2),
            "query" => {
                executor.replies.borrow_mut().truncate(3);
            }
            "dangling" => {
                let mut replies = executor.replies.borrow_mut();
                replies[5].1 = count(1);
                replies.push((
                    query_parameters(Some("equilibrium"), 0),
                    vec![row(&[
                        (
                            "from_path",
                            GraphValue::String(
                                "equilibrium/ids_properties/homogeneous_time".into(),
                            ),
                        ),
                        ("to_path", GraphValue::String("other/time".into())),
                    ])],
                ));
            }
            _ => {
                let mut event = raw_event(
                    "data_type",
                    "data_type",
                    GraphValue::String("INT_0D".into()),
                    GraphValue::String("INT_0D".into()),
                );
                if fault == "owner" {
                    event.insert("owners".into(), GraphValue::List(vec![]));
                }
                if fault == "release" {
                    event.insert("releases".into(), GraphValue::List(vec![]));
                }
                add_raw_event(&executor, event);
                if fault == "event_duplicate" {
                    let mut replies = executor.replies.borrow_mut();
                    replies[4].1 = count(2);
                    let duplicate = replies[5].1[0].clone();
                    replies[5].1.push(duplicate);
                }
            }
        }
        assert!(acquire_raw(executor).is_err(), "accepted {fault} evidence");
    }
}

#[test]
fn raw_acquisition_rejects_malformed_metadata_event_values() {
    for (field, old, new) in [("data_type", "INT_0D", "BOGUS"), ("ndim", "0", "255")] {
        let executor = scalar_executor();
        add_raw_event(
            &executor,
            raw_event(
                field,
                field,
                GraphValue::String(old.into()),
                GraphValue::String(new.into()),
            ),
        );
        assert!(
            acquire_raw(executor).is_err(),
            "accepted malformed {field} history"
        );
    }
}

#[test]
fn raw_addition_property_must_agree_with_its_event_history() {
    let executor = scalar_executor();
    add_raw_event(
        &executor,
        raw_event(
            "data_type",
            "data_type",
            GraphValue::String("FLT_0D".into()),
            GraphValue::String("STRUCTURE".into()),
        ),
    );
    assert!(
        acquire_raw(executor).is_err(),
        "discarded a contradictory INT_0D addition anchor"
    );
}

#[test]
fn raw_array_cannot_use_a_coordinate_absent_at_an_endpoint() {
    use crate::conversion::conversion_map::{Direction, Outcome};
    let executor = scalar_executor();
    {
        let mut replies = executor.replies.borrow_mut();
        replies[2].1 = count(2);
        let mut target = replies[3].1[0].clone();
        target.insert("path".into(), GraphValue::String("equilibrium/axis".into()));
        target.insert(
            "introduced".into(),
            GraphValue::List(vec![GraphValue::String("4.1.1".into())]),
        );
        let node = &mut replies[3].1[0];
        node.insert(
            "path".into(),
            GraphValue::String("equilibrium/value".into()),
        );
        node.insert("data_type".into(), GraphValue::String("FLT_1D".into()));
        node.insert("ndim".into(), GraphValue::Integer(1));
        node.insert("timebase".into(), GraphValue::String("time".into()));
        node.insert(
            "coordinate_relationships".into(),
            GraphValue::List(vec![GraphValue::List(vec![
                GraphValue::Integer(1),
                GraphValue::String("equilibrium/axis".into()),
                GraphValue::String("path".into()),
            ])]),
        );
        replies[3].1.push(target);
    }
    let map = acquire_raw(executor).unwrap();
    assert!(matches!(
        map.resolve("value", Direction::Forward).unwrap().outcome,
        Outcome::Refusal(_)
    ));
}

#[test]
fn raw_identifier_enum_change_is_a_local_refusal() {
    use crate::conversion::conversion_map::{Direction, Outcome};
    let executor = scalar_executor();
    add_raw_event(
        &executor,
        raw_event(
            "identifier_enum_name",
            "structure_changed",
            GraphValue::String("old_enum".into()),
            GraphValue::String("new_enum".into()),
        ),
    );
    let map = acquire_raw(executor).unwrap();
    assert!(matches!(
        map.resolve("ids_properties/homogeneous_time", Direction::Forward)
            .unwrap()
            .outcome,
        Outcome::Refusal(_)
    ));
}

#[test]
fn raw_structure_timebase_does_not_hide_a_dangling_coordinate() {
    use crate::conversion::conversion_map::{Direction, Outcome};
    let executor = scalar_executor();
    {
        let mut replies = executor.replies.borrow_mut();
        let node = &mut replies[3].1[0];
        node.insert(
            "data_type".into(),
            GraphValue::String("STRUCT_ARRAY".into()),
        );
        node.insert("ndim".into(), GraphValue::Integer(1));
        node.insert("timebase".into(), GraphValue::String("time".into()));
        node.insert(
            "coordinate_relationships".into(),
            GraphValue::List(vec![GraphValue::List(vec![
                GraphValue::Integer(1),
                GraphValue::String("equilibrium/missing".into()),
                GraphValue::String("path".into()),
            ])]),
        );
    }
    let map = acquire_raw(executor).unwrap();
    assert!(matches!(
        map.resolve("ids_properties/homogeneous_time", Direction::Forward)
            .unwrap()
            .outcome,
        Outcome::Refusal(_)
    ));
}
