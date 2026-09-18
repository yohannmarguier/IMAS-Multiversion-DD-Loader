//! Controlled graph-fact acquisition for the first runtime-map tracer.
//!
//! This module is deliberately disconnected from occurrence opening.  It
//! proves that a complete IDS scope can become the existing resolver's map
//! without making graph transport or runtime source selection a production
//! concern.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use super::conversion_map::{
    ArtifactDdVersion, CocosConvention, ConversionMap, EndpointInventory, EndpointNode,
    EndpointNodeKind, Fidelity, LoadError, Rel, SelectorStage, Side, TypedConversionMap,
    TypedFromEntry, TypedRedefine, TypedRule, TypedSignFlip,
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

/// Provenance attached to a COCOS label in the selected graph snapshot.
/// Unsupported sources stay distinct from a missing source so neither can
/// accidentally certify a factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CocosLabelSource {
    Xml,
    InferredSignFlip,
    InferredExpression,
    Other,
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
    /// endpoint. A compound expression is not executable evidence for this
    /// shim: supported factors come from a known class and conventions.
    pub cocos_transformation_expression: Option<String>,
    /// The graph's provenance for `cocos_label_transformation`. A backfilled
    /// label and a raw declaration are different evidence forms, so a label
    /// without an accepted source never certifies a factor.
    pub cocos_label_source: Option<CocosLabelSource>,
}

/// One unversioned `HAS_COORDINATE` fact retained beside a node's versioned
/// endpoint metadata. It can corroborate an exactly spelled raw declaration,
/// but cannot erase the raw history's index notation or prove absence when it
/// is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoordinateRelationship {
    pub dimension: usize,
    pub target_path: String,
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
    /// Dated predecessor names declared by this node's NBC history.  The
    /// spelling is kept on the declaring (newer) node because a local name is
    /// relative to that node's parent, not to the IDS root.
    pub rename_declarations: Vec<GraphRename>,
    pub coordinate_relationships: Vec<CoordinateRelationship>,
    pub endpoints: Vec<EndpointMetadata>,
}

/// One dated previous-name declaration from a node's NBC history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphRename {
    pub release: ArtifactDdVersion,
    pub previous_name: String,
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
    /// The producer's unit-change classification, present only for a
    /// field-qualified `units` event. The endpoint unit strings themselves
    /// do not establish whether changing a declaration changes values.
    pub unit_change: Option<UnitChangeEvidence>,
    /// Coordinate/timebase behaviour established by the producer for this
    /// event. Raw coordinate declarations and current relationship targets
    /// are not enough to manufacture this verdict: their omission and their
    /// differing index notation are known limits of the graph snapshot.
    pub coordinate_evidence: Option<CoordinateChangeEvidence>,
}

/// Value-behaviour evidence attached to one producer-classified unit event.
///
/// Cosmetic spelling and sentinel resolution are declaration-only. Dimensional
/// compatibility proves neither scale nor offset, while confirmed scale or
/// offset evidence establishes a numerical transformation this shim cannot
/// run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnitChangeEvidence {
    /// Textual spelling changes without changing values.
    Cosmetic,
    /// A known sentinel spelling is resolved to its declaration.
    SentinelResolved,
    /// Dimensions match, but value scale and offset remain unknown.
    DimensionallyCompatible,
    /// Evidence establishes a numerical scale or offset this shim cannot apply.
    RequiredScaleOrOffset,
}

