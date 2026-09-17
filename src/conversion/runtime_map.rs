//! Controlled graph-fact acquisition for the first runtime-map tracer.
//!
//! This module is deliberately disconnected from occurrence opening.  It
//! proves that a complete IDS scope can become the existing resolver's map
//! without making graph transport or runtime source selection a production
//! concern.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

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

/// The default upper bound for one complete graph-backed map acquisition.
pub(crate) const DEFAULT_ACQUISITION_DEADLINE: Duration = Duration::from_secs(5);

/// A named cooperative boundary within one acquisition attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcquisitionStage {
    Connection,
    Source,
    Query,
    Decoding,
    ScopeValidation,
    RuleConstruction,
    MapValidation,
    Publication,
}

/// Monotonic time supplied to an acquisition attempt.
pub(crate) trait AcquisitionClock: Send + Sync {
    fn now(&self) -> Duration;
}

struct SystemClock {
    started: Instant,
}

impl SystemClock {
    fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl AcquisitionClock for SystemClock {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }
}

/// A test-only observation boundary around acquisition stages. Production
/// attempts use the no-op implementation, while controlled tests advance a
/// monotonic clock at a precise boundary instead of sleeping.
pub(crate) trait AttemptObserver: Send + Sync {
    fn entered(&self, stage: AcquisitionStage);
}

struct NoopAttemptObserver;

impl AttemptObserver for NoopAttemptObserver {
    fn entered(&self, _stage: AcquisitionStage) {}
}

/// A testable synchronization boundary for request joining. It observes no map
/// contents and production uses the no-op implementation.
pub(crate) trait CoordinatorObserver: Send + Sync {
    fn joined_attempt(&self);
}

struct NoopCoordinatorObserver;

impl CoordinatorObserver for NoopCoordinatorObserver {
    fn joined_attempt(&self) {}
}

/// One non-restartable deadline shared by acquisition, decoding, map
/// construction, validation and publication.
pub(crate) struct AcquisitionAttempt {
    deadline: Duration,
    clock: Arc<dyn AcquisitionClock>,
    started_at: Duration,
    observer: Arc<dyn AttemptObserver>,
    cancelled: AtomicBool,
    expired_stage: Mutex<Option<AcquisitionStage>>,
}

impl AcquisitionAttempt {
    pub(crate) fn new(
        deadline: Duration,
        clock: Arc<dyn AcquisitionClock>,
        observer: Arc<dyn AttemptObserver>,
    ) -> Self {
        let started_at = clock.now();
        Self {
            deadline,
            clock,
            started_at,
            observer,
            cancelled: AtomicBool::new(false),
            expired_stage: Mutex::new(None),
        }
    }

    /// Enters a stage and checks the same shared deadline both before and
    /// after controllable stage work begins.
    pub(crate) fn enter(&self, stage: AcquisitionStage) -> Result<Duration, AttemptExpired> {
        self.check(stage)?;
        self.observer.entered(stage);
        self.check(stage)
    }

    /// Checks cancellation without starting a new stage or refreshing time.
    pub(crate) fn check(&self, stage: AcquisitionStage) -> Result<Duration, AttemptExpired> {
        let elapsed = self.clock.now().saturating_sub(self.started_at);
        if self.cancelled.load(Ordering::Acquire) || elapsed >= self.deadline {
            let mut expired_stage = self
                .expired_stage
                .lock()
                .expect("acquisition attempt mutex is not poisoned");
            let stage = *expired_stage.get_or_insert(stage);
            self.cancelled.store(true, Ordering::Release);
            return Err(AttemptExpired { stage });
        }
        Ok(self.deadline.saturating_sub(elapsed))
    }

