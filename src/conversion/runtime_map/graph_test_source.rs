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
