use super::*;
use crate::conversion::runtime_map::{
    AcquisitionAttempt, AcquisitionClock, AcquisitionStage, AttemptObserver,
};
use std::cell::RefCell;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Row = std::collections::BTreeMap<String, GraphValue>;
type Reply = (std::collections::BTreeMap<String, GraphValue>, Vec<Row>);

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
                    "owner_ids",
                    GraphValue::List(vec![GraphValue::String("equilibrium".to_string())]),
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
fn pinned_graph_returns_a_complete_equilibrium_scope() {
    let config = Neo4jConfig {
        uri: std::env::var("NEO4J_URI").expect("graph CI supplies NEO4J_URI"),
        username: std::env::var("NEO4J_USERNAME").expect("graph CI supplies NEO4J_USERNAME"),
        password: std::env::var("NEO4J_PASSWORD").expect("graph CI supplies NEO4J_PASSWORD"),
        database: "neo4j".to_string(),
        page_size: 256,
        connection_timeout: std::time::Duration::from_secs(5),
    };
    let attempt = AcquisitionAttempt::new(
        Duration::from_secs(30),
        Arc::new(ManualClock::default()),
        Arc::new(NoopObserver),
    );
    let executor =
        BoltExecutor::connect(&config, &attempt).expect("pinned graph accepts Bolt connections");
    let source = Neo4jScopeSource::new(executor, config.page_size).expect("CI page size is valid");
    let scope = source
        .load_raw_scope("equilibrium", &attempt)
        .expect("pinned graph returns every requested scope stream");
    assert!(
        scope
            .versions
            .iter()
            .any(|row| { row.get("release") == Some(&GraphValue::String("3.39.0".to_string())) })
    );
    assert!(
        scope
            .versions
            .iter()
            .any(|row| { row.get("release") == Some(&GraphValue::String("4.1.1".to_string())) })
    );
    assert!(scope.nodes.iter().any(|row| {
        row.get("path")
            == Some(&GraphValue::String(
                "ids_properties/version_put/data_dictionary".to_string(),
            ))
    }));
    assert!(
        !scope.events.is_empty(),
        "live history stream must not be elided"
    );
    assert!(
        !scope.successors.is_empty(),
        "live successor stream must not be elided"
    );
}
