use super::*;
use crate::conversion::conversion_map::{Direction, Outcome, RefusalReason, Rel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
struct ControlledSource {
    result: Result<IdsGraphFacts, GraphSourceError>,
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

fn node(path: &str, left: EndpointMetadata, right: EndpointMetadata) -> GraphNode {
    GraphNode {
        ids: "equilibrium".to_string(),
        path: path.to_string(),
        endpoints: vec![left, right],
    }
}

fn complete_identity_scope() -> IdsGraphFacts {
    IdsGraphFacts {
        complete: true,
        versions: vec![version("3.39.0", Some("11")), version("4.1.1", Some("17"))],
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
        events: Vec::new(),
        successors: Vec::new(),
    }
}

fn request() -> MapRequest {
    MapRequest {
        ids: "equilibrium".to_string(),
        stored_dd: ArtifactDdVersion::new("3.39.0").expect("fixture release is valid"),
        hli_dd: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
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
fn acquisition_keeps_missing_endpoint_evidence_unresolved() {
    let mut facts = complete_identity_scope();
    facts.nodes[2].endpoints.pop();
    let source = ControlledSource { result: Ok(facts) };

    assert!(matches!(
        RuntimeMapAcquirer::new(source).acquire(&request()),
        Err(AcquisitionFailure::UnresolvedEndpoint { path, release })
            if path == "time_slice/profiles_1d/rho_tor" && release.to_string() == "4.1.1"
    ));
}

#[test]
fn acquisition_refuses_uninterpreted_graph_evidence_instead_of_defaulting_to_identity() {
    let mut facts = complete_identity_scope();
    facts.events.push(GraphEvent {
        id: "type-change:coordinates_type:4.0.0".to_string(),
        path: "grids_ggd/grid/space/coordinates_type".to_string(),
        release: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
        field: "data_type".to_string(),
        kind: "structure_changed".to_string(),
    });
    let source = ControlledSource { result: Ok(facts) };

    assert!(matches!(
        RuntimeMapAcquirer::new(source).acquire(&request()),
        Err(AcquisitionFailure::UninterpretedEvent { id })
            if id == "type-change:coordinates_type:4.0.0"
    ));
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
