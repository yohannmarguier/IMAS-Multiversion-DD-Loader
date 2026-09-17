use super::*;
use crate::conversion::conversion_map::{Direction, Outcome, RefusalReason, Rel};

#[derive(Clone)]
struct ControlledSource {
    result: Result<IdsGraphFacts, GraphSourceError>,
}

impl GraphFactsSource for ControlledSource {
    fn load_ids_facts(&self, _ids: &str) -> Result<IdsGraphFacts, GraphSourceError> {
        self.result.clone()
    }
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
        }],
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
fn acquisition_keeps_missing_endpoint_evidence_as_a_local_refusal() {
    let mut facts = complete_identity_scope();
    facts.nodes[2].endpoints.remove(0);
    let source = ControlledSource { result: Ok(facts) };

    let map = RuntimeMapAcquirer::new(source)
        .acquire(&request())
        .expect("an unanchored path must not hide independent endpoint evidence");
    assert_eq!(
        map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("unanchored path remains claimed")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
    assert!(matches!(
        map.resolve("time_slice", Direction::Forward),
        Some(explanation) if matches!(explanation.outcome, Outcome::Path { .. })
    ));
}

#[test]
fn acquisition_replays_a_field_qualified_generic_event() {
    let source = ControlledSource {
        result: Ok(complete_identity_scope()),
    };

    assert!(matches!(
        RuntimeMapAcquirer::new(source).acquire(&request()),
        Ok(map) if matches!(
            map.resolve("grids_ggd/grid/space/coordinates_type", Direction::Forward),
            Some(explanation) if explanation.outcome == Outcome::Refusal(RefusalReason::UnservableRetype)
        )
    ));
}

#[test]
fn acquisition_replays_removal_and_reappearance_at_the_event_release() {
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
        },
        GraphEvent {
            id: "rho_tor:path_added:4.1.1".to_string(),
            path: "time_slice/profiles_1d/rho_tor".to_string(),
            release: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
            field: "path".to_string(),
            kind: "path_added".to_string(),
            old_value: None,
            new_value: None,
        },
    ]);

    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("a reappearance is present at its addition release");
    assert_eq!(
        map.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
            .expect("reused spelling remains claimed")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
}

#[test]
fn acquisition_rejects_a_contradictory_metadata_ledger() {
    let mut facts = complete_identity_scope();
    facts.events.push(GraphEvent {
        id: "rho_tor:ndim:4.1.1".to_string(),
        path: "time_slice/profiles_1d/rho_tor".to_string(),
        release: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
        field: "ndim".to_string(),
        kind: "structure_changed".to_string(),
        old_value: Some("2".to_string()),
        new_value: Some("1".to_string()),
    });

    assert!(matches!(
        RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) }).acquire(&request()),
        Err(AcquisitionFailure::ContradictoryHistory { path, field, release })
            if path == "time_slice/profiles_1d/rho_tor" && field == "ndim" && release.to_string() == "4.1.1"
    ));
}

#[test]
fn acquisition_rejects_non_literal_coordinate_history() {
    let mut facts = complete_identity_scope();
    facts.events.push(GraphEvent {
        id: "rho_tor:coordinates:4.1.1".to_string(),
        path: "time_slice/profiles_1d/rho_tor".to_string(),
        release: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
        field: "coordinates".to_string(),
        kind: "coordinates_changed".to_string(),
        old_value: Some("[]".to_string()),
        new_value: Some("__import__('os').system('false')".to_string()),
    });

    assert!(matches!(
        RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) }).acquire(&request()),
        Err(AcquisitionFailure::InvalidEventValue { id }) if id == "rho_tor:coordinates:4.1.1"
    ));
}

#[test]
fn acquisition_rejects_a_generic_event_whose_id_disagrees_with_its_field() {
    let mut facts = complete_identity_scope();
    facts.events[0].field = "ndim".to_string();

    assert!(matches!(
        RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) }).acquire(&request()),
        Err(AcquisitionFailure::InvalidEventValue { id })
            if id == "coordinates_type:data_type:4.1.1"
    ));
}

#[test]
fn acquisition_requires_each_requested_release_to_be_in_the_catalogue() {
    let mut facts = complete_identity_scope();
    facts
        .versions
        .retain(|version| version.release.to_string() != "4.1.1");

    assert!(matches!(
        RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) }).acquire(&request()),
        Err(AcquisitionFailure::MissingRequestedRelease { release }) if release.to_string() == "4.1.1"
    ));
}

#[test]
fn acquisition_treats_a_removal_as_absent_at_its_event_release() {
    let mut facts = complete_identity_scope();
    facts.events.push(GraphEvent {
        id: "rho_tor:path_removed:4.0.0".to_string(),
        path: "time_slice/profiles_1d/rho_tor".to_string(),
        release: ArtifactDdVersion::new("4.0.0").expect("fixture release is valid"),
        field: "path".to_string(),
        kind: "path_removed".to_string(),
        old_value: None,
        new_value: None,
    });
    let mut boundary_request = request();
    boundary_request.hli_dd = ArtifactDdVersion::new("4.0.0").expect("fixture release is valid");

    let map = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&boundary_request)
        .expect("a known absence remains a local map result");
    assert_eq!(
        map.resolve("time_slice/profiles_1d/rho_tor", Direction::Reverse)
            .expect("stored-only path remains claimed")
            .outcome,
        Outcome::Refusal(RefusalReason::Unmappable)
    );
}

#[test]
fn acquisition_replay_is_independent_of_version_and_event_row_order() {
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
        },
        GraphEvent {
            id: "rho_tor:path_added:4.1.1".to_string(),
            path: "time_slice/profiles_1d/rho_tor".to_string(),
            release: ArtifactDdVersion::new("4.1.1").expect("fixture release is valid"),
            field: "path".to_string(),
            kind: "path_added".to_string(),
            old_value: None,
            new_value: None,
        },
    ]);
    let expected = RuntimeMapAcquirer::new(ControlledSource {
        result: Ok(facts.clone()),
    })
    .acquire(&request())
    .expect("ordered rows acquire");
    facts.versions.reverse();
    facts.events.reverse();
    let shuffled = RuntimeMapAcquirer::new(ControlledSource { result: Ok(facts) })
        .acquire(&request())
        .expect("shuffled rows acquire");

    assert_eq!(
        expected.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward),
        shuffled.resolve("time_slice/profiles_1d/rho_tor", Direction::Forward)
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
