//! Neo4j-backed collection of the pinned DD graph's complete IDS scope.
//!
//! This is deliberately a boundary, rather than a second map compiler.  It
//! obtains every stream that the later historical interpreter needs and makes
//! pagination, row decoding, and scope mistakes explicit before those facts
//! reach [`super::RuntimeMapAcquirer`].  In particular, no `LIMIT` in this
//! module is a lineage limit: it is only a page size and every page is counted.

use std::collections::{BTreeMap, HashSet};
use std::sync::mpsc;
use std::time::Duration;

use neo4j::driver::auth::AuthToken;
use neo4j::driver::{ConnectionConfig, Driver, DriverConfig, RoutingControl};
use neo4j::transaction::TransactionTimeout;
use neo4j::{ValueReceive, ValueSend};

use super::{AcquisitionAttempt, AcquisitionStage, AttemptExpired, GraphSourceError};
use crate::conversion::conversion_map::{ArtifactDdVersion, CocosConvention};

const VERSIONS: &str = "MATCH (v:DDVersion) RETURN v.id AS release, v.cocos AS cocos ORDER BY v.id SKIP $skip LIMIT $limit";
const VERSIONS_COUNT: &str = "MATCH (v:DDVersion) RETURN count(v) AS count";
const NODES: &str = "MATCH (n:IMASNode {ids: $ids}) RETURN n.id AS path, n.ids AS ids, n.data_type AS data_type, n.ndim AS ndim, n.units AS units, [(n)-[r:HAS_COORDINATE]->(coordinate) | [r.dimension, coordinate.id]] AS coordinate_relationships, n.timebase AS timebase, n.change_nbc_version AS change_nbc_version, n.change_nbc_previous_name AS change_nbc_previous_name, n.change_nbc_previous_type AS change_nbc_previous_type, n.cocos_label_transformation AS cocos_label_transformation, n.cocos_transformation_expression AS cocos_transformation_expression, n.cocos_label_source AS cocos_label_source, n.renamed_to AS renamed_to, [(n)-[:INTRODUCED_IN]->(v:DDVersion) | v.id] AS introduced, [(n)-[:DEPRECATED_IN]->(v:DDVersion) | v.id] AS deprecated ORDER BY n.id SKIP $skip LIMIT $limit";
const NODES_COUNT: &str = "MATCH (n:IMASNode {ids: $ids}) RETURN count(n) AS count";
const EVENTS: &str = "MATCH (n:IMASNode {ids: $ids})<-[:FOR_IMAS_PATH]-(c:IMASNodeChange)-[:IN_VERSION]->(v:DDVersion) MATCH (c)-[:FOR_IMAS_PATH]->(owner:IMASNode) RETURN c.id AS id, n.id AS path, v.id AS release, c.change_type AS kind, c.old_value AS old_value, c.new_value AS new_value, c.semantic_type AS semantic_type, c.unit_change_subtype AS unit_change_subtype, collect(DISTINCT owner.ids) AS owner_ids ORDER BY c.id SKIP $skip LIMIT $limit";
const EVENTS_COUNT: &str = "MATCH (n:IMASNode {ids: $ids})<-[:FOR_IMAS_PATH]-(c:IMASNodeChange)-[:IN_VERSION]->(v:DDVersion) RETURN count(c) AS count";
const SUCCESSORS: &str = "MATCH (from:IMASNode {ids: $ids})-[:RENAMED_TO]->(to:IMASNode {ids: $ids}) RETURN from.id AS from_path, to.id AS to_path ORDER BY from.id, to.id SKIP $skip LIMIT $limit";
const SUCCESSORS_COUNT: &str = "MATCH (from:IMASNode {ids: $ids})-[:RENAMED_TO]->(to:IMASNode {ids: $ids}) RETURN count(*) AS count";

/// A typed Cypher value.  `Null` is distinct from a missing column and from
/// an empty string/list, which is essential for the snapshot's COCOS fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphValue {
    Null,
    Integer(i64),
    String(String),
    List(Vec<GraphValue>),
}

#[derive(Clone)]
pub(crate) struct BoundQuery {
    pub text: &'static str,
    pub parameters: BTreeMap<String, GraphValue>,
    pub timeout: Duration,
}