/// Conversion behaviour established for a coordinate or timebase change.
///
/// The shim can preserve a proven equivalent representation, but it has no
/// resampler. An unbounded finding is deliberately an acquisition failure:
/// marking every potentially affected endpoint safe would hide uncertainty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoordinateChangeEvidence {
    Equivalent,
    RequiresResampling,
    UnboundedScope,
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
    UnboundedCoordinateScope {
        path: String,
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
        let direct_renames = direct_renames(&facts, request, attempt)?;
        let coexistence = coexistence_plans(&facts, request, attempt)?;
        let coexistence_paths = coexistence.paths.clone();
        validate_coordinate_scope(&facts, &request.hli_dd, &request.stored_dd, attempt)?;
        let mut rules = coexistence.rules;
        let mut hli_endpoint = Vec::with_capacity(facts.nodes.len());
        let mut stored_endpoint = Vec::with_capacity(facts.nodes.len());
        let mut redefines = Vec::new();
        let mut sign_flips = Vec::new();
        let mut endpoint_evidence_complete = true;
        let mut retyped_anchors = Vec::new();
        for node in &facts.nodes {
            if coexistence_paths.contains(&node.path) {
                add_endpoint_node(
                    &mut hli_endpoint,
                    node,
                    &replay_endpoint(&facts, node, &request.hli_dd)?,
                );
                add_endpoint_node(
                    &mut stored_endpoint,
                    node,
                    &replay_endpoint(&facts, node, &request.stored_dd)?,
                );
            }
        }
        for node in &facts.nodes {
            attempt
                .check(AcquisitionStage::RuleConstruction)
                .map_err(|expired| AcquisitionFailure::TimedOut {
                    stage: expired.stage,
                })?;
            if let (
                EndpointState::Present {
                    metadata: hli_metadata,
                    ..
                },
                EndpointState::Present {
                    metadata: stored_metadata,
                    ..
                },
            ) = (
                replay_endpoint(&facts, node, &request.hli_dd)?,
                replay_endpoint(&facts, node, &request.stored_dd)?,
            ) && metadata_is_retyped(&hli_metadata, &stored_metadata)
            {
                retyped_anchors.push(node.path.as_str());
            }
        }
        for node in &facts.nodes {
            attempt
                .check(AcquisitionStage::RuleConstruction)
                .map_err(|expired| AcquisitionFailure::TimedOut {
                    stage: expired.stage,
                })?;
            let hli_metadata = replay_endpoint(&facts, node, &request.hli_dd)?;
            let stored_metadata = replay_endpoint(&facts, node, &request.stored_dd)?;
            if coexistence_paths.contains(&node.path) {
                continue;
            }
            if let Some(rename) = direct_renames
                .iter()
                .find(|rename| rename.left == node.path)
            {
                add_endpoint_node(&mut hli_endpoint, node, &hli_metadata);
                add_endpoint_node(&mut stored_endpoint, node, &stored_metadata);
                rules.push(TypedRule {
                    id: format!("rename:{}:{}", rename.left, rename.right),
                    rel: rename.rel,
                    selector_stage: rename.selector_stage,
                    left: Some(rename.left.clone()),
                    right: Some(rename.right.clone()),
                    froms: Vec::new(),
                    fidelity_forward: Fidelity::Exact,
                    fidelity_reverse: Fidelity::Exact,
                });
                continue;
            }
            if direct_renames
                .iter()
                .any(|rename| rename.right == node.path)
            {
                add_endpoint_node(&mut hli_endpoint, node, &hli_metadata);
                add_endpoint_node(&mut stored_endpoint, node, &stored_metadata);
                continue;
            }
            let inherits_retyped_anchor = retyped_anchors
                .iter()
                .any(|anchor| path_is_descendant_of(&node.path, anchor));
            let (rel, selector_stage, left, right, fidelity) =
                match (&hli_metadata, &stored_metadata) {
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
                        let cocos_evidence = collect_cocos_evidence(
                            &facts,
                            node,
                            hli_metadata,
                            stored_metadata,
                            &request.hli_dd,
                            &request.stored_dd,
                        )?;
                        let cocos_factor = derive_cocos_factor(
                            hli_metadata,
                            stored_metadata,
                            hli.cocos.as_ref(),
                            stored.cocos.as_ref(),
                            &cocos_evidence,
                        );
                        if matches!(
                            cocos_factor,
                            CocosFactorResolution::Unresolved | CocosFactorResolution::Unsupported
                        ) {
                            (
                                Rel::Identical,
                                SelectorStage::Exact,
                                Some(node.path.clone()),
                                Some(node.path.clone()),
                                Fidelity::Unmappable,
                            )
                        } else {
                            let relation = if hli_start != stored_start {
                                // A reused spelling starts a distinct historical role.
                                // Unit evidence cannot establish that the roles carry
                                // the same value semantics.
                                EndpointRelation::Unresolved
                            } else {
                                endpoint_relation(
                                    &facts,
                                    node,
                                    hli_metadata,
                                    stored_metadata,
                                    &direct_renames,
                                )
                            };
                            match relation {
                                EndpointRelation::Retyped => (
                                    Rel::Retyped,
                                    SelectorStage::Subtree,
                                    Some(node.path.clone()),
                                    Some(node.path.clone()),
                                    Fidelity::Unmappable,
                                ),
                                EndpointRelation::UnitRedefinition => {
                                    redefines.push(TypedRedefine {
                                        glob: node.path.clone(),
                                        fidelity_forward: Fidelity::Exact,
                                        fidelity_reverse: Fidelity::Exact,
                                    });
                                    (
                                        Rel::Identical,
                                        SelectorStage::Exact,
                                        Some(node.path.clone()),
                                        Some(node.path.clone()),
                                        Fidelity::Exact,
                                    )
                                }
                                EndpointRelation::Exact => {
                                    if let CocosFactorResolution::SignFlip {
                                        from_cocos,
                                        to_cocos,
                                    } = cocos_factor
                                    {
                                        sign_flips.push(TypedSignFlip {
                                            path: node.path.clone(),
                                            from_cocos,
                                            to_cocos,
                                        });
                                    }
                                    (
                                        Rel::Identical,
                                        SelectorStage::Exact,
                                        Some(node.path.clone()),
                                        Some(node.path.clone()),
                                        Fidelity::Exact,
                                    )
                                }
                                EndpointRelation::CoordinateResampling
                                | EndpointRelation::CoordinateUnresolved
                                | EndpointRelation::Unresolved => (
                                    Rel::Identical,
                                    SelectorStage::Exact,
                                    Some(node.path.clone()),
                                    Some(node.path.clone()),
                                    Fidelity::Unmappable,
                                ),
                            }
                        }
                    }
                    (EndpointState::Present { metadata, .. }, EndpointState::Absent) => {
                        hli_endpoint.push(endpoint_node(node, metadata));
                        if inherits_retyped_anchor {
                            // The parent retype is a subtree refusal. A one-sided
                            // descendant must not shadow it with a generic absence.
                            continue;
                        }
                        (
                            Rel::LeftOnly,
                            SelectorStage::Exact,
                            Some(node.path.clone()),
                            None,
                            Fidelity::Unmappable,
                        )
                    }
                    (EndpointState::Absent, EndpointState::Present { metadata, .. }) => {
                        stored_endpoint.push(endpoint_node(node, metadata));
                        if inherits_retyped_anchor {
                            // See the matching left-only case above.
                            continue;
                        }
                        (
                            Rel::RightOnly,
                            SelectorStage::Exact,
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
                            SelectorStage::Exact,
                            Some(node.path.clone()),
                            Some(node.path.clone()),
                            Fidelity::Unmappable,
                        )
                    }
                };
            let rule_id = match (&hli_metadata, &stored_metadata) {
                (
                    EndpointState::Present {
                        metadata: hli_metadata,
                        interval_start: hli_start,
                    },
                    EndpointState::Present {
                        metadata: stored_metadata,
                        interval_start: stored_start,
                    },
                ) if hli_start == stored_start => endpoint_rule_id(
                    endpoint_relation(&facts, node, hli_metadata, stored_metadata, &direct_renames),
                    node,
                ),
                _ => None,
            };
            rules.push(TypedRule {
                id: rule_id.unwrap_or_else(|| format!("endpoint:{}", node.path)),
                rel,
                selector_stage,
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
            sign_flips,
            redefines,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectRename {
    left: String,
    right: String,
    rel: Rel,
    selector_stage: SelectorStage,
}

struct CoexistencePlan {
    rules: Vec<TypedRule>,
    paths: HashSet<String>,
}

/// Builds a candidate plan only when a dated predecessor declaration and its
/// successor witness establish exactly one spelling on one endpoint and both
/// spellings on the other. The successor's date fixes precedence; graph row
/// order and similar names are never consulted.
fn coexistence_plans(
    facts: &IdsGraphFacts,
    request: &MapRequest,
    attempt: &AcquisitionAttempt,
) -> Result<CoexistencePlan, AcquisitionFailure> {
    let (earlier, later) = chronological_endpoints(request);
    let nodes: HashMap<_, _> = facts
        .nodes
        .iter()
        .map(|node| (node.path.as_str(), node))
        .collect();
    let mut rules = Vec::new();
    let mut paths = HashSet::new();

    for successor in &facts.nodes {
        for declaration in &successor.rename_declarations {
            attempt
                .check(AcquisitionStage::RuleConstruction)
                .map_err(|expired| AcquisitionFailure::TimedOut {
                    stage: expired.stage,
                })?;
            if numeric_release(&declaration.release) < numeric_release(earlier)
                || numeric_release(&declaration.release) > numeric_release(later)
            {
                continue;
            }
            let Some(predecessor_path) =
                normalize_previous_name(&declaration.previous_name, &successor.path, &request.ids)
            else {
                continue;
            };
            let Some(predecessor) = nodes.get(predecessor_path.as_str()) else {
                continue;
            };
            if !facts.successors.iter().any(|edge| {
                normalize_ids_path(&edge.from_path, &request.ids) == predecessor_path
                    && normalize_ids_path(&edge.to_path, &request.ids) == successor.path
            }) {
                continue;
            }

            let EndpointState::Present {
                metadata: predecessor_at_start,
                ..
            } = replay_endpoint(facts, predecessor, earlier)?
            else {
                continue;
            };
            let EndpointState::Present {
                metadata: successor_at_end,
                ..
            } = replay_endpoint(facts, successor, later)?
            else {
                continue;
            };
            // A path-only candidate is valid only when all the representation
            // facts it carries are identical. COCOS evidence deliberately
            // fails this check until candidate-specific transforms are built.
            let history_is_servable = same_representation(&predecessor_at_start, &successor_at_end);

            let predecessor_hli = replay_endpoint(facts, predecessor, &request.hli_dd)?;
            let successor_hli = replay_endpoint(facts, successor, &request.hli_dd)?;
            let predecessor_stored = replay_endpoint(facts, predecessor, &request.stored_dd)?;
            let successor_stored = replay_endpoint(facts, successor, &request.stored_dd)?;
            let hli_count = usize::from(matches!(predecessor_hli, EndpointState::Present { .. }))
                + usize::from(matches!(successor_hli, EndpointState::Present { .. }));
            let stored_count =
                usize::from(matches!(predecessor_stored, EndpointState::Present { .. }))
                    + usize::from(matches!(successor_stored, EndpointState::Present { .. }));
            if !matches!((hli_count, stored_count), (1, 2) | (2, 1)) {
                continue;
            }
            if !history_is_servable {
                return Err(invalid_node(
                    successor,
                    "a coexistence correspondence lacks a servable value representation",
                ));
            }

            let (sole_endpoint, candidate_endpoints) = if hli_count == 1 {
                (
                    (&predecessor_hli, &successor_hli),
                    [&predecessor_stored, &successor_stored],
                )
            } else {
                (
                    (&predecessor_stored, &successor_stored),
                    [&predecessor_hli, &successor_hli],
                )
            };
            let sole_metadata = match sole_endpoint {
                (EndpointState::Present { metadata, .. }, EndpointState::Absent)
                | (EndpointState::Absent, EndpointState::Present { metadata, .. }) => metadata,
                _ => unreachable!("one coexistence side has exactly one endpoint"),
            };
            if candidate_endpoints
                .iter()
                .filter_map(|state| match state {
                    EndpointState::Present { metadata, .. } => Some(metadata),
                    EndpointState::Absent => None,
                    EndpointState::Unanchored => None,
                })
                .any(|candidate| !same_representation(sole_metadata, candidate))
            {
                return Err(invalid_node(
                    successor,
                    "an endpoint-valid coexistence candidate lacks a servable representation",
                ));
            }

            let froms = vec![
                TypedFromEntry {
                    path: successor.path.clone(),
                    precedence: 1,
                },
                TypedFromEntry {
                    path: predecessor_path.clone(),
                    precedence: 2,
                },
            ];
            let rule = if hli_count == 1 {
                let left = if matches!(successor_hli, EndpointState::Present { .. }) {
                    successor.path.clone()
                } else {
                    predecessor_path.clone()
                };
                TypedRule {
                    id: format!("coexistence-split:{predecessor_path}:{}", successor.path),
                    rel: Rel::Split,
                    selector_stage: SelectorStage::Exact,
                    left: Some(left),
                    right: None,
                    froms,
                    fidelity_forward: Fidelity::Exact,
                    fidelity_reverse: Fidelity::Exact,
                }
            } else {
                let right = if matches!(successor_stored, EndpointState::Present { .. }) {
                    successor.path.clone()
                } else {
                    predecessor_path.clone()
                };
                TypedRule {
                    id: format!("coexistence-merged:{predecessor_path}:{}", successor.path),
                    rel: Rel::Merged,
                    selector_stage: SelectorStage::Exact,
                    left: None,
                    right: Some(right),
                    froms,
                    fidelity_forward: Fidelity::Exact,
                    fidelity_reverse: Fidelity::Exact,
                }
            };
            paths.extend([predecessor_path, successor.path.clone()]);
            rules.push(rule);
        }
    }
    Ok(CoexistencePlan { rules, paths })
}

fn add_endpoint_node(endpoint: &mut Vec<EndpointNode>, node: &GraphNode, state: &EndpointState) {
    if let EndpointState::Present { metadata, .. } = state {
        endpoint.push(endpoint_node(node, metadata));
    }
}

/// Identifies only a dated, direct predecessor declaration corroborated by
/// the flattened successor stream.  The successor is a witness, never a
/// substitute for the declaration's date or value semantics.
fn direct_renames(
    facts: &IdsGraphFacts,
    request: &MapRequest,
    attempt: &AcquisitionAttempt,
) -> Result<Vec<DirectRename>, AcquisitionFailure> {
    let (earlier, later) = chronological_endpoints(request);
    let nodes: HashMap<_, _> = facts
        .nodes
        .iter()
        .map(|node| (node.path.as_str(), node))
        .collect();
    let mut candidates = Vec::new();

    for newer in &facts.nodes {
        for declaration in &newer.rename_declarations {
            attempt
                .check(AcquisitionStage::RuleConstruction)
                .map_err(|expired| AcquisitionFailure::TimedOut {
                    stage: expired.stage,
                })?;
            if !release_is_between(&declaration.release, earlier, later) {
                continue;
            }
            let Some(previous) =
                normalize_previous_name(&declaration.previous_name, &newer.path, &request.ids)
            else {
                continue;
            };
            let Some(older) = nodes.get(previous.as_str()) else {
                continue;
            };
            if !facts.successors.iter().any(|successor| {
                normalize_ids_path(&successor.from_path, &request.ids) == previous
                    && normalize_ids_path(&successor.to_path, &request.ids) == newer.path
            }) {
                continue;
            }
            let EndpointState::Present {
                metadata: older_metadata,
                ..
            } = replay_endpoint(facts, older, earlier)?
            else {
                continue;
            };
            if !matches!(replay_endpoint(facts, older, later)?, EndpointState::Absent) {
                continue;
            }
            if !matches!(
                replay_endpoint(facts, newer, earlier)?,
                EndpointState::Absent
            ) {
                continue;
            }
            let EndpointState::Present {
                metadata: newer_metadata,
                ..
            } = replay_endpoint(facts, newer, later)?
            else {
                continue;
            };
            if !same_representation(&older_metadata, &newer_metadata) {
                continue;
            }
            let moved_parent = older_metadata.kind == GraphNodeKind::Structure
                && newer_metadata.kind == GraphNodeKind::Structure
                && parent_path(&previous) != parent_path(&newer.path);
            candidates.push((previous, newer.path.clone(), moved_parent));
        }
    }

    candidates.sort();
    candidates.dedup();
    Ok(candidates
        .iter()
        .filter(|(previous, newer, _)| {
            candidates
                .iter()
                .filter(|(candidate_previous, _, _)| candidate_previous == previous)
                .count()
                == 1
                && candidates
                    .iter()
                    .filter(|(_, candidate_newer, _)| candidate_newer == newer)
                    .count()
                    == 1
        })
        .map(|(previous, newer, moved_parent)| {
            let rel = if *moved_parent {
                Rel::Moved
            } else {
                Rel::Renamed
            };
            // A parent relation does not certify every spelling below it.
            // Each propagated descendant must have its own endpoint-backed
            // correspondence, so moved anchors remain exact selectors too.
            let selector_stage = SelectorStage::Exact;
            if request.hli_dd == *earlier {
                DirectRename {
                    left: previous.clone(),
                    right: newer.clone(),
                    rel,
                    selector_stage,
                }
            } else {
                DirectRename {
                    left: newer.clone(),
                    right: previous.clone(),
                    rel,
                    selector_stage,
                }
            }
        })
        .collect())
}

fn chronological_endpoints(request: &MapRequest) -> (&ArtifactDdVersion, &ArtifactDdVersion) {
    if numeric_release(&request.hli_dd) < numeric_release(&request.stored_dd) {
        (&request.hli_dd, &request.stored_dd)
    } else {
        (&request.stored_dd, &request.hli_dd)
    }
}

fn release_is_between(
    release: &ArtifactDdVersion,
    earlier: &ArtifactDdVersion,
    later: &ArtifactDdVersion,
) -> bool {
    let release = numeric_release(release);
    numeric_release(earlier) < release && release <= numeric_release(later)
}

fn normalize_previous_name(previous_name: &str, declaring_path: &str, ids: &str) -> Option<String> {
    let is_ids_absolute = previous_name == ids
        || previous_name
            .strip_prefix(ids)
            .is_some_and(|remainder| remainder.starts_with('/'));
    let absolute = previous_name.starts_with('/') || is_ids_absolute;
    let previous_name = previous_name.trim_start_matches('/');
    let previous_name = if is_ids_absolute {
        normalize_ids_path(previous_name, ids)
    } else {
        previous_name.to_string()
    };
    let mut segments = if absolute {
        Vec::new()
    } else {
        declaring_path
            .rsplit_once('/')
            .map_or_else(Vec::new, |(parent, _)| {
                parent.split('/').map(str::to_string).collect()
            })
    };
    for segment in previous_name.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment.to_string()),
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

fn parent_path(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn normalize_ids_path(path: &str, ids: &str) -> String {
    strip_ids_prefix(path, ids)
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

    for node in &facts.nodes {
        let mut coordinate_dimensions = HashSet::new();
        for relationship in &node.coordinate_relationships {
            attempt
                .check(AcquisitionStage::ScopeValidation)
                .map_err(|expired| AcquisitionFailure::TimedOut {
                    stage: expired.stage,
                })?;
            if !coordinate_dimensions.insert(relationship.dimension) {
                return Err(invalid_node(
                    node,
                    "coordinate relationships repeat a dimension",
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
        let from_path = normalize_ids_path(&successor.from_path, &request.ids);
        let to_path = normalize_ids_path(&successor.to_path, &request.ids);
        if !paths.contains(from_path.as_str()) || !paths.contains(to_path.as_str()) {
            return Err(AcquisitionFailure::InvalidNode {
                path: from_path,
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
    if !matches!(
        field,
        "data_type" | "ndim" | "units" | "timebase" | "coordinates"
    ) {
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
        // Raw COCOS label events describe the XML history.  They do not
        // overwrite a separately-proven backfilled class on the endpoint:
        // the two evidence forms are intentionally not interchangeable.
        "cocos_label_transformation"
        | "documentation"
        | "lifecycle_status"
        | "maxoccur"
        | "identifier_enum" => Ok(field),
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
        && left.cocos_label_source == right.cocos_label_source
}

/// A direct predecessor declaration cannot serve as value evidence when either
/// endpoint carries COCOS metadata, even if the two raw values happen to match.
fn same_representation(left: &EndpointMetadata, right: &EndpointMetadata) -> bool {
    same_metadata_values(left, right)
        && !endpoint_has_cocos_evidence(left)
        && !endpoint_has_cocos_evidence(right)
}

fn invalid_event(event: &GraphEvent) -> AcquisitionFailure {
    AcquisitionFailure::InvalidEventValue {
        id: event.id.clone(),
    }
}

#[derive(Clone, Copy)]
enum EndpointRelation {
    Exact,
    Retyped,
    UnitRedefinition,
    CoordinateResampling,
    CoordinateUnresolved,
    Unresolved,
}

fn endpoint_rule_id(relation: EndpointRelation, node: &GraphNode) -> Option<String> {
    match relation {
        EndpointRelation::CoordinateResampling => {
            Some(format!("coordinate-resampling:{}", node.path))
        }
        EndpointRelation::CoordinateUnresolved => {
            Some(format!("coordinate-unresolved:{}", node.path))
        }
        _ => None,
    }
}

/// Classifies the endpoint behaviour that the existing map representation can
/// express without turning a unit declaration into an invented conversion.
///
/// Type and rank evidence takes precedence: an otherwise exact spelling is
/// still the engine's unconditional retype refusal. Coordinate and timebase
/// interpretation remains outside this unit-only step, so a discrepancy there
/// stays a localized unresolved conversion until its own evidence is handled.
fn endpoint_relation(
    facts: &IdsGraphFacts,
    node: &GraphNode,
    hli: &EndpointMetadata,
    stored: &EndpointMetadata,
    direct_renames: &[DirectRename],
) -> EndpointRelation {
    if metadata_is_retyped(hli, stored) {
        return EndpointRelation::Retyped;
    }
    match coordinate_evidence_between(facts, node, hli, stored, direct_renames) {
        CoordinateEvidenceVerdict::Equivalent => {}
        CoordinateEvidenceVerdict::RequiresResampling => {
            return EndpointRelation::CoordinateResampling;
        }
        CoordinateEvidenceVerdict::Unresolved => return EndpointRelation::CoordinateUnresolved,
    }

    match unit_evidence_between(facts, node, &hli.release, &stored.release) {
        UnitEvidenceVerdict::DeclarationOnly => EndpointRelation::Exact,
        UnitEvidenceVerdict::RequiredUnsupportedTransformation => {
            EndpointRelation::UnitRedefinition
        }
        UnitEvidenceVerdict::Unresolved => EndpointRelation::Unresolved,
        UnitEvidenceVerdict::NoChange if hli.unit == stored.unit => EndpointRelation::Exact,
        UnitEvidenceVerdict::NoChange => EndpointRelation::Unresolved,
    }
}

enum CoordinateEvidenceVerdict {
    Equivalent,
    RequiresResampling,
    Unresolved,
}

/// Coordinates are conversion evidence only when the two endpoint declarations
/// positively match. The graph's unversioned relationship edges can be absent
/// or strip index notation, so empty or unequal collections do not establish
/// either equivalence or resampling. A known resampling event remains a
/// path-local unsupported conversion.
fn coordinate_evidence_between(
    facts: &IdsGraphFacts,
    node: &GraphNode,
    hli: &EndpointMetadata,
    stored: &EndpointMetadata,
    direct_renames: &[DirectRename],
) -> CoordinateEvidenceVerdict {
    if facts.events.iter().any(|event| {
        event.path == node.path
            && event_applies_between(event, &hli.release, &stored.release)
            && event.coordinate_evidence == Some(CoordinateChangeEvidence::RequiresResampling)
    }) {
        return CoordinateEvidenceVerdict::RequiresResampling;
    }

    let Some(hli_timebase) = hli.timebase_path.as_deref() else {
        return CoordinateEvidenceVerdict::Unresolved;
    };
    let Some(stored_timebase) = stored.timebase_path.as_deref() else {
        return CoordinateEvidenceVerdict::Unresolved;
    };
    if !paths_correspond(hli_timebase, stored_timebase, direct_renames)
        || hli.coordinate_paths.is_empty()
        || hli.coordinate_paths.len() != stored.coordinate_paths.len()
    {
        return CoordinateEvidenceVerdict::Unresolved;
    }
    let paths_match = hli
        .coordinate_paths
        .iter()
        .zip(&stored.coordinate_paths)
        .all(|(hli_path, stored_path)| paths_correspond(hli_path, stored_path, direct_renames));
    if !paths_match {
        return CoordinateEvidenceVerdict::Unresolved;
    }

    if coordinate_relationships_corroborate(facts, node, hli, stored)
        || facts.events.iter().any(|event| {
            event.path == node.path
                && event_applies_between(event, &hli.release, &stored.release)
                && event.coordinate_evidence == Some(CoordinateChangeEvidence::Equivalent)
        })
    {
        CoordinateEvidenceVerdict::Equivalent
    } else {
        CoordinateEvidenceVerdict::Unresolved
    }
}

/// An unversioned relationship is affirmative evidence only when it preserves
/// each raw endpoint declaration at the same dimension. Missing or normalized
/// relationships deliberately leave the historical change unresolved.
fn coordinate_relationships_corroborate(
    facts: &IdsGraphFacts,
    node: &GraphNode,
    hli: &EndpointMetadata,
    stored: &EndpointMetadata,
) -> bool {
    let mut dimensions = HashSet::new();
    node.coordinate_relationships.len() == hli.coordinate_paths.len()
        && node.coordinate_relationships.iter().all(|relationship| {
            dimensions.insert(relationship.dimension)
                && relationship.dimension < hli.coordinate_paths.len()
                // A `HAS_COORDINATE` target may be an `IMASCoordinateSpec`,
                // not an IDS node. A target outside this node inventory is
                // therefore merely non-corroborating, never a scope failure.
                && facts
                    .nodes
                    .iter()
                    .any(|candidate| candidate.path == relationship.target_path)
                && hli
                    .coordinate_paths
                    .get(relationship.dimension)
                    .zip(stored.coordinate_paths.get(relationship.dimension))
                    .is_some_and(|(hli_path, stored_path)| {
                        hli_path == &relationship.target_path
                            && stored_path == &relationship.target_path
                    })
        })
}

fn paths_correspond(hli_path: &str, stored_path: &str, direct_renames: &[DirectRename]) -> bool {
    hli_path == stored_path
        || direct_renames
            .iter()
            .any(|rename| rename.left == hli_path && rename.right == stored_path)
}

fn event_applies_between(
    event: &GraphEvent,
    first: &ArtifactDdVersion,
    second: &ArtifactDdVersion,
) -> bool {
    let first = numeric_release(first);
    let second = numeric_release(second);
    let (earlier, later) = if first <= second {
        (first, second)
    } else {
        (second, first)
    };
    let release = numeric_release(&event.release);
    release > earlier && release <= later
}

fn validate_coordinate_scope(
    facts: &IdsGraphFacts,
    first: &ArtifactDdVersion,
    second: &ArtifactDdVersion,
    attempt: &AcquisitionAttempt,
) -> Result<(), AcquisitionFailure> {
    for event in &facts.events {
        attempt
            .check(AcquisitionStage::RuleConstruction)
            .map_err(|expired| AcquisitionFailure::TimedOut {
                stage: expired.stage,
            })?;
        if !event_applies_between(event, first, second) {
            continue;
        }
        if event.coordinate_evidence.is_some()
            && !matches!(event_field(event), Ok("coordinates" | "timebase"))
        {
            return Err(invalid_event(event));
        }
        if event.coordinate_evidence == Some(CoordinateChangeEvidence::UnboundedScope) {
            return Err(AcquisitionFailure::UnboundedCoordinateScope {
                path: event.path.clone(),
            });
        }
    }
    Ok(())
}

fn metadata_is_retyped(hli: &EndpointMetadata, stored: &EndpointMetadata) -> bool {
    hli.kind != stored.kind || hli.data_type != stored.data_type || hli.ndim != stored.ndim
}

fn path_is_descendant_of(path: &str, ancestor: &str) -> bool {
    path.strip_prefix(ancestor)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

enum UnitEvidenceVerdict {
    NoChange,
    DeclarationOnly,
    RequiredUnsupportedTransformation,
    Unresolved,
}

/// Folds only the producer classification for applicable unit events. A
/// compatible dimension is explicitly not a factor-one proof: without a
/// declaration-only classification, a scale or offset may still be required.
fn unit_evidence_between(
    facts: &IdsGraphFacts,
    node: &GraphNode,
    first: &ArtifactDdVersion,
    second: &ArtifactDdVersion,
) -> UnitEvidenceVerdict {
    let mut declaration_only = false;
    let mut unresolved = false;
    for event in &facts.events {
        if event.path != node.path || event_field(event).ok() != Some("units") {
            continue;
        }
        if !event_applies_between(event, first, second) {
            continue;
        }
        match event.unit_change {
            Some(UnitChangeEvidence::Cosmetic | UnitChangeEvidence::SentinelResolved) => {
                declaration_only = true;
            }
            Some(UnitChangeEvidence::RequiredScaleOrOffset) => {
                return UnitEvidenceVerdict::RequiredUnsupportedTransformation;
            }
            Some(UnitChangeEvidence::DimensionallyCompatible) | None => unresolved = true,
        }
    }
    if unresolved {
        UnitEvidenceVerdict::Unresolved
    } else if declaration_only {
        UnitEvidenceVerdict::DeclarationOnly
    } else {
        UnitEvidenceVerdict::NoChange
    }
}

/// The only COCOS factor the existing value engine can execute is a sign
/// change. Evidence that would require an expression evaluator, a scale
/// converter, or a guessed convention remains a local refusal.
enum CocosFactorResolution {
    Identity,
    SignFlip {
        from_cocos: CocosConvention,
        to_cocos: CocosConvention,
    },
    Unresolved,
    Unsupported,
}

fn derive_cocos_factor(
    hli: &EndpointMetadata,
    stored: &EndpointMetadata,
    hli_cocos: Option<&CocosConvention>,
    stored_cocos: Option<&CocosConvention>,
    evidence: &CocosEvidence,
) -> CocosFactorResolution {
    if evidence.is_empty() {
        return CocosFactorResolution::Identity;
    }
    if evidence.has_incompatible_history {
        return CocosFactorResolution::Unsupported;
    }
    let (Some(hli_cocos), Some(stored_cocos)) = (hli_cocos, stored_cocos) else {
        return CocosFactorResolution::Unresolved;
    };
    if hli.kind != GraphNodeKind::Leaf || stored.kind != GraphNodeKind::Leaf {
        return CocosFactorResolution::Unsupported;
    }
    let (Some(hli_label), Some(stored_label)) =
        (supported_cocos_label(hli), supported_cocos_label(stored))
    else {
        return CocosFactorResolution::Unsupported;
    };
    if hli_label != stored_label {
        return CocosFactorResolution::Unresolved;
    }
    if hli_cocos == stored_cocos {
        return CocosFactorResolution::Identity;
    }
    if is_supported_sign_flip_pair(hli_cocos, stored_cocos) {
        return CocosFactorResolution::SignFlip {
            from_cocos: hli_cocos.clone(),
            to_cocos: stored_cocos.clone(),
        };
    }
    CocosFactorResolution::Unsupported
}

fn is_supported_sign_flip_pair(hli: &CocosConvention, stored: &CocosConvention) -> bool {
    matches!((hli.as_str(), stored.as_str()), ("11", "17") | ("17", "11"))
}

fn supported_cocos_label(metadata: &EndpointMetadata) -> Option<&str> {
    if metadata.cocos_transformation_expression.is_some()
        || !matches!(
            metadata.cocos_label_source,
            Some(CocosLabelSource::Xml | CocosLabelSource::InferredSignFlip)
        )
    {
        return None;
    }
    match metadata.cocos_label_transformation.as_deref() {
        Some("psi_like" | "dodpsi_like") => metadata.cocos_label_transformation.as_deref(),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CocosEvidenceForm {
    EndpointLabel,
    RawLabelHistory,
    Documentation,
}

/// Evidence is deduplicated by transition/path before factor selection.  The
/// forms corroborate one scientific change but never compose into repeated
/// numerical transforms.
#[derive(Default)]
struct CocosEvidence {
    forms: BTreeSet<CocosEvidenceForm>,
    has_incompatible_history: bool,
}

impl CocosEvidence {
    fn is_empty(&self) -> bool {
        self.forms.is_empty()
    }
}

fn collect_cocos_evidence(
    facts: &IdsGraphFacts,
    node: &GraphNode,
    hli_metadata: &EndpointMetadata,
    stored_metadata: &EndpointMetadata,
    hli_release: &ArtifactDdVersion,
    stored_release: &ArtifactDdVersion,
) -> Result<CocosEvidence, AcquisitionFailure> {
    let mut evidence = CocosEvidence::default();
    if endpoint_has_cocos_evidence(hli_metadata) || endpoint_has_cocos_evidence(stored_metadata) {
        evidence.forms.insert(CocosEvidenceForm::EndpointLabel);
    }
    for event in facts.events.iter().filter(|event| {
        event.path == node.path
            && cocos_release_is_between(&event.release, hli_release, stored_release)
    }) {
        let field = event.id.rsplit(':').nth(1).unwrap_or_default();
        if !matches!(field, "cocos_label_transformation" | "documentation") {
            continue;
        }
        match event_field(event)? {
            "cocos_label_transformation" => {
                let (Some(old), Some(new)) =
                    (event.old_value.as_deref(), event.new_value.as_deref())
                else {
                    return Err(invalid_event(event));
                };
                if old == new {
                    return Err(invalid_event(event));
                }
                // An add or clear is history that can corroborate the class
                // provenance. Replacing one raw label with another describes
                // an unmodelled factor and cannot be silently merged.
                if !old.is_empty() && !new.is_empty() {
                    evidence.has_incompatible_history = true;
                }
                evidence.forms.insert(CocosEvidenceForm::RawLabelHistory);
            }
            "documentation" => {
                if event.old_value.is_none() || event.new_value.is_none() {
                    return Err(invalid_event(event));
                }
                evidence.forms.insert(CocosEvidenceForm::Documentation);
            }
            _ => {}
        }
    }
    Ok(evidence)
}

fn endpoint_has_cocos_evidence(endpoint: &EndpointMetadata) -> bool {
    endpoint.cocos_label_transformation.is_some()
        || endpoint.cocos_transformation_expression.is_some()
        || endpoint.cocos_label_source.is_some()
}

fn cocos_release_is_between(
    candidate: &ArtifactDdVersion,
    first: &ArtifactDdVersion,
    second: &ArtifactDdVersion,
) -> bool {
    let candidate = numeric_release(candidate);
    let first = numeric_release(first);
    let second = numeric_release(second);
    let (lower, upper) = if first <= second {
        (first, second)
    } else {
        (second, first)
    };
    lower <= candidate && candidate <= upper
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
