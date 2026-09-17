//! Controlled graph-fact acquisition for the first runtime-map tracer.
//!
//! This module is deliberately disconnected from occurrence opening.  It
//! proves that a complete IDS scope can become the existing resolver's map
//! without making graph transport or runtime source selection a production
//! concern.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use super::conversion_map::{
    ArtifactDdVersion, CocosConvention, ConversionMap, EndpointInventory, EndpointNode,
    EndpointNodeKind, Fidelity, LoadError, Rel, SelectorStage, Side, TypedConversionMap, TypedRule,
};

#[cfg(feature = "graph-test-source")]
pub(crate) mod graph_test_source;
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
    /// Lifecycle edges supplement the event ledger without collapsing a
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
    /// Wire values remain strings and are decoded only for their qualified
    /// field; replay never evaluates them.
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
        if self.cancelled.load(AtomicOrdering::Acquire) || elapsed >= self.deadline {
            let mut expired_stage = self
                .expired_stage
                .lock()
                .expect("acquisition attempt mutex is not poisoned");
            let stage = *expired_stage.get_or_insert(stage);
            self.cancelled.store(true, AtomicOrdering::Release);
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
        let mut hli_endpoint = Vec::with_capacity(facts.nodes.len());
        let mut stored_endpoint = Vec::with_capacity(facts.nodes.len());
        let mut endpoint_evidence_complete = true;
        for node in &facts.nodes {
            attempt
                .check(AcquisitionStage::RuleConstruction)
                .map_err(|expired| AcquisitionFailure::TimedOut {
                    stage: expired.stage,
                })?;
            let hli_metadata = replay_endpoint(&facts, node, &request.hli_dd)?;
            let stored_metadata = replay_endpoint(&facts, node, &request.stored_dd)?;
            let (rel, left, right, fidelity) = match (&hli_metadata, &stored_metadata) {
                (
                    EndpointState::Present {
                        metadata: hli_metadata,
                        interval_start: hli_start,
                    },
                    EndpointState::Present {
                        metadata: stored_metadata,
                        interval_start: stored_start,
                    },
                ) => {
                    hli_endpoint.push(endpoint_node(node, hli_metadata));
                    stored_endpoint.push(endpoint_node(node, stored_metadata));
                    if hli_start != stored_start
                        || has_cocos_evidence(hli_metadata, stored_metadata)
                    {
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
                    } else {
                        (
                            Rel::Retyped,
                            Some(node.path.clone()),
                            Some(node.path.clone()),
                            Fidelity::Unmappable,
                        )
                    }
                }
                (EndpointState::Present { metadata, .. }, EndpointState::Absent) => {
                    hli_endpoint.push(endpoint_node(node, metadata));
                    (
                        Rel::LeftOnly,
                        Some(node.path.clone()),
                        None,
                        Fidelity::Unmappable,
                    )
                }
                (EndpointState::Absent, EndpointState::Present { metadata, .. }) => {
                    stored_endpoint.push(endpoint_node(node, metadata));
                    (
                        Rel::RightOnly,
                        None,
                        Some(node.path.clone()),
                        Fidelity::Unmappable,
                    )
                }
                (EndpointState::Absent, EndpointState::Absent) => continue,
                _ => {
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

        attempt
            .enter(AcquisitionStage::MapValidation)
            .map_err(|expired| AcquisitionFailure::TimedOut {
                stage: expired.stage,
            })?;
        let map = ConversionMap::from_typed(TypedConversionMap {
            ids: request.ids.clone(),
            left: Some(hli),
            right: Some(stored),
            left_endpoint: endpoint_inventory(hli_endpoint, endpoint_evidence_complete),
            right_endpoint: endpoint_inventory(stored_endpoint, endpoint_evidence_complete),
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
        .expect("present interval has a start");
    let Some(mut metadata) = node
        .endpoints
        .iter()
        .find(|metadata| metadata.release == releases[interval_start])
        .cloned()
    else {
        return Ok(EndpointState::Unanchored);
    };
    for release in &releases[interval_start + 1..=requested_index] {
        for event in metadata_events_at(facts, node, release) {
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

fn sorted_releases(versions: &[GraphVersion]) -> Vec<ArtifactDdVersion> {
    let mut releases: Vec<_> = versions
        .iter()
        .map(|version| version.release.clone())
        .collect();
    releases.sort_by_key(numeric_release);
    releases
}

fn numeric_release(release: &ArtifactDdVersion) -> Vec<u32> {
    release
        .to_string()
        .split('.')
        .map(|part| part.parse().expect("validated release component"))
        .collect()
}

fn presence_timeline(
    facts: &IdsGraphFacts,
    node: &GraphNode,
    releases: &[ArtifactDdVersion],
) -> Result<Vec<bool>, AcquisitionFailure> {
    let mut present = false;
    let mut result = Vec::with_capacity(releases.len());
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
        result.push(present);
    }
    Ok(result)
}

fn rename_paths(event: &GraphEvent, ids: &str) -> Result<(String, String), AcquisitionFailure> {
    let (Some(old), Some(new)) = (event.old_value.as_deref(), event.new_value.as_deref()) else {
        return Err(invalid_event(event));
    };
    Ok((strip_ids_prefix(old, ids), strip_ids_prefix(new, ids)))
}

fn strip_ids_prefix(path: &str, ids: &str) -> String {
    path.strip_prefix(ids)
        .and_then(|remainder| remainder.strip_prefix('/'))
        .unwrap_or(path)
        .to_string()
}

fn metadata_events_at<'a>(
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
    let (Some(old), Some(new)) = (event.old_value.as_deref(), event.new_value.as_deref()) else {
        return Err(invalid_event(event));
    };
    if metadata_value(metadata, field)? != old {
        return Err(AcquisitionFailure::ContradictoryHistory {
            path: event.path.clone(),
            field: field.to_string(),
            release: event.release.clone(),
        });
    }
    set_metadata_value(metadata, field, new, &event.id)
}

fn event_field(event: &GraphEvent) -> Result<&str, AcquisitionFailure> {
    let field = event.id.rsplit(':').nth(1).unwrap_or_default();
    if field.is_empty() || (!event.field.is_empty() && event.field != field) {
        return Err(invalid_event(event));
    }
    match field {
        "data_type" | "ndim" | "units" | "timebase" | "coordinates" => Ok(field),
        "documentation" | "lifecycle_status" | "maxoccur" | "identifier_enum" => Ok("ignored"),
        _ => Err(invalid_event(event)),
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
        _ => unreachable!(),
    }
}

fn set_metadata_value(
    metadata: &mut EndpointMetadata,
    field: &str,
    value: &str,
    id: &str,
) -> Result<(), AcquisitionFailure> {
    match field {
        "data_type" if !value.is_empty() => metadata.data_type = value.to_string(),
        "ndim" => {
            metadata.ndim = value
                .parse()
                .map_err(|_| AcquisitionFailure::InvalidEventValue { id: id.to_string() })?
        }
        "units" => metadata.unit = (!value.is_empty()).then(|| value.to_string()),
        "timebase" => metadata.timebase_path = (!value.is_empty()).then(|| value.to_string()),
        "coordinates" => metadata.coordinate_paths = parse_string_list(value, id)?,
        "ignored" => {}
        _ => return Err(AcquisitionFailure::InvalidEventValue { id: id.to_string() }),
    }
    Ok(())
}

fn render_list(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn parse_string_list(value: &str, id: &str) -> Result<Vec<String>, AcquisitionFailure> {
    let bytes = value.as_bytes();
    if bytes.first() != Some(&b'[') || bytes.last() != Some(&b']') {
        return Err(AcquisitionFailure::InvalidEventValue { id: id.to_string() });
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
            return Err(AcquisitionFailure::InvalidEventValue { id: id.to_string() });
        }
        index += 1;
        let mut item = String::new();
        while index < bytes.len() - 1 && bytes[index] != quote {
            if bytes[index] == b'\\' {
                index += 1;
                if index == bytes.len() - 1 {
                    return Err(AcquisitionFailure::InvalidEventValue { id: id.to_string() });
                }
            }
            item.push(bytes[index] as char);
            index += 1;
        }
        if index == bytes.len() - 1 {
            return Err(AcquisitionFailure::InvalidEventValue { id: id.to_string() });
        }
        index += 1;
        values.push(item);
        while index < bytes.len() - 1 && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index < bytes.len() - 1 {
            if bytes[index] != b',' {
                return Err(AcquisitionFailure::InvalidEventValue { id: id.to_string() });
            }
            index += 1;
        }
    }
    Ok(values)
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

fn invalid_event(event: &GraphEvent) -> AcquisitionFailure {
    AcquisitionFailure::InvalidEventValue {
        id: event.id.clone(),
    }
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
