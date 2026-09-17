//! Controlled graph-fact acquisition for the first runtime-map tracer.
//!
//! This module is deliberately disconnected from occurrence opening.  It
//! proves that a complete IDS scope can become the existing resolver's map
//! without making graph transport or runtime source selection a production
//! concern.

use std::collections::HashSet;

use super::conversion_map::{
    ArtifactDdVersion, CocosConvention, ConversionMap, Fidelity, LoadError, Rel, SelectorStage,
    Side, TypedConversionMap, TypedRule,
};

/// The IDS and exact DD endpoints a caller wants to serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MapRequest {
    pub ids: String,
    pub stored_dd: ArtifactDdVersion,
    pub hli_dd: ArtifactDdVersion,
}

/// One released DD version supplied by the graph's version stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphVersion {
    pub release: ArtifactDdVersion,
    pub cocos: Option<CocosConvention>,
}

/// The graph's distinction between a structure and a data-bearing leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphNodeKind {
    Structure,
    Leaf,
}

/// Metadata established for one exact endpoint where a path is present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EndpointMetadata {
    pub release: ArtifactDdVersion,
    pub kind: GraphNodeKind,
    pub data_type: String,
    pub ndim: u8,
    pub unit: Option<String>,
    pub timebase_path: Option<String>,
    pub coordinate_paths: Vec<String>,
    /// The graph's `cocos_label_transformation` value for this exact
    /// endpoint. `None` is the controlled row's observed null, not a guessed
    /// factor of one.
    pub cocos_label_transformation: Option<String>,
    /// The graph's `cocos_transformation_expression` value for this exact
    /// endpoint. This tracer does not interpret expressions yet.
    pub cocos_transformation_expression: Option<String>,
}

/// One IDS-relative node row together with every endpoint metadata row the
/// controlled source established for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphNode {
    pub ids: String,
    pub path: String,
    pub endpoints: Vec<EndpointMetadata>,
}

/// One versioned graph event. The tracer validates references but refuses to
/// treat a non-empty event stream as an identity proof before it can interpret
/// the event's semantic change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphEvent {
    pub id: String,
    pub path: String,
    pub release: ArtifactDdVersion,
    pub field: String,
    pub kind: String,
}

/// A directed correspondence edge from the graph's successor stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphSuccessor {
    pub from_path: String,
    pub to_path: String,
}

/// The complete, IDS-scoped result of the four controlled graph streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdsGraphFacts {
    pub complete: bool,
    pub versions: Vec<GraphVersion>,
    pub nodes: Vec<GraphNode>,
    pub events: Vec<GraphEvent>,
    pub successors: Vec<GraphSuccessor>,
}

/// A graph transport or query failure, intentionally distinct from a fact or
/// construction failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphSourceError(pub String);

/// Internal dependency supplying one complete IDS scope.
pub(crate) trait GraphFactsSource {
    fn load_ids_facts(&self, ids: &str) -> Result<IdsGraphFacts, GraphSourceError>;
}

/// Why acquisition could not return a complete validated map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AcquisitionFailure {
    Source(GraphSourceError),
    IncompleteScope,
    EmptyScope,
    MissingRequestedRelease {
        release: ArtifactDdVersion,
    },
    DuplicateRelease {
        release: ArtifactDdVersion,
    },
    InvalidNode {
        path: String,
        reason: String,
    },
    UnresolvedEndpoint {
        path: String,
        release: ArtifactDdVersion,
    },
    UninterpretedEvent {
        id: String,
    },
    UninterpretedSuccessor {
        from_path: String,
        to_path: String,
    },
    Construction(LoadError),
}

/// Acquires a complete conversion map through an internal graph-fact source.
pub(crate) struct RuntimeMapAcquirer<S> {
    source: S,
}

