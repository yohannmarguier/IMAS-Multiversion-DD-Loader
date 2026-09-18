//! Controlled graph-fact source for the graph-selected C-ABI tracer.
//!
//! This exists only in the separately built test shim. It drives the same
//! complete-map acquisition coordinator used by a future live graph source,
//! while keeping recording-stub scenarios hermetic and production source
//! selection unchanged.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::{
    AcquisitionAttempt, CocosLabelSource, CoordinateChangeEvidence, CoordinateRelationship,
    EndpointMetadata, GraphEvent, GraphFactsSource, GraphNode, GraphNodeKind, GraphRename,
    GraphSourceError, GraphSuccessor, GraphVersion, IdsGraphFacts, UnitChangeEvidence,
};
use crate::conversion::conversion_map::{ArtifactDdVersion, CocosConvention};

pub(crate) struct GraphTestSource;

static ONCE_SCOPE_LOADS: AtomicUsize = AtomicUsize::new(0);
static RECOVERING_SCOPE_LOADS: AtomicUsize = AtomicUsize::new(0);

impl GraphFactsSource for GraphTestSource {
    fn load_ids_facts(
        &self,
        ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        match ids {
            "equilibrium" => Ok(classified_equilibrium_scope()),
            "coexisting_equilibrium" => Ok(coexisting_equilibrium_scope(ids, GraphNodeKind::Leaf)),
            "coexisting_arraystruct_equilibrium" => {
                Ok(coexisting_equilibrium_scope(ids, GraphNodeKind::Structure))
            }
            "moved_descendants" => Ok(moved_descendants_scope()),
            "pulse_schedule" => Ok(pulse_schedule_historical_scope()),
            // This source becomes unavailable after one completed scope. The
            // graph-stage ABI scenarios use it to prove that a retained map
            // survives root closure without contacting the graph again.
            "equilibrium_once" if ONCE_SCOPE_LOADS.fetch_add(1, Ordering::SeqCst) == 0 => {
                Ok(identity_scope(ids))
            }
            "equilibrium_once" => Err(GraphSourceError(
                "controlled graph source was shut down after its first scope".to_string(),
            )),
            // A later open must own a fresh acquisition after a failed one.
            // The first failure and later complete scope make that retry
            // externally observable through the unchanged C ABI.
            "recovering_equilibrium"
                if RECOVERING_SCOPE_LOADS.fetch_add(1, Ordering::SeqCst) == 0 =>
            {
                Err(GraphSourceError(
                    "controlled graph source is temporarily unavailable".to_string(),
                ))
            }
            "recovering_equilibrium" => Ok(identity_scope(ids)),
            "unknown_cocos" => Ok(cocos_equilibrium_scope(
                ids,
                Some("11"),
                Some("17"),
                "unknown_like",
                CocosLabelSource::InferredSignFlip,
                None,
            )),
            "compound_cocos" => Ok(cocos_equilibrium_scope(
                ids,
                Some("11"),
                Some("17"),
                "psi_like",
                CocosLabelSource::InferredExpression,
                Some("-psi_like / q"),
            )),
            "missing_cocos" => Ok(cocos_equilibrium_scope(
                ids,
                None,
                Some("17"),
                "psi_like",
                CocosLabelSource::Xml,
                None,
            )),
            _ => Err(GraphSourceError(format!(
                "controlled graph source has no complete scope for IDS {ids}"
            ))),
        }
    }
}

fn graph_release(value: &str) -> ArtifactDdVersion {
    ArtifactDdVersion::new(value).expect("the controlled graph release is valid")
}

fn convention(value: Option<&str>) -> Option<CocosConvention> {
    value.map(|value| CocosConvention::new(value).expect("the controlled convention is valid"))
}
fn leaf(ids: &str, path: &str) -> GraphNode {
    GraphNode {
        ids: ids.to_string(),
        path: path.to_string(),
        introduced: vec![graph_release("3.39.0")],
        removed: Vec::new(),
        rename_declarations: Vec::new(),
        coordinate_relationships: vec![CoordinateRelationship {
            dimension: 0,
            target_path: "time".to_string(),
        }],
        endpoints: ["3.39.0", "4.1.1"]
            .into_iter()
            .map(|endpoint_release| EndpointMetadata {
                release: graph_release(endpoint_release),
                kind: GraphNodeKind::Leaf,
                data_type: "FLT_1D".to_string(),
                ndim: 1,
                unit: None,
                timebase_path: Some("time".to_string()),
                coordinate_paths: vec!["time".to_string()],
                cocos_label_transformation: None,
                cocos_transformation_expression: None,
                cocos_label_source: None,
            })
            .collect(),
    }
}

