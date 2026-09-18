use super::*;
use crate::conversion::conversion_map::{Direction, Outcome, RefusalReason, Rel, RuleExplanation};
use crate::conversion::conversion_map::{TransformationDirection, ValueTransformation};
use crate::registry::context_registry::REGISTRY;
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
        timebase_path: Some("time".to_string()),
        coordinate_paths: vec!["time".to_string()],
        cocos_label_transformation: None,
        cocos_transformation_expression: None,
        cocos_label_source: None,
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
        coordinate_evidence: None,
    }
}

fn representation_event(
    path: &str,
    field: &str,
    old_value: &str,
    new_value: &str,
    coordinate_evidence: Option<CoordinateChangeEvidence>,
) -> GraphEvent {
    GraphEvent {
        id: format!("{path}:{field}:4.1.1"),
        path: path.to_string(),
        release: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
        field: field.to_string(),
        kind: format!("{field}_changed"),
        old_value: Some(old_value.to_string()),
        new_value: Some(new_value.to_string()),
        unit_change: None,
        coordinate_evidence,
    }
}

fn node(path: &str, left: EndpointMetadata, right: EndpointMetadata) -> GraphNode {
    GraphNode {
        ids: "equilibrium".to_string(),
        path: path.to_string(),
        introduced: vec![ArtifactDdVersion::new("3.39.0").expect("fixture release is valid")],
        removed: Vec::new(),
        rename_declarations: Vec::new(),
        coordinate_relationships: vec![CoordinateRelationship {
            dimension: 0,
            target_path: "time".to_string(),
        }],
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
            node(
                "time",
                endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1),
                endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1),
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
            coordinate_evidence: None,
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

fn direct_rename_facts() -> IdsGraphFacts {
    let mut facts = complete_identity_scope();
    facts.nodes.extend([
        node(
            "time_slice/global_quantities/beta_normal",
            endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1),
            endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1),
        ),
        node(
            "time_slice/global_quantities/beta_tor_norm",
            endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1),
            endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1),
        ),
    ]);

    // The predecessor name is local to the declaring node. Endpoint histories
    // make beta_normal old-only and beta_tor_norm new-only.
    facts.nodes[6].rename_declarations.push(GraphRename {
        release: ArtifactDdVersion::new("4.0.0").expect("fixture release is valid"),
        previous_name: "beta_normal".to_string(),
    });
    facts.nodes[6]
        .endpoints
        .push(endpoint("4.0.0", GraphNodeKind::Leaf, "FLT_1D", 1));
    facts.nodes[5].removed =
        vec![ArtifactDdVersion::new("4.0.0").expect("fixture release is valid")];
    facts.nodes[6].introduced =
        vec![ArtifactDdVersion::new("4.0.0").expect("fixture release is valid")];
    facts.successors.push(GraphSuccessor {
        from_path: "equilibrium/time_slice/global_quantities/beta_normal".to_string(),
        to_path: "equilibrium/time_slice/global_quantities/beta_tor_norm".to_string(),
    });
    facts
}

