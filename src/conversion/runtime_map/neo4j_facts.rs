//! Decode the pinned release's wire schema. Historical interpretation belongs
//! to runtime_map's replay, never to this transport adapter.
use super::neo4j_graph::{GraphValue, Neo4jRawScope};
use super::*;
use std::collections::BTreeMap;

type Row = BTreeMap<String, GraphValue>;

pub(super) fn decode(
    ids: &str,
    raw: Neo4jRawScope,
    attempt: &AcquisitionAttempt,
) -> Result<IdsGraphFacts, GraphSourceError> {
    let mut facts = IdsGraphFacts {
        complete: true,
        versions: Vec::new(),
        nodes: Vec::new(),
        events: Vec::new(),
        successors: Vec::new(),
    };
    for row in raw.versions {
        check(attempt)?;
        facts.versions.push(GraphVersion {
            release: version(&required(&row, "release")?)?,
            cocos: optional(&row, "cocos")?
                .map(CocosConvention::new)
                .transpose()
                .map_err(error)?,
        });
    }
    let latest = facts
        .versions
        .iter()
        .max_by_key(|v| numeric_release(&v.release))
        .ok_or_else(|| error("empty release catalogue"))?
        .release
        .clone();
    for row in raw.events {
        check(attempt)?;
        let id = required(&row, "id")?;
        let field = id
            .rsplit(':')
            .nth(1)
            .ok_or_else(|| error("event ID lacks a field"))?
            .to_string();
        let kind = required(&row, "kind")?;
        if kind != field
            && !(kind == "structure_changed"
                && matches!(field.as_str(), "ndim" | "identifier_enum_name"))
        {
            return Err(error(format!("unsupported event kind {kind} for {field}")));
        }

        let subtype = optional(&row, "unit_change_subtype")?;
        let semantic_type = optional(&row, "semantic_type")?;
        let unit_change = match (field.as_str(), subtype.as_deref()) {
            ("units", Some("cosmetic")) => Some(UnitChangeEvidence::Cosmetic),
            ("units", Some("sentinel_resolved")) => Some(UnitChangeEvidence::SentinelResolved),
            ("units", Some("dim_equivalent")) => Some(UnitChangeEvidence::DimensionallyCompatible),
            _ => None,
        };
        let event = GraphEvent {
            id,
            field,
            kind,
            path: path(ids, &required(&row, "path")?)?,
            release: version(&required(&row, "release")?)?,
            old_value: optional(&row, "old_value")?,
            new_value: optional(&row, "new_value")?,
            unit_change,
            semantic_type,
            coordinate_evidence: None,
        };
        if !matches!(
            event.kind.as_str(),
            "path_added" | "path_removed" | "path_renamed"
        ) {
            let field = event_field(&event).map_err(error)?;
            for value in [&event.old_value, &event.new_value] {
                let valid = match field {
                    "data_type" => value.as_deref().and_then(data_type_rank).is_some(),
                    "ndim" => value
                        .as_deref()
                        .and_then(|value| value.parse::<u8>().ok())
                        .is_some_and(|rank| rank <= 7),
                    "coordinates" => value
                        .as_deref()
                        .is_some_and(|value| parse_string_list(value, &event.id).is_ok()),
                    _ => true,
                };
                if !valid {
                    return Err(error("malformed metadata event value"));
                }
            }
        }
        facts.events.push(event);
    }
    let node_paths: std::collections::HashSet<_> = raw
        .nodes
        .iter()
        .map(|row| required(row, "path"))
        .collect::<Result<_, _>>()?;
    for row in raw.nodes {
        check(attempt)?;
        let node_path = path(ids, &required(&row, "path")?)?;
        let data_type = required(&row, "data_type")?;
        let ndim = match row.get("ndim") {
            Some(GraphValue::Integer(n)) => u8::try_from(*n).map_err(error)?,
            _ => return Err(error("ndim must be an integer")),
        };
        if data_type_rank(&data_type) != Some(ndim) {
            return Err(error("unknown or inconsistent type/rank"));
        }
        let kind = if matches!(data_type.as_str(), "STRUCTURE" | "STRUCT_ARRAY") {
            GraphNodeKind::Structure
        } else {
            GraphNodeKind::Leaf
        };
        let mut coordinates = Vec::new();
        let mut coordinate_scope_complete = true;
        for value in list(&row, "coordinate_relationships")? {
            let GraphValue::List(pair) = value else {
                return Err(error("invalid coordinate relationship"));
            };
            let [
                GraphValue::Integer(dimension),
                GraphValue::String(target),
                GraphValue::String(kind),
            ] = pair.as_slice()
            else {
                return Err(error("invalid coordinate relationship"));
            };
            if kind == "path" && !node_paths.contains(target) {
                coordinate_scope_complete = false;
            }
            coordinates.push(CoordinateRelationship {
                dimension: usize::try_from(*dimension)
                    .map_err(error)?
                    .checked_sub(1)
                    .ok_or_else(|| error("coordinate dimensions start at one"))?,
                target_path: strip_ids_prefix(target, ids),
            });
        }
        let mut declarations = Vec::new();
        let names = optional(&row, "change_nbc_previous_name")?;
        let dates = optional(&row, "change_nbc_version")?;
        let description = optional(&row, "change_nbc_description")?;
        let _previous_type = optional(&row, "change_nbc_previous_type")?;
        if let Some(names) = names.filter(|s| !s.is_empty()) {
            let dates = dates.ok_or_else(|| error("previous names lack declaration dates"))?;
            let names: Vec<_> = names.split(',').map(str::trim).collect();
            let dates: Vec<_> = dates.split(',').map(str::trim).collect();
            if dates.len() != names.len() {
                return Err(error("NBC names and dates are not aligned"));
            }
            if matches!(
                description.as_deref(),
                Some("aos_renamed" | "leaf_renamed" | "structure_renamed")
            ) {
                for (name, date) in names.into_iter().zip(dates) {
                    if name.is_empty() {
                        return Err(error("empty previous name"));
                    }
                    let release = version(date)?;
                    if !facts.versions.iter().any(|known| known.release == release) {
                        return Err(error("NBC date is outside the release catalogue"));
                    }
                    declarations.push(GraphRename {
                        release,
                        previous_name: name.to_string(),
                    });
                }
            }
        }
        let source = optional(&row, "cocos_label_source")?.map(|s| match s.as_str() {
            "xml" => CocosLabelSource::Xml,
            "inferred_sign_flip" => CocosLabelSource::InferredSignFlip,
            "inferred_forward" => CocosLabelSource::InferredForward,
            "inferred_expression" => CocosLabelSource::InferredExpression,
            _ => CocosLabelSource::Other,
        });
        coordinates.sort_by_key(|coordinate| coordinate.dimension);
        let coordinate_paths = if coordinate_scope_complete
            && coordinates.len() == usize::from(ndim)
            && coordinates
                .iter()
                .enumerate()
                .all(|(i, coordinate)| coordinate.dimension == i)
        {
            coordinates
                .iter()
                .map(|coordinate| coordinate.target_path.clone())
                .collect()
        } else {
            Vec::new()
        };
        let metadata = EndpointMetadata {
            release: latest.clone(),
            kind,
            data_type,
            ndim,
            unit: optional(&row, "units")?,
            timebase_path: optional(&row, "timebase")?,
            coordinate_paths,
            cocos_label_transformation: optional(&row, "cocos_label_transformation")?,
            cocos_transformation_expression: optional(&row, "cocos_transformation_expression")?,
            cocos_label_source: source,
        };
        facts.nodes.push(GraphNode {
            ids: ids.to_string(),
            path: node_path,
            introduced: release_list(&row, "introduced")?,
            removed: release_list(&row, "deprecated")?,
            rename_declarations: declarations,
            coordinate_relationships: coordinates,
            endpoints: Vec::new(),
            source_metadata: Some(metadata),
        });
    }
    for row in raw.successors {
        check(attempt)?;
        facts.successors.push(GraphSuccessor {
            from_path: path(ids, &required(&row, "from_path")?)?,
            to_path: path(ids, &required(&row, "to_path")?)?,
        });
    }
    Ok(facts)
}