fn structure(ids: &str, path: &str) -> GraphNode {
    let mut node = leaf(ids, path);
    for endpoint in &mut node.endpoints {
        endpoint.kind = GraphNodeKind::Structure;
        endpoint.data_type = "STRUCTURE".to_string();
        endpoint.ndim = 0;
    }
    node
}

fn old_only(mut node: GraphNode) -> GraphNode {
    node.endpoints.truncate(1);
    node.removed = vec![graph_release("4.0.0")];
    node
}

fn new_only(mut node: GraphNode) -> GraphNode {
    let endpoint = node
        .endpoints
        .get(1)
        .expect("controlled nodes carry a 4.1.1 endpoint")
        .clone();
    node.endpoints.remove(0);
    node.endpoints.push(EndpointMetadata {
        release: graph_release("4.0.0"),
        ..endpoint
    });
    node.introduced = vec![graph_release("4.0.0")];
    node
}

fn renamed_leaf(ids: &str, path: &str, introduced: &str, removed: Option<&str>) -> GraphNode {
    let mut node = leaf(ids, path);
    node.introduced = vec![graph_release(introduced)];
    node.removed = removed.into_iter().map(graph_release).collect();
    if !node
        .endpoints
        .iter()
        .any(|endpoint| endpoint.release == graph_release(introduced))
    {
        node.endpoints.push(EndpointMetadata {
            release: graph_release(introduced),
            kind: GraphNodeKind::Leaf,
            data_type: "FLT_1D".to_string(),
            ndim: 1,
            unit: None,
            timebase_path: Some("time".to_string()),
            coordinate_paths: vec!["time".to_string()],
            cocos_label_transformation: None,
            cocos_transformation_expression: None,
            cocos_label_source: None,
        });
    }
    node
}

fn cocos_psi_leaf(
    ids: &str,
    label: &str,
    source: CocosLabelSource,
    expression: Option<&str>,
) -> GraphNode {
    let mut node = leaf(ids, "time_slice/profiles_1d/psi");
    for endpoint in &mut node.endpoints {
        endpoint.cocos_label_transformation = Some(label.to_string());
        endpoint.cocos_label_source = Some(source);
        endpoint.cocos_transformation_expression = expression.map(str::to_string);
    }
    node
}

fn unit_leaf(ids: &str, path: &str, stored_unit: &str, hli_unit: &str) -> GraphNode {
    let mut node = leaf(ids, path);
    node.endpoints[0].unit = Some(stored_unit.to_string());
    node.endpoints[1].unit = Some(hli_unit.to_string());
    node
}

fn unit_event(
    path: &str,
    old_value: &str,
    new_value: &str,
    unit_change: UnitChangeEvidence,
) -> GraphEvent {
    GraphEvent {
        id: format!("{path}:units:4.1.1"),
        path: path.to_string(),
        release: graph_release("4.1.1"),
        field: "units".to_string(),
        kind: "units_changed".to_string(),
        old_value: Some(old_value.to_string()),
        new_value: Some(new_value.to_string()),
        unit_change: Some(unit_change),
        coordinate_evidence: None,
    }
}

fn resampling_event(path: &str) -> GraphEvent {
    GraphEvent {
        id: format!("{path}:timebase:4.1.1"),
        path: path.to_string(),
        release: graph_release("4.1.1"),
        field: "timebase".to_string(),
        kind: "timebase_changed".to_string(),
        old_value: Some("time".to_string()),
        new_value: Some("time".to_string()),
        unit_change: None,
        coordinate_evidence: Some(CoordinateChangeEvidence::RequiresResampling),
    }
}

fn cocos_equilibrium_scope(
    ids: &str,
    left_cocos: Option<&str>,
    right_cocos: Option<&str>,
    psi_label: &str,
    psi_source: CocosLabelSource,
    psi_expression: Option<&str>,
) -> IdsGraphFacts {
    IdsGraphFacts {
        complete: true,
        versions: [
            ("3.39.0", left_cocos),
            ("4.0.0", Some("17")),
            ("4.1.1", right_cocos),
        ]
        .into_iter()
        .map(|(release_text, cocos)| GraphVersion {
            release: graph_release(release_text),
            cocos: convention(cocos),
        })
        .collect(),
        nodes: vec![
            leaf(ids, "time"),
            leaf(ids, "ids_properties/version_put/data_dictionary"),
            cocos_psi_leaf(ids, psi_label, psi_source, psi_expression),
        ],
        // The pinned graph records the raw label clearing independently of
        // the later inferred sign-flip class.  Replaying it must not erase
        // that provenance-qualified class or count a second transform.
        events: vec![GraphEvent {
            id: "psi:cocos_label_transformation:4.0.0".to_string(),
            path: "time_slice/profiles_1d/psi".to_string(),
            release: graph_release("4.0.0"),
            field: "cocos_label_transformation".to_string(),
            kind: "metadata_changed".to_string(),
            old_value: Some("psi".to_string()),
            new_value: Some(String::new()),
            unit_change: None,
            coordinate_evidence: None,
        }],
        successors: Vec::new(),
    }
}