    fn timeout_failure(&self) -> Option<AcquisitionFailure> {
        self.expired_stage
            .lock()
            .expect("acquisition attempt mutex is not poisoned")
            .map(|stage| AcquisitionFailure::TimedOut { stage })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AttemptExpired {
    pub(crate) stage: AcquisitionStage,
}

impl From<AttemptExpired> for AcquisitionFailure {
    fn from(expired: AttemptExpired) -> Self {
        Self::TimedOut {
            stage: expired.stage,
        }
    }
}

/// A graph transport or query failure, intentionally distinct from a fact or
/// construction failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphSourceError(pub String);

/// Internal dependency supplying one complete IDS scope. Implementations use
/// the supplied attempt for every transport and local stage; they must not
/// substitute a new deadline for its remaining time.
pub(crate) trait GraphFactsSource {
    fn load_ids_facts(
        &self,
        ids: &str,
        attempt: &AcquisitionAttempt,
    ) -> Result<IdsGraphFacts, GraphSourceError>;
}

/// Why acquisition could not return a complete validated map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AcquisitionFailure {
    TimedOut {
        stage: AcquisitionStage,
    },
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
    deadline: Duration,
    clock: Arc<dyn AcquisitionClock>,
    observer: Arc<dyn AttemptObserver>,
}

impl<S> RuntimeMapAcquirer<S> {
    pub(crate) fn new(source: S) -> Self {
        Self::with_clock_and_observer(
            source,
            DEFAULT_ACQUISITION_DEADLINE,
            Arc::new(SystemClock::new()),
            Arc::new(NoopAttemptObserver),
        )
    }

    /// Uses a caller-selected whole-attempt deadline with the production
    /// monotonic clock. The configured duration is not a per-query timeout.
    pub(crate) fn with_deadline(source: S, deadline: Duration) -> Self {
        Self::with_clock_and_observer(
            source,
            deadline,
            Arc::new(SystemClock::new()),
            Arc::new(NoopAttemptObserver),
        )
    }

    pub(crate) fn with_clock_and_observer(
        source: S,
        deadline: Duration,
        clock: Arc<dyn AcquisitionClock>,
        observer: Arc<dyn AttemptObserver>,
    ) -> Self {
        Self {
            source,
            deadline,
            clock,
            observer,
        }
    }
}

impl<S: GraphFactsSource> RuntimeMapAcquirer<S> {
    pub(crate) fn acquire(
        &self,
        request: &MapRequest,
    ) -> Result<ConversionMap, AcquisitionFailure> {
        let attempt = AcquisitionAttempt::new(
            self.deadline,
            Arc::clone(&self.clock),
            Arc::clone(&self.observer),
        );
        self.acquire_with_attempt(request, &attempt)
    }