fn moved_parent_facts() -> IdsGraphFacts {
    let mut facts = complete_identity_scope();
    let release = |value| ArtifactDdVersion::new(value).expect("fixture release is valid");
    let old_parent = "time_slice/legacy/profiles_1d";
    let new_parent = "time_slice/current/profiles_1d";

    let mut old_parent_node = node(
        old_parent,
        endpoint("3.39.0", GraphNodeKind::Structure, "STRUCTURE", 0),
        endpoint("4.1.1", GraphNodeKind::Structure, "STRUCTURE", 0),
    );
    old_parent_node.endpoints.truncate(1);
    old_parent_node.removed = vec![release("4.0.0")];

    let mut new_parent_node = node(
        new_parent,
        endpoint("3.39.0", GraphNodeKind::Structure, "STRUCTURE", 0),
        endpoint("4.1.1", GraphNodeKind::Structure, "STRUCTURE", 0),
    );
    new_parent_node.endpoints.remove(0);
    new_parent_node
        .endpoints
        .push(endpoint("4.0.0", GraphNodeKind::Structure, "STRUCTURE", 0));
    new_parent_node.introduced = vec![release("4.0.0")];
    new_parent_node.rename_declarations = vec![GraphRename {
        release: release("4.0.0"),
        previous_name: "../legacy/profiles_1d".to_string(),
    }];

    let old_gap = format!("{old_parent}/gap");
    let new_gap = format!("{new_parent}/gap");
    let mut old_gap_node = node(
        &old_gap,
        endpoint("3.39.0", GraphNodeKind::Structure, "STRUCTURE", 0),
        endpoint("4.1.1", GraphNodeKind::Structure, "STRUCTURE", 0),
    );
    old_gap_node.endpoints.truncate(1);
    old_gap_node.removed = vec![release("4.0.0")];
    let mut new_gap_node = node(
        &new_gap,
        endpoint("3.39.0", GraphNodeKind::Structure, "STRUCTURE", 0),
        endpoint("4.1.1", GraphNodeKind::Structure, "STRUCTURE", 0),
    );
    new_gap_node.endpoints.remove(0);
    new_gap_node
        .endpoints
        .push(endpoint("4.0.0", GraphNodeKind::Structure, "STRUCTURE", 0));
    new_gap_node.introduced = vec![release("4.0.0")];
    new_gap_node.rename_declarations = vec![GraphRename {
        release: release("4.0.0"),
        previous_name: "../../legacy/profiles_1d/gap".to_string(),
    }];

    let old_r = format!("{old_parent}/gap/r");
    let new_r = format!("{new_parent}/gap/r");
    let mut old_r_node = node(
        &old_r,
        endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1),
        endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1),
    );
    old_r_node.endpoints.truncate(1);
    old_r_node.removed = vec![release("4.0.0")];
    let mut new_r_node = node(
        &new_r,
        endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1),
        endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1),
    );
    new_r_node.endpoints.remove(0);
    new_r_node
        .endpoints
        .push(endpoint("4.0.0", GraphNodeKind::Leaf, "FLT_1D", 1));
    new_r_node.introduced = vec![release("4.0.0")];
    new_r_node.rename_declarations = vec![GraphRename {
        release: release("4.0.0"),
        previous_name: "../../../legacy/profiles_1d/gap/r".to_string(),
    }];

    let old_identifier = format!("{old_parent}/gap/identifier");
    let mut old_identifier_node = node(
        &old_identifier,
        endpoint("3.39.0", GraphNodeKind::Leaf, "STR_0D", 0),
        endpoint("4.1.1", GraphNodeKind::Leaf, "STR_0D", 0),
    );
    old_identifier_node.endpoints.truncate(1);
    old_identifier_node.removed = vec![release("4.0.0")];

    let old_escaping = format!("{old_parent}/escaped");
    let new_escaping = "time_slice/outside/escaped";
    let mut old_escaping_node = node(
        &old_escaping,
        endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1),
        endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1),
    );
    old_escaping_node.endpoints.truncate(1);
    old_escaping_node.removed = vec![release("4.0.0")];
    let mut new_escaping_node = node(
        new_escaping,
        endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1),
        endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1),
    );
    new_escaping_node.endpoints.remove(0);
    new_escaping_node
        .endpoints
        .push(endpoint("4.0.0", GraphNodeKind::Leaf, "FLT_1D", 1));
    new_escaping_node.introduced = vec![release("4.0.0")];
    new_escaping_node.rename_declarations = vec![GraphRename {
        release: release("4.0.0"),
        previous_name: "../legacy/profiles_1d/escaped".to_string(),
    }];

    let mut escaped_root_node = node(
        "time_slice/current/profiles_1d/unsafe",
        endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1),
        endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1),
    );
    escaped_root_node.endpoints.remove(0);
    escaped_root_node
        .endpoints
        .push(endpoint("4.0.0", GraphNodeKind::Leaf, "FLT_1D", 1));
    escaped_root_node.introduced = vec![release("4.0.0")];
    escaped_root_node.rename_declarations = vec![GraphRename {
        release: release("4.0.0"),
        previous_name: "../../../../outside".to_string(),
    }];

    facts.nodes.extend([
        old_parent_node,
        new_parent_node,
        old_gap_node,
        new_gap_node,
        old_r_node,
        new_r_node,
        old_identifier_node,
        old_escaping_node,
        new_escaping_node,
        escaped_root_node,
    ]);
    facts.successors.extend([
        GraphSuccessor {
            from_path: old_parent.to_string(),
            to_path: new_parent.to_string(),
        },
        GraphSuccessor {
            from_path: old_r,
            to_path: new_r,
        },
        GraphSuccessor {
            from_path: old_gap,
            to_path: new_gap,
        },
        GraphSuccessor {
            from_path: old_escaping,
            to_path: new_escaping.to_string(),
        },
    ]);
    facts
}

fn historical_node(
    path: &str,
    introduced: &str,
    removed: Option<&str>,
    kind: GraphNodeKind,
) -> GraphNode {
    GraphNode {
        ids: "pulse_schedule".to_string(),
        path: path.to_string(),
        introduced: vec![ArtifactDdVersion::new(introduced).expect("fixture release is valid")],
        removed: removed
            .into_iter()
            .map(|release| ArtifactDdVersion::new(release).expect("fixture release is valid"))
            .collect(),
        rename_declarations: Vec::new(),
        coordinate_relationships: vec![CoordinateRelationship {
            dimension: 0,
            target_path: "time".to_string(),
        }],
        endpoints: vec![endpoint(introduced, kind, "FLT_1D", 1)],
    }
}

