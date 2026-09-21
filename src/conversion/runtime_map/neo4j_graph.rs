//! Neo4j-backed collection of the pinned DD graph's complete IDS scope.
//!
//! This is deliberately a boundary, rather than a second map compiler.  It
//! obtains every stream that the later historical interpreter needs and makes
//! pagination, row decoding, and scope mistakes explicit before those facts
//! reach [`super::RuntimeMapAcquirer`].  In particular, no `LIMIT` in this
//! module is a lineage limit: it is only a page size and every page is counted.

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use neo4rs::{BoltList, BoltNull, BoltType, ConfigBuilder, Graph};

use super::{AcquisitionAttempt, AcquisitionStage, AttemptExpired, GraphSourceError};
use crate::conversion::conversion_map::{ArtifactDdVersion, CocosConvention};

const VERSIONS: &str = "MATCH (v:DDVersion) RETURN v.id AS release, toString(v.cocos) AS cocos ORDER BY v.id SKIP $skip LIMIT $limit";
const VERSIONS_COUNT: &str = "MATCH (v:DDVersion) RETURN count(v) AS count";
const NODES: &str = "MATCH (n:IMASNode {ids: $ids}) RETURN n.id AS path, n.ids AS ids, n.data_type AS data_type, n.ndim AS ndim, n.unit AS units, [(n)-[r:HAS_COORDINATE]->(coordinate) | [r.dimension, coordinate.id, CASE WHEN coordinate:IMASNode THEN 'path' WHEN coordinate:IMASCoordinateSpec THEN 'spec' ELSE 'unknown' END]] AS coordinate_relationships, n.timebasepath AS timebase, n.change_nbc_version AS change_nbc_version, n.change_nbc_description AS change_nbc_description, n.change_nbc_previous_name AS change_nbc_previous_name, n.change_nbc_previous_type AS change_nbc_previous_type, n.cocos_label_transformation AS cocos_label_transformation, n.cocos_transformation_expression AS cocos_transformation_expression, n.cocos_label_source AS cocos_label_source, n.renamed_to AS renamed_to, [(n)-[:INTRODUCED_IN]->(v:DDVersion) | v.id] AS introduced, [(n)-[:DEPRECATED_IN]->(v:DDVersion) | v.id] AS deprecated ORDER BY n.id SKIP $skip LIMIT $limit";
const NODES_COUNT: &str = "MATCH (n:IMASNode {ids: $ids}) RETURN count(n) AS count";
const EVENTS: &str = "MATCH (n:IMASNode {ids: $ids})<-[:FOR_IMAS_PATH]-(c:IMASNodeChange) RETURN c.id AS id, n.id AS path, head([(c)-[:IN_VERSION]->(v:DDVersion) | v.id]) AS release, [(c)-[:IN_VERSION]->(v:DDVersion) | v.id] AS releases, [(c)-[:FOR_IMAS_PATH]->(owner:IMASNode) | owner.id] AS owners, c.change_type AS kind, c.old_value AS old_value, c.new_value AS new_value, c.semantic_type AS semantic_type, c.unit_change_subtype AS unit_change_subtype ORDER BY c.id SKIP $skip LIMIT $limit";
const EVENTS_COUNT: &str =
    "MATCH (n:IMASNode {ids: $ids})<-[:FOR_IMAS_PATH]-(c:IMASNodeChange) RETURN count(c) AS count";
const SUCCESSORS: &str = "MATCH (from:IMASNode {ids: $ids})-[:RENAMED_TO]->(to:IMASNode) RETURN from.id AS from_path, to.id AS to_path ORDER BY from.id, to.id SKIP $skip LIMIT $limit";
const SUCCESSORS_COUNT: &str =
    "MATCH (from:IMASNode {ids: $ids})-[:RENAMED_TO]->(to:IMASNode) RETURN count(*) AS count";

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

/// Small seam around the selected Bolt driver.  Tests use it to
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

/// One current-thread async runtime owns the Bolt I/O. Tokio cancellation
/// drops the in-progress connection future at the remaining attempt deadline,
/// so no detached worker or blocked socket can survive a timed-out query.
pub(crate) struct BoltExecutor {
    driver: Mutex<Option<Graph>>,
    runtime: tokio::runtime::Runtime,
}

impl BoltExecutor {
    pub(crate) fn connect(
        config: &Neo4jConfig,
        attempt: &AcquisitionAttempt,
    ) -> Result<Self, GraphSourceError> {
        let remaining = attempt
            .enter(AcquisitionStage::Connection)
            .map_err(attempt_error)?;
        let connection_timeout = config.connection_timeout.min(remaining);
        let driver_config = ConfigBuilder::default()
            .uri(&config.uri)
            .user(&config.username)
            .password(&config.password)
            .db(config.database.as_str())
            .fetch_size(config.page_size)
            .max_connections(1)
            .build()
            .map_err(|error| GraphSourceError(format!("invalid Neo4j configuration: {error}")))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| GraphSourceError(format!("cannot start Neo4j runtime: {error}")))?;
        let driver = runtime
            .block_on(async {
                tokio::time::timeout(connection_timeout, Graph::connect(driver_config)).await
            })
            .map_err(|_| {
                GraphSourceError("Neo4j connection exceeded the acquisition deadline".to_string())
            })?
            .map_err(|error| GraphSourceError(format!("Neo4j connection failed: {error}")))?;
        attempt
            .check(AcquisitionStage::Connection)
            .map_err(attempt_error)?;
        Ok(Self {
            driver: Mutex::new(Some(driver)),
            runtime,
        })
    }
}