    /// Completes one request using the caller-owned attempt. This is the
    /// coordinator seam: a joiner must use the attempt it joined rather than
    /// silently resetting its deadline.
    fn acquire_with_attempt(
        &self,
        request: &MapRequest,
        attempt: &AcquisitionAttempt,
    ) -> Result<ConversionMap, AcquisitionFailure> {
        attempt.enter(AcquisitionStage::Source).map_err(|expired| {
            AcquisitionFailure::TimedOut {
                stage: expired.stage,
            }
        })?;
        let facts = match self.source.load_ids_facts(&request.ids, attempt) {
            Ok(facts) => {
                attempt.check(AcquisitionStage::Source)?;
                facts
            }
            Err(source) => match attempt.check(AcquisitionStage::Source) {
                Ok(_) => return Err(AcquisitionFailure::Source(source)),
                Err(expired) => return Err(expired.into()),
            },
        };
        attempt
            .enter(AcquisitionStage::ScopeValidation)
            .map_err(|expired| AcquisitionFailure::TimedOut {
                stage: expired.stage,
            })?;
        validate_complete_scope(&facts, request, attempt)?;

        attempt
            .enter(AcquisitionStage::RuleConstruction)
            .map_err(|expired| AcquisitionFailure::TimedOut {
                stage: expired.stage,
            })?;
        let hli = graph_side(&facts.versions, &request.hli_dd, attempt)?;
        let stored = graph_side(&facts.versions, &request.stored_dd, attempt)?;
        let mut rules = Vec::with_capacity(facts.nodes.len());
        for node in &facts.nodes {
            attempt
                .check(AcquisitionStage::RuleConstruction)
                .map_err(|expired| AcquisitionFailure::TimedOut {
                    stage: expired.stage,
                })?;
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

        attempt
            .enter(AcquisitionStage::MapValidation)
            .map_err(|expired| AcquisitionFailure::TimedOut {
                stage: expired.stage,
            })?;
        let map = ConversionMap::from_typed(TypedConversionMap {
            ids: request.ids.clone(),
            left: Some(hli),
            right: Some(stored),
            left_endpoint: endpoint_inventory(&facts.nodes, &request.hli_dd, attempt)?,
            right_endpoint: endpoint_inventory(&facts.nodes, &request.stored_dd, attempt)?,
            default_identical: false,
            rules,
            sign_flips: Vec::new(),
            redefines: Vec::new(),
        })
        .map_err(|error| {
            attempt
                .timeout_failure()
                .unwrap_or(AcquisitionFailure::Construction(error))
        })?;
        attempt
            .enter(AcquisitionStage::Publication)
            .map_err(|expired| AcquisitionFailure::TimedOut {
                stage: expired.stage,
            })?;
        Ok(map)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MapCacheKey {
    ids: String,
    stored_dd: String,
    hli_dd: String,
}

impl From<&MapRequest> for MapCacheKey {
    fn from(request: &MapRequest) -> Self {
        Self {
            ids: request.ids.clone(),
            stored_dd: request.stored_dd.to_string(),
            hli_dd: request.hli_dd.to_string(),
        }
    }
}

type MapAcquisitionResult = Result<Arc<ConversionMap>, AcquisitionFailure>;

struct SharedMapAttempt {
    attempt: Arc<AcquisitionAttempt>,
    result: Mutex<Option<MapAcquisitionResult>>,
    completed: Condvar,
}

impl SharedMapAttempt {
    fn new(attempt: AcquisitionAttempt) -> Self {
        Self {
            attempt: Arc::new(attempt),
            result: Mutex::new(None),
            completed: Condvar::new(),
        }
    }

    fn publish(&self, result: MapAcquisitionResult) {
        let mut published = self
            .result
            .lock()
            .expect("shared map-attempt mutex is not poisoned");
        if published.is_none() {
            *published = Some(result);
            self.completed.notify_all();
        }
    }

    fn wait(&self) -> MapAcquisitionResult {
        let mut published = self
            .result
            .lock()
            .expect("shared map-attempt mutex is not poisoned");
        while published.is_none() {
            let remaining = self.attempt.check(AcquisitionStage::Publication)?;
            let (next, timeout) = self
                .completed
                .wait_timeout(published, remaining)
                .expect("shared map-attempt mutex is not poisoned");
            published = next;
            if published.is_none() && timeout.timed_out() {
                // Record the timeout on the shared attempt before returning
                // it. A leader that finishes source work later then observes
                // this same terminal stage instead of publishing a different
                // timeout reason to the callers that joined it.
                if let Err(expired) = self.attempt.check(AcquisitionStage::Publication) {
                    return Err(expired.into());
                }
                return Err(AcquisitionFailure::TimedOut {
                    stage: AcquisitionStage::Publication,
                });
            }
        }
        published
            .as_ref()
            .expect("shared map attempt must publish before waking waiters")
            .clone()
    }
}

#[derive(Default)]
struct CoordinatorState {
    maps: HashMap<MapCacheKey, Arc<ConversionMap>>,
    attempts: HashMap<MapCacheKey, Arc<SharedMapAttempt>>,
}

enum CoordinatorDecision {
    Lead(Arc<SharedMapAttempt>),
    Join(Arc<SharedMapAttempt>),
    Expired(Arc<SharedMapAttempt>, AcquisitionFailure),
}

/// Shares one complete acquisition attempt per exact map key and keeps only
/// successful maps for the process lifetime. All graph work and all waits are
/// outside the short coordinator mutex critical sections.
pub(crate) struct RuntimeMapCoordinator<S> {
    acquirer: RuntimeMapAcquirer<S>,
    state: Mutex<CoordinatorState>,
    observer: Arc<dyn CoordinatorObserver>,
}

impl<S> RuntimeMapCoordinator<S> {
    pub(crate) fn new(source: S) -> Self {
        Self {
            acquirer: RuntimeMapAcquirer::new(source),
            state: Mutex::new(CoordinatorState::default()),
            observer: Arc::new(NoopCoordinatorObserver),
        }
    }

    pub(crate) fn with_clock_and_observer(
        source: S,
        deadline: Duration,
        clock: Arc<dyn AcquisitionClock>,
        observer: Arc<dyn AttemptObserver>,
    ) -> Self {
        Self {
            acquirer: RuntimeMapAcquirer::with_clock_and_observer(
                source, deadline, clock, observer,
            ),
            state: Mutex::new(CoordinatorState::default()),
            observer: Arc::new(NoopCoordinatorObserver),
        }
    }

    pub(crate) fn with_observers(
        source: S,
        deadline: Duration,
        clock: Arc<dyn AcquisitionClock>,
        attempt_observer: Arc<dyn AttemptObserver>,
        coordinator_observer: Arc<dyn CoordinatorObserver>,
    ) -> Self {
        Self {
            acquirer: RuntimeMapAcquirer::with_clock_and_observer(
                source,
                deadline,
                clock,
                attempt_observer,
            ),
            state: Mutex::new(CoordinatorState::default()),
            observer: coordinator_observer,
        }
    }
}

impl<S: GraphFactsSource> RuntimeMapCoordinator<S> {
    /// Acquires a retained map or joins the exact in-flight attempt already
    /// responsible for this IDS and direction. A failure is delivered to all
    /// joiners but deliberately never stored as a reusable cache result.
    pub(crate) fn acquire(&self, request: &MapRequest) -> MapAcquisitionResult {
        let key = MapCacheKey::from(request);
        let decision = {
            let mut state = self
                .state
                .lock()
                .expect("runtime map coordinator mutex is not poisoned");
            if let Some(map) = state.maps.get(&key) {
                return Ok(Arc::clone(map));
            }
            if let Some(attempt) = state.attempts.get(&key) {
                let attempt = Arc::clone(attempt);
                match attempt.attempt.check(AcquisitionStage::Publication) {
                    Ok(_) => CoordinatorDecision::Join(attempt),
                    Err(expired) => {
                        state.attempts.remove(&key);
                        CoordinatorDecision::Expired(attempt, expired.into())
                    }
                }
            } else {
                let attempt = Arc::new(SharedMapAttempt::new(AcquisitionAttempt::new(
                    self.acquirer.deadline,
                    Arc::clone(&self.acquirer.clock),
                    Arc::clone(&self.acquirer.observer),
                )));
                state.attempts.insert(key.clone(), Arc::clone(&attempt));
                CoordinatorDecision::Lead(attempt)
            }
        };

        let attempt = match decision {
            CoordinatorDecision::Lead(attempt) => attempt,
            CoordinatorDecision::Join(attempt) => {
                self.observer.joined_attempt();
                let result = attempt.wait();
                if let Err(failure) = &result {
                    attempt.publish(Err(failure.clone()));
                    let mut state = self
                        .state
                        .lock()
                        .expect("runtime map coordinator mutex is not poisoned");
                    if state
                        .attempts
                        .get(&key)
                        .is_some_and(|current| Arc::ptr_eq(current, &attempt))
                    {
                        state.attempts.remove(&key);
                    }
                }
                return result;
            }
            CoordinatorDecision::Expired(attempt, expired) => {
                attempt.publish(Err(expired.clone()));
                return Err(expired);
            }
        };

        let result = self
            .acquirer
            .acquire_with_attempt(request, &attempt.attempt)
            .map(Arc::new);
        attempt.publish(result.clone());

        let mut state = self
            .state
            .lock()
            .expect("runtime map coordinator mutex is not poisoned");
        if state
            .attempts
            .get(&key)
            .is_some_and(|current| Arc::ptr_eq(current, &attempt))
        {
            state.attempts.remove(&key);
            if let Ok(map) = &result {
                state.maps.insert(key, Arc::clone(map));
            }
        }
        result
    }
}

fn validate_complete_scope(
    facts: &IdsGraphFacts,
    request: &MapRequest,
    attempt: &AcquisitionAttempt,
) -> Result<(), AcquisitionFailure> {
    attempt
        .check(AcquisitionStage::ScopeValidation)
        .map_err(|expired| AcquisitionFailure::TimedOut {
            stage: expired.stage,
        })?;
    if !facts.complete {
        return Err(AcquisitionFailure::IncompleteScope);
    }
    if facts.nodes.is_empty() {
        return Err(AcquisitionFailure::EmptyScope);
    }

    let mut releases = Vec::new();
    for version in &facts.versions {
        attempt
            .check(AcquisitionStage::ScopeValidation)
            .map_err(|expired| AcquisitionFailure::TimedOut {
                stage: expired.stage,
            })?;
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
        attempt
            .check(AcquisitionStage::ScopeValidation)
            .map_err(|expired| AcquisitionFailure::TimedOut {
                stage: expired.stage,
            })?;
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
            attempt
                .check(AcquisitionStage::ScopeValidation)
                .map_err(|expired| AcquisitionFailure::TimedOut {
                    stage: expired.stage,
                })?;
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
        attempt
            .check(AcquisitionStage::ScopeValidation)
            .map_err(|expired| AcquisitionFailure::TimedOut {
                stage: expired.stage,
            })?;
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
        attempt
            .check(AcquisitionStage::ScopeValidation)
            .map_err(|expired| AcquisitionFailure::TimedOut {
                stage: expired.stage,
            })?;
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
    attempt: &AcquisitionAttempt,
) -> Result<Side, AcquisitionFailure> {
    for version in versions {
        attempt
            .check(AcquisitionStage::RuleConstruction)
            .map_err(|expired| AcquisitionFailure::TimedOut {
                stage: expired.stage,
            })?;
        if version.release == *requested {
            return Ok(Side {
                dd: version.release.clone(),
                cocos: version.cocos.clone(),
            });
        }
    }
    Err(AcquisitionFailure::MissingRequestedRelease {
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

/// Adapts the complete endpoint metadata stream into the existing map's
/// delete-safety inventory. Presence at both requested endpoints was checked
/// before construction, so this preserves structures as structures rather
/// than treating every graph row as a leaf.
fn endpoint_inventory(
    nodes: &[GraphNode],
    requested: &ArtifactDdVersion,
    attempt: &AcquisitionAttempt,
) -> Result<EndpointInventory, AcquisitionFailure> {
    nodes
        .iter()
        .map(|node| {
            attempt
                .check(AcquisitionStage::MapValidation)
                .map_err(|expired| AcquisitionFailure::TimedOut {
                    stage: expired.stage,
                })?;
            let metadata = endpoint_for(node, requested)?;
            Ok(EndpointNode {
                path: node.path.clone(),
                kind: match metadata.kind {
                    GraphNodeKind::Leaf => EndpointNodeKind::Leaf,
                    GraphNodeKind::Structure => EndpointNodeKind::Structure,
                },
            })
        })
        .collect::<Result<Vec<_>, AcquisitionFailure>>()
        .map(EndpointInventory::complete)
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