fn pulse_schedule_historical_facts() -> IdsGraphFacts {
    let mut beam = historical_node("ec/beam", "3.40.0", None, GraphNodeKind::Structure);
    beam.rename_declarations = vec![
        GraphRename {
            release: ArtifactDdVersion::new("3.26.0").expect("fixture release is valid"),
            previous_name: "antenna".to_string(),
        },
        GraphRename {
            release: ArtifactDdVersion::new("3.40.0").expect("fixture release is valid"),
            previous_name: "launcher".to_string(),
        },
    ];
    let mut steering = historical_node(
        "ec/beam/steering_angle_pol",
        "3.40.0",
        None,
        GraphNodeKind::Leaf,
    );
    steering.rename_declarations = vec![GraphRename {
        release: ArtifactDdVersion::new("3.26.0").expect("fixture release is valid"),
        previous_name: "launching_angle_pol".to_string(),
    }];

    IdsGraphFacts {
        complete: true,
        versions: ["3.22.0", "3.25.0", "3.26.0", "3.30.0", "3.40.0"]
            .into_iter()
            .map(|release| version(release, None))
            .collect(),
        nodes: vec![
            historical_node(
                "ec/antenna",
                "3.22.0",
                Some("3.26.0"),
                GraphNodeKind::Structure,
            ),
            historical_node(
                "ec/launcher",
                "3.26.0",
                Some("3.40.0"),
                GraphNodeKind::Structure,
            ),
            beam,
            historical_node(
                "ec/antenna/launching_angle_pol",
                "3.22.0",
                Some("3.26.0"),
                GraphNodeKind::Leaf,
            ),
            historical_node(
                "ec/launcher/steering_angle_pol",
                "3.26.0",
                Some("3.40.0"),
                GraphNodeKind::Leaf,
            ),
            steering,
        ],
        events: Vec::new(),
        successors: vec![
            GraphSuccessor {
                from_path: "ec/antenna".to_string(),
                to_path: "ec/beam".to_string(),
            },
            GraphSuccessor {
                from_path: "ec/launcher".to_string(),
                to_path: "ec/beam".to_string(),
            },
            GraphSuccessor {
                from_path: "ec/antenna/launching_angle_pol".to_string(),
                to_path: "ec/beam/steering_angle_pol".to_string(),
            },
            GraphSuccessor {
                from_path: "ec/launcher/steering_angle_pol".to_string(),
                to_path: "ec/beam/steering_angle_pol".to_string(),
            },
        ],
    }
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

fn historical_request(stored_dd: &str, hli_dd: &str) -> MapRequest {
    MapRequest {
        ids: "pulse_schedule".to_string(),
        stored_dd: ArtifactDdVersion::new(stored_dd).expect("fixture release is valid"),
        hli_dd: ArtifactDdVersion::new(hli_dd).expect("fixture release is valid"),
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
fn registry_access_remains_available_while_a_map_acquisition_waits() {
    let source = GateSource::new(complete_identity_scope());
    let joined = Arc::new(JoinObserver::new());
    let coordinator = Arc::new(RuntimeMapCoordinator::with_observers(
        source.clone(),
        Duration::from_secs(5),
        Arc::new(SystemClock::new()),
        Arc::new(NoopAttemptObserver),
        joined.clone(),
    ));
    let leader_coordinator = Arc::clone(&coordinator);
    let leader = thread::spawn(move || leader_coordinator.acquire(&request()));
    source.wait_until_started();

    let joiner_coordinator = Arc::clone(&coordinator);
    let joiner = thread::spawn(move || joiner_coordinator.acquire(&request()));
    joined.wait_until_joined();

    assert!(REGISTRY.lookup(i32::MIN).is_none());

    source.release();
    assert!(leader.join().expect("leader must not panic").is_ok());
    assert!(joiner.join().expect("joiner must not panic").is_ok());
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
        rename_declarations: Vec::new(),
        coordinate_relationships: vec![CoordinateRelationship {
            dimension: 0,
            target_path: "time".to_string(),
        }],
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
fn acquisition_interprets_coordinate_and_timebase_evidence_without_guessing() {
    let mut facts = complete_identity_scope();
    let rho_tor = &mut facts.nodes[2];
    for endpoint in &mut rho_tor.endpoints {
        endpoint.coordinate_paths = vec!["time_slice/profiles_1d/rho_tor_norm".to_string()];
        endpoint.timebase_path = Some("time".to_string());
    }
    facts.nodes.push(node(
        "time_slice/profiles_1d/rho_tor_norm",
        endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1),
        endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1),
    ));
    facts.events.extend([
        representation_event(
            "time_slice/profiles_1d/rho_tor",
            "coordinates",
            "['time_slice/profiles_1d/rho_tor_norm']",
            "['time_slice/profiles_1d/rho_tor_norm']",
            Some(CoordinateChangeEvidence::Equivalent),
        ),
        representation_event(
            "time_slice/profiles_1d/rho_tor",
            "timebase",
            "time",
            "time",
            Some(CoordinateChangeEvidence::Equivalent),
        ),
    ]);

    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("established matching coordinate and timebase evidence must serve the path");
    assert!(matches!(
        map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("the proved endpoint is claimed")
            .outcome,
        Outcome::Path {
            value_transformation: ValueTransformation::None,
            ..
        }
    ));
}

#[test]
fn acquisition_uses_an_established_path_correspondence_for_coordinate_equivalence() {
    let mut facts = direct_rename_facts();
    for endpoint in &mut facts.nodes[2].endpoints {
        endpoint.timebase_path = Some("time".to_string());
    }
    facts.nodes[2].endpoints[0].coordinate_paths =
        vec!["time_slice/global_quantities/beta_normal".to_string()];
    facts.nodes[2].endpoints[1].coordinate_paths =
        vec!["time_slice/global_quantities/beta_tor_norm".to_string()];
    facts.events.push(representation_event(
        "time_slice/profiles_1d/rho_tor",
        "coordinates",
        "['time_slice/global_quantities/beta_normal']",
        "['time_slice/global_quantities/beta_tor_norm']",
        Some(CoordinateChangeEvidence::Equivalent),
    ));

    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("the direct coordinate correspondence is established in this direction");
    assert!(matches!(
        map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("the proved endpoint is claimed")
            .outcome,
        Outcome::Path {
            value_transformation: ValueTransformation::None,
            ..
        }
    ));
}

#[test]
fn acquisition_does_not_treat_empty_or_unequal_coordinate_sets_as_a_conversion_verdict() {
    let mut empty_facts = complete_identity_scope();
    empty_facts.nodes[2].endpoints[0].coordinate_paths.clear();
    empty_facts.nodes[2].endpoints[1].coordinate_paths.clear();
    let empty = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(empty_facts),
    })
    .acquire(&request())
    .expect("unknown coordinate evidence stays localized");
    assert_eq!(
        empty
            .resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("the endpoint remains claimed")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );

    let mut unequal_facts = complete_identity_scope();
    unequal_facts.nodes[2].endpoints[0].coordinate_paths = vec!["rho_old".to_string()];
    unequal_facts.nodes[2].endpoints[1].coordinate_paths = vec!["rho_new".to_string()];
    unequal_facts.events.push(representation_event(
        "time_slice/profiles_1d/rho_tor",
        "coordinates",
        "['rho_old']",
        "['rho_new']",
        None,
    ));
    let unequal = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(unequal_facts),
    })
    .acquire(&request())
    .expect("unequal lists alone do not establish resampling");
    assert_eq!(
        unequal
            .resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("the endpoint remains claimed")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
}

