//! What stored path an HLI argument means, and at what fidelity.
//!
//! Before this module existed, [`crate::conversion::conversion_map::Outcome`] was
//! interpreted at three independent sites in `src/interpose.rs`, each deriving
//! a different subset of its meaning: `translate_down` derived a `CString`
//! or nothing, the read seam derived a [`ReadPath`] with fidelity and
//! candidates, and the context-opening seams derived one concrete spelling,
//! no-source, or a refusal. This module is the one place that answers the
//! question instead, so no consumer re-derives the enum. [`resolve`] answers
//! it once; each ABI seam applies its own named narrowing.
//!
//! It knows nothing about seams, attempts, loops or IMAS-Core: it takes a
//! live [`ConversionRecord`] and a raw HLI argument, and answers either "what
//! stored path does this mean", then lets a context open, read, write, or
//! delete narrowing state what that particular seam can safely enact.
//!
//! Issue #101 (part B); see ADR 0015 for the layering this belongs to.

use std::ffi::{CStr, CString, c_char};

use crate::conversion::conversion_map::{
    Direction, Fidelity, Outcome, RefusalReason, Rel, ValueTransformation,
};
use crate::registry::context_registry::ConversionRecord;

/// `ptr` as a borrowed `&str`, or `None` if it is null or not valid UTF-8.
fn c_str_or_none<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller's own contract requires `ptr`, when non-null, to be
    // a valid NUL-terminated C string.
    unsafe { CStr::from_ptr(ptr) }.to_str().ok()
}

/// The narrow question `al_begin_global_action`'s `datapath` translation asks
/// of an [`Outcome`]: is there a concrete resolved path at all? `datapath` is
/// near-inert (CLAUDE.md), so a merged/split candidate plan or a declared
/// value transformation is not a reason to refuse here — only a data read
/// can try candidates or apply a transformation, and the eventual
/// `al_read_data` on this occurrence is what actually enforces those. A
/// no-source or refusal outcome is likewise not this seam's call to make:
/// forward `datapath` unchanged and let that later read report why.
pub(crate) fn datapath_translation(outcome: Outcome) -> Option<String> {
    match outcome {
        Outcome::Path { resolved_path, .. } => Some(resolved_path),
        Outcome::NoSource | Outcome::Refusal(_) => None,
    }
}

/// The result of resolving one path-bearing context argument against a
/// mismatched conversion record.
pub(crate) enum ContextPathResolution {
    /// No usable caller path at all: forward it unchanged.
    Forward,
    /// A real path argument no rule and no document-level default claims.
    Unclaimed,
    /// A concrete stored-DD spelling for IMAS-Core to receive.
    Translated(CString),
    /// The artifact says no stored counterpart exists.
    NoSource,
    /// The artifact or this seam deliberately declines to serve the path.
    Refusal(String),
}

/// The richer path result used only by `al_read_data`: an ordered candidate
/// plan retains each candidate's fidelity and value transformation until a
/// stored source actually returns data.
pub(crate) enum ReadPath {
    Forward,
    Translated(TranslatedReadPath),
    NoSource(Fidelity),
    Refusal {
        reason: String,
        dd_path: String,
        fidelity: Fidelity,
    },
}

/// The one complete answer path conversion gives every seam.  `Single` and
/// `Plan` mirror the rule declaration: renamed/moved/identity rules produce a
/// single source, while merged/split rules produce a plan, even if a future
/// artifact declares only one candidate in that plan.
pub(crate) enum Resolved {
    Forward,
    Single(Candidate),
    Plan(Vec<Candidate>),
    NoSource(Fidelity),
    Unclaimed,
    Refusal {
        reason: String,
        dd_path: String,
        fidelity: Fidelity,
    },
}

/// A stored source with the metadata each narrowing needs.  `precedence`
/// belongs to a source inside a declared plan; `requested_precedence` belongs
/// to the HLI spelling which selected the rule, and is deliberately separate.
pub(crate) struct Candidate {
    path: CString,
    stored_dd_path: String,
    dd_path: String,
    fidelity: Fidelity,
    value_transformation: ValueTransformation,
    precedence: Option<u32>,
    requested_precedence: Option<u32>,
}

struct ResolutionMetadata {
    dd_path: String,
    fidelity: Fidelity,
    requested_precedence: Option<u32>,
}

/// The one stored-DD spelling a write may safely hand to IMAS-Core, plus the
/// read-direction transformation the seam policy must invert before it can
/// prepare its own stored-DD buffer.
pub(crate) enum WritePath {
    /// No path was supplied, so preserve IMAS-Core's own handling.
    Forward,
    /// One concrete stored-DD spelling for IMAS-Core to receive.
    Translated {
        path: CString,
        value_transformation: ValueTransformation,
    },
    /// An ordered plan whose first candidate is the only stored path a write
    /// may change. Later candidates stay untouched and earn a potential-loss
    /// entry only after that one write succeeds.
    Candidates(Vec<WriteCandidate>),
    /// The supplied HLI-DD path cannot safely be written through this seam.
    Refusal {
        reason: String,
        dd_path: String,
        /// The entry in [`WRITE_CHECKS`] that rejected this argument. `None`
        /// is a later path-construction failure, not one of the ordered
        /// refusal checks.
        check_index: Option<usize>,
    },
}