fn identity_scope(ids: &str) -> IdsGraphFacts {
    IdsGraphFacts {
        complete: true,
        versions: ["3.39.0", "4.1.1"]
            .into_iter()
            .map(|release_text| GraphVersion {
                release: graph_release(release_text),
                cocos: None,
            })
            .collect(),
        nodes: vec![
            leaf(ids, "time"),
            leaf(ids, "ids_properties/version_put/data_dictionary"),
        ],
        events: Vec::new(),
        successors: Vec::new(),
    }
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
        introduced: vec![graph_release(introduced)],
        removed: removed.into_iter().map(graph_release).collect(),
        rename_declarations: Vec::new(),
        coordinate_relationships: vec![CoordinateRelationship {
            dimension: 0,
            target_path: "time".to_string(),
        }],
        endpoints: vec![EndpointMetadata {
            release: graph_release(introduced),
            kind,
            data_type: match kind {
                GraphNodeKind::Leaf => "FLT_1D".to_string(),
                GraphNodeKind::Structure => "STRUCTURE".to_string(),
            },
            ndim: match kind {
                GraphNodeKind::Leaf => 1,
                GraphNodeKind::Structure => 0,
            },
            unit: None,
            timebase_path: Some("time".to_string()),
            coordinate_paths: vec!["time".to_string()],
            cocos_label_transformation: None,
            cocos_transformation_expression: None,
            cocos_label_source: None,
        }],
    }
}