#[test]
fn acquisition_requires_a_complete_relationship_or_producer_verdict_for_raw_coordinate_matches() {
    let mut facts = complete_identity_scope();
    facts.nodes[2].coordinate_relationships.clear();
    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("unconfirmed coordinate evidence remains localized");

    let explanation = map
        .resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
        .expect("the endpoint remains claimed");
    assert_eq!(
        explanation.outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
    assert_eq!(
        explanation.rule_id.as_deref(),
        Some("coordinate-unresolved:time_slice/profiles_1d/rho_tor")
    );
}

#[test]
fn acquisition_rejects_duplicate_and_localizes_dangling_coordinate_relationships() {
    let mut duplicate = complete_identity_scope();
    duplicate.nodes[2]
        .coordinate_relationships
        .push(CoordinateRelationship {
            dimension: 0,
            target_path: "time".to_string(),
        });
    assert!(matches!(
        RuntimeMapAcquirer::new(ControlledSource {
            result: Ok(duplicate),
        })
        .acquire(&request()),
        Err(AcquisitionFailure::InvalidNode { reason, .. }) if reason == "coordinate relationships repeat a dimension"
    ));

    let mut dangling = complete_identity_scope();
    dangling.nodes[2].coordinate_relationships[0].target_path = "missing_coordinate".to_string();
    let map = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(dangling),
    })
    .acquire(&request())
    .expect("a coordinate-spec or unavailable target is bounded to this endpoint");
    assert_eq!(
        map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("the endpoint remains claimed")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
}

#[test]
fn acquisition_refuses_known_resampling_and_fails_unbounded_coordinate_scope() {
    let mut resampling_facts = complete_identity_scope();
    resampling_facts.nodes[2].endpoints[1].timebase_path = Some("rho_tor_time".to_string());
    resampling_facts.events.push(representation_event(
        "time_slice/profiles_1d/rho_tor",
        "timebase",
        "time",
        "rho_tor_time",
        Some(CoordinateChangeEvidence::RequiresResampling),
    ));
    let resampling = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(resampling_facts),
    })
    .acquire(&request())
    .expect("a path-local unsupported resampling remains a usable complete map");
    assert_eq!(
        resampling
            .resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("the endpoint remains claimed")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
    assert_eq!(
        resampling
            .resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("the endpoint retains its evidence cause")
            .rule_id
            .as_deref(),
        Some("coordinate-resampling:time_slice/profiles_1d/rho_tor")
    );

    let mut unbounded_facts = complete_identity_scope();
    unbounded_facts.events.push(representation_event(
        "time_slice/profiles_1d/rho_tor",
        "coordinates",
        "[]",
        "[]",
        Some(CoordinateChangeEvidence::UnboundedScope),
    ));
    assert!(matches!(
        RuntimeMapAcquirer::new(ControlledSource {
            result: Ok(unbounded_facts),
        })
        .acquire(&request()),
        Err(AcquisitionFailure::UnboundedCoordinateScope { .. })
    ));
}

