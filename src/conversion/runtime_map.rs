//! Controlled graph-fact acquisition for the first runtime-map tracer.
//!
//! This module is deliberately disconnected from occurrence opening.  It
//! proves that a complete IDS scope can become the existing resolver's map
//! without making graph transport or runtime source selection a production
//! concern.

use std::cmp::Ordering;
use std::collections::HashSet;

use super::conversion_map::{
    ArtifactDdVersion, CocosConvention, ConversionMap, EndpointInventory, EndpointNode,
    EndpointNodeKind, Fidelity, LoadError, Rel, SelectorStage, Side, TypedConversionMap, TypedRule,
};

pub(crate) mod neo4j_graph;

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
    /// Lifecycle edges are supplementary anchors for the event ledger.  They
    /// record the first introduction and removals, but do not collapse a
    /// reappearance into one lifetime.
    pub introduced: Vec<ArtifactDdVersion>,
    pub removed: Vec<ArtifactDdVersion>,
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
    /// The released graph stores event payloads as strings.  Replay decodes
    /// only the field qualified by this event; it never evaluates the text.
    pub old_value: Option<String>,
    pub new_value: Option<String>,
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
    ContradictoryHistory {
        path: String,
        field: String,
        release: ArtifactDdVersion,
    },
    InvalidEventValue {
        id: String,
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
        let mut hli_endpoint = Vec::with_capacity(facts.nodes.len());
        let mut stored_endpoint = Vec::with_capacity(facts.nodes.len());
        let mut endpoint_evidence_complete = true;
        for node in &facts.nodes {
            let hli_metadata = replay_endpoint(&facts, node, &request.hli_dd)?;
            let stored_metadata = replay_endpoint(&facts, node, &request.stored_dd)?;
            let (rel, left, right, fidelity) = match (&hli_metadata, &stored_metadata) {
                (
                    EndpointState::Present {
                        metadata: hli_metadata,
                        interval_start: hli_interval_start,
                    },
                    EndpointState::Present {
                        metadata: stored_metadata,
                        interval_start: stored_interval_start,
                    },
                ) => {
                    hli_endpoint.push(endpoint_node(node, hli_metadata));
                    stored_endpoint.push(endpoint_node(node, stored_metadata));
                    if hli_interval_start != stored_interval_start {
                        // A re-used spelling names different historical roles
                        // until correspondence evidence proves otherwise.
                        (
                            Rel::Identical,
                            Some(node.path.clone()),
                            Some(node.path.clone()),
                            Fidelity::Unmappable,
                        )
                    } else if same_representation(hli_metadata, stored_metadata) {
                        (
                            Rel::Identical,
                            Some(node.path.clone()),
                            Some(node.path.clone()),
                            Fidelity::Exact,
                        )
                    } else if has_cocos_evidence(hli_metadata, stored_metadata) {
                        (
                            Rel::Identical,
                            Some(node.path.clone()),
                            Some(node.path.clone()),
                            Fidelity::Unmappable,
                        )
                    } else {
                        (
                            Rel::Retyped,
                            Some(node.path.clone()),
                            Some(node.path.clone()),
                            Fidelity::Unmappable,
                        )
                    }
                }
                (
                    EndpointState::Present {
                        metadata: hli_metadata,
                        ..
                    },
                    EndpointState::Absent,
                ) => {
                    hli_endpoint.push(endpoint_node(node, hli_metadata));
                    (
                        Rel::LeftOnly,
                        Some(node.path.clone()),
                        None,
                        Fidelity::Unmappable,
                    )
                }
                (
                    EndpointState::Absent,
                    EndpointState::Present {
                        metadata: stored_metadata,
                        ..
                    },
                ) => {
                    stored_endpoint.push(endpoint_node(node, stored_metadata));
                    (
                        Rel::RightOnly,
                        None,
                        Some(node.path.clone()),
                        Fidelity::Unmappable,
                    )
                }
                (EndpointState::Absent, EndpointState::Absent) => continue,
                _ => {
                    // The path's spelling is known at the requested endpoint,
                    // but no interval-local metadata anchor exists.  Claim it
                    // in both directions as a refusal; falling through would
                    // falsely turn uncertainty into an unclaimed identity.
                    endpoint_evidence_complete = false;
                    (
                        Rel::Identical,
                        Some(node.path.clone()),
                        Some(node.path.clone()),
                        Fidelity::Unmappable,
                    )
                }
            };
            rules.push(TypedRule {
                id: format!("endpoint:{}", node.path),
                rel,
                selector_stage: SelectorStage::Exact,
                left,
                right,
                froms: Vec::new(),
                fidelity_forward: fidelity,
                fidelity_reverse: fidelity,
            });
        }

        let hli_endpoint = endpoint_inventory(hli_endpoint, endpoint_evidence_complete);
        let stored_endpoint = endpoint_inventory(stored_endpoint, endpoint_evidence_complete);

        ConversionMap::from_typed(TypedConversionMap {
            ids: request.ids.clone(),
            left: Some(hli),
            right: Some(stored),
            left_endpoint: hli_endpoint,
            right_endpoint: stored_endpoint,
            default_identical: false,
            rules,
            sign_flips: Vec::new(),
            redefines: Vec::new(),
        })
        .map_err(AcquisitionFailure::Construction)
    }
}