/// Small seam around the selected synchronous Bolt driver.  Tests use it to
/// return shuffled, malformed and short pages without a live service.
pub(crate) trait CypherExecutor {
    fn execute(
        &self,
        query: BoundQuery,
    ) -> Result<Vec<BTreeMap<String, GraphValue>>, GraphSourceError>;
}

/// The transport configuration is kept internal.  The public C ABI never
/// reads credentials or has a graph control surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Neo4jConfig {
    pub uri: String,
    pub username: String,
    pub password: String,
    pub database: String,
    pub page_size: usize,
    pub connection_timeout: Duration,
}

/// The official Bolt driver has bounded TCP connection/acquisition settings,
/// a server-side transaction timeout, and fully consumes each result stream.
/// Connection and query bounds consume the same caller-owned attempt rather
/// than starting a per-operation timer.
pub(crate) struct BoltExecutor {
    driver: std::sync::Arc<Driver>,
    database: std::sync::Arc<String>,
}

impl BoltExecutor {
    pub(crate) fn connect(
        config: &Neo4jConfig,
        attempt: &AcquisitionAttempt,
    ) -> Result<Self, GraphSourceError> {
        let remaining = attempt
            .enter(AcquisitionStage::Connection)
            .map_err(attempt_error)?;
        let connection: ConnectionConfig = config
            .uri
            .parse()
            .map_err(|error| GraphSourceError(format!("invalid Neo4j URI: {error}")))?;
        let connection_timeout = config.connection_timeout.min(remaining);
        let driver_config = DriverConfig::new()
            .with_auth(std::sync::Arc::new(AuthToken::new_basic_auth(
                &config.username,
                &config.password,
            )))
            .with_connection_timeout(connection_timeout)
            .with_connection_acquisition_timeout(connection_timeout);
        attempt
            .check(AcquisitionStage::Connection)
            .map_err(attempt_error)?;
        Ok(Self {
            driver: std::sync::Arc::new(Driver::new(connection, driver_config)),
            database: std::sync::Arc::new(config.database.clone()),
        })
    }
}

impl CypherExecutor for BoltExecutor {
    fn execute(
        &self,
        query: BoundQuery,
    ) -> Result<Vec<BTreeMap<String, GraphValue>>, GraphSourceError> {
        let deadline = query.timeout;
        let driver = std::sync::Arc::clone(&self.driver);
        let database = std::sync::Arc::clone(&self.database);
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = sender.send(run_bolt_query(driver, database, query));
        });
        receive_before_deadline(receiver, deadline)
    }
}

fn receive_before_deadline<T>(
    receiver: mpsc::Receiver<Result<T, GraphSourceError>>,
    deadline: Duration,
) -> Result<T, GraphSourceError> {
    receiver
        .recv_timeout(deadline)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => {
                GraphSourceError("Neo4j query exceeded the acquisition deadline".to_string())
            }
            mpsc::RecvTimeoutError::Disconnected => {
                GraphSourceError("Neo4j query worker ended without a result".to_string())
            }
        })?
}

fn run_bolt_query(
    driver: std::sync::Arc<Driver>,
    database: std::sync::Arc<String>,
    query: BoundQuery,
) -> Result<Vec<BTreeMap<String, GraphValue>>, GraphSourceError> {
    let timeout = i64::try_from(query.timeout.as_millis())
        .ok()
        .and_then(TransactionTimeout::from_millis)
        .ok_or_else(|| GraphSourceError("query has no remaining time".to_string()))?;
    let parameters = query
        .parameters
        .into_iter()
        .map(|(key, value)| (key, to_bolt(value)))
        .collect::<std::collections::HashMap<_, _>>();
    driver
        .execute_query(query.text)
        .with_database(database)
        .with_routing_control(RoutingControl::Read)
        .with_parameters(parameters)
        .with_transaction_timeout(timeout)
        .run()
        .map_err(|error| GraphSourceError(format!("Neo4j query failed: {error}")))?
        .records
        .into_iter()
        .map(|record| {
            record
                .entries()
                .map(|(key, value)| Ok((key.to_string(), from_bolt(value)?)))
                .collect::<Result<BTreeMap<_, _>, GraphSourceError>>()
        })
        .collect()
}