#[test]
fn acquisition_ignores_coordinate_events_outside_the_requested_release_pair() {
    let mut facts = complete_identity_scope();
    facts.versions.push(version("5.0.0", Some("17")));
    facts.events.push(GraphEvent {
        id: "rho_tor:coordinates:5.0.0".to_string(),
        path: "time_slice/profiles_1d/rho_tor".to_string(),
        release: ArtifactDdVersion::new("5.0.0").expect("fixture release is valid"),
        field: "coordinates".to_string(),
        kind: "coordinates_changed".to_string(),
        old_value: Some("['time']".to_string()),
        new_value: Some("['time']".to_string()),
        unit_change: None,
        coordinate_evidence: Some(CoordinateChangeEvidence::UnboundedScope),
    });

    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("a later unbounded finding cannot invalidate this release pair");
    assert!(matches!(
        map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("the independently proved endpoint is claimed")
            .outcome,
        Outcome::Path { .. }
    ));
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
            coordinate_evidence: None,
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
            coordinate_evidence: None,
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

#[test]
fn acquisition_emits_an_evidenced_direct_rename_in_both_directions() {
    let facts = direct_rename_facts();

    let mut shuffled = facts.clone();
    shuffled.versions.reverse();
    shuffled.nodes.reverse();
    shuffled.successors.reverse();

    let forward = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(facts.clone()),
    })
    .acquire(&request())
    .expect("the direct rename has complete endpoint evidence");
    let shuffled_forward = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(shuffled),
    })
    .acquire(&request())
    .expect("input ordering cannot alter direct-rename resolution");
    let reverse = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&MapRequest {
            ids: "equilibrium".to_string(),
            stored_dd: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
            hli_dd: ArtifactDdVersion::new("3.39.0").expect("fixture release is valid"),
        })
        .expect("the inverse request uses the same evidenced relation");

    for (map, direction, requested, expected) in [
        (
            &forward,
            Direction::Forward,
            "time_slice/global_quantities/beta_tor_norm",
            "time_slice/global_quantities/beta_normal",
        ),
        (
            &reverse,
            Direction::Forward,
            "time_slice/global_quantities/beta_normal",
            "time_slice/global_quantities/beta_tor_norm",
        ),
    ] {
        let explanation = map
            .resolve(requested, direction)
            .expect("the caller path must be claimed");
        assert_eq!(explanation.rel, Some(Rel::Renamed));
        assert!(matches!(
            explanation.outcome,
            Outcome::Path { ref resolved_path, .. } if resolved_path == expected
        ));
    }
    assert_eq!(
        forward.resolve(
            "time_slice/global_quantities/beta_tor_norm",
            Direction::Forward,
        ),
        shuffled_forward.resolve(
            "time_slice/global_quantities/beta_tor_norm",
            Direction::Forward,
        )
    );
    assert_eq!(
        forward.resolve("not/from/the/complete/scope", Direction::Forward),
        None
    );
}

#[test]
fn acquisition_relates_dated_historical_endpoints_without_promoting_witnesses() {
    let facts = pulse_schedule_historical_facts();
    let newer_hli = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(facts.clone()),
    })
    .acquire(&historical_request("3.25.0", "3.30.0"))
    .expect("dated antenna and launcher roles establish a complete historical map");
    let older_hli = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&historical_request("3.30.0", "3.25.0"))
        .expect("the inverse request uses the same historical roles");

    for (map, parent, child, expected_parent, expected_child) in [
        (
            &newer_hli,
            "ec/launcher",
            "ec/launcher/steering_angle_pol",
            "ec/antenna",
            "ec/antenna/launching_angle_pol",
        ),
        (
            &older_hli,
            "ec/antenna",
            "ec/antenna/launching_angle_pol",
            "ec/launcher",
            "ec/launcher/steering_angle_pol",
        ),
    ] {
        assert!(matches!(
            map.resolve(parent, Direction::Forward),
            Some(RuleExplanation {
                rel: Some(Rel::Renamed),
                outcome: Outcome::Path { ref resolved_path, .. },
                ..
            }) if resolved_path == expected_parent
        ));
        assert!(matches!(
            map.resolve(child, Direction::Forward),
            Some(RuleExplanation {
                rel: Some(Rel::Renamed),
                outcome: Outcome::Path { ref resolved_path, .. },
                ..
            }) if resolved_path == expected_child
        ));
        assert_eq!(map.resolve("ec/beam", Direction::Forward), None);
        assert_eq!(
            map.resolve("ec/beam/steering_angle_pol", Direction::Forward),
            None
        );
    }

    let mut shuffled = pulse_schedule_historical_facts();
    shuffled.versions.reverse();
    shuffled.nodes.reverse();
    shuffled.successors.reverse();
    shuffled
        .nodes
        .iter_mut()
        .for_each(|node| node.rename_declarations.reverse());
    let shuffled = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(shuffled),
    })
    .acquire(&historical_request("3.25.0", "3.30.0"))
    .expect("row order cannot change dated endpoint roles");
    for path in ["ec/launcher", "ec/launcher/steering_angle_pol"] {
        assert_eq!(
            newer_hli.resolve(path, Direction::Forward),
            shuffled.resolve(path, Direction::Forward)
        );
    }
}