fn endpoint_node(node: &GraphNode, metadata: &EndpointMetadata) -> EndpointNode {
    EndpointNode {
        path: node.path.clone(),
        kind: match metadata.kind {
            GraphNodeKind::Leaf => EndpointNodeKind::Leaf,
            GraphNodeKind::Structure => EndpointNodeKind::Structure,
        },
    }
}

fn endpoint_inventory(nodes: Vec<EndpointNode>, complete: bool) -> EndpointInventory {
    if complete {
        EndpointInventory::complete(nodes)
    } else {
        EndpointInventory::incomplete(nodes)
    }
}

enum EndpointState {
    Present {
        metadata: EndpointMetadata,
        interval_start: ArtifactDdVersion,
    },
    Absent,
    Unanchored,
}

fn replay_endpoint(
    facts: &IdsGraphFacts,
    node: &GraphNode,
    requested: &ArtifactDdVersion,
) -> Result<EndpointState, AcquisitionFailure> {
    let releases = sorted_releases(&facts.versions);
    let Some(requested_index) = releases.iter().position(|release| release == requested) else {
        return Err(AcquisitionFailure::MissingRequestedRelease {
            release: requested.clone(),
        });
    };
    let presence = presence_timeline(facts, node, &releases)?;
    if !presence[requested_index] {
        return Ok(EndpointState::Absent);
    }
    let interval_start = (0..=requested_index)
        .rev()
        .find(|&index| presence[index] && (index == 0 || !presence[index - 1]))
        .expect("a present requested endpoint starts an interval");
    let Some(mut metadata) = node
        .endpoints
        .iter()
        .find(|metadata| metadata.release == releases[interval_start])
        .cloned()
    else {
        return Ok(EndpointState::Unanchored);
    };
    for release in &releases[interval_start + 1..=requested_index] {
        for event in events_at(facts, node, release) {
            apply_metadata_event(&mut metadata, event)?;
        }
    }
    if let Some(observed) = node
        .endpoints
        .iter()
        .find(|metadata| metadata.release == *requested)
        && !same_metadata_values(observed, &metadata)
    {
        return Err(AcquisitionFailure::ContradictoryHistory {
            path: node.path.clone(),
            field: "endpoint metadata".to_string(),
            release: requested.clone(),
        });
    }
    metadata.release = requested.clone();
    Ok(EndpointState::Present {
        metadata,
        interval_start: releases[interval_start].clone(),
    })
}

fn same_metadata_values(left: &EndpointMetadata, right: &EndpointMetadata) -> bool {
    left.kind == right.kind
        && left.data_type == right.data_type
        && left.ndim == right.ndim
        && left.unit == right.unit
        && left.timebase_path == right.timebase_path
        && left.coordinate_paths == right.coordinate_paths
        && left.cocos_label_transformation == right.cocos_label_transformation
        && left.cocos_transformation_expression == right.cocos_transformation_expression
}