impl CypherExecutor for BoltExecutor {
    fn execute(
        &self,
        query: BoundQuery,
    ) -> Result<Vec<BTreeMap<String, GraphValue>>, GraphSourceError> {
        let deadline = query.timeout;
        let mut driver = self
            .driver
            .lock()
            .expect("Neo4j driver mutex is not poisoned");
        let graph = driver
            .as_ref()
            .ok_or_else(|| GraphSourceError("Neo4j executor was cancelled".to_string()))?;
        match self
            .runtime
            .block_on(async { tokio::time::timeout(deadline, run_bolt_query(graph, query)).await })
        {
            Ok(result) => result,
            Err(_) => {
                // Dropping the sole graph/pool owner closes the connection
                // whose read future was just cancelled.
                driver.take();
                Err(GraphSourceError(
                    "Neo4j query exceeded the acquisition deadline".to_string(),
                ))
            }
        }
    }
}

async fn run_bolt_query(
    graph: &Graph,
    query: BoundQuery,
) -> Result<Vec<BTreeMap<String, GraphValue>>, GraphSourceError> {
    let mut cypher = neo4rs::query(query.text);
    for (key, value) in query.parameters {
        cypher = cypher.param(&key, to_bolt(value));
    }
    let mut stream = graph
        .execute(cypher)
        .await
        .map_err(|error| GraphSourceError(format!("Neo4j query failed: {error}")))?;
    let mut rows = Vec::new();
    while let Some(row) = stream
        .next()
        .await
        .map_err(|error| GraphSourceError(format!("Neo4j result failed: {error}")))?
    {
        let values = row
            .to_strict::<BTreeMap<String, BoltType>>()
            .map_err(|error| GraphSourceError(format!("Neo4j returned an invalid row: {error}")))?
            .into_iter()
            .map(|(key, value)| Ok((key, from_bolt(value)?)))
            .collect::<Result<BTreeMap<_, _>, GraphSourceError>>()?;
        rows.push(values);
    }
    Ok(rows)
}

fn to_bolt(value: GraphValue) -> BoltType {
    match value {
        GraphValue::Null => BoltType::Null(BoltNull),
        GraphValue::Integer(value) => BoltType::Integer(value.into()),
        GraphValue::String(value) => BoltType::String(value.into()),
        GraphValue::List(values) => BoltType::List(BoltList::from(
            values.into_iter().map(to_bolt).collect::<Vec<_>>(),
        )),
    }
}

fn from_bolt(value: BoltType) -> Result<GraphValue, GraphSourceError> {
    match value {
        BoltType::Null(_) => Ok(GraphValue::Null),
        BoltType::Integer(value) => Ok(GraphValue::Integer(value.value)),
        BoltType::String(value) => Ok(GraphValue::String(value.value)),
        BoltType::List(values) => values
            .value
            .into_iter()
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
            let [
                GraphValue::Integer(dimension),
                GraphValue::String(target),
                GraphValue::String(kind),
            ] = values.as_slice()
            else {
                return Err(GraphSourceError(
                    "coordinate relationship has an invalid dimension or target".to_string(),
                ));
            };
            if *dimension < 1
                || target.is_empty()
                || !matches!(kind.as_str(), "path" | "spec")
                || !dimensions.insert(*dimension)
            {
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
        for (column, expected) in [
            ("owners", path),
            ("releases", required_string(row, "release")?),
        ] {
            match row.get(column) {
                Some(GraphValue::List(values))
                    if values.as_slice() == [GraphValue::String(expected)] => {}
                _ => {
                    return Err(GraphSourceError(format!(
                        "event must have exactly one matching {column} reference"
                    )));
                }
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

impl<E: CypherExecutor> super::GraphFactsSource for Neo4jScopeSource<E> {
    fn load_ids_facts(
        &self,
        ids: &str,
        attempt: &AcquisitionAttempt,
    ) -> Result<super::IdsGraphFacts, GraphSourceError> {
        super::neo4j_facts::decode(ids, self.load_raw_scope(ids, attempt)?, attempt)
    }
}

/// Connect only after the acquirer has started its one attempt.
pub(crate) struct Neo4jFactsSource(pub Neo4jConfig);
impl super::GraphFactsSource for Neo4jFactsSource {
    fn load_ids_facts(
        &self,
        ids: &str,
        attempt: &AcquisitionAttempt,
    ) -> Result<super::IdsGraphFacts, GraphSourceError> {
        let executor = BoltExecutor::connect(&self.0, attempt)?;
        Neo4jScopeSource::new(executor, self.0.page_size)?.load_ids_facts(ids, attempt)
    }
}