#[test]
fn acquisition_localizes_unreliable_historical_roles() {
    let assert_unmappable = |facts: IdsGraphFacts| {
        let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
            .acquire(&historical_request("3.25.0", "3.30.0"))
            .expect("an ambiguous local role must not invalidate independent paths");
        assert_eq!(
            map.resolve("ec/launcher", Direction::Forward)
                .expect("the later endpoint remains claimed")
                .outcome,
            Outcome::Refusal(RefusalReason::Unmappable)
        );
    };

    let mut conflicting_date = pulse_schedule_historical_facts();
    conflicting_date.nodes[2]
        .rename_declarations
        .push(GraphRename {
            release: ArtifactDdVersion::new("3.26.0").expect("fixture release is valid"),
            previous_name: "not_antenna".to_string(),
        });
    assert_unmappable(conflicting_date);

    let mut self_referential_role = pulse_schedule_historical_facts();
    self_referential_role.nodes[2].rename_declarations[0].previous_name = "beam".to_string();
    assert_unmappable(self_referential_role);

    let mut successor_cycle = pulse_schedule_historical_facts();
    successor_cycle.successors.push(GraphSuccessor {
        from_path: "ec/beam".to_string(),
        to_path: "ec/antenna".to_string(),
    });
    let map = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(successor_cycle),
    })
    .acquire(&historical_request("3.25.0", "3.30.0"))
    .expect("a local successor cycle must not invalidate independent paths");
    for path in ["ec/launcher", "ec/launcher/steering_angle_pol"] {
        assert_eq!(
            map.resolve(path, Direction::Forward)
                .expect("each role depending on the cycle remains claimed")
                .outcome,
            Outcome::Refusal(RefusalReason::Unmappable)
        );
    }

    let mut reused_spelling = pulse_schedule_historical_facts();
    reused_spelling.nodes[0]
        .introduced
        .push(ArtifactDdVersion::new("3.30.0").expect("fixture release is valid"));
    assert_unmappable(reused_spelling);

    let mut missing_interval_anchor = pulse_schedule_historical_facts();
    missing_interval_anchor.nodes[0].endpoints.clear();
    assert_unmappable(missing_interval_anchor);

    let mut ignored_earlier_conflict = pulse_schedule_historical_facts();
    ignored_earlier_conflict.nodes[2]
        .rename_declarations
        .push(GraphRename {
            release: ArtifactDdVersion::new("3.26.0").expect("fixture release is valid"),
            previous_name: "not_antenna".to_string(),
        });
    let map = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(ignored_earlier_conflict),
    })
    .acquire(&historical_request("3.30.0", "3.40.0"))
    .expect("a local history conflict must not invalidate the complete map");
    assert_eq!(
        map.resolve("ec/beam", Direction::Forward)
            .expect("the later endpoint remains claimed")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
}

#[test]
fn acquisition_moves_a_parent_without_hiding_its_child_exceptions() {
    let facts = moved_parent_facts();
    let forward = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(facts.clone()),
    })
    .acquire(&request())
    .expect("the moved parent has complete endpoint evidence");
    let reverse = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&MapRequest {
            ids: "equilibrium".to_string(),
            stored_dd: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
            hli_dd: ArtifactDdVersion::new("3.39.0").expect("fixture release is valid"),
        })
        .expect("the inverse request keeps the same child evidence");

    let parent = forward
        .resolve("time_slice/current/profiles_1d", Direction::Forward)
        .expect("the moved parent must be claimed");
    assert_eq!(parent.rel, Some(Rel::Moved));
    assert!(matches!(
        parent.outcome,
        Outcome::Path { ref resolved_path, .. }
            if resolved_path == "time_slice/legacy/profiles_1d"
    ));

    let surviving_child = forward
        .resolve("time_slice/current/profiles_1d/gap/r", Direction::Forward)
        .expect("the surviving child must be claimed");
    assert_eq!(surviving_child.fidelity, Fidelity::Exact);
    assert!(matches!(
        surviving_child.outcome,
        Outcome::Path { ref resolved_path, .. }
            if resolved_path == "time_slice/legacy/profiles_1d/gap/r"
    ));

    assert_eq!(
        reverse
            .resolve(
                "time_slice/legacy/profiles_1d/gap/identifier",
                Direction::Forward
            )
            .expect("the missing child must be claimed rather than inherited")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );

    assert!(matches!(
        reverse
            .resolve("time_slice/legacy/profiles_1d/escaped", Direction::Forward)
            .expect("the escaping child must be claimed")
            .outcome,
        Outcome::Path { ref resolved_path, .. }
            if resolved_path == "time_slice/outside/escaped"
    ));

    assert_eq!(
        forward
            .resolve("time_slice/current/profiles_1d/unsafe", Direction::Forward)
            .expect("an IDS-root escape remains explicitly unresolved")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
    assert_eq!(
        forward.resolve(
            "time_slice/current/profiles_1d/unrecorded_descendant",
            Direction::Forward,
        ),
        None,
        "a parent move cannot manufacture an unrecorded descendant"
    );
}