fn sorted_releases(versions: &[GraphVersion]) -> Vec<ArtifactDdVersion> {
    let mut releases: Vec<_> = versions
        .iter()
        .map(|version| version.release.clone())
        .collect();
    releases.sort_by(numeric_release_order);
    releases
}

fn numeric_release_order(left: &ArtifactDdVersion, right: &ArtifactDdVersion) -> Ordering {
    let parse = |release: &ArtifactDdVersion| {
        release
            .to_string()
            .split('.')
            .map(|component| {
                component
                    .parse::<u32>()
                    .expect("validated release component")
            })
            .collect::<Vec<_>>()
    };
    parse(left).cmp(&parse(right))
}

fn presence_timeline(
    facts: &IdsGraphFacts,
    node: &GraphNode,
    releases: &[ArtifactDdVersion],
) -> Result<Vec<bool>, AcquisitionFailure> {
    let mut present = false;
    let mut timeline = Vec::with_capacity(releases.len());
    for release in releases {
        let mut added = node.introduced.iter().any(|anchor| anchor == release);
        let mut removed = node.removed.iter().any(|anchor| anchor == release);
        for event in &facts.events {
            if event.release != *release {
                continue;
            }
            match event.kind.as_str() {
                "path_added" if event.path == node.path => added = true,
                "path_removed" if event.path == node.path => removed = true,
                "path_renamed" => {
                    let (old, new) = rename_paths(event, &node.ids)?;
                    added |= new == node.path;
                    removed |= old == node.path;
                }
                _ => {}
            }
        }
        if added && removed {
            return Err(AcquisitionFailure::ContradictoryHistory {
                path: node.path.clone(),
                field: "presence".to_string(),
                release: release.clone(),
            });
        }
        if added {
            present = true;
        }
        if removed {
            present = false;
        }
        timeline.push(present);
    }
    Ok(timeline)
}

fn rename_paths(event: &GraphEvent, ids: &str) -> Result<(String, String), AcquisitionFailure> {
    let Some(old) = event.old_value.as_deref() else {
        return Err(AcquisitionFailure::InvalidEventValue {
            id: event.id.clone(),
        });
    };
    let Some(new) = event.new_value.as_deref() else {
        return Err(AcquisitionFailure::InvalidEventValue {
            id: event.id.clone(),
        });
    };
    Ok((strip_ids_prefix(old, ids), strip_ids_prefix(new, ids)))
}

fn strip_ids_prefix(path: &str, ids: &str) -> String {
    path.strip_prefix(ids)
        .and_then(|remainder| remainder.strip_prefix('/'))
        .unwrap_or(path)
        .to_string()
}

fn events_at<'a>(
    facts: &'a IdsGraphFacts,
    node: &GraphNode,
    release: &ArtifactDdVersion,
) -> Vec<&'a GraphEvent> {
    let mut events: Vec<_> = facts
        .events
        .iter()
        .filter(|event| event.path == node.path && event.release == *release)
        .filter(|event| {
            !matches!(
                event.kind.as_str(),
                "path_added" | "path_removed" | "path_renamed"
            )
        })
        .collect();
    events.sort_by(|left, right| left.id.cmp(&right.id));
    events
}

fn apply_metadata_event(
    metadata: &mut EndpointMetadata,
    event: &GraphEvent,
) -> Result<(), AcquisitionFailure> {
    let field = event_field(event)?;
    if field == "ignored" {
        return Ok(());
    }
    let Some(old) = event.old_value.as_deref() else {
        return Err(AcquisitionFailure::InvalidEventValue {
            id: event.id.clone(),
        });
    };
    let Some(new) = event.new_value.as_deref() else {
        return Err(AcquisitionFailure::InvalidEventValue {
            id: event.id.clone(),
        });
    };
    let current = metadata_value(metadata, field)?;
    if current != old {
        return Err(AcquisitionFailure::ContradictoryHistory {
            path: event.path.clone(),
            field: field.to_string(),
            release: event.release.clone(),
        });
    }
    set_metadata_value(metadata, field, new, &event.id)
}