fn to_bolt(value: GraphValue) -> ValueSend {
    match value {
        GraphValue::Null => ValueSend::Null,
        GraphValue::Integer(value) => ValueSend::Integer(value),
        GraphValue::String(value) => ValueSend::String(value),
        GraphValue::List(values) => ValueSend::List(values.into_iter().map(to_bolt).collect()),
    }
}

fn from_bolt(value: &ValueReceive) -> Result<GraphValue, GraphSourceError> {
    match value {
        ValueReceive::Null => Ok(GraphValue::Null),
        ValueReceive::Integer(value) => Ok(GraphValue::Integer(*value)),
        ValueReceive::String(value) => Ok(GraphValue::String(value.clone())),
        ValueReceive::List(values) => values
            .iter()
            .map(from_bolt)
            .collect::<Result<Vec<_>, _>>()
            .map(GraphValue::List),
        _ => Err(GraphSourceError(
            "Neo4j returned a value outside the DD query projection".to_string(),
        )),
    }
}

/// Complete raw scope retrieval. Endpoint reconstruction deliberately stays
/// out of this type: the graph stores lifecycle/history facts, and a later
/// interpreter must turn those facts into exact endpoint metadata. Every
/// query and decoding check consumes the supplied acquisition attempt.
pub(crate) struct Neo4jScopeSource<E> {
    executor: E,
    page_size: usize,
}

impl<E> Neo4jScopeSource<E> {
    pub(crate) fn new(executor: E, page_size: usize) -> Result<Self, GraphSourceError> {
        if page_size == 0 {
            return Err(GraphSourceError(
                "Neo4j page size must be nonzero".to_string(),
            ));
        }
        Ok(Self {
            executor,
            page_size,
        })
    }
}

impl<E: CypherExecutor> Neo4jScopeSource<E> {
    /// Retrieves and validates all four graph streams.  `IdsGraphFacts` is
    /// intentionally not manufactured here: doing so would make current node
    /// properties pretend to be historical endpoint metadata.
    pub(crate) fn load_raw_scope(
        &self,
        ids: &str,
        attempt: &AcquisitionAttempt,
    ) -> Result<Neo4jRawScope, GraphSourceError> {
        let versions = self.pages(VERSIONS_COUNT, VERSIONS, None, attempt)?;
        let nodes = self.pages(NODES_COUNT, NODES, Some(ids), attempt)?;
        let events = self.pages(EVENTS_COUNT, EVENTS, Some(ids), attempt)?;
        let successors = self.pages(SUCCESSORS_COUNT, SUCCESSORS, Some(ids), attempt)?;
        attempt
            .enter(AcquisitionStage::Decoding)
            .map_err(attempt_error)?;
        validate_raw_scope(ids, &versions, &nodes, &events, &successors, attempt)?;
        Ok(Neo4jRawScope {
            versions,
            nodes,
            events,
            successors,
        })
    }

    fn pages(
        &self,
        count_query: &'static str,
        page_query: &'static str,
        ids: Option<&str>,
        attempt: &AcquisitionAttempt,
    ) -> Result<Vec<BTreeMap<String, GraphValue>>, GraphSourceError> {
        let count_rows = self.execute(
            BoundQuery {
                text: count_query,
                parameters: parameters(ids, 0, self.page_size),
                timeout: attempt
                    .enter(AcquisitionStage::Query)
                    .map_err(attempt_error)?,
            },
            attempt,
        )?;
        let count = count_rows
            .as_slice()
            .first()
            .filter(|_| count_rows.len() == 1)
            .and_then(|row| row.get("count"))
            .and_then(|value| match value {
                GraphValue::Integer(value) if *value >= 0 => usize::try_from(*value).ok(),
                _ => None,
            })
            .ok_or_else(|| {
                GraphSourceError("count query did not return one non-negative integer".to_string())
            })?;
        let mut rows = Vec::with_capacity(count);
        while rows.len() < count {
            let page = self.execute(
                BoundQuery {
                    text: page_query,
                    parameters: parameters(ids, rows.len(), self.page_size),
                    timeout: attempt
                        .enter(AcquisitionStage::Query)
                        .map_err(attempt_error)?,
                },
                attempt,
            )?;
            if page.is_empty() {
                return Err(GraphSourceError(format!(
                    "pagination ended at {} rows but count was {count}",
                    rows.len()
                )));
            }
            if page.len() > self.page_size || page.len() > count - rows.len() {
                return Err(GraphSourceError(
                    "page exceeded its declared count boundary".to_string(),
                ));
            }
            rows.extend(page);
        }
        Ok(rows)
    }