#[test]
fn acquisition_keeps_an_uncorroborated_or_scientifically_unproven_rename_unmappable() {
    let mut facts = direct_rename_facts();
    facts.successors.clear();

    let uncorroborated = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(facts.clone()),
    })
    .acquire(&request())
    .expect("a missing witness localizes to the named endpoints");
    assert_eq!(
        uncorroborated
            .resolve(
                "time_slice/global_quantities/beta_tor_norm",
                Direction::Forward,
            )
            .expect("the caller path must be traced")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );

    facts.successors.push(GraphSuccessor {
        from_path: "time_slice/global_quantities/beta_normal".to_string(),
        to_path: "time_slice/global_quantities/beta_tor_norm".to_string(),
    });
    facts.nodes[5].endpoints[0].cocos_label_transformation = Some("psi_like".to_string());
    let scientifically_unproven = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("a missing value proof localizes to the named endpoints");
    assert_eq!(
        scientifically_unproven
            .resolve(
                "time_slice/global_quantities/beta_tor_norm",
                Direction::Forward,
            )
            .expect("the caller path must be traced")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
}

#[test]
fn acquisition_leaves_a_coexisting_rename_declaration_as_two_endpoint_rules() {
    let mut facts = direct_rename_facts();
    facts.nodes[5].removed.clear();
    facts.nodes[6].introduced =
        vec![ArtifactDdVersion::new("3.39.0").expect("fixture release is valid")];
    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("coexistence has two independently established endpoint spellings");

    let explanation = map
        .resolve(
            "time_slice/global_quantities/beta_tor_norm",
            Direction::Forward,
        )
        .expect("the coexisting caller path must remain claimed");
    assert_eq!(explanation.rel, Some(Rel::Identical));
    assert!(matches!(
    explanation.outcome,
    Outcome::Path { ref resolved_path, .. }
        if resolved_path == "time_slice/global_quantities/beta_tor_norm"
    ));
}

#[test]
fn acquisition_traces_missing_or_conflicting_direct_predecessors_as_refusals() {
    let mut missing = direct_rename_facts();
    missing.nodes[6].rename_declarations[0].previous_name = "not_beta_normal".to_string();
    let missing = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(missing),
    })
    .acquire(&request())
    .expect("a missing predecessor localizes to the declared newer endpoint");
    assert_eq!(
        missing
            .resolve(
                "time_slice/global_quantities/beta_tor_norm",
                Direction::Forward,
            )
            .expect("the caller path must be traced")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );

    let mut conflicting = direct_rename_facts();
    let mut rival = node(
        "time_slice/global_quantities/beta_other",
        endpoint("3.39.0", GraphNodeKind::Leaf, "FLT_1D", 1),
        endpoint("4.1.1", GraphNodeKind::Leaf, "FLT_1D", 1),
    );
    rival.introduced = vec![ArtifactDdVersion::new("4.0.0").expect("fixture release is valid")];
    rival
        .endpoints
        .push(endpoint("4.0.0", GraphNodeKind::Leaf, "FLT_1D", 1));
    rival.rename_declarations.push(GraphRename {
        release: ArtifactDdVersion::new("4.0.0").expect("fixture release is valid"),
        previous_name: "beta_normal".to_string(),
    });
    conflicting.nodes.push(rival);
    conflicting.successors.push(GraphSuccessor {
        from_path: "time_slice/global_quantities/beta_normal".to_string(),
        to_path: "time_slice/global_quantities/beta_other".to_string(),
    });
    let conflicting = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(conflicting),
    })
    .acquire(&request())
    .expect("a conflicting predecessor localizes to each newer endpoint");
    assert_eq!(
        conflicting
            .resolve(
                "time_slice/global_quantities/beta_tor_norm",
                Direction::Forward,
            )
            .expect("the caller path must be traced")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
}

#[test]
fn acquisition_derives_one_psi_sign_flip_from_endpoint_conventions() {
    let mut facts = complete_identity_scope();
    for endpoint in &mut facts.nodes[2].endpoints {
        endpoint.cocos_label_transformation = Some("psi_like".to_string());
        endpoint.cocos_label_source = Some(CocosLabelSource::InferredSignFlip);
    }
    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("the supported psi factor must construct a map");

    for direction in [Direction::Forward, Direction::Reverse] {
        assert!(matches!(
            map.resolve("time_slice/profiles_1d/rho_tor", direction)
                .expect("the COCOS-labelled endpoint must be claimed")
                .outcome,
            Outcome::Path {
                value_transformation: ValueTransformation::SignFlip {
                    direction: TransformationDirection::ToHli,
                    ..
                },
                ..
            }
        ));
    }
}