fn event_field(event: &GraphEvent) -> Result<&str, AcquisitionFailure> {
    let id_field = event.id.rsplit(':').nth(1).unwrap_or_default();
    if id_field.is_empty() || (!event.field.is_empty() && event.field != id_field) {
        return Err(AcquisitionFailure::InvalidEventValue {
            id: event.id.clone(),
        });
    }
    let field = id_field;
    match field {
        "data_type" | "ndim" | "units" | "timebase" | "coordinates" => Ok(field),
        // Events outside the endpoint representation do not change its
        // reconstructed metadata.
        "documentation" | "lifecycle_status" | "maxoccur" | "identifier_enum" => Ok("ignored"),
        _ => Err(AcquisitionFailure::InvalidEventValue {
            id: event.id.clone(),
        }),
    }
}

fn metadata_value(metadata: &EndpointMetadata, field: &str) -> Result<String, AcquisitionFailure> {
    match field {
        "data_type" => Ok(metadata.data_type.clone()),
        "ndim" => Ok(metadata.ndim.to_string()),
        "units" => Ok(metadata.unit.clone().unwrap_or_default()),
        "timebase" => Ok(metadata.timebase_path.clone().unwrap_or_default()),
        "coordinates" => Ok(render_list(&metadata.coordinate_paths)),
        "ignored" => Ok(String::new()),
        _ => unreachable!("event_field only returns tracked fields"),
    }
}

fn set_metadata_value(
    metadata: &mut EndpointMetadata,
    field: &str,
    value: &str,
    event_id: &str,
) -> Result<(), AcquisitionFailure> {
    match field {
        "data_type" if !value.is_empty() => metadata.data_type = value.to_string(),
        "ndim" => {
            metadata.ndim =
                value
                    .parse::<u8>()
                    .map_err(|_| AcquisitionFailure::InvalidEventValue {
                        id: event_id.to_string(),
                    })?;
        }
        "units" => metadata.unit = (!value.is_empty()).then(|| value.to_string()),
        "timebase" => metadata.timebase_path = (!value.is_empty()).then(|| value.to_string()),
        "coordinates" => metadata.coordinate_paths = parse_string_list(value, event_id)?,
        "ignored" => {}
        _ => {
            return Err(AcquisitionFailure::InvalidEventValue {
                id: event_id.to_string(),
            });
        }
    }
    Ok(())
}

fn render_list(values: &[String]) -> String {
    let quoted = values
        .iter()
        .map(|value| format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{quoted}]")
}

fn parse_string_list(value: &str, event_id: &str) -> Result<Vec<String>, AcquisitionFailure> {
    let bytes = value.as_bytes();
    if bytes.first() != Some(&b'[') || bytes.last() != Some(&b']') {
        return Err(AcquisitionFailure::InvalidEventValue {
            id: event_id.to_string(),
        });
    }
    let mut index = 1;
    let mut values = Vec::new();
    while index < bytes.len() - 1 {
        while index < bytes.len() - 1 && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == bytes.len() - 1 {
            break;
        }
        let quote = bytes[index];
        if quote != b'\'' && quote != b'\"' {
            return Err(AcquisitionFailure::InvalidEventValue {
                id: event_id.to_string(),
            });
        }
        index += 1;
        let mut item = String::new();
        while index < bytes.len() - 1 && bytes[index] != quote {
            if bytes[index] == b'\\' {
                index += 1;
                if index == bytes.len() - 1 {
                    return Err(AcquisitionFailure::InvalidEventValue {
                        id: event_id.to_string(),
                    });
                }
            }
            item.push(bytes[index] as char);
            index += 1;
        }
        if index == bytes.len() - 1 {
            return Err(AcquisitionFailure::InvalidEventValue {
                id: event_id.to_string(),
            });
        }
        index += 1;
        values.push(item);
        while index < bytes.len() - 1 && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index < bytes.len() - 1 {
            if bytes[index] != b',' {
                return Err(AcquisitionFailure::InvalidEventValue {
                    id: event_id.to_string(),
                });
            }
            index += 1;
        }
    }
    Ok(values)
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
        for anchor in node.introduced.iter().chain(&node.removed) {
            if !releases.iter().any(|known| known == anchor) {
                return Err(invalid_node(
                    node,
                    "lifecycle edge names a release outside the version stream",
                ));
            }
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