fn pulse_schedule_historical_scope() -> IdsGraphFacts {
    let mut beam = historical_node("ec/beam", "3.40.0", None, GraphNodeKind::Structure);
    beam.rename_declarations = vec![
        GraphRename {
            release: graph_release("3.26.0"),
            previous_name: "antenna".to_string(),
        },
        GraphRename {
            release: graph_release("3.40.0"),
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
        release: graph_release("3.26.0"),
        previous_name: "launching_angle_pol".to_string(),
    }];

    IdsGraphFacts {
        complete: true,
        versions: ["3.22.0", "3.25.0", "3.26.0", "3.30.0", "3.40.0"]
            .into_iter()
            .map(|release| GraphVersion {
                release: graph_release(release),
                cocos: None,
            })
            .collect(),
        nodes: vec![
            historical_node("time", "3.22.0", None, GraphNodeKind::Leaf),
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

fn coexisting_rename_pair(
    ids: &str,
    predecessor_path: &str,
    successor_path: &str,
    kind: GraphNodeKind,
) -> [GraphNode; 2] {
    let mut predecessor = node_of_kind(ids, predecessor_path, kind);
    predecessor.endpoints.truncate(1);
    predecessor
        .endpoints
        .push(coexistence_endpoint("3.42.0", kind));
    predecessor.removed = vec![graph_release("4.0.0")];

    let mut successor = node_of_kind(ids, successor_path, kind);
    successor.endpoints.remove(0);
    successor
        .endpoints
        .push(coexistence_endpoint("3.42.0", kind));
    successor.introduced = vec![graph_release("3.42.0")];
    successor.rename_declarations = vec![GraphRename {
        release: graph_release("3.42.0"),
        previous_name: predecessor_path
            .rsplit_once('/')
            .map_or(predecessor_path, |(_, name)| name)
            .to_string(),
    }];
    [predecessor, successor]
}

fn node_of_kind(ids: &str, path: &str, kind: GraphNodeKind) -> GraphNode {
    match kind {
        GraphNodeKind::Structure => structure(ids, path),
        GraphNodeKind::Leaf => leaf(ids, path),
    }
}

fn coexistence_endpoint(release: &str, kind: GraphNodeKind) -> EndpointMetadata {
    EndpointMetadata {
        release: graph_release(release),
        kind,
        data_type: match kind {
            GraphNodeKind::Structure => "STRUCTURE".to_string(),
            GraphNodeKind::Leaf => "FLT_1D".to_string(),
        },
        ndim: match kind {
            GraphNodeKind::Structure => 0,
            GraphNodeKind::Leaf => 1,
        },
        unit: None,
        timebase_path: Some("time".to_string()),
        coordinate_paths: vec!["time".to_string()],
        cocos_label_transformation: None,
        cocos_transformation_expression: None,
        cocos_label_source: None,
    }
}

fn coexisting_equilibrium_scope(ids: &str, j_kind: GraphNodeKind) -> IdsGraphFacts {
    let j_predecessor = "time_slice/constraints/j_tor";
    let j_successor = "time_slice/constraints/j_phi";
    let b_predecessor = "time_slice/global_quantities/magnetic_axis/b_field_tor";
    let b_successor = "time_slice/global_quantities/magnetic_axis/b_field_phi";
    let [j_tor, j_phi] = coexisting_rename_pair(ids, j_predecessor, j_successor, j_kind);
    let [b_field_tor, b_field_phi] =
        coexisting_rename_pair(ids, b_predecessor, b_successor, GraphNodeKind::Leaf);
    IdsGraphFacts {
        complete: true,
        versions: ["3.39.0", "3.42.0", "4.0.0", "4.1.1"]
            .into_iter()
            .map(|release| GraphVersion {
                release: graph_release(release),
                cocos: None,
            })
            .collect(),
        nodes: vec![
            leaf(ids, "time"),
            leaf(ids, "ids_properties/version_put/data_dictionary"),
            j_tor,
            j_phi,
            b_field_tor,
            b_field_phi,
        ],
        events: Vec::new(),
        successors: vec![
            GraphSuccessor {
                from_path: j_predecessor.to_string(),
                to_path: j_successor.to_string(),
            },
            GraphSuccessor {
                from_path: b_predecessor.to_string(),
                to_path: b_successor.to_string(),
            },
        ],
    }
}

fn moved_descendants_scope() -> IdsGraphFacts {
    let ids = "moved_descendants";
    let old_parent = "time_slice/legacy/profiles_1d";
    let new_parent = "time_slice/current/profiles_1d";
    let old_gap = format!("{old_parent}/gap");
    let new_gap = format!("{new_parent}/gap");
    let old_r = format!("{old_parent}/gap/r");
    let new_r = format!("{new_parent}/gap/r");
    let old_identifier = format!("{old_parent}/gap/identifier");
    let old_escaping = format!("{old_parent}/escaped");
    let new_escaping = "time_slice/outside/escaped";

    let mut new_parent_node = new_only(structure(ids, new_parent));
    new_parent_node.rename_declarations = vec![GraphRename {
        release: graph_release("4.0.0"),
        previous_name: "../legacy/profiles_1d".to_string(),
    }];
    let mut new_gap_node = new_only(structure(ids, &new_gap));
    new_gap_node.rename_declarations = vec![GraphRename {
        release: graph_release("4.0.0"),
        previous_name: "../../legacy/profiles_1d/gap".to_string(),
    }];
    let mut new_r_node = new_only(leaf(ids, &new_r));
    new_r_node.rename_declarations = vec![GraphRename {
        release: graph_release("4.0.0"),
        previous_name: "../../../legacy/profiles_1d/gap/r".to_string(),
    }];
    let mut new_escaping_node = new_only(leaf(ids, new_escaping));
    new_escaping_node.rename_declarations = vec![GraphRename {
        release: graph_release("4.0.0"),
        previous_name: "../legacy/profiles_1d/escaped".to_string(),
    }];

    IdsGraphFacts {
        complete: true,
        versions: ["3.39.0", "4.0.0", "4.1.1"]
            .into_iter()
            .map(|release| GraphVersion {
                release: graph_release(release),
                cocos: None,
            })
            .collect(),
        nodes: vec![
            leaf(ids, "time"),
            leaf(ids, "ids_properties/version_put/data_dictionary"),
            old_only(structure(ids, old_parent)),
            new_parent_node,
            old_only(structure(ids, &old_gap)),
            new_gap_node,
            old_only(leaf(ids, &old_r)),
            new_r_node,
            old_only(leaf(ids, &old_identifier)),
            old_only(leaf(ids, &old_escaping)),
            new_escaping_node,
        ],
        events: Vec::new(),
        successors: vec![
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
        ],
    }
}

fn classified_equilibrium_scope() -> IdsGraphFacts {
    let ids = "equilibrium";
    let mut facts = IdsGraphFacts {
        complete: true,
        versions: [
            ("3.39.0", Some("11")),
            ("3.42.0", Some("11")),
            ("4.0.0", Some("17")),
            ("4.1.1", Some("17")),
        ]
        .into_iter()
        .map(|(release_text, cocos)| GraphVersion {
            release: graph_release(release_text),
            cocos: convention(cocos),
        })
        .collect(),
        nodes: vec![
            unit_leaf(ids, "time", "s", "second"),
            unit_leaf(ids, "unit_dimensionally_compatible", "m", "cm"),
            unit_leaf(ids, "unit_requires_scale_or_offset", "m", "cm"),
            leaf(ids, "resampling_timebase"),
            leaf(ids, "ids_properties/version_put/data_dictionary"),
            cocos_psi_leaf(ids, "psi_like", CocosLabelSource::InferredSignFlip, None),
            renamed_leaf(
                ids,
                "time_slice/global_quantities/beta_normal",
                "3.39.0",
                Some("4.0.0"),
            ),
            GraphNode {
                rename_declarations: vec![GraphRename {
                    release: graph_release("4.0.0"),
                    previous_name: "beta_normal".to_string(),
                }],
                ..renamed_leaf(
                    ids,
                    "time_slice/global_quantities/beta_tor_norm",
                    "4.0.0",
                    None,
                )
            },
            renamed_leaf(ids, "time_slice/constraints/j_tor", "3.39.0", Some("4.0.0")),
            GraphNode {
                rename_declarations: vec![GraphRename {
                    release: graph_release("3.42.0"),
                    previous_name: "j_tor".to_string(),
                }],
                ..renamed_leaf(ids, "time_slice/constraints/j_phi", "3.42.0", None)
            },
        ],
        events: vec![
            unit_event("time", "s", "second", UnitChangeEvidence::SentinelResolved),
            unit_event(
                "unit_dimensionally_compatible",
                "m",
                "cm",
                UnitChangeEvidence::DimensionallyCompatible,
            ),
            unit_event(
                "unit_requires_scale_or_offset",
                "m",
                "cm",
                UnitChangeEvidence::RequiredScaleOrOffset,
            ),
            resampling_event("resampling_timebase"),
            GraphEvent {
                id: "psi:cocos_label_transformation:4.0.0".to_string(),
                path: "time_slice/profiles_1d/psi".to_string(),
                release: graph_release("4.0.0"),
                field: "cocos_label_transformation".to_string(),
                kind: "metadata_changed".to_string(),
                old_value: Some("psi".to_string()),
                new_value: Some(String::new()),
                unit_change: None,
                coordinate_evidence: None,
            },
        ],
        successors: vec![
            GraphSuccessor {
                from_path: "equilibrium/time_slice/global_quantities/beta_normal".to_string(),
                to_path: "equilibrium/time_slice/global_quantities/beta_tor_norm".to_string(),
            },
            GraphSuccessor {
                from_path: "time_slice/constraints/j_tor".to_string(),
                to_path: "time_slice/constraints/j_phi".to_string(),
            },
        ],
    };
    let old_gap = "time_slice/boundary/gap";
    let new_gap = "time_slice/boundary_separatrix/gap";
    let old_r = format!("{old_gap}/r");
    let new_r = format!("{new_gap}/r");
    let mut new_gap_node = new_only(structure(ids, new_gap));
    new_gap_node.rename_declarations = vec![GraphRename {
        release: graph_release("4.0.0"),
        previous_name: "../boundary/gap".to_string(),
    }];
    let mut new_r_node = new_only(leaf(ids, &new_r));
    new_r_node.rename_declarations = vec![GraphRename {
        release: graph_release("4.0.0"),
        previous_name: "../../boundary/gap/r".to_string(),
    }];
    facts.nodes.extend([
        old_only(structure(ids, old_gap)),
        new_gap_node,
        old_only(leaf(ids, &old_r)),
        new_r_node,
        new_only(leaf(ids, &format!("{new_gap}/identifier"))),
    ]);
    facts.successors.extend([
        GraphSuccessor {
            from_path: old_gap.to_string(),
            to_path: new_gap.to_string(),
        },
        GraphSuccessor {
            from_path: old_r,
            to_path: new_r,
        },
    ]);
    for node in facts
        .nodes
        .iter_mut()
        .filter(|node| node.path.starts_with("time_slice/constraints/j_"))
    {
        for endpoint in &mut node.endpoints {
            endpoint.cocos_label_transformation = Some("psi_like".to_string());
        }
    }
    facts
}