    fn execute(
        &self,
        query: BoundQuery,
        attempt: &AcquisitionAttempt,
    ) -> Result<Vec<BTreeMap<String, GraphValue>>, GraphSourceError> {
        let rows = self.executor.execute(query)?;
        attempt
            .check(AcquisitionStage::Query)
            .map_err(attempt_error)?;
        Ok(rows)
    }
}

fn attempt_error(expired: AttemptExpired) -> GraphSourceError {
    GraphSourceError(format!(
        "acquisition attempt timed out during {:?}",
        expired.stage
    ))
}

fn parameters(ids: Option<&str>, skip: usize, limit: usize) -> BTreeMap<String, GraphValue> {
    let mut result = BTreeMap::from([
        ("skip".to_string(), GraphValue::Integer(skip as i64)),
        ("limit".to_string(), GraphValue::Integer(limit as i64)),
    ]);
    if let Some(ids) = ids {
        result.insert("ids".to_string(), GraphValue::String(ids.to_string()));
    }
    result
}

/// Schema-faithful raw streams retained until endpoint presence and metadata
/// can be derived without treating current properties as history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Neo4jRawScope {
    pub versions: Vec<BTreeMap<String, GraphValue>>,
    pub nodes: Vec<BTreeMap<String, GraphValue>>,
    pub events: Vec<BTreeMap<String, GraphValue>>,
    pub successors: Vec<BTreeMap<String, GraphValue>>,
}

fn required_string(
    row: &BTreeMap<String, GraphValue>,
    column: &str,
) -> Result<String, GraphSourceError> {
    match row.get(column) {
        Some(GraphValue::String(value)) if !value.is_empty() => Ok(value.clone()),
        Some(GraphValue::Null) => Err(GraphSourceError(format!("{column} is typed null"))),
        _ => Err(GraphSourceError(format!(
            "{column} is missing or not a non-empty string"
        ))),
    }
}

