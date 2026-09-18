//! Controlled graph-fact source for the graph-selected C-ABI tracer.
//!
//! This exists only in the separately built test shim.  It drives the same
//! complete-map acquisition coordinator used by a future live graph source,
//! while keeping the recording-stub scenarios hermetic and leaving production
//! source selection unchanged.

use super::{
    AcquisitionAttempt, EndpointMetadata, GraphEvent, GraphFactsSource, GraphNode, GraphNodeKind,
    GraphSourceError, GraphVersion, IdsGraphFacts, UnitChangeEvidence,
};
use crate::conversion::conversion_map::ArtifactDdVersion;

pub(crate) struct GraphTestSource;

impl GraphFactsSource for GraphTestSource {
    fn load_ids_facts(
        &self,
        ids: &str,
        _attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError> {
        if ids != "equilibrium" {
            return Err(GraphSourceError(format!(
                "controlled graph source has no complete scope for IDS {ids}"
            )));
        }
        Ok(classified_equilibrium_scope())
    }
}

fn graph_release(value: &str) -> ArtifactDdVersion {
    ArtifactDdVersion::new(value).expect("the controlled graph release is valid")
}

fn leaf(path: &str) -> GraphNode {
    GraphNode {
        ids: "equilibrium".to_string(),
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
            })
            .collect(),
    }
}

fn unit_leaf(path: &str, stored_unit: &str, hli_unit: &str) -> GraphNode {
    let mut node = leaf(path);
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
    }
}

fn classified_equilibrium_scope() -> IdsGraphFacts {
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
            unit_leaf("time", "s", "second"),
            unit_leaf("unit_dimensionally_compatible", "m", "cm"),
            unit_leaf("unit_requires_scale_or_offset", "m", "cm"),
            leaf("ids_properties/version_put/data_dictionary"),
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
        ],
        successors: Vec::new(),
    }
}