impl<S> RuntimeMapAcquirer<S> {
    pub(crate) fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S: GraphFactsSource> RuntimeMapAcquirer<S> {
    pub(crate) fn acquire(
        &self,
        request: &MapRequest,
    ) -> Result<ConversionMap, AcquisitionFailure> {
        let facts = self
            .source
            .load_ids_facts(&request.ids)
            .map_err(AcquisitionFailure::Source)?;
        validate_complete_scope(&facts, request)?;

        let hli = graph_side(&facts.versions, &request.hli_dd)?;
        let stored = graph_side(&facts.versions, &request.stored_dd)?;
        let mut rules = Vec::with_capacity(facts.nodes.len());
        for node in &facts.nodes {
            let hli_metadata = endpoint_for(node, &request.hli_dd)?;
            let stored_metadata = endpoint_for(node, &request.stored_dd)?;
            let (rel, fidelity) = if same_representation(hli_metadata, stored_metadata) {
                // The existing map's explicit two-sided identity rule is the
                // narrow representation for a proved path identity. Keeping
                // the default off means a caller path outside this complete
                // scope remains unclaimed rather than becoming identity.
                (Rel::Identical, Fidelity::Exact)
            } else if has_cocos_evidence(hli_metadata, stored_metadata) {
                // A COCOS label or expression establishes that values need
                // additional interpretation. This narrow tracer has neither
                // a supported-factor calculation nor evidence of factor one,
                // so leave the path's structural representation intact and
                // make the value uncertainty explicit to the resolver.
                (Rel::Identical, Fidelity::Unmappable)
            } else {
                // The tracer has no semantic transformation interpreter.
                // A representation difference stays localized through the
                // existing retype refusal instead of contaminating proved
                // identities elsewhere in the IDS.
                (Rel::Retyped, Fidelity::Unmappable)
            };
            rules.push(TypedRule {
                id: format!("endpoint:{}", node.path),
                rel,
                selector_stage: SelectorStage::Exact,
                left: Some(node.path.clone()),
                right: Some(node.path.clone()),
                froms: Vec::new(),
                fidelity_forward: fidelity,
                fidelity_reverse: fidelity,
            });
        }

        ConversionMap::from_typed(TypedConversionMap {
            ids: request.ids.clone(),
            left: Some(hli),
            right: Some(stored),
            default_identical: false,
            rules,
            sign_flips: Vec::new(),
            redefines: Vec::new(),
        })
        .map_err(AcquisitionFailure::Construction)
    }
}

fn validate_complete_scope(
    facts: &IdsGraphFacts,
    request: &MapRequest,
) -> Result<(), AcquisitionFailure> {
    if !facts.complete {
        return Err(AcquisitionFailure::IncompleteScope);
    }
    if facts.nodes.is_empty() {
        return Err(AcquisitionFailure::EmptyScope);
    }

    let mut releases = Vec::new();
    for version in &facts.versions {
        if releases.iter().any(|release| release == &version.release) {
            return Err(AcquisitionFailure::DuplicateRelease {
                release: version.release.clone(),
            });
        }
        releases.push(version.release.clone());
    }
    for release in [&request.hli_dd, &request.stored_dd] {
        if !releases.iter().any(|known| known == release) {
            return Err(AcquisitionFailure::MissingRequestedRelease {
                release: release.clone(),
            });
        }
    }

    let mut paths = HashSet::new();
    for node in &facts.nodes {
        if node.ids != request.ids {
            return Err(invalid_node(
                node,
                "the IDS-scoped node stream returned another IDS",
            ));
        }
        if node.path.is_empty() {
            return Err(invalid_node(node, "path must not be empty"));
        }
        if !paths.insert(node.path.as_str()) {
            return Err(invalid_node(node, "duplicate path in the node stream"));
        }
        let mut endpoint_releases = Vec::new();
        for endpoint in &node.endpoints {
            if !releases.iter().any(|known| known == &endpoint.release) {
                return Err(invalid_node(
                    node,
                    "endpoint metadata names a release outside the version stream",
                ));
            }
            if endpoint_releases.contains(&endpoint.release) {
                return Err(invalid_node(
                    node,
                    "more than one metadata row names the same release",
                ));
            }
            endpoint_releases.push(endpoint.release.clone());
        }
    }

    for event in &facts.events {
        if !paths.contains(event.path.as_str())
            || !releases.iter().any(|known| known == &event.release)
        {
            return Err(AcquisitionFailure::InvalidNode {
                path: event.path.clone(),
                reason: "event does not reference a node and release in this complete scope"
                    .to_string(),
            });
        }
    }
    if let Some(event) = facts.events.first() {
        // Event semantics are intentionally outside this first tracer. A
        // valid but unprocessed event cannot be smuggled into an identity.
        return Err(AcquisitionFailure::UninterpretedEvent {
            id: event.id.clone(),
        });
    }

    for successor in &facts.successors {
        if !paths.contains(successor.from_path.as_str())
            || !paths.contains(successor.to_path.as_str())
        {
            return Err(AcquisitionFailure::InvalidNode {
                path: successor.from_path.clone(),
                reason: "successor does not reference nodes in this complete scope".to_string(),
            });
        }
    }
    if let Some(successor) = facts.successors.first() {
        // Correspondence needs chronology and endpoint-role interpretation;
        // this tracer has neither, so it must fail honestly.
        return Err(AcquisitionFailure::UninterpretedSuccessor {
            from_path: successor.from_path.clone(),
            to_path: successor.to_path.clone(),
        });
    }
    Ok(())
}

fn graph_side(
    versions: &[GraphVersion],
    requested: &ArtifactDdVersion,
) -> Result<Side, AcquisitionFailure> {
    versions
        .iter()
        .find(|version| version.release == *requested)
        .map(|version| Side {
            dd: version.release.clone(),
            cocos: version.cocos.clone(),
        })
        .ok_or_else(|| AcquisitionFailure::MissingRequestedRelease {
            release: requested.clone(),
        })
}

fn endpoint_for<'a>(
    node: &'a GraphNode,
    requested: &ArtifactDdVersion,
) -> Result<&'a EndpointMetadata, AcquisitionFailure> {
    node.endpoints
        .iter()
        .find(|endpoint| endpoint.release == *requested)
        .ok_or_else(|| AcquisitionFailure::UnresolvedEndpoint {
            path: node.path.clone(),
            release: requested.clone(),
        })
}

/// Endpoint presence is established by the release attached to each metadata
/// row. Identity compares the independently reconstructed representation, not
/// that endpoint label itself.
fn same_representation(left: &EndpointMetadata, right: &EndpointMetadata) -> bool {
    left.kind == right.kind
        && left.data_type == right.data_type
        && left.ndim == right.ndim
        && left.unit == right.unit
        && left.timebase_path == right.timebase_path
        && left.coordinate_paths == right.coordinate_paths
        && !has_cocos_evidence(left, right)
}

fn has_cocos_evidence(left: &EndpointMetadata, right: &EndpointMetadata) -> bool {
    left.cocos_label_transformation.is_some()
        || right.cocos_label_transformation.is_some()
        || left.cocos_transformation_expression.is_some()
        || right.cocos_transformation_expression.is_some()
}

fn invalid_node(node: &GraphNode, reason: &str) -> AcquisitionFailure {
    AcquisitionFailure::InvalidNode {
        path: node.path.clone(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
#[path = "tests/runtime_map.rs"]
mod tests;