fn validate_raw_scope(
    ids: &str,
    versions: &[BTreeMap<String, GraphValue>],
    nodes: &[BTreeMap<String, GraphValue>],
    events: &[BTreeMap<String, GraphValue>],
    successors: &[BTreeMap<String, GraphValue>],
    attempt: &AcquisitionAttempt,
) -> Result<(), GraphSourceError> {
    let mut releases = HashSet::new();
    for row in versions {
        attempt
            .check(AcquisitionStage::Decoding)
            .map_err(attempt_error)?;
        let release = required_string(row, "release")?;
        ArtifactDdVersion::new(&release)
            .map_err(|error| GraphSourceError(format!("invalid release {release}: {error}")))?;
        if !releases.insert(release) {
            return Err(GraphSourceError("duplicate release row".to_string()));
        }
        if let Some(value) = row.get("cocos") {
            match value {
                GraphValue::Null => {}
                GraphValue::String(value) if !value.is_empty() => {
                    CocosConvention::new(value).map_err(|error| {
                        GraphSourceError(format!("invalid COCOS value: {error}"))
                    })?;
                }
                _ => {
                    return Err(GraphSourceError(
                        "COCOS must be a typed null or non-empty string".to_string(),
                    ));
                }
            }
        } else {
            return Err(GraphSourceError("COCOS column missing".to_string()));
        }
    }
    let mut paths = HashSet::new();
    for row in nodes {
        attempt
            .check(AcquisitionStage::Decoding)
            .map_err(attempt_error)?;
        if required_string(row, "ids")? != ids {
            return Err(GraphSourceError(
                "node stream escaped its requested IDS".to_string(),
            ));
        }
        let path = required_string(row, "path")?;
        if !paths.insert(path) {
            return Err(GraphSourceError("duplicate node path".to_string()));
        }
        let relationships = match row.get("coordinate_relationships") {
            Some(GraphValue::List(relationships)) => relationships,
            Some(GraphValue::Null) => {
                return Err(GraphSourceError(
                    "coordinate relationships must be an empty list, not typed null".to_string(),
                ));
            }
            _ => {
                return Err(GraphSourceError(
                    "coordinate relationships are missing or not a list".to_string(),
                ));
            }
        };
        let mut dimensions = HashSet::new();
        for relationship in relationships {
            let GraphValue::List(values) = relationship else {
                return Err(GraphSourceError(
                    "coordinate relationship is not a two-value list".to_string(),
                ));
            };
            let [GraphValue::Integer(dimension), GraphValue::String(target)] = values.as_slice()
            else {
                return Err(GraphSourceError(
                    "coordinate relationship has an invalid dimension or target".to_string(),
                ));
            };
            if *dimension < 0 || target.is_empty() || !dimensions.insert(*dimension) {
                return Err(GraphSourceError(
                    "coordinate relationships repeat or contain an invalid dimension".to_string(),
                ));
            }
        }
        for lifecycle_column in ["introduced", "deprecated"] {
            let values = match row.get(lifecycle_column) {
                Some(GraphValue::List(values)) => values,
                Some(GraphValue::Null) => {
                    return Err(GraphSourceError(format!(
                        "{lifecycle_column} must be an empty list, not typed null"
                    )));
                }
                _ => {
                    return Err(GraphSourceError(format!(
                        "{lifecycle_column} is missing or not a list"
                    )));
                }
            };
            let mut lifecycle_releases = HashSet::new();
            for value in values {
                attempt
                    .check(AcquisitionStage::Decoding)
                    .map_err(attempt_error)?;
                let GraphValue::String(release) = value else {
                    return Err(GraphSourceError(format!(
                        "{lifecycle_column} contains a non-string release"
                    )));
                };
                if !releases.contains(release) {
                    return Err(GraphSourceError(format!(
                        "{lifecycle_column} references a release outside the version stream"
                    )));
                }
                if !lifecycle_releases.insert(release) {
                    return Err(GraphSourceError(format!(
                        "{lifecycle_column} repeats a release"
                    )));
                }
            }
        }
    }
    let mut event_ids = HashSet::new();
    for row in events {
        attempt
            .check(AcquisitionStage::Decoding)
            .map_err(attempt_error)?;
        let id = required_string(row, "id")?;
        if !event_ids.insert(id) {
            return Err(GraphSourceError("duplicate event ID".to_string()));
        }
        let path = required_string(row, "path")?;
        if !paths.contains(&path) {
            return Err(GraphSourceError(
                "event references a path outside the node stream".to_string(),
            ));
        }
        if !releases.contains(&required_string(row, "release")?) {
            return Err(GraphSourceError(
                "event references a release outside the version stream".to_string(),
            ));
        }
        required_string(row, "kind")?;
        match row.get("owner_ids") {
            Some(GraphValue::List(owners))
                if owners.len() == 1
                    && matches!(owners.as_slice(), [GraphValue::String(owner)] if owner == ids) => {
            }
            _ => {
                return Err(GraphSourceError(
                    "event is not wholly owned by the requested IDS".to_string(),
                ));
            }
        }
    }
    let mut successor_pairs = HashSet::new();
    for row in successors {
        attempt
            .check(AcquisitionStage::Decoding)
            .map_err(attempt_error)?;
        let from_path = required_string(row, "from_path")?;
        let to_path = required_string(row, "to_path")?;
        if !paths.contains(&from_path) || !paths.contains(&to_path) {
            return Err(GraphSourceError(
                "successor references a path outside the node stream".to_string(),
            ));
        }
        if !successor_pairs.insert((from_path, to_path)) {
            return Err(GraphSourceError("duplicate successor row".to_string()));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/neo4j_graph.rs"]
mod tests;