pub(crate) struct WriteCandidate {
    /// The spelling IMAS-Core receives: anchor-stripped when the caller's own
    /// argument was relative to a live context.
    pub(crate) path: CString,
    /// The same candidate as one complete DD path from the IDS root, which is
    /// what a loss-log entry must name — a caller draining the log is looking
    /// for the stored spelling that now holds a stale value, and an
    /// anchor-relative fragment does not tell them where to find it (ADR 0016
    /// decision 4).
    pub(crate) stored_dd_path: String,
    pub(crate) precedence: u32,
    pub(crate) value_transformation: ValueTransformation,
}

/// The stored-DD spelling or spellings a delete may safely remove, or the
/// reason the delete must refuse. Unlike a write, which may change only its
/// declared primary source, deleting one stored field needs no value
/// transformation and an ordered candidate plan is safe here, because
/// deleting every possible source asserts the same absence.
pub(crate) enum DeletePath {
    /// No path was supplied, or it was empty: preserve IMAS-Core's
    /// whole-DATAOBJECT delete handling.
    Forward,
    /// One concrete stored-DD spelling for IMAS-Core to remove.
    Translated(CString),
    /// Every stored candidate that can satisfy the HLI path, in declared
    /// precedence order.
    Candidates(Vec<CString>),
    /// The supplied HLI-DD path cannot safely be removed through this seam.
    Refusal { reason: String, dd_path: String },
}

pub(crate) struct TranslatedReadPath {
    /// `pub(crate)` so the read loop (`src/seam_policy.rs`) can turn each
    /// candidate into a forwarding attempt; this module never inspects them
    /// past constructing and ordering them.
    pub(crate) paths: Vec<ResolvedReadPath>,
}

pub(crate) struct ResolvedReadPath {
    pub(crate) path: CString,
    pub(crate) fidelity: Fidelity,
    pub(crate) value_transformation: ValueTransformation,
}

pub(crate) fn translated_read_fidelity(path: Option<&TranslatedReadPath>) -> Fidelity {
    path.and_then(|path| path.paths.first())
        .map_or(Fidelity::Exact, |path| path.fidelity)
}

/// One path-bearing ABI argument that the conversion map claims, in the form
/// both path resolvers need before they can differ: whether the caller spelled
/// it absolutely, its absolute HLI-DD spelling, and the rule that explains it.
struct ClaimedArgument {
    is_absolute: bool,
    hli_absolute: String,
    explanation: crate::conversion::conversion_map::RuleExplanation,
}

/// The ABI role of a path argument. A write resolves `field` and `timebase`
/// through the same map, but their refusal policies deliberately differ.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgumentRole {
    Any,
    Field,
    Timebase,
    Path,
}

impl ArgumentRole {
    fn serves(self, actual: Self) -> bool {
        self == Self::Any || self == actual
    }
}

#[derive(Clone, Copy)]
enum WriteCheck {
    Unclaimed,
    ImmutableStamp,
    SharedRefusal,
    NonPrimarySource,
    NoStoredSource,
    TimebaseTransformation,
    InvertibleTransformation,
}

/// The complete write refusal order. The role tag is part of the policy.
const WRITE_CHECKS: &[(ArgumentRole, WriteCheck)] = &[
    (ArgumentRole::Any, WriteCheck::Unclaimed),
    (ArgumentRole::Any, WriteCheck::ImmutableStamp),
    (ArgumentRole::Any, WriteCheck::SharedRefusal),
    (ArgumentRole::Any, WriteCheck::NonPrimarySource),
    (ArgumentRole::Any, WriteCheck::NoStoredSource),
    (ArgumentRole::Timebase, WriteCheck::TimebaseTransformation),
    (ArgumentRole::Field, WriteCheck::InvertibleTransformation),
];

#[derive(Clone, Copy)]
enum DeleteCheck {
    Unclaimed,
    ImmutableStamp,
    SharedRefusal,
    NonPrimarySource,
    NoStoredSource,
    EscapingSubtree,
}

/// The complete delete refusal order.
const DELETE_CHECKS: &[(ArgumentRole, DeleteCheck)] = &[
    (ArgumentRole::Path, DeleteCheck::Unclaimed),
    (ArgumentRole::Path, DeleteCheck::ImmutableStamp),
    (ArgumentRole::Path, DeleteCheck::SharedRefusal),
    (ArgumentRole::Path, DeleteCheck::NonPrimarySource),
    (ArgumentRole::Path, DeleteCheck::NoStoredSource),
    (ArgumentRole::Path, DeleteCheck::EscapingSubtree),
];

fn write_check_index(check: WriteCheck) -> usize {
    WRITE_CHECKS
        .iter()
        .position(|(role, candidate)| {
            role.serves(*role)
                && std::mem::discriminant(candidate) == std::mem::discriminant(&check)
        })
        .expect("write check is listed")
}

fn delete_check_is_listed(check: DeleteCheck) {
    debug_assert!(
        DELETE_CHECKS
            .iter()
            .any(|(_, candidate)| std::mem::discriminant(candidate)
                == std::mem::discriminant(&check))
    );
}

/// What one path-bearing ABI argument amounts to, before
/// a seam-specific narrowing differs on what to do with it.
///
/// The two reasons an argument yields no rule are kept apart here on purpose.
/// They used to share one `None`, which forced every caller to re-derive the
/// distinction from `raw` after the fact — the arraystruct seam did, and the
/// read seam did not, which is how an unclaimed read path came to be
/// forwarded to IMAS-Core.
enum RawArgument {
    /// No usable path to translate: null, or empty as `timebase` routinely
    /// is. There is nothing to resolve, so the argument is forwarded exactly
    /// as received.
    Absent,
    /// A real path argument that no rule and no document-level default
    /// claims. The embedded artifact carries an identity default so this
    /// cannot arise from it today, but a future artifact may not, and an
    /// absent rule is never permission to invent a stored spelling.
    Unclaimed,
    /// The conversion map claims it, with the rule that explains it attached.
    Claimed(ClaimedArgument),
}

