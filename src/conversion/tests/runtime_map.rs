use super::*;
use crate::conversion::conversion_map::{Direction, Outcome, RefusalReason, Rel, RuleExplanation};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone)]
struct ControlledSource {
    result: Result<IdsGraphFacts, GraphSourceError>,
}

#[derive(Clone)]
struct GateSource {
    result: Result<IdsGraphFacts, GraphSourceError>,
    loads: Arc<AtomicUsize>,
    started: Arc<(Mutex<bool>, Condvar)>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl GateSource {
    fn new(facts: IdsGraphFacts) -> Self {
        Self {
            result: Ok(facts),
            loads: Arc::new(AtomicUsize::new(0)),
            started: Arc::new((Mutex::new(false), Condvar::new())),
            release: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    fn failing(message: &str) -> Self {
        Self {
            result: Err(GraphSourceError(message.to_string())),
            loads: Arc::new(AtomicUsize::new(0)),
            started: Arc::new((Mutex::new(false), Condvar::new())),
            release: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    fn wait_until_started(&self) {
        let (started, wake) = &*self.started;
        let mut started = started.lock().expect("gate start mutex is not poisoned");
        while !*started {
            started = wake
                .wait(started)
                .expect("gate start mutex is not poisoned");
        }
    }

    fn release(&self) {
        let (release, wake) = &*self.release;
        *release.lock().expect("gate release mutex is not poisoned") = true;
        wake.notify_all();
    }
}

impl GraphFactsSource for GateSource {
    fn load_ids_facts(
        &self,
        _ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        let (started, wake) = &*self.started;
        *started.lock().expect("gate start mutex is not poisoned") = true;
        wake.notify_all();

        let (release, wake) = &*self.release;
        let mut release = release.lock().expect("gate release mutex is not poisoned");
        while !*release {
            release = wake
                .wait(release)
                .expect("gate release mutex is not poisoned");
        }
        self.result.clone()
    }
}

#[derive(Clone)]
struct SequencedSource {
    results: Arc<Mutex<VecDeque<Result<IdsGraphFacts, GraphSourceError>>>>,
    loads: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct ShutdownSource {
    facts: IdsGraphFacts,
    online: Arc<AtomicBool>,
    loads: Arc<AtomicUsize>,
}

impl ShutdownSource {
    fn new(facts: IdsGraphFacts) -> Self {
        Self {
            facts,
            online: Arc::new(AtomicBool::new(true)),
            loads: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn shut_down(&self) {
        self.online.store(false, Ordering::SeqCst);
    }
}

impl GraphFactsSource for ShutdownSource {
    fn load_ids_facts(
        &self,
        _ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        if self.online.load(Ordering::SeqCst) {
            Ok(self.facts.clone())
        } else {
            Err(GraphSourceError("graph shut down".to_string()))
        }
    }
}

impl SequencedSource {
    fn new(results: impl IntoIterator<Item = Result<IdsGraphFacts, GraphSourceError>>) -> Self {
        Self {
            results: Arc::new(Mutex::new(results.into_iter().collect())),
            loads: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl GraphFactsSource for SequencedSource {
    fn load_ids_facts(
        &self,
        _ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        self.results
            .lock()
            .expect("sequence mutex is not poisoned")
            .pop_front()
            .expect("test source has one result per expected load")
    }
}

#[derive(Clone)]
struct ParallelGateSource {
    loads: Arc<AtomicUsize>,
    started: mpsc::Sender<String>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl GraphFactsSource for ParallelGateSource {
    fn load_ids_facts(
        &self,
        ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        self.started
            .send(ids.to_string())
            .expect("test receiver remains available");
        let (release, wake) = &*self.release;
        let mut release = release.lock().expect("parallel gate mutex is not poisoned");
        while !*release {
            release = wake
                .wait(release)
                .expect("parallel gate mutex is not poisoned");
        }
        Ok(complete_identity_scope_for(ids))
    }
}

#[derive(Clone)]
struct FirstCallGateSource {
    facts: IdsGraphFacts,
    loads: Arc<AtomicUsize>,
    started: Arc<(Mutex<bool>, Condvar)>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl FirstCallGateSource {
    fn new(facts: IdsGraphFacts) -> Self {
        Self {
            facts,
            loads: Arc::new(AtomicUsize::new(0)),
            started: Arc::new((Mutex::new(false), Condvar::new())),
            release: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    fn wait_until_started(&self) {
        let (started, wake) = &*self.started;
        let mut started = started
            .lock()
            .expect("first-call start mutex is not poisoned");
        while !*started {
            started = wake
                .wait(started)
                .expect("first-call start mutex is not poisoned");
        }
    }

    fn release_first_call(&self) {
        let (release, wake) = &*self.release;
        *release
            .lock()
            .expect("first-call release mutex is not poisoned") = true;
        wake.notify_all();
    }
}

impl GraphFactsSource for FirstCallGateSource {
    fn load_ids_facts(
        &self,
        _ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        if self.loads.fetch_add(1, Ordering::SeqCst) == 0 {
            let (started, wake) = &*self.started;
            *started
                .lock()
                .expect("first-call start mutex is not poisoned") = true;
            wake.notify_all();
            let (release, wake) = &*self.release;
            let mut release = release
                .lock()
                .expect("first-call release mutex is not poisoned");
            while !*release {
                release = wake
                    .wait(release)
                    .expect("first-call release mutex is not poisoned");
            }
        }
        Ok(self.facts.clone())
    }
}

impl GraphFactsSource for ControlledSource {
    fn load_ids_facts(
        &self,
        _ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        self.result.clone()
    }
}

struct SourceThatExpires {
    clock: Arc<ManualClock>,
}

#[derive(Clone)]
struct SourceEnteringStage {
    stage: AcquisitionStage,
    facts: IdsGraphFacts,
}

impl GraphFactsSource for SourceEnteringStage {
    fn load_ids_facts(
        &self,
        _ids: &str,
        attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        attempt.enter(self.stage).map_err(|expired| {
            GraphSourceError(format!("source timed out during {:?}", expired.stage))
        })?;
        Ok(self.facts.clone())
    }
}

impl GraphFactsSource for SourceThatExpires {
    fn load_ids_facts(
        &self,
        _ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        self.clock.advance(Duration::from_secs(5));
        Err(GraphSourceError(
            "transport ended after its deadline".to_string(),
        ))
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

struct AdvanceAtStage {
    clock: Arc<ManualClock>,
    stage: AcquisitionStage,
}

struct JoinObserver {
    joined: Mutex<bool>,
    wake: Condvar,
}

impl JoinObserver {
    fn new() -> Self {
        Self {
            joined: Mutex::new(false),
            wake: Condvar::new(),
        }
    }

    fn wait_until_joined(&self) {
        let mut joined = self
            .joined
            .lock()
            .expect("join observer mutex is not poisoned");
        while !*joined {
            joined = self
                .wake
                .wait(joined)
                .expect("join observer mutex is not poisoned");
        }
    }
}

impl CoordinatorObserver for JoinObserver {
    fn joined_attempt(&self) {
        *self
            .joined
            .lock()
            .expect("join observer mutex is not poisoned") = true;
        self.wake.notify_all();
    }
}

impl AttemptObserver for AdvanceAtStage {
    fn entered(&self, stage: AcquisitionStage) {
        if stage == self.stage {
            self.clock.advance(Duration::from_secs(5));
        }
    }
}

fn acquirer_expiring_at<S>(source: S, stage: AcquisitionStage) -> RuntimeMapAcquirer<S> {
    let clock = Arc::new(ManualClock::default());
    RuntimeMapAcquirer::with_clock_and_observer(
        source,
        Duration::from_secs(5),
        clock.clone(),
        Arc::new(AdvanceAtStage { clock, stage }),
    )
}

fn version(value: &str, cocos: Option<&str>) -> GraphVersion {
    GraphVersion {
        release: ArtifactDdVersion::new(value).expect("fixture release is valid"),
        cocos: cocos.map(|value| CocosConvention::new(value).expect("fixture COCOS is valid")),
    }
}

fn endpoint(release: &str, kind: GraphNodeKind, data_type: &str, ndim: u8) -> EndpointMetadata {
    EndpointMetadata {
        release: ArtifactDdVersion::new(release).expect("fixture release is valid"),
        kind,
        data_type: data_type.to_string(),
        ndim,
        unit: None,
        timebase_path: None,
        coordinate_paths: Vec::new(),
        cocos_label_transformation: None,
        cocos_transformation_expression: None,
    }
}

fn endpoint_with_unit(
    release: &str,
    kind: GraphNodeKind,
    data_type: &str,
    ndim: u8,
    unit: &str,
) -> EndpointMetadata {
    let mut endpoint = endpoint(release, kind, data_type, ndim);
    endpoint.unit = Some(unit.to_string());
    endpoint
}

fn unit_event(
    path: &str,
    old_value: &str,
    new_value: &str,
    evidence: UnitChangeEvidence,
) -> GraphEvent {
    GraphEvent {
        id: format!("{path}:units:4.1.1"),
        path: path.to_string(),
        release: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
        field: "units".to_string(),
        kind: "units_changed".to_string(),
        old_value: Some(old_value.to_string()),
        new_value: Some(new_value.to_string()),
        unit_change: Some(evidence),
    }
}

fn node(path: &str, left: EndpointMetadata, right: EndpointMetadata) -> GraphNode {
    GraphNode {
        ids: "equilibrium".to_string(),
        path: path.to_string(),
        introduced: vec![ArtifactDdVersion::new("3.39.0").expect("fixture release is valid")],
        removed: Vec::new(),
        endpoints: vec![left, right],
    }
}

fn complete_identity_scope() -> IdsGraphFacts {
    IdsGraphFacts {
        complete: true,
        versions: vec![
            version("3.39.0", Some("11")),
            version("4.0.0", Some("17")),
            version("4.1.1", Some("17")),
        ],
        nodes: vec![
            node(
                "time_slice",
                endpoint("3.39.0", GraphNodeKind::Structure, "STRUCTURE", 0),
                endpoint("4.1.1", GraphNodeKind::Structure, "STRUCTURE", 0),
            ),
            node(
                "ids_properties/version_put/data_dictionary",
                endpoint("3.39.0", GraphNodeKind::Leaf, "STR_0D", 0),
                endpoint("4.1.1", GraphNodeKind::Leaf, "STR_0D", 0),
            ),
            node(
                "time_slice/profiles_1d/rho_tor",
                endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1),
                endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1),
            ),
            node(
                "grids_ggd/grid/space/coordinates_type",
                endpoint("3.39.0", GraphNodeKind::Leaf, "INT_1D", 1),
                endpoint("4.1.1", GraphNodeKind::Leaf, "STRUCT_ARRAY", 1),
            ),
        ],
        events: vec![GraphEvent {
            id: "coordinates_type:data_type:4.1.1".to_string(),
            path: "grids_ggd/grid/space/coordinates_type".to_string(),
            release: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
            field: "data_type".to_string(),
            kind: "structure_changed".to_string(),
            old_value: Some("INT_1D".to_string()),
            new_value: Some("STRUCT_ARRAY".to_string()),
            unit_change: None,
        }],
        successors: Vec::new(),
    }
}

fn complete_identity_scope_for(ids: &str) -> IdsGraphFacts {
    let mut facts = complete_identity_scope();
    for node in &mut facts.nodes {
        node.ids = ids.to_string();
    }
    facts
}

fn request() -> MapRequest {
    MapRequest {
        ids: "equilibrium".to_string(),
        stored_dd: ArtifactDdVersion::new("3.39.0").expect("fixture release is valid"),
        hli_dd: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
    }
}

fn request_for(ids: &str) -> MapRequest {
    MapRequest {
        ids: ids.to_string(),
        ..request()
    }
}

#[test]
fn concurrent_same_key_requests_share_one_construction_and_one_result() {
    let source = GateSource::new(complete_identity_scope());
    let joined = Arc::new(JoinObserver::new());
    let coordinator = Arc::new(RuntimeMapCoordinator::with_observers(
        source.clone(),
        Duration::from_secs(5),
        Arc::new(SystemClock::new()),
        Arc::new(NoopAttemptObserver),
        joined.clone(),
    ));
    let first_coordinator = Arc::clone(&coordinator);
    let first = thread::spawn(move || first_coordinator.acquire(&request()));
    source.wait_until_started();

    let second_coordinator = Arc::clone(&coordinator);
    let second = thread::spawn(move || second_coordinator.acquire(&request()));
    joined.wait_until_joined();
    source.release();

    let first = first
        .join()
        .expect("first requester must not panic")
        .expect("first requester must receive the completed map");
    let second = second
        .join()
        .expect("second requester must not panic")
        .expect("second requester must receive the completed map");

    assert_eq!(source.loads.load(Ordering::SeqCst), 1);
    for map in [first, second] {
        assert!(
            map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
                .is_some()
        );
    }
}

#[test]
fn same_key_joiners_keep_the_original_attempt_deadline() {
    let source = GateSource::new(complete_identity_scope());
    let joined = Arc::new(JoinObserver::new());
    let clock = Arc::new(ManualClock::default());
    let coordinator = Arc::new(RuntimeMapCoordinator::with_observers(
        source.clone(),
        Duration::from_secs(5),
        clock.clone(),
        Arc::new(NoopAttemptObserver),
        joined.clone(),
    ));
    let first_coordinator = Arc::clone(&coordinator);
    let first = thread::spawn(move || first_coordinator.acquire(&request()));
    source.wait_until_started();
    clock.advance(Duration::from_secs(4));

    let second_coordinator = Arc::clone(&coordinator);
    let second = thread::spawn(move || second_coordinator.acquire(&request()));
    joined.wait_until_joined();
    clock.advance(Duration::from_secs(1));
    source.release();

    for result in [
        first.join().expect("first requester must not panic"),
        second.join().expect("second requester must not panic"),
    ] {
        assert!(matches!(
            result,
            Err(AcquisitionFailure::TimedOut {
                stage: AcquisitionStage::Source,
            })
        ));
    }
    assert_eq!(source.loads.load(Ordering::SeqCst), 1);
}

#[test]
fn expiry_publishes_one_terminal_failure_to_the_leader_and_joiner() {
    let source = GateSource::new(complete_identity_scope());
    let joined = Arc::new(JoinObserver::new());
    let clock = Arc::new(ManualClock::default());
    let coordinator = Arc::new(RuntimeMapCoordinator::with_observers(
        source.clone(),
        Duration::from_secs(5),
        clock.clone(),
        Arc::new(NoopAttemptObserver),
        joined.clone(),
    ));
    let first_coordinator = Arc::clone(&coordinator);
    let first = thread::spawn(move || first_coordinator.acquire(&request()));
    source.wait_until_started();
    let second_coordinator = Arc::clone(&coordinator);
    let second = thread::spawn(move || second_coordinator.acquire(&request()));
    joined.wait_until_joined();

    clock.advance(Duration::from_secs(5));
    let expiry = coordinator.acquire(&request());
    source.release();
    for result in [
        expiry,
        first.join().expect("first requester must not panic"),
        second.join().expect("second requester must not panic"),
    ] {
        assert!(matches!(
            result,
            Err(AcquisitionFailure::TimedOut {
                stage: AcquisitionStage::Publication,
            })
        ));
    }
    assert_eq!(source.loads.load(Ordering::SeqCst), 1);
}

#[test]
fn successful_maps_outlive_callers_and_do_not_contact_the_graph_again() {
    let source = ShutdownSource::new(complete_identity_scope());
    let coordinator = RuntimeMapCoordinator::new(source.clone());

    let first = coordinator
        .acquire(&request())
        .expect("first request must acquire the map");
    source.shut_down();
    drop(first);
    let retained = coordinator
        .acquire(&request())
        .expect("retained map must work after graph shutdown");

    assert_eq!(source.loads.load(Ordering::SeqCst), 1);
    assert!(
        retained
            .resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .is_some()
    );
}

#[test]
fn cache_keys_keep_opposite_directions_of_one_ids_separate() {
    let source =
        SequencedSource::new([Ok(complete_identity_scope()), Ok(complete_identity_scope())]);
    let coordinator = RuntimeMapCoordinator::new(source.clone());
    let reverse = MapRequest {
        ids: "equilibrium".to_string(),
        stored_dd: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
        hli_dd: ArtifactDdVersion::new("3.39.0").expect("fixture release is valid"),
    };

    assert!(coordinator.acquire(&request()).is_ok());
    assert!(coordinator.acquire(&reverse).is_ok());
    assert_eq!(source.loads.load(Ordering::SeqCst), 2);
}

#[test]
fn concurrent_joiners_receive_the_same_terminal_failure_and_a_later_request_retries() {
    let source = GateSource::failing("graph unavailable");
    let joined = Arc::new(JoinObserver::new());
    let coordinator = Arc::new(RuntimeMapCoordinator::with_observers(
        source.clone(),
        Duration::from_secs(5),
        Arc::new(SystemClock::new()),
        Arc::new(NoopAttemptObserver),
        joined.clone(),
    ));
    let first_coordinator = Arc::clone(&coordinator);
    let first = thread::spawn(move || first_coordinator.acquire(&request()));
    source.wait_until_started();
    let second_coordinator = Arc::clone(&coordinator);
    let second = thread::spawn(move || second_coordinator.acquire(&request()));
    joined.wait_until_joined();
    source.release();

    for result in [
        first.join().expect("first requester must not panic"),
        second.join().expect("second requester must not panic"),
    ] {
        assert!(matches!(
            result,
            Err(AcquisitionFailure::Source(GraphSourceError(message))) if message == "graph unavailable"
        ));
    }
    assert_eq!(source.loads.load(Ordering::SeqCst), 1);

    let source = SequencedSource::new([
        Err(GraphSourceError("first attempt failed".to_string())),
        Ok(complete_identity_scope()),
    ]);
    let coordinator = RuntimeMapCoordinator::new(source.clone());
    assert!(coordinator.acquire(&request()).is_err());
    assert!(coordinator.acquire(&request()).is_ok());
    assert_eq!(source.loads.load(Ordering::SeqCst), 2);
}

#[test]
fn different_keys_begin_graph_work_independently() {
    let (started, received) = mpsc::channel();
    let source = ParallelGateSource {
        loads: Arc::new(AtomicUsize::new(0)),
        started,
        release: Arc::new((Mutex::new(false), Condvar::new())),
    };
    let release = Arc::clone(&source.release);
    let coordinator = Arc::new(RuntimeMapCoordinator::new(source.clone()));
    let first_coordinator = Arc::clone(&coordinator);
    let first = thread::spawn(move || first_coordinator.acquire(&request_for("equilibrium")));
    let second_coordinator = Arc::clone(&coordinator);
    let second = thread::spawn(move || second_coordinator.acquire(&request_for("pulse_schedule")));

    let first_id = received
        .recv_timeout(Duration::from_secs(1))
        .expect("first key must reach graph work");
    let second_id = received
        .recv_timeout(Duration::from_secs(1))
        .expect("second key must reach graph work without waiting for the first");
    assert_ne!(first_id, second_id);
    let (released, wake) = &*release;
    *released
        .lock()
        .expect("parallel gate mutex is not poisoned") = true;
    wake.notify_all();

    assert!(
        first
            .join()
            .expect("first requester must not panic")
            .is_ok()
    );
    assert!(
        second
            .join()
            .expect("second requester must not panic")
            .is_ok()
    );
    assert_eq!(source.loads.load(Ordering::SeqCst), 2);
}

#[test]
fn an_expired_attempt_cannot_replace_a_later_retry() {
    let source = FirstCallGateSource::new(complete_identity_scope());
    let clock = Arc::new(ManualClock::default());
    let coordinator = Arc::new(RuntimeMapCoordinator::with_clock_and_observer(
        source.clone(),
        Duration::from_secs(5),
        clock.clone(),
        Arc::new(NoopAttemptObserver),
    ));
    let first_coordinator = Arc::clone(&coordinator);
    let first = thread::spawn(move || first_coordinator.acquire(&request()));
    source.wait_until_started();
    clock.advance(Duration::from_secs(5));

    assert!(matches!(
        coordinator.acquire(&request()),
        Err(AcquisitionFailure::TimedOut { .. })
    ));
    let retry = coordinator
        .acquire(&request())
        .expect("later request must start a new attempt after expiry");
    source.release_first_call();
    assert!(matches!(
        first.join().expect("expired requester must not panic"),
        Err(AcquisitionFailure::TimedOut { .. })
    ));
    let retained = coordinator
        .acquire(&request())
        .expect("late completion must not displace the retry result");

    assert_eq!(source.loads.load(Ordering::SeqCst), 2);
    for map in [retry, retained] {
        assert!(
            map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
                .is_some()
        );
    }
}

#[test]
fn acquisition_returns_a_complete_identity_map_and_localized_retype_refusal() {
    let source = ControlledSource {
        result: Ok(complete_identity_scope()),
    };
    let map = RuntimeMapAcquirer::new(source)
        .acquire(&request())
        .expect("complete controlled facts must acquire a map");

    for direction in [Direction::Forward, Direction::Reverse] {
        let identity = map
            .resolve("time_slice/profiles_1d/rho_tor", direction)
            .expect("known endpoint must resolve");
        assert_eq!(identity.rel, Some(Rel::Identical));
        assert!(matches!(
            identity.outcome,
            Outcome::Path { ref resolved_path, .. } if resolved_path == "time_slice/profiles_1d/rho_tor"
        ));

        let refusal = map
            .resolve("grids_ggd/grid/space/coordinates_type", direction)
            .expect("unsupported endpoint must resolve to a local refusal");
        assert_eq!(
            refusal.outcome,
            Outcome::Refusal(RefusalReason::UnservableRetype)
        );
    }

    assert_eq!(
        map.resolve("not/from/the/complete/scope", Direction::Forward),
        None
    );
    assert!(map.delete_target_is_leaf(Direction::Forward, "time_slice/profiles_1d/rho_tor"));
    assert!(!map.delete_target_is_leaf(Direction::Forward, "time_slice"));
}

#[test]
fn acquisition_classifies_unit_evidence_without_conflating_it_with_retypes() {
    let mut facts = complete_identity_scope();
    facts.nodes[2].endpoints = vec![
        endpoint_with_unit("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1, "m"),
        endpoint_with_unit("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1, "metre"),
    ];
    facts.nodes.extend([
        node(
            "time_slice/unit_dimensionally_compatible",
            endpoint_with_unit("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1, "m"),
            endpoint_with_unit("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1, "cm"),
        ),
        node(
            "time_slice/unit_sentinel_resolved",
            endpoint_with_unit("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1, "1"),
            endpoint_with_unit("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1, "dimensionless"),
        ),
        node(
            "time_slice/unit_requires_scale_or_offset",
            endpoint_with_unit("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1, "m"),
            endpoint_with_unit("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1, "cm"),
        ),
    ]);
    facts.nodes.push(GraphNode {
        ids: "equilibrium".to_string(),
        path: "grids_ggd/grid/space/coordinates_type/identifier".to_string(),
        introduced: vec![ArtifactDdVersion::new("4.1.1").expect("fixture release is valid")],
        removed: Vec::new(),
        endpoints: vec![endpoint("4.1.1", GraphNodeKind::Leaf, "STR_0D", 0)],
    });
    facts.events.extend([
        unit_event(
            "time_slice/profiles_1d/rho_tor",
            "m",
            "metre",
            UnitChangeEvidence::Cosmetic,
        ),
        unit_event(
            "time_slice/unit_dimensionally_compatible",
            "m",
            "cm",
            UnitChangeEvidence::DimensionallyCompatible,
        ),
        unit_event(
            "time_slice/unit_sentinel_resolved",
            "1",
            "dimensionless",
            UnitChangeEvidence::SentinelResolved,
        ),
        unit_event(
            "time_slice/unit_requires_scale_or_offset",
            "m",
            "cm",
            UnitChangeEvidence::RequiredScaleOrOffset,
        ),
    ]);

    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("unit uncertainty is local to the affected paths");

    for direction in [Direction::Forward, Direction::Reverse] {
        assert!(matches!(
            map.resolve("time_slice/profiles_1d/rho_tor", direction),
            Some(RuleExplanation {
                outcome: Outcome::Path { .. },
                fidelity: Fidelity::Exact,
                ..
            })
        ));
        assert_eq!(
            map.resolve("time_slice/unit_dimensionally_compatible", direction)
                .expect("the unresolved unit path remains claimed")
                .outcome,
            Outcome::Refusal(RefusalReason::Unmappable)
        );
        assert!(matches!(
            map.resolve("time_slice/unit_sentinel_resolved", direction),
            Some(RuleExplanation {
                outcome: Outcome::Path { .. },
                fidelity: Fidelity::Exact,
                ..
            })
        ));
        assert_eq!(
            map.resolve("time_slice/unit_requires_scale_or_offset", direction)
                .expect("the unsupported unit path remains claimed")
                .outcome,
            Outcome::Refusal(RefusalReason::UnitRedefinition)
        );
        assert_eq!(
            map.resolve("grids_ggd/grid/space/coordinates_type", direction)
                .expect("the reconstructed type change remains claimed")
                .outcome,
            Outcome::Refusal(RefusalReason::UnservableRetype)
        );
        assert_eq!(
            map.resolve(
                "grids_ggd/grid/space/coordinates_type/identifier",
                direction
            )
            .expect("a new descendant inherits its parent's retype refusal")
            .outcome,
            Outcome::Refusal(RefusalReason::UnservableRetype)
        );
    }
}

#[test]
fn acquisition_preserves_a_whole_source_failure() {
    let source = ControlledSource {
        result: Err(GraphSourceError("graph unavailable".to_string())),
    };

    assert!(matches!(
        RuntimeMapAcquirer::new(source).acquire(&request()),
        Err(AcquisitionFailure::Source(GraphSourceError(message))) if message == "graph unavailable"
    ));
}

#[test]
fn an_expired_source_error_is_reported_as_timeout_not_transport_failure() {
    let clock = Arc::new(ManualClock::default());
    let acquirer = RuntimeMapAcquirer::with_clock_and_observer(
        SourceThatExpires {
            clock: clock.clone(),
        },
        Duration::from_secs(5),
        clock,
        Arc::new(NoopAttemptObserver),
    );

    assert!(matches!(
        acquirer.acquire(&request()),
        Err(AcquisitionFailure::TimedOut {
            stage: AcquisitionStage::Source,
        })
    ));
}

#[test]
fn acquisition_times_out_during_each_cooperating_stage() {
    for stage in [
        AcquisitionStage::Source,
        AcquisitionStage::ScopeValidation,
        AcquisitionStage::RuleConstruction,
        AcquisitionStage::MapValidation,
    ] {
        let source = ControlledSource {
            result: Ok(complete_identity_scope()),
        };
        assert!(matches!(
            acquirer_expiring_at(source, stage).acquire(&request()),
            Err(AcquisitionFailure::TimedOut { stage: actual }) if actual == stage
        ));
    }
}

#[test]
fn source_stages_consume_the_same_attempt_and_report_their_own_timeout() {
    for stage in [
        AcquisitionStage::Connection,
        AcquisitionStage::Query,
        AcquisitionStage::Decoding,
    ] {
        assert!(matches!(
            acquirer_expiring_at(
                SourceEnteringStage {
                    stage,
                    facts: complete_identity_scope(),
                },
                stage,
            )
            .acquire(&request()),
            Err(AcquisitionFailure::TimedOut { stage: actual }) if actual == stage
        ));
    }
}

#[test]
fn acquisition_rejects_a_map_that_expires_immediately_before_publication() {
    let source = ControlledSource {
        result: Ok(complete_identity_scope()),
    };

    assert!(matches!(
        acquirer_expiring_at(source, AcquisitionStage::Publication).acquire(&request()),
        Err(AcquisitionFailure::TimedOut {
            stage: AcquisitionStage::Publication,
        })
    ));
}

#[test]
fn acquisition_starts_a_fresh_deadline_for_each_request() {
    let clock = Arc::new(ManualClock::default());
    let acquirer = RuntimeMapAcquirer::with_clock_and_observer(
        ControlledSource {
            result: Ok(complete_identity_scope()),
        },
        Duration::from_secs(5),
        clock.clone(),
        Arc::new(NoopAttemptObserver),
    );

    acquirer
        .acquire(&request())
        .expect("the first attempt must acquire a map");
    clock.advance(Duration::from_secs(10));
    acquirer
        .acquire(&request())
        .expect("a later request must receive its own full deadline");
}

#[test]
fn acquisition_keeps_missing_endpoint_evidence_as_a_local_refusal() {
    let mut facts = complete_identity_scope();
    facts.nodes[2].endpoints.remove(0);
    let source = ControlledSource { result: Ok(facts) };

    let map = RuntimeMapAcquirer::new(source)
        .acquire(&request())
        .expect("independent paths remain usable");
    assert_eq!(
        map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("path is claimed")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
}

#[test]
fn acquisition_replays_a_field_qualified_generic_event() {
    assert!(matches!(
        RuntimeMapAcquirer::new(ControlledSource { result: Ok(complete_identity_scope()) }).acquire(&request()),
        Ok(map) if matches!(map.resolve("grids_ggd/grid/space/coordinates_type", Direction::Forward), Some(explanation) if explanation.outcome == Outcome::Refusal(RefusalReason::UnservableRetype))
    ));
}

#[test]
fn acquisition_keeps_reused_spelling_across_a_reappearance_unmappable() {
    let mut facts = complete_identity_scope();
    facts.events.extend([
        GraphEvent {
            id: "rho_tor:path_removed:4.0.0".to_string(),
            path: "time_slice/profiles_1d/rho_tor".to_string(),
            release: ArtifactDdVersion::new("4.0.0").expect("fixture release is valid"),
            field: "path".to_string(),
            kind: "path_removed".to_string(),
            old_value: None,
            new_value: None,
            unit_change: None,
        },
        GraphEvent {
            id: "rho_tor:path_added:4.1.1".to_string(),
            path: "time_slice/profiles_1d/rho_tor".to_string(),
            release: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
            field: "path".to_string(),
            kind: "path_added".to_string(),
            old_value: None,
            new_value: None,
            unit_change: None,
        },
    ]);
    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("reappearance is a known endpoint");
    assert_eq!(
        map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("reused spelling is claimed")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
}

#[test]
fn acquisition_rejects_a_generic_event_whose_id_disagrees_with_its_field() {
    let mut facts = complete_identity_scope();
    facts.events[0].field = "ndim".to_string();
    assert!(
        matches!(RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) }).acquire(&request()), Err(AcquisitionFailure::InvalidEventValue { id }) if id == "coordinates_type:data_type:4.1.1")
    );
}

#[test]
fn acquisition_requires_each_requested_release_to_be_in_the_catalogue() {
    let mut facts = complete_identity_scope();
    facts
        .versions
        .retain(|version| version.release.to_string() != "4.1.1");
    assert!(
        matches!(RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) }).acquire(&request()), Err(AcquisitionFailure::MissingRequestedRelease { release }) if release.to_string() == "4.1.1")
    );
}

#[test]
fn acquisition_requires_a_complete_scope() {
    let mut facts = complete_identity_scope();
    facts.complete = false;
    let source = ControlledSource { result: Ok(facts) };

    assert!(matches!(
        RuntimeMapAcquirer::new(source).acquire(&request()),
        Err(AcquisitionFailure::IncompleteScope)
    ));
}

#[test]
fn acquisition_refuses_cocos_sensitive_values_without_a_proven_factor() {
    let mut facts = complete_identity_scope();
    facts.nodes[2].endpoints[0].cocos_label_transformation = Some("psi_like".to_string());
    facts.nodes[2].endpoints[1].cocos_label_transformation = Some("psi_like".to_string());
    let source = ControlledSource { result: Ok(facts) };
    let map = RuntimeMapAcquirer::new(source)
        .acquire(&request())
        .expect("a localized value refusal must not invalidate proved paths");

    let explanation = map
        .resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
        .expect("the COCOS-labelled endpoint must be claimed");
    assert_eq!(
        explanation.outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
}