#[test]
fn acquisition_deduplicates_corroborating_cocos_evidence() {
    let mut facts = complete_identity_scope();
    for endpoint in &mut facts.nodes[2].endpoints {
        endpoint.cocos_label_transformation = Some("psi_like".to_string());
        endpoint.cocos_label_source = Some(CocosLabelSource::InferredSignFlip);
    }
    facts.events.extend([
        GraphEvent {
            id: "rho_tor:cocos_label_transformation:4.0.0".to_string(),
            path: "time_slice/profiles_1d/rho_tor".to_string(),
            release: ArtifactDdVersion::new("4.0.0").expect("fixture release is valid"),
            field: "cocos_label_transformation".to_string(),
            kind: "metadata_changed".to_string(),
            old_value: Some("psi".to_string()),
            new_value: Some(String::new()),
            unit_change: None,
            coordinate_evidence: None,
        },
        GraphEvent {
            id: "rho_tor:documentation:4.0.0".to_string(),
            path: "time_slice/profiles_1d/rho_tor".to_string(),
            release: ArtifactDdVersion::new("4.0.0").expect("fixture release is valid"),
            field: "documentation".to_string(),
            kind: "metadata_changed".to_string(),
            old_value: Some("poloidal flux".to_string()),
            new_value: Some("COCOS convention changed".to_string()),
            unit_change: None,
            coordinate_evidence: None,
        },
    ]);
    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("corroborating history must not add a second factor");

    assert!(matches!(
        map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("the path remains covered by its one sign flip")
            .outcome,
        Outcome::Path {
            value_transformation: ValueTransformation::SignFlip { .. },
            ..
        }
    ));
}

#[test]
fn acquisition_refuses_a_conflicting_raw_cocos_label_replacement() {
    let mut facts = complete_identity_scope();
    for endpoint in &mut facts.nodes[2].endpoints {
        endpoint.cocos_label_transformation = Some("psi_like".to_string());
        endpoint.cocos_label_source = Some(CocosLabelSource::InferredSignFlip);
    }
    facts.events.push(GraphEvent {
        id: "rho_tor:cocos_label_transformation:4.0.0".to_string(),
        path: "time_slice/profiles_1d/rho_tor".to_string(),
        release: ArtifactDdVersion::new("4.0.0").expect("fixture release is valid"),
        field: "cocos_label_transformation".to_string(),
        kind: "metadata_changed".to_string(),
        old_value: Some("psi".to_string()),
        new_value: Some("phi".to_string()),
        unit_change: None,
        coordinate_evidence: None,
    });
    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("a path-local scientific conflict must not reject other paths");

    assert_eq!(
        map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("the conflicting endpoint remains explicitly covered")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
}

#[test]
fn acquisition_localizes_unservable_or_out_of_pair_cocos_evidence() {
    let mutations: [fn(&mut IdsGraphFacts); 4] = [
        |facts| {
            for endpoint in &mut facts.nodes[2].endpoints {
                endpoint.cocos_label_transformation = Some("unknown_like".to_string());
                endpoint.cocos_label_source = Some(CocosLabelSource::Xml);
            }
        },
        |facts| {
            for endpoint in &mut facts.nodes[2].endpoints {
                endpoint.cocos_label_transformation = Some("psi_like".to_string());
                endpoint.cocos_label_source = Some(CocosLabelSource::InferredExpression);
                endpoint.cocos_transformation_expression = Some("-psi_like / q".to_string());
            }
        },
        |facts| {
            for endpoint in &mut facts.nodes[2].endpoints {
                endpoint.cocos_label_transformation = Some("psi_like".to_string());
                endpoint.cocos_label_source = Some(CocosLabelSource::Xml);
            }
            facts.versions[0].cocos = None;
        },
        |facts| {
            facts.versions.push(version("5.0.0", Some("17")));
            let mut future = endpoint("5.0.0", GraphNodeKind::Leaf, "FLT_1D", 1);
            future.cocos_label_transformation = Some("psi_like".to_string());
            future.cocos_label_source = Some(CocosLabelSource::InferredSignFlip);
            facts.nodes[2].endpoints.push(future);
        },
    ];
    for (index, mutate) in mutations.into_iter().enumerate() {
        let mut facts = complete_identity_scope();
        mutate(&mut facts);
        let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
            .acquire(&request())
            .expect("a local COCOS verdict must not reject the complete map");
        let outcome = map
            .resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("the endpoint must remain claimed")
            .outcome;
        if index == 3 {
            assert!(matches!(
                outcome,
                Outcome::Path {
                    value_transformation: ValueTransformation::None,
                    ..
                }
            ));
        } else {
            assert_eq!(outcome, Outcome::Refusal(RefusalReason::Unmappable));
        }
        assert!(matches!(
            map.resolve("time_slice", Direction::Forward)
                .expect("independent paths remain available")
                .outcome,
            Outcome::Path {
                value_transformation: ValueTransformation::None,
                ..
            }
        ));
    }
}