/// The one resolver's preamble.
fn claimed_argument(record: &ConversionRecord, raw: *const c_char) -> RawArgument {
    let Some(raw) = c_str_or_none(raw).filter(|path| !path.is_empty()) else {
        return RawArgument::Absent;
    };
    let is_absolute = raw.starts_with('/');
    let hli_absolute = join_hli_path(&record.resolved_path, raw);
    let Some(explanation) = record
        .map
        .resolve(&hli_absolute, record.direction_to_stored)
    else {
        return RawArgument::Unclaimed;
    };
    RawArgument::Claimed(ClaimedArgument {
        is_absolute,
        hli_absolute,
        explanation,
    })
}

/// Resolves one HLI path exactly once.  The operation-specific narrowings
/// below apply their ADR policy after this mechanical answer is available.
pub(crate) fn resolve(record: &ConversionRecord, raw: *const c_char) -> Resolved {
    let argument = match claimed_argument(record, raw) {
        RawArgument::Absent => return Resolved::Forward,
        RawArgument::Unclaimed => return Resolved::Unclaimed,
        RawArgument::Claimed(argument) => argument,
    };
    let ClaimedArgument {
        is_absolute,
        hli_absolute,
        explanation,
    } = argument;
    let metadata = ResolutionMetadata {
        dd_path: hli_absolute.clone(),
        fidelity: read_fidelity(explanation.fidelity, explanation.rel),
        requested_precedence: explanation.precedence,
    };

    let dd_path_for_refusal = hli_absolute.clone();
    let refusal = move |reason| Resolved::Refusal {
        reason,
        dd_path: dd_path_for_refusal.clone(),
        fidelity: Fidelity::Unmappable,
    };
    match explanation.outcome {
        Outcome::Refusal(reason) => refusal(refusal_reason_message(reason)),
        Outcome::NoSource => Resolved::NoSource(metadata.fidelity),
        Outcome::Path {
            resolved_path,
            value_transformation,
            candidates,
        } if matches!(explanation.rel, Some(Rel::Merged | Rel::Split)) => {
            let candidates = if candidates.is_empty() {
                // The rule still declared a merged/split plan even when this
                // direction names its sole result through `resolved_path`.
                // This chooses the plan from `rel`, never from its length.
                vec![crate::conversion::conversion_map::CandidatePath {
                    path: resolved_path,
                    precedence: metadata.requested_precedence.unwrap_or(1),
                    value_transformation,
                }]
            } else {
                candidates
            };
            candidates
                .into_iter()
                .map(|candidate| {
                    candidate_from_path(
                        record,
                        candidate.path,
                        is_absolute,
                        &metadata,
                        candidate.value_transformation,
                        Some(candidate.precedence),
                    )
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Resolved::Plan)
                .unwrap_or_else(refusal)
        }
        Outcome::Path {
            resolved_path,
            value_transformation,
            ..
        } => candidate_from_path(
            record,
            resolved_path,
            is_absolute,
            &metadata,
            value_transformation,
            None,
        )
        .map(Resolved::Single)
        .unwrap_or_else(refusal),
    }
}

fn candidate_from_path(
    record: &ConversionRecord,
    stored_dd_path: String,
    is_absolute: bool,
    metadata: &ResolutionMetadata,
    value_transformation: ValueTransformation,
    precedence: Option<u32>,
) -> Result<Candidate, String> {
    stored_c_path(record, &stored_dd_path, is_absolute).map(|path| Candidate {
        path,
        stored_dd_path,
        dd_path: metadata.dd_path.clone(),
        fidelity: metadata.fidelity,
        value_transformation,
        precedence,
        requested_precedence: metadata.requested_precedence,
    })
}

/// ADR 0016 decision 3 / user story 47: each seam receives only the answers
/// it can enact.  Context opens cannot execute transformations or plans.
pub(crate) fn narrow_context_path(resolved: Resolved) -> ContextPathResolution {
    match resolved {
        Resolved::Forward => ContextPathResolution::Forward,
        Resolved::Unclaimed => ContextPathResolution::Unclaimed,
        Resolved::NoSource(_) => ContextPathResolution::NoSource,
        Resolved::Refusal { reason, .. } => ContextPathResolution::Refusal(reason),
        Resolved::Single(candidate) if candidate.value_transformation == ValueTransformation::None =>
            ContextPathResolution::Translated(candidate.path),
        Resolved::Single(_) => ContextPathResolution::Refusal(
            "this path needs a value transformation, which only a data read can apply".to_string(),
        ),
        Resolved::Plan(_) => ContextPathResolution::Refusal(
            "this path is served by several stored candidates, and only a data read can try them in turn".to_string(),
        ),
    }
}

/// User story 47: an unclaimed read returns not-found rather than forwarding
/// an HLI spelling to stored data.  Single and Plan share one read shape.
pub(crate) fn narrow_read_path(resolved: Resolved) -> ReadPath {
    match resolved {
        Resolved::Forward => ReadPath::Forward,
        Resolved::Unclaimed => ReadPath::NoSource(Fidelity::Unmappable),
        Resolved::NoSource(fidelity) => ReadPath::NoSource(fidelity),
        Resolved::Refusal {
            reason,
            dd_path,
            fidelity,
        } => ReadPath::Refusal {
            reason,
            dd_path,
            fidelity,
        },
        Resolved::Single(candidate) => ReadPath::Translated(TranslatedReadPath {
            paths: vec![resolved_read_path(candidate)],
        }),
        Resolved::Plan(candidates) => ReadPath::Translated(TranslatedReadPath {
            paths: candidates.into_iter().map(resolved_read_path).collect(),
        }),
    }
}

fn resolved_read_path(candidate: Candidate) -> ResolvedReadPath {
    ResolvedReadPath {
        path: candidate.path,
        fidelity: candidate.fidelity,
        value_transformation: candidate.value_transformation,
    }
}

fn inverted_for_write(transformation: ValueTransformation) -> ValueTransformation {
    transformation
        .inverse()
        .expect("the write narrowing rejects non-invertible transformations")
}

/// ADR 0016 decision 3: write-specific policy narrows one shared answer.
pub(crate) fn narrow_write_path(
    record: &ConversionRecord,
    raw: *const c_char,
    role: ArgumentRole,
    resolved: Resolved,
) -> WritePath {
    let refusal = |reason, dd_path, check_index| WritePath::Refusal {
        reason,
        dd_path,
        check_index,
    };
    match resolved {
        Resolved::Forward => WritePath::Forward,
        Resolved::Unclaimed => refusal(
            "this path is unclaimed by the conversion map".to_string(),
            caller_dd_path(record, raw),
            Some(write_check_index(WriteCheck::Unclaimed)),
        ),
        Resolved::NoSource(_) => refusal(
            "this path has no stored source".to_string(),
            caller_dd_path(record, raw),
            Some(write_check_index(WriteCheck::NoStoredSource)),
        ),
        Resolved::Refusal {
            reason, dd_path, ..
        } => refusal(
            reason,
            dd_path,
            Some(write_check_index(WriteCheck::SharedRefusal)),
        ),
        Resolved::Single(candidate) => narrow_single_write(role, candidate, refusal),
        Resolved::Plan(candidates) => {
            if let Some(candidate) = candidates.first() {
                if candidate.dd_path == "ids_properties/version_put/data_dictionary" {
                    return refusal(
                        "the DD-version stamp is immutable under a version mismatch".to_string(),
                        candidate.dd_path.clone(),
                        Some(write_check_index(WriteCheck::ImmutableStamp)),
                    );
                }
                if candidate
                    .requested_precedence
                    .is_some_and(|precedence| precedence != 1)
                {
                    return refusal(
                        "this path is a non-primary source and cannot write a shared stored slot"
                            .to_string(),
                        candidate.dd_path.clone(),
                        Some(write_check_index(WriteCheck::NonPrimarySource)),
                    );
                }
            }
            let Some(primary) = candidates
                .iter()
                .find(|candidate| candidate.precedence == Some(1))
            else {
                return refusal(
                    "this candidate plan has no precedence-1 source for a write".to_string(),
                    caller_dd_path(record, raw),
                    None,
                );
            };
            if let Some(path) = write_candidate_refusal(role, primary) {
                return refusal(path.0, path.1, path.2);
            }
            WritePath::Candidates(candidates.into_iter().map(write_candidate).collect())
        }
    }
}

fn narrow_single_write(
    role: ArgumentRole,
    candidate: Candidate,
    refusal: impl FnOnce(String, String, Option<usize>) -> WritePath,
) -> WritePath {
    if let Some((reason, dd_path, check_index)) = write_candidate_refusal(role, &candidate) {
        return refusal(reason, dd_path, check_index);
    }
    WritePath::Translated {
        path: candidate.path,
        value_transformation: inverted_for_write(candidate.value_transformation),
    }
}

fn write_candidate_refusal(
    role: ArgumentRole,
    candidate: &Candidate,
) -> Option<(String, String, Option<usize>)> {
    if candidate.dd_path == "ids_properties/version_put/data_dictionary" {
        Some((
            "the DD-version stamp is immutable under a version mismatch".to_string(),
            candidate.dd_path.clone(),
            Some(write_check_index(WriteCheck::ImmutableStamp)),
        ))
    } else if candidate
        .requested_precedence
        .is_some_and(|precedence| precedence != 1)
    {
        Some((
            "this path is a non-primary source and cannot write a shared stored slot".to_string(),
            candidate.dd_path.clone(),
            Some(write_check_index(WriteCheck::NonPrimarySource)),
        ))
    } else if role == ArgumentRole::Timebase
        && candidate.value_transformation != ValueTransformation::None
    {
        Some((
            "this timebase needs a value transformation, which al_write_data cannot apply"
                .to_string(),
            candidate.dd_path.clone(),
            Some(write_check_index(WriteCheck::TimebaseTransformation)),
        ))
    } else if role == ArgumentRole::Field && candidate.value_transformation.inverse().is_none() {
        Some((
            "this path needs a value transformation that cannot be inverted for a write"
                .to_string(),
            candidate.dd_path.clone(),
            Some(write_check_index(WriteCheck::InvertibleTransformation)),
        ))
    } else {
        None
    }
}

fn write_candidate(candidate: Candidate) -> WriteCandidate {
    WriteCandidate {
        path: candidate.path,
        stored_dd_path: candidate.stored_dd_path,
        precedence: candidate
            .precedence
            .expect("a plan candidate declares precedence"),
        value_transformation: inverted_for_write(candidate.value_transformation),
    }
}

/// ADR 0017 decision 4: delete-specific policy narrows the same answer, but
/// retains a plan because the seam policy itself must call Core for each path.
pub(crate) fn narrow_delete_path(
    record: &ConversionRecord,
    raw: *const c_char,
    resolved: Resolved,
) -> DeletePath {
    match resolved {
        Resolved::Forward => DeletePath::Forward,
        Resolved::Unclaimed => DeletePath::Refusal {
            reason: "this path is unclaimed by the conversion map".to_string(),
            dd_path: caller_dd_path(record, raw),
        },
        Resolved::NoSource(_) => DeletePath::Refusal {
            reason: "this path has no stored source".to_string(),
            dd_path: caller_dd_path(record, raw),
        },
        Resolved::Refusal {
            reason, dd_path, ..
        } => DeletePath::Refusal { reason, dd_path },
        Resolved::Single(candidate) => narrow_single_delete(record, candidate),
        Resolved::Plan(candidates) => {
            for candidate in &candidates {
                if let Some(refusal) = delete_candidate_refusal(record, candidate) {
                    return refusal;
                }
            }
            DeletePath::Candidates(
                candidates
                    .into_iter()
                    .map(|candidate| candidate.path)
                    .collect(),
            )
        }
    }
}

fn narrow_single_delete(record: &ConversionRecord, candidate: Candidate) -> DeletePath {
    if let Some(refusal) = delete_candidate_refusal(record, &candidate) {
        return refusal;
    }
    DeletePath::Translated(candidate.path)
}

fn delete_candidate_refusal(
    record: &ConversionRecord,
    candidate: &Candidate,
) -> Option<DeletePath> {
    let reason = if matches!(
        candidate.dd_path.as_str(),
        "ids_properties"
            | "ids_properties/version_put"
            | "ids_properties/version_put/data_dictionary"
    ) {
        delete_check_is_listed(DeleteCheck::ImmutableStamp);
        Some("this delete would remove the DD-version stamp while stored data remains")
    } else if candidate
        .requested_precedence
        .is_some_and(|precedence| precedence != 1)
    {
        delete_check_is_listed(DeleteCheck::NonPrimarySource);
        Some("this path is a non-primary source and cannot delete a shared stored slot")
    } else if !is_equilibrium_leaf(record, &candidate.dd_path)
        && !record.map.subtree_delete_is_trivial(
            &candidate.dd_path,
            &candidate.stored_dd_path,
            record.direction_to_stored,
        )
    {
        delete_check_is_listed(DeleteCheck::EscapingSubtree);
        Some("this subtree delete would leave data at a stored path outside the requested subtree")
    } else {
        None
    };
    reason.map(|reason| DeletePath::Refusal {
        reason: reason.to_string(),
        dd_path: candidate.dd_path.clone(),
    })
}

fn caller_dd_path(record: &ConversionRecord, raw: *const c_char) -> String {
    c_str_or_none(raw)
        .filter(|path| !path.is_empty())
        .map(|path| join_hli_path(&record.resolved_path, path))
        .unwrap_or_else(|| record.resolved_path.clone())
}

/// `al_delete_data` gives the shim a path string but no datatype or other
/// marker distinguishing an IDS leaf from a container. The one embedded
/// equilibrium artifact is shipped with its real DD leaf inventories, so the
/// leaf-only delete policy can answer that question before IMAS-Core is
/// called. This is a safety classification only, not conversion-rule
/// selection; ADR 0013 decision 6 records the narrow exception to the
/// inventories' proof role. A future generated artifact must carry the
/// equivalent inventory before this seam can serve it; today it cannot be a
/// live conversion map.
fn is_equilibrium_leaf(record: &ConversionRecord, hli_path: &str) -> bool {
    const LEFT_LEAVES: &str = include_str!("../../docs/inventory/equilibrium-3.39.0.txt");
    const RIGHT_LEAVES: &str = include_str!("../../docs/inventory/equilibrium-4.1.1.txt");

    let inventory = match record.direction_to_stored {
        Direction::Forward => LEFT_LEAVES,
        Direction::Reverse => RIGHT_LEAVES,
    };
    inventory.lines().any(|leaf| leaf == hli_path)
}

/// Distinguishes a conditional merged/split conversion from an unconditional
/// lossy conversion only where a read exposes the verdict to the caller. The
/// conversion map retains its literal `lossy` declaration; ADR 0008 assigns
/// the potential-loss meaning from the selected rule kind.
fn read_fidelity(fidelity: Fidelity, rel: Option<Rel>) -> Fidelity {
    match (fidelity, rel) {
        (Fidelity::Lossy, Some(Rel::Merged | Rel::Split)) => Fidelity::PotentiallyLossy,
        _ => fidelity,
    }
}

/// Turns a resolved stored-DD path into the exact spelling IMAS-Core must
/// receive: absolute when the caller spelled its argument absolutely,
/// otherwise stripped back to this context's own stored anchor. Both
/// path-bearing seam resolves this spelling through the shared resolver, so
/// its relative-anchor behavior and failures are worded once.
fn stored_c_path(
    record: &ConversionRecord,
    resolved_path: &str,
    is_absolute: bool,
) -> Result<CString, String> {
    let translated = if is_absolute {
        format!("/{resolved_path}")
    } else {
        let anchor = stored_anchor(record)?;
        strip_anchor(&anchor, resolved_path).ok_or_else(|| {
            "translated path does not lie beneath this context's stored anchor".to_string()
        })?
    };
    CString::new(translated)
        .map_err(|_| "translated field contains an interior NUL byte".to_string())
}

/// Resolves the context's HLI-DD anchor to its stored-DD spelling. A child
/// record deliberately retains its HLI-DD anchor, so a renamed AOS container
/// must be converted here before a relative Core argument can be formed.
fn stored_anchor(record: &ConversionRecord) -> Result<String, String> {
    if record.resolved_path.is_empty() {
        return Ok(String::new());
    }
    let Some(explanation) = record
        .map
        .resolve(&record.resolved_path, record.direction_to_stored)
    else {
        return Err("context anchor has no stored-DD conversion rule".to_string());
    };
    match explanation.outcome {
        Outcome::Refusal(reason) => Err(refusal_reason_message(reason)),
        Outcome::NoSource => Err("context anchor has no stored source".to_string()),
        Outcome::Path { .. } if matches!(explanation.rel, Some(Rel::Merged | Rel::Split)) => Err(
            "this path is served by several stored candidates, and only a data read can try them in turn".to_string(),
        ),
        Outcome::Path { value_transformation, .. }
            if value_transformation != ValueTransformation::None => Err(
                "this path needs a value transformation, which only a data read can apply".to_string(),
            ),
        Outcome::Path { resolved_path, .. } => Ok(resolved_path),
    }
}

/// Joins `anchor` (a context's own resolved path, in the HLI's own DD
/// spelling) with a relative `raw` path argument, or resolves `raw` from the
/// IDS root when it is absolute (a leading `/`) — the same relative-vs-
/// absolute rule every path/field argument follows (CLAUDE.md).
pub(crate) fn join_hli_path(anchor: &str, raw: &str) -> String {
    match raw.strip_prefix('/') {
        Some(root_relative) => root_relative.to_string(),
        None if anchor.is_empty() => raw.to_string(),
        None => format!("{anchor}/{raw}"),
    }
}

/// The portion of `resolved_path` (a full path in the stored DD's spelling)
/// past `anchor` (this context's own stored-DD path) — the field IMAS-Core
/// actually expects relative to `ctx_id`.
fn strip_anchor(anchor: &str, resolved_path: &str) -> Option<String> {
    if anchor.is_empty() {
        return Some(resolved_path.to_string());
    }
    resolved_path
        .strip_prefix(anchor)
        .and_then(|rest| rest.strip_prefix('/'))
        .map(str::to_string)
}

/// A short, stable description of one [`RefusalReason`]. Full refusal-
/// message formatting (naming the DD path and both versions, with
/// CLAUDE.md's fixed truncation order) is issue #58's contract; a caller of
/// this only needs `conversion_refusal`'s existing `IMAS-MVDD:`-prefixed
/// wrapper.
fn refusal_reason_message(reason: RefusalReason) -> String {
    match reason {
        RefusalReason::UnservableRetype => {
            "this path's container changed shape and cannot be served".to_string()
        }
        RefusalReason::UnitRedefinition => {
            "this path's unit was redefined and cannot be converted".to_string()
        }
        RefusalReason::Unmappable => {
            "this path has no safe conversion between DD versions".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::conversion_map::{ConversionMap, Direction};
    use std::sync::Arc;

    /// Builds the resolver seam directly. Path resolution has no registry
    /// policy of its own, so tests for it must not coordinate through the
    /// process-global registry or invent an IDS cache key.
    fn record(artifact: &str, resolved_path: &str) -> ConversionRecord {
        ConversionRecord {
            resolved_path: resolved_path.to_string(),
            pulse_ctx_id: 0,
            map: Arc::new(ConversionMap::load(artifact).expect("fixture artifact must load")),
            root_id: 0,
            direction_to_stored: Direction::Forward,
            stored_version: "4.1.1".parse().expect("known release"),
            hli_version: "3.39.0".parse().expect("known release"),
            parent_id: None,
        }
    }

    /// User story 47: "As an HLI reading through a known version mismatch, I
    /// want a path that no rule claims a source for to return not-found
    /// (`code == 0`, null data), so that an unclaimed path is never silently
    /// forwarded under the wrong DD spelling."
    ///
    /// This is asserted here rather than through the C ABI because the
    /// embedded artifact carries `<default rel="identical"/>`, so
    /// `ConversionMap::resolve` never returns `None` for it and no stub test
    /// can reach the branch. A fixture artifact with no document-level default
    /// is the only way to reach it, and `record_root`'s map-creating closure is
    /// the seam that admits one.
    #[test]
    fn an_unclaimed_read_path_returns_not_found_rather_than_forwarding() {
        // No <default>: a path no rule claims is genuinely unclaimed here.
        const NO_DEFAULT_ARTIFACT: &str = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="rename-claimed" rel="renamed" left="claimed" right="claimed_new">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let record = record(NO_DEFAULT_ARTIFACT, "");

        // The rule the fixture does carry still resolves, so an unclaimed
        // verdict below cannot be the whole map failing to match anything.
        // Asserting the *stored spelling* rather than just "it translated"
        // also pins that this record really resolves through the fixture: the
        // approved map would rename nothing and hand back `claimed` itself
        // through its identity default.
        let claimed = CString::new("claimed").expect("no interior NUL");
        match narrow_read_path(resolve(&record, claimed.as_ptr())) {
            ReadPath::Translated(translated) => assert_eq!(
                translated
                    .paths
                    .first()
                    .expect("a translated path carries at least one candidate")
                    .path
                    .to_str()
                    .expect("the fixture's spellings are ASCII"),
                "claimed_new",
                "this record must resolve through the fixture map, not a \
                 cached map for another version pair"
            ),
            _ => panic!("the fixture's own rule must still translate"),
        }

        let unclaimed = CString::new("nothing/claims/this").expect("no interior NUL");
        match narrow_read_path(resolve(&record, unclaimed.as_ptr())) {
            ReadPath::NoSource(fidelity) => assert_eq!(
                fidelity,
                Fidelity::Unmappable,
                "the caller must be able to see why the read came back empty"
            ),
            ReadPath::Forward => panic!(
                "an unclaimed path was forwarded to IMAS-Core under the HLI's own \
                 DD spelling, which user story 47 forbids"
            ),
            _ => panic!("an unclaimed path must resolve to not-found"),
        }

        // An argument with no path at all is a different thing entirely and
        // must still forward: `timebase` is routinely empty, and forwarding it
        // is how a read without one works.
        let empty = CString::new("").expect("no interior NUL");
        assert!(
            matches!(
                narrow_read_path(resolve(&record, empty.as_ptr())),
                ReadPath::Forward
            ),
            "an empty argument is absent, not unclaimed"
        );
        assert!(
            matches!(
                narrow_read_path(resolve(&record, std::ptr::null())),
                ReadPath::Forward
            ),
            "a null argument is absent, not unclaimed"
        );
    }

    /// Issue #126 pins the map-derived write refusals with an isolated
    /// artifact. The registry caches maps by IDS name as well as version
    /// pair, so this fixture must not claim the shipped `equilibrium` key:
    /// another test can keep that map live while this one runs.
    #[test]
    fn write_pre_resolution_refusals_keep_the_shared_guard_ahead_of_rule_specific_ones() {
        const ARTIFACT: &str = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="retyped-wins" rel="retyped" left="shape" right="shape">
                  <fidelity forward="unmappable" reverse="unmappable"/>
                </rule>
                <rule id="declared-impossible" rel="renamed" left="impossible" right="stored">
                  <fidelity forward="unmappable" reverse="exact"/>
                </rule>
                <rule id="no-stored-slot" rel="left_only" left="missing">
                  <fidelity forward="lossy" reverse="unmappable"/>
                </rule>
                <rule id="collides-and-unmappable" rel="merged" right="folded">
                  <from left="primary" precedence="1"/>
                  <from left="secondary" precedence="2"/>
                  <fidelity forward="unmappable" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let record = record(ARTIFACT, "");

        let assert_refusal = |path: &str, expected_reason: &str| {
            let path = CString::new(path).expect("fixture paths contain no NUL");
            match narrow_write_path(
                &record,
                path.as_ptr(),
                ArgumentRole::Field,
                resolve(&record, path.as_ptr()),
            ) {
                WritePath::Refusal {
                    reason, dd_path, ..
                } => {
                    assert_eq!(reason, expected_reason);
                    assert_eq!(dd_path, path.to_str().expect("fixture paths are ASCII"));
                }
                WritePath::Forward | WritePath::Translated { .. } | WritePath::Candidates(_) => {
                    panic!("{path:?} must refuse before IMAS-Core")
                }
            }
        };

        assert_refusal(
            "shape",
            "this path's container changed shape and cannot be served",
        );
        assert_refusal(
            "impossible",
            "this path has no safe conversion between DD versions",
        );
        assert_refusal("missing", "this path has no stored source");

        // The only configuration in which the order is observable at all:
        // `secondary` is a precedence-2 `<from>`, so the write-specific
        // collision guard claims it, *and* its rule is declared `unmappable`
        // in the direction under test, so the shared guard claims it too.
        // Reporting the rule's own reason is what proves the shared guard ran
        // first; swapping the two guards leaves every assertion above green
        // and turns this one into "non-primary source".
        assert_refusal(
            "secondary",
            "this path has no safe conversion between DD versions",
        );
    }

    /// Issue #126 / review finding S-J3: the delete narrowing carries the
    /// same two guards in the same order, so one rule earns one reason at
    /// both seams. The write's own order is pinned by
    /// `write_pre_resolution_refusals_keep_the_shared_guard_ahead_of_rule_specific_ones`
    /// directly above; this is the delete half of the same claim.
    ///
    /// The third refusal issue #126 names — a value transformation that
    /// cannot be inverted — is not orderable against these two and so is
    /// absent from both fixtures deliberately: it lives in
    /// `seam_policy::run_write`, which can only see a transformation *after*
    /// resolution has produced one, and a delete carries no value at all.
    #[test]
    fn delete_pre_resolution_refusals_keep_the_shared_guard_ahead_of_rule_specific_ones() {
        const ARTIFACT: &str = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="collides-and-unmappable" rel="merged" right="folded">
                  <from left="primary" precedence="1"/>
                  <from left="secondary" precedence="2"/>
                  <fidelity forward="unmappable" reverse="exact"/>
                </rule>
                <rule id="collides-only" rel="merged" right="other_folded">
                  <from left="other_primary" precedence="1"/>
                  <from left="other_secondary" precedence="2"/>
                  <fidelity forward="lossy" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let record = record(ARTIFACT, "");

        let assert_refusal = |path: &str, expected_reason: &str| {
            let path = CString::new(path).expect("fixture paths contain no NUL");
            match narrow_delete_path(&record, path.as_ptr(), resolve(&record, path.as_ptr())) {
                DeletePath::Refusal { reason, dd_path } => {
                    assert_eq!(reason, expected_reason);
                    assert_eq!(dd_path, path.to_str().expect("fixture paths are ASCII"));
                }
                DeletePath::Forward | DeletePath::Translated(_) | DeletePath::Candidates(_) => {
                    panic!("{path:?} must refuse before IMAS-Core")
                }
            }
        };

        // Both guards claim `secondary`; the shared one answers, exactly as it
        // does at the write seam.
        assert_refusal(
            "secondary",
            "this path has no safe conversion between DD versions",
        );
        // Only the delete-specific guard claims `other_secondary`, so its
        // reason is the one a caller sees — the hoist above did not swallow
        // the collision guard.
        assert_refusal(
            "other_secondary",
            "this path is a non-primary source and cannot delete a shared stored slot",
        );
    }

    /// Issue #131 / ADR 0017 decision 4: a structure delete resolves and
    /// deletes when it is trivial, and refuses before IMAS-Core when it is
    /// not. The delete narrowing's leaf/structure classification runs
    /// against the real embedded equilibrium inventories regardless of this
    /// fixture map, so both paths below — invented names that appear in
    /// neither inventory — are classified as structures by construction,
    /// which is what lets this fixture exercise the escaping-rule check at
    /// all without needing a real DD leaf name.
    #[test]
    fn delete_narrowing_admits_a_trivial_structure_and_refuses_an_escaping_one() {
        const ARTIFACT: &str = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <default rel="identical"/>
              <rules>
                <rule id="move-out" rel="moved"
                      left="escaping_root/leaf" right="elsewhere/leaf" subtree="yes">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
                <rule id="rename-within" rel="renamed"
                      left="trivial_root/old_name" right="trivial_root/new_name">
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let record = record(ARTIFACT, "");

        let trivial = CString::new("trivial_root").expect("no interior NUL");
        match narrow_delete_path(
            &record,
            trivial.as_ptr(),
            resolve(&record, trivial.as_ptr()),
        ) {
            DeletePath::Translated(path) => {
                assert_eq!(path.to_str().expect("ASCII"), "trivial_root");
            }
            DeletePath::Refusal { reason, .. } => {
                panic!("a trivial structure delete must resolve, refused instead: {reason}")
            }
            DeletePath::Forward | DeletePath::Candidates(_) => {
                panic!("a trivial structure delete must resolve to one translated path")
            }
        }

        let escaping = CString::new("escaping_root").expect("no interior NUL");
        match narrow_delete_path(
            &record,
            escaping.as_ptr(),
            resolve(&record, escaping.as_ptr()),
        ) {
            DeletePath::Refusal { reason, dd_path } => {
                assert_eq!(
                    reason,
                    "this subtree delete would leave data at a stored path outside the \
                     requested subtree"
                );
                assert_eq!(dd_path, "escaping_root");
            }
            _ => panic!("an escaping-rule subtree delete must refuse before IMAS-Core"),
        }
    }

    #[test]
    fn a_one_source_merged_rule_remains_a_plan_at_every_narrowing() {
        const ARTIFACT: &str = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="fold-one" rel="merged" right="folded">
                  <from left="source" precedence="1"/>
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let record = record(ARTIFACT, "");
        let source = CString::new("source").expect("no interior NUL");

        assert!(matches!(
            resolve(&record, source.as_ptr()),
            Resolved::Plan(_)
        ));
        assert!(matches!(
            narrow_write_path(
                &record,
                source.as_ptr(),
                ArgumentRole::Field,
                resolve(&record, source.as_ptr()),
            ),
            WritePath::Candidates(_)
        ));
        assert!(matches!(
            narrow_delete_path(&record, source.as_ptr(), resolve(&record, source.as_ptr())),
            DeletePath::Candidates(_)
        ));
    }

    #[test]
    fn join_hli_path_appends_a_relative_path_under_a_nonempty_anchor() {
        assert_eq!(
            join_hli_path("time_slice", "global_quantities/beta_tor_norm"),
            "time_slice/global_quantities/beta_tor_norm"
        );
    }

    #[test]
    fn join_hli_path_uses_the_relative_path_verbatim_under_an_empty_anchor() {
        assert_eq!(join_hli_path("", "time_slice"), "time_slice");
    }

    #[test]
    fn join_hli_path_resolves_an_absolute_path_from_the_ids_root_ignoring_the_anchor() {
        assert_eq!(
            join_hli_path("time_slice", "/ids_properties/comment"),
            "ids_properties/comment"
        );
    }

    #[test]
    fn strip_anchor_returns_the_resolved_path_verbatim_under_an_empty_anchor() {
        assert_eq!(
            strip_anchor("", "time_slice/global_quantities/beta_normal"),
            Some("time_slice/global_quantities/beta_normal".to_string())
        );
    }

    #[test]
    fn strip_anchor_removes_a_matching_anchor_prefix() {
        assert_eq!(
            strip_anchor("time_slice", "time_slice/global_quantities/beta_normal"),
            Some("global_quantities/beta_normal".to_string())
        );
    }

    #[test]
    fn strip_anchor_fails_when_the_resolved_path_does_not_start_with_the_anchor() {
        assert_eq!(
            strip_anchor("time_slice", "grids_ggd/grid/space/coordinates_type"),
            None
        );
    }
}
