//! Controlled graph-fact source for the graph-selected C-ABI tracer.
//!
//! This exists only in the separately built test shim.  It drives the same
//! complete-map acquisition coordinator used by a future live graph source,
//! while keeping the recording-stub scenarios hermetic and leaving production
//! source selection unchanged.

use super::{
    AcquisitionAttempt, CocosLabelSource, EndpointMetadata, GraphEvent, GraphFactsSource,
    GraphNode, GraphNodeKind, GraphSourceError, GraphVersion, IdsGraphFacts,
};
use crate::conversion::conversion_map::{ArtifactDdVersion, CocosConvention};

pub(crate) struct GraphTestSource;

impl GraphFactsSource for GraphTestSource {
    fn load_ids_facts(
        &self,
        ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        match ids {
            "equilibrium" => Ok(cocos_equilibrium_scope(
                ids,
                Some("11"),
                Some("17"),
                "psi_like",
                CocosLabelSource::InferredSignFlip,
                None,
            )),
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
        endpoints: ["3.39.0", "4.1.1"]
            .into_iter()
            .map(|endpoint_release| EndpointMetadata {
                release: graph_release(endpoint_release),
                kind: GraphNodeKind::Leaf,
                data_type: "FLT_1D".to_string(),
                ndim: 1,
                unit: None,
                timebase_path: None,
                coordinate_paths: Vec::new(),
                cocos_label_transformation: None,
                cocos_transformation_expression: None,
                cocos_label_source: None,
            })
            .collect(),
    }
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
        }],
        successors: Vec::new(),
    }
}
