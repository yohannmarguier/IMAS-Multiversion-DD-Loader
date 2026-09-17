//! Controlled graph-fact source for the graph-selected C-ABI tracer.
//!
//! This exists only in the separately built test shim.  It drives the same
//! complete-map acquisition coordinator used by a future live graph source,
//! while keeping the recording-stub scenarios hermetic and leaving production
//! source selection unchanged.

use super::{
    AcquisitionAttempt, EndpointMetadata, GraphFactsSource, GraphNode, GraphNodeKind,
    GraphSourceError, GraphVersion, IdsGraphFacts,
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
        Ok(identity_equilibrium_scope())
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

fn identity_equilibrium_scope() -> IdsGraphFacts {
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
            leaf("time"),
            leaf("ids_properties/version_put/data_dictionary"),
        ],
        events: Vec::new(),
        successors: Vec::new(),
    }
}