fn check(attempt: &AcquisitionAttempt) -> Result<(), GraphSourceError> {
    attempt
        .check(AcquisitionStage::Decoding)
        .map(|_| ())
        .map_err(error)
}
fn error(value: impl std::fmt::Debug) -> GraphSourceError {
    GraphSourceError(format!("invalid graph evidence: {value:?}"))
}
fn optional(row: &Row, key: &str) -> Result<Option<String>, GraphSourceError> {
    match row.get(key) {
        Some(GraphValue::Null) => Ok(None),
        Some(GraphValue::String(value)) => Ok(Some(value.clone())),
        _ => Err(error(format!("missing or malformed {key}"))),
    }
}
fn required(row: &Row, key: &str) -> Result<String, GraphSourceError> {
    optional(row, key)?
        .filter(|v| !v.is_empty())
        .ok_or_else(|| error(format!("null or empty {key}")))
}
fn list<'a>(row: &'a Row, key: &str) -> Result<&'a [GraphValue], GraphSourceError> {
    match row.get(key) {
        Some(GraphValue::List(values)) => Ok(values),
        _ => Err(error(format!("missing or malformed {key}"))),
    }
}
fn version(value: &str) -> Result<ArtifactDdVersion, GraphSourceError> {
    ArtifactDdVersion::new(value).map_err(error)
}
fn release_list(row: &Row, key: &str) -> Result<Vec<ArtifactDdVersion>, GraphSourceError> {
    list(row, key)?
        .iter()
        .map(|v| match v {
            GraphValue::String(s) => version(s),
            _ => Err(error("invalid lifecycle release")),
        })
        .collect()
}
fn path(ids: &str, value: &str) -> Result<String, GraphSourceError> {
    let value = value
        .strip_prefix(&format!("{ids}/"))
        .ok_or_else(|| error("path is not qualified by its IDS"))?;
    if value.is_empty() || value.split('/').any(|p| matches!(p, "" | "." | "..")) {
        return Err(error("invalid IDS path"));
    }
    Ok(value.to_string())
}
