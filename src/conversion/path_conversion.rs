//! What stored path an HLI argument means, and at what fidelity.
//!
//! Before this module existed, [`crate::conversion::conversion_map::Outcome`] was
//! interpreted at three independent sites in the interposition layer, each
//! deriving a different subset of its meaning: global-action datapath
//! translation derived a concrete spelling or nothing, the read seam derived
//! a [`ReadPath`] with fidelity and candidates, and the context-opening seams
//! derived one concrete spelling, no-source, or a refusal. Those sites are
//! `src/interpose/occurrence.rs` and `src/interpose/read.rs` today. This module is the
//! one place that answers the question instead, so no consumer re-derives the
//! enum. [`resolve`] answers it once; each ABI seam applies its own named
//! narrowing.
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
    /// The artifact claimed the path but names no stored counterpart.
    /// `requested_precedence` travels with it because the non-primary check
    /// sits ahead of the no-stored-source check in both seams' declared
    /// orders, and so must be answerable without a candidate to read it from.
    NoSource {
        fidelity: Fidelity,
        requested_precedence: Option<u32>,
    },
    Unclaimed,
    /// The artifact or the shared map declines to serve the rule.
    /// `requested_precedence` travels with it for the same reason
    /// [`Resolved::NoSource`] carries one: which of the two checks that can
    /// claim such a path answers first is decided by the declared order, not
    /// by withholding the fact one of them reads.
    Refusal {
        reason: String,
        dd_path: String,
        fidelity: Fidelity,
        requested_precedence: Option<u32>,
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
    Refusal { reason: String, dd_path: String },
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
    /// Inverted to point at stored data on the precedence-1 candidate — the
    /// one slot this write may change, and the only candidate whose
    /// transformation the seam policy reads. Every other candidate keeps the
    /// read-direction transformation the map declared, because it is never
    /// written and inverting it can fail.
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
    /// `pub(crate)` so the read loop (`src/conversion/seam_policy.rs`) can turn each
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
    /// Whether a check tagged `self` serves an argument supplied in `role`.
    /// This is what keeps the last entries of each list from firing on the
    /// wrong argument: a write resolves `field` and `timebase` through one
    /// list, and refusing a field for a timebase's rule would reject every
    /// legitimate COCOS sign flip.
    fn serves(self, role: Self) -> bool {
        self == Self::Any || self == role
    }
}

/// The DD-version stamp. A write must never rewrite it under a mismatch: this
/// call can translate one field, not migrate the occurrence into the HLI DD
/// version (ADR 0016 decision 5).
const DD_VERSION_STAMP: &str = "ids_properties/version_put/data_dictionary";

/// The stamp and the two containers that hold it. *Deleting* any of the three
/// takes the stamp with it (ADR 0016 decision 6), whereas *writing* the two
/// containers describes the writing library rather than the DD and stays
/// ordinary — which is why only the delete list reads this.
const DD_VERSION_STAMP_ANCESTRY: &[&str] = &[
    "ids_properties",
    "ids_properties/version_put",
    DD_VERSION_STAMP,
];

/// What the ordered checks below are allowed to look at.
///
/// The two resolution-level facts are held apart from the resolution's shape
/// on purpose: the stamp check and the non-primary check read only these, and
/// therefore give the same answer whether the map produced one stored source,
/// a plan, or none at all. That is what lets both sit ahead of the shared
/// guard in the declared order without needing a candidate to exist.
struct CheckSubject<'a> {
    /// The absolute HLI-DD spelling a refusal names — the caller's own joined
    /// path where no rule claimed it.
    dd_path: &'a str,
    /// The precedence the rule assigned the HLI spelling, present whenever the
    /// map claimed it. Deliberately distinct from a candidate's own precedence
    /// inside a declared plan.
    requested_precedence: Option<u32>,
    shape: CheckShape<'a>,
}

/// The shape a resolution presented to the checks.
enum CheckShape<'a> {
    /// A real path argument no rule and no document-level default claims.
    Unclaimed,
    /// The artifact says no stored counterpart exists.
    NoStoredSource,
    /// The shared conversion map declined to serve the rule, with its reason.
    SharedRefusal(&'a str),
    /// The map named a stored source: the one candidate a write may change, or
    /// the candidate a delete is currently considering. `None` is a declared
    /// plan holding no source this seam may act on — the resolution-level
    /// checks still run, and the narrowing reports the missing slot after
    /// they have had their say.
    Candidate(Option<&'a Candidate>),
}

impl<'a> CheckSubject<'a> {
    fn candidate(&self) -> Option<&'a Candidate> {
        match self.shape {
            CheckShape::Candidate(candidate) => candidate,
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WriteCheck {
    Unclaimed,
    ImmutableStamp,
    SharedRefusal,
    NonPrimarySource,
    NoStoredSource,
    TimebaseTransformation,
    InvertibleTransformation,
}

/// The complete write refusal order, and the one artifact that fixes it:
/// [`first_write_refusal`] walks exactly this list in exactly this order, so a
/// fifth operation cannot inherit a different order by imitating a sibling's
/// `if` chain (ADR 0016 decision 9, ADR 0021 decision 4). The shared guard is
/// at a fixed position here by construction rather than by convention.
///
/// The role tag is part of the policy, not decoration — see
/// [`ArgumentRole::serves`].
const WRITE_CHECKS: &[(ArgumentRole, WriteCheck)] = &[
    (ArgumentRole::Any, WriteCheck::Unclaimed),
    (ArgumentRole::Any, WriteCheck::ImmutableStamp), // ADR 0016 decision 5
    (ArgumentRole::Any, WriteCheck::SharedRefusal),  // ADR 0016 decision 9 step 1
    (ArgumentRole::Any, WriteCheck::NonPrimarySource), // ADR 0016 decision 2
    (ArgumentRole::Any, WriteCheck::NoStoredSource), // ADR 0016 decision 3
    (ArgumentRole::Timebase, WriteCheck::TimebaseTransformation),
    (ArgumentRole::Field, WriteCheck::InvertibleTransformation), // ADR 0016 decision 7
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DeleteCheck {
    Unclaimed,
    ImmutableStamp,
    SharedRefusal,
    NonPrimarySource,
    NoStoredSource,
    EscapingSubtree,
}

/// The complete delete refusal order, walked by [`first_delete_refusal`] with
/// the same guarantee [`WRITE_CHECKS`] carries. Every entry serves the seam's
/// one `path` argument, so no entry is role-narrowed here.
const DELETE_CHECKS: &[(ArgumentRole, DeleteCheck)] = &[
    (ArgumentRole::Path, DeleteCheck::Unclaimed),
    (ArgumentRole::Path, DeleteCheck::ImmutableStamp), // ADR 0016 decision 6
    (ArgumentRole::Path, DeleteCheck::SharedRefusal),
    (ArgumentRole::Path, DeleteCheck::NonPrimarySource),
    (ArgumentRole::Path, DeleteCheck::NoStoredSource),
    (ArgumentRole::Path, DeleteCheck::EscapingSubtree), // ADR 0017
];

/// The first write refusal the declared order reaches, or `None` where every
/// check that serves `role` passes.
fn first_write_refusal(role: ArgumentRole, subject: &CheckSubject<'_>) -> Option<String> {
    WRITE_CHECKS
        .iter()
        .filter(|(tag, _)| tag.serves(role))
        .find_map(|(_, check)| write_check_refusal(*check, subject))
}

fn write_check_refusal(check: WriteCheck, subject: &CheckSubject<'_>) -> Option<String> {
    match check {
        WriteCheck::Unclaimed => matches!(subject.shape, CheckShape::Unclaimed)
            .then(|| "this path is unclaimed by the conversion map".to_string()),
        WriteCheck::ImmutableStamp => (subject.dd_path == DD_VERSION_STAMP)
            .then(|| "the DD-version stamp is immutable under a version mismatch".to_string()),
        // A shared refusal is decided here rather than at the non-primary
        // check below it: a rule that cannot be served at all must not appear
        // to be merely a collision risk.
        WriteCheck::SharedRefusal => match subject.shape {
            CheckShape::SharedRefusal(reason) => Some(reason.to_string()),
            _ => None,
        },
        WriteCheck::NonPrimarySource => subject
            .requested_precedence
            .is_some_and(|precedence| precedence != 1)
            .then(|| {
                "this path is a non-primary source and cannot write a shared stored slot"
                    .to_string()
            }),
        WriteCheck::NoStoredSource => matches!(subject.shape, CheckShape::NoStoredSource)
            .then(|| "this path has no stored source".to_string()),
        WriteCheck::TimebaseTransformation => subject
            .candidate()
            .is_some_and(|candidate| candidate.value_transformation != ValueTransformation::None)
            .then(|| {
                "this timebase needs a value transformation, which al_write_data cannot apply"
                    .to_string()
            }),
        WriteCheck::InvertibleTransformation => subject
            .candidate()
            .is_some_and(|candidate| candidate.value_transformation.inverse().is_none())
            .then(|| {
                "this path needs a value transformation that cannot be inverted for a write"
                    .to_string()
            }),
    }
}

/// The first delete refusal the declared order reaches, or `None` where every
/// check passes.
fn first_delete_refusal(
    record: &ConversionRecord,
    role: ArgumentRole,
    subject: &CheckSubject<'_>,
) -> Option<String> {
    DELETE_CHECKS
        .iter()
        .filter(|(tag, _)| tag.serves(role))
        .find_map(|(_, check)| delete_check_refusal(*check, record, subject))
}

fn delete_check_refusal(
    check: DeleteCheck,
    record: &ConversionRecord,
    subject: &CheckSubject<'_>,
) -> Option<String> {
    match check {
        DeleteCheck::Unclaimed => matches!(subject.shape, CheckShape::Unclaimed)
            .then(|| "this path is unclaimed by the conversion map".to_string()),
        DeleteCheck::ImmutableStamp => {
            DD_VERSION_STAMP_ANCESTRY
                .contains(&subject.dd_path)
                .then(|| {
                    "this delete would remove the DD-version stamp while stored data remains"
                        .to_string()
                })
        }
        DeleteCheck::SharedRefusal => match subject.shape {
            CheckShape::SharedRefusal(reason) => Some(reason.to_string()),
            _ => None,
        },
        DeleteCheck::NonPrimarySource => subject
            .requested_precedence
            .is_some_and(|precedence| precedence != 1)
            .then(|| {
                "this path is a non-primary source and cannot delete a shared stored slot"
                    .to_string()
            }),
        DeleteCheck::NoStoredSource => matches!(subject.shape, CheckShape::NoStoredSource)
            .then(|| "this path has no stored source".to_string()),
        DeleteCheck::EscapingSubtree => subject
            .candidate()
            .is_some_and(|candidate| {
                !is_equilibrium_leaf(record, &candidate.dd_path)
                    && !record.map.subtree_delete_is_trivial(
                        &candidate.dd_path,
                        &candidate.stored_dd_path,
                        record.direction_to_stored,
                    )
            })
            .then(|| {
                "this subtree delete would leave data at a stored path outside the requested \
                 subtree"
                    .to_string()
            }),
    }
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
    let precedence_for_refusal = metadata.requested_precedence;
    let refusal = move |reason| Resolved::Refusal {
        reason,
        dd_path: dd_path_for_refusal.clone(),
        fidelity: Fidelity::Unmappable,
        requested_precedence: precedence_for_refusal,
    };
    match explanation.outcome {
        Outcome::Refusal(reason) => refusal(refusal_reason_message(reason)),
        Outcome::NoSource => Resolved::NoSource {
            fidelity: metadata.fidelity,
            requested_precedence: metadata.requested_precedence,
        },
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
        Resolved::NoSource { .. } => ContextPathResolution::NoSource,
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
        Resolved::NoSource { fidelity, .. } => ReadPath::NoSource(fidelity),
        Resolved::Refusal {
            reason,
            dd_path,
            fidelity,
            ..
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
///
/// Every refusal below comes from one walk of [`WRITE_CHECKS`]; the match only
/// says what each resolution shape looks like to those checks. That is what
/// keeps the declared order and the enacted order the same artifact.
pub(crate) fn narrow_write_path(
    record: &ConversionRecord,
    raw: *const c_char,
    role: ArgumentRole,
    resolved: Resolved,
) -> WritePath {
    if matches!(resolved, Resolved::Forward) {
        return WritePath::Forward;
    }
    let caller = caller_dd_path(record, raw);
    // The primary is the one stored slot a write may change, so it is also the
    // candidate the role-specific checks judge. A plan that declares none
    // still runs every resolution-level check first.
    let primary = match &resolved {
        Resolved::Single(candidate) => Some(candidate),
        Resolved::Plan(candidates) => candidates
            .iter()
            .find(|candidate| candidate.precedence == Some(1)),
        _ => None,
    };
    if let Some(refusal) = write_subject(&resolved, &caller, primary)
        .as_ref()
        .and_then(|subject| {
            first_write_refusal(role, subject).map(|reason| WritePath::Refusal {
                reason,
                dd_path: subject.dd_path.to_string(),
            })
        })
    {
        return refusal;
    }
    match resolved {
        Resolved::Single(candidate) => WritePath::Translated {
            path: candidate.path,
            value_transformation: inverted_for_write(candidate.value_transformation),
        },
        Resolved::Plan(candidates) if primary.is_some() => {
            WritePath::Candidates(candidates.into_iter().map(write_candidate).collect())
        }
        // A declared plan naming no precedence-1 source has no slot to write.
        // This is not one of the ordered checks: those judge the rule, and
        // this reports that the rule named nothing this seam can act on.
        Resolved::Plan(_) => WritePath::Refusal {
            reason: "this candidate plan has no precedence-1 source for a write".to_string(),
            dd_path: caller,
        },
        // `Forward` returned above; the three shapes naming no stored source
        // are each covered by an entry in `WRITE_CHECKS` and refused there.
        other => write_shape_refusal(&other, caller),
    }
}

/// The subject one resolution presents to [`WRITE_CHECKS`].
fn write_subject<'a>(
    resolved: &'a Resolved,
    caller: &'a str,
    primary: Option<&'a Candidate>,
) -> Option<CheckSubject<'a>> {
    let subject = match resolved {
        Resolved::Forward => return None,
        Resolved::Unclaimed => CheckSubject {
            dd_path: caller,
            requested_precedence: None,
            shape: CheckShape::Unclaimed,
        },
        Resolved::NoSource {
            requested_precedence,
            ..
        } => CheckSubject {
            dd_path: caller,
            requested_precedence: *requested_precedence,
            shape: CheckShape::NoStoredSource,
        },
        Resolved::Refusal {
            reason,
            dd_path,
            requested_precedence,
            ..
        } => CheckSubject {
            dd_path,
            requested_precedence: *requested_precedence,
            shape: CheckShape::SharedRefusal(reason),
        },
        Resolved::Single(candidate) => CheckSubject {
            dd_path: &candidate.dd_path,
            requested_precedence: candidate.requested_precedence,
            shape: CheckShape::Candidate(Some(candidate)),
        },
        Resolved::Plan(candidates) => CheckSubject {
            dd_path: candidates.first().map_or(caller, |first| &first.dd_path),
            requested_precedence: candidates
                .first()
                .and_then(|first| first.requested_precedence),
            shape: CheckShape::Candidate(primary),
        },
    };
    Some(subject)
}

/// The refusal a shape naming no stored source earns. Reached only if a future
/// shape escapes [`WRITE_CHECKS`]: reporting the list's own no-stored-slot
/// verdict refuses safely rather than writing a path no check has judged.
fn write_shape_refusal(resolved: &Resolved, caller: String) -> WritePath {
    let (reason, dd_path) = match resolved {
        Resolved::Refusal {
            reason, dd_path, ..
        } => (reason.clone(), dd_path.clone()),
        _ => ("this path has no stored source".to_string(), caller),
    };
    WritePath::Refusal { reason, dd_path }
}

fn write_candidate(candidate: Candidate) -> WriteCandidate {
    let precedence = candidate
        .precedence
        .expect("a plan candidate declares precedence");
    WriteCandidate {
        path: candidate.path,
        stored_dd_path: candidate.stored_dd_path,
        precedence,
        // Only the precedence-1 candidate is ever written, and the seam policy
        // reads only its transformation; the rest exist to be named in the
        // loss log. Inverting them too would `expect` on a transformation that
        // no write can reach, and WriteCheck::InvertibleTransformation judges
        // the primary alone — so a non-invertible non-primary would abort the
        // process across the C ABI rather than refuse.
        value_transformation: if precedence == 1 {
            inverted_for_write(candidate.value_transformation)
        } else {
            candidate.value_transformation
        },
    }
}

/// ADR 0017 decision 4: delete-specific policy narrows the same answer, but
/// retains a plan because the seam policy itself must call Core for each path.
pub(crate) fn narrow_delete_path(
    record: &ConversionRecord,
    raw: *const c_char,
    resolved: Resolved,
) -> DeletePath {
    if matches!(resolved, Resolved::Forward) {
        return DeletePath::Forward;
    }
    let caller = caller_dd_path(record, raw);
    // Unlike a write, a delete acts on every candidate, so every candidate is
    // judged. The resolution-level checks read only `dd_path` and the rule's
    // own precedence, which every candidate shares, so walking the list once
    // per candidate cannot report a later check ahead of an earlier one.
    if let Some(refusal) = delete_subjects(&resolved, &caller)
        .iter()
        .find_map(|subject| {
            first_delete_refusal(record, ArgumentRole::Path, subject).map(|reason| {
                DeletePath::Refusal {
                    reason,
                    dd_path: subject.dd_path.to_string(),
                }
            })
        })
    {
        return refusal;
    }
    match resolved {
        Resolved::Single(candidate) => DeletePath::Translated(candidate.path),
        Resolved::Plan(candidates) => DeletePath::Candidates(
            candidates
                .into_iter()
                .map(|candidate| candidate.path)
                .collect(),
        ),
        // `Forward` returned above; the three shapes naming no stored source
        // are each covered by an entry in `DELETE_CHECKS` and refused there.
        other => delete_shape_refusal(&other, caller),
    }
}

/// Every subject one resolution presents to [`DELETE_CHECKS`], in the order a
/// delete would act on them.
fn delete_subjects<'a>(resolved: &'a Resolved, caller: &'a str) -> Vec<CheckSubject<'a>> {
    let candidate_subject = |candidate: &'a Candidate| CheckSubject {
        dd_path: &candidate.dd_path,
        requested_precedence: candidate.requested_precedence,
        shape: CheckShape::Candidate(Some(candidate)),
    };
    match resolved {
        Resolved::Forward => Vec::new(),
        Resolved::Unclaimed => vec![CheckSubject {
            dd_path: caller,
            requested_precedence: None,
            shape: CheckShape::Unclaimed,
        }],
        Resolved::NoSource {
            requested_precedence,
            ..
        } => vec![CheckSubject {
            dd_path: caller,
            requested_precedence: *requested_precedence,
            shape: CheckShape::NoStoredSource,
        }],
        Resolved::Refusal {
            reason,
            dd_path,
            requested_precedence,
            ..
        } => vec![CheckSubject {
            dd_path,
            requested_precedence: *requested_precedence,
            shape: CheckShape::SharedRefusal(reason),
        }],
        Resolved::Single(candidate) => vec![candidate_subject(candidate)],
        Resolved::Plan(candidates) => candidates.iter().map(candidate_subject).collect(),
    }
}

/// The refusal a shape naming no stored source earns, for the same reason
/// [`write_shape_refusal`] exists.
fn delete_shape_refusal(resolved: &Resolved, caller: String) -> DeletePath {
    let (reason, dd_path) = match resolved {
        Resolved::Refusal {
            reason, dd_path, ..
        } => (reason.clone(), dd_path.clone()),
        _ => ("this path has no stored source".to_string(), caller),
    };
    DeletePath::Refusal { reason, dd_path }
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
    use crate::conversion::conversion_map::{ConversionMap, Direction, TransformationDirection};
    use std::sync::Arc;

    /// Builds the resolver seam directly. Path resolution has no registry
    /// policy of its own, so tests for it must not coordinate through the
    /// process-global registry or invent an IDS cache key.
    fn record(artifact: &str, resolved_path: &str) -> ConversionRecord {
        ConversionRecord {
            resolved_path: resolved_path.to_string(),
            pulse_ctx_id: 0,
            dataobjectname: String::new(),
            pulse_uri: String::new(),
            map: Arc::new(ConversionMap::load(artifact).expect("fixture artifact must load")),
            root_id: 0,
            direction_to_stored: Direction::Forward,
            stored_version: "4.1.1".parse().expect("known release"),
            hli_version: "3.39.0".parse().expect("known release"),
            parent_id: None,
        }
    }

    /// A record whose stored side is the artifact's left side, so a merged
    /// rule resolves into the multi-candidate plan a write must narrow.
    fn reverse_record(artifact: &str) -> ConversionRecord {
        ConversionRecord {
            resolved_path: String::new(),
            pulse_ctx_id: 0,
            dataobjectname: String::new(),
            pulse_uri: String::new(),
            map: Arc::new(ConversionMap::load(artifact).expect("fixture artifact must load")),
            root_id: 0,
            direction_to_stored: Direction::Reverse,
            stored_version: "3.39.0".parse().expect("known release"),
            hli_version: "4.1.1".parse().expect("known release"),
            parent_id: None,
        }
    }

    /// Only the one stored slot a write may change carries a transformation
    /// pointing at stored data. The others are named in the loss log and never
    /// written, so inverting them would be work whose only reachable effect is
    /// to `expect` on a transformation no write can apply — aborting the
    /// process across the C ABI instead of refusing.
    #[test]
    fn a_write_plan_inverts_its_primary_candidate_alone() {
        const ARTIFACT: &str = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="folds" rel="merged" right="folded">
                  <from left="primary" precedence="1"/>
                  <from left="secondary" precedence="2"/>
                  <fidelity forward="exact" reverse="exact"/>
                </rule>
              </rules>
              <transforms>
                <cocos from="11" to="17">
                  <flip path="folded"/>
                </cocos>
              </transforms>
            </ids-map>
        "#;
        let record = reverse_record(ARTIFACT);
        let path = CString::new("folded").expect("fixture paths contain no NUL");

        let WritePath::Candidates(candidates) = narrow_write_path(
            &record,
            path.as_ptr(),
            ArgumentRole::Field,
            resolve(&record, path.as_ptr()),
        ) else {
            panic!("a merged rule resolved from its single side is a write plan")
        };

        let direction = |precedence: u32| match &candidates
            .iter()
            .find(|candidate| candidate.precedence == precedence)
            .expect("the fixture declares both precedences")
            .value_transformation
        {
            ValueTransformation::SignFlip { direction, .. } => Some(*direction),
            ValueTransformation::None => None,
        };

        assert_eq!(
            direction(1),
            Some(TransformationDirection::ToStored),
            "the primary is the slot being written, so its flip must point at stored data"
        );
        assert_eq!(
            direction(2),
            Some(TransformationDirection::ToHli),
            "a candidate that is never written keeps the transformation the map declared"
        );
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
    /// ADR 0016 decision 9 pins the refusal order by test, not by the order
    /// of match arms. `WRITE_CHECKS` and `DELETE_CHECKS` are that order, and
    /// the tests below pin every adjacent pair in each list.
    ///
    /// Each pair is asserted in both directions the pair admits: a path both
    /// checks claim must report the *earlier* check's reason, and a path only
    /// the later check claims must still report the later one. The first
    /// assertion fails if the two are swapped; the second fails if the earlier
    /// check is widened until it swallows the later one.
    ///
    /// Four adjacent pairs cannot be asserted that way, because the two checks
    /// cannot both claim one path. Those tests say so and assert the
    /// structural reason instead of inventing a fixture that cannot exist
    /// (ADR 0011).
    ///
    /// Verified by swapping each adjacent pair in the two lists in turn and
    /// rebuilding with the test binary deleted first:
    ///
    /// | pair | write | delete |
    /// |---|---|---|
    /// | unclaimed ↔ stamp | caught | caught |
    /// | stamp ↔ shared guard | caught | caught |
    /// | shared guard ↔ non-primary | caught | caught |
    /// | non-primary ↔ no stored source | cannot co-occur | cannot co-occur |
    /// | no stored source ↔ candidate check | cannot co-occur | cannot co-occur |
    /// | timebase ↔ invertible | caught (by role) | — |
    ///
    /// A future artifact that makes one of the "cannot co-occur" rows
    /// reachable must replace that test's structural assertion with a real
    /// co-occurrence fixture — the assertion is written to fail if the reason
    /// it records stops holding.
    fn assert_write_refusal(
        record: &ConversionRecord,
        role: ArgumentRole,
        path: &str,
        expected_reason: &str,
    ) {
        let path = CString::new(path).expect("fixture paths contain no NUL");
        match narrow_write_path(record, path.as_ptr(), role, resolve(record, path.as_ptr())) {
            WritePath::Refusal { reason, dd_path } => {
                assert_eq!(reason, expected_reason);
                assert_eq!(dd_path, path.to_str().expect("fixture paths are ASCII"));
            }
            WritePath::Forward | WritePath::Translated { .. } | WritePath::Candidates(_) => {
                panic!("{path:?} must refuse before IMAS-Core")
            }
        }
    }

    fn assert_delete_refusal(record: &ConversionRecord, path: &str, expected_reason: &str) {
        let path = CString::new(path).expect("fixture paths contain no NUL");
        match narrow_delete_path(record, path.as_ptr(), resolve(record, path.as_ptr())) {
            DeletePath::Refusal { reason, dd_path } => {
                assert_eq!(reason, expected_reason);
                assert_eq!(dd_path, path.to_str().expect("fixture paths are ASCII"));
            }
            DeletePath::Forward | DeletePath::Translated(_) | DeletePath::Candidates(_) => {
                panic!("{path:?} must refuse before IMAS-Core")
            }
        }
    }

    /// No `<default>`, so any path no rule names is unclaimed.
    const NO_DEFAULT: &str = r#"
        <ids-map ids="equilibrium" format-version="1">
          <side id="left" dd="3.39.0" cocos="11"/>
          <side id="right" dd="4.1.1" cocos="17"/>
          <rules>
            <rule id="claimed" rel="renamed" left="claimed" right="stored_claimed">
              <fidelity forward="exact" reverse="exact"/>
            </rule>
          </rules>
        </ids-map>
    "#;

    /// Every refusal a claimed path can earn, under an identity default.
    const CLAIMED: &str = r#"
        <ids-map ids="equilibrium" format-version="1">
          <side id="left" dd="3.39.0" cocos="11"/>
          <side id="right" dd="4.1.1" cocos="17"/>
          <default rel="identical"/>
          <rules>
            <rule id="stamp-unservable" rel="retyped"
                  left="ids_properties/version_put/data_dictionary"
                  right="ids_properties/version_put/data_dictionary">
              <fidelity forward="unmappable" reverse="unmappable"/>
            </rule>
            <rule id="stamp-container-unservable" rel="retyped"
                  left="ids_properties" right="ids_properties">
              <fidelity forward="unmappable" reverse="unmappable"/>
            </rule>
            <rule id="unservable" rel="retyped" left="shape" right="shape">
              <fidelity forward="unmappable" reverse="unmappable"/>
            </rule>
            <rule id="no-stored-slot" rel="left_only" left="missing">
              <fidelity forward="lossy" reverse="unmappable"/>
            </rule>
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
            <rule id="flips" rel="renamed" left="flipped_hli" right="flipped">
              <fidelity forward="exact" reverse="exact"/>
            </rule>
          </rules>
          <transforms>
            <cocos from="11" to="17">
              <flip path="flipped"/>
            </cocos>
          </transforms>
        </ids-map>
    "#;

    #[test]
    fn write_order_unclaimed_precedes_the_stamp_check() {
        let unclaimed = record(NO_DEFAULT, "");
        // The stamp's own spelling, claimed by nothing: both checks apply.
        assert_write_refusal(
            &unclaimed,
            ArgumentRole::Field,
            "ids_properties/version_put/data_dictionary",
            "this path is unclaimed by the conversion map",
        );
        // Claimed, so only the stamp check applies.
        assert_write_refusal(
            &record(CLAIMED, ""),
            ArgumentRole::Field,
            "ids_properties/version_put/data_dictionary",
            "the DD-version stamp is immutable under a version mismatch",
        );
    }

    #[test]
    fn write_order_the_stamp_check_precedes_the_shared_guard() {
        let record = record(CLAIMED, "");
        // The stamp path carries an unservable rule: both checks apply.
        assert_write_refusal(
            &record,
            ArgumentRole::Field,
            "ids_properties/version_put/data_dictionary",
            "the DD-version stamp is immutable under a version mismatch",
        );
        // The same unservable rule shape on an ordinary path.
        assert_write_refusal(
            &record,
            ArgumentRole::Field,
            "shape",
            "this path's container changed shape and cannot be served",
        );
    }

    #[test]
    fn write_order_the_shared_guard_precedes_the_non_primary_check() {
        let record = record(CLAIMED, "");
        // `secondary` is a precedence-2 source *and* its rule is unmappable
        // forward, so both claim it. A rule that cannot be served at all must
        // not appear to be merely a collision risk.
        assert_write_refusal(
            &record,
            ArgumentRole::Field,
            "secondary",
            "this path has no safe conversion between DD versions",
        );
        // Servable, so only the collision guard claims it.
        assert_write_refusal(
            &record,
            ArgumentRole::Field,
            "other_secondary",
            "this path is a non-primary source and cannot write a shared stored slot",
        );
    }

    #[test]
    fn write_order_the_non_primary_check_precedes_the_no_stored_source_check() {
        let record = record(CLAIMED, "");
        // The two cannot both claim a path: a precedence is only ever declared
        // by a `<from>` inside a merged or split rule, and resolving such a
        // source always names the folded counterpart — never no source at all.
        // So each is asserted alone, and the assertion that they cannot
        // co-occur is the resolver's own: no `NoSource` carries a precedence.
        let path = CString::new("missing").expect("fixture paths contain no NUL");
        assert!(matches!(
            resolve(&record, path.as_ptr()),
            Resolved::NoSource {
                requested_precedence: None,
                ..
            }
        ));
        assert_write_refusal(
            &record,
            ArgumentRole::Field,
            "other_secondary",
            "this path is a non-primary source and cannot write a shared stored slot",
        );
        assert_write_refusal(
            &record,
            ArgumentRole::Field,
            "missing",
            "this path has no stored source",
        );
    }

    #[test]
    fn write_order_the_no_stored_source_check_precedes_the_timebase_check() {
        let record = record(CLAIMED, "");
        // These cannot both claim a path either: the timebase check reads a
        // candidate's value transformation, and a resolution with no stored
        // source has no candidate to read.
        let path = CString::new("missing").expect("fixture paths contain no NUL");
        let resolved = resolve(&record, path.as_ptr());
        let caller = caller_dd_path(&record, path.as_ptr());
        assert!(
            write_subject(&resolved, &caller, None)
                .expect("a claimed path presents a subject")
                .candidate()
                .is_none()
        );
        assert_write_refusal(
            &record,
            ArgumentRole::Timebase,
            "missing",
            "this path has no stored source",
        );
        assert_write_refusal(
            &record,
            ArgumentRole::Timebase,
            "flipped_hli",
            "this timebase needs a value transformation, which al_write_data cannot apply",
        );
    }

    #[test]
    fn write_order_the_timebase_and_field_checks_never_serve_one_argument() {
        // The last adjacent pair cannot be observed in one call at all: one
        // entry serves `timebase` and the other serves `field`, and a single
        // argument is exactly one of the two. Their relative position is
        // therefore unobservable, and this asserts the reason rather than
        // pinning an order no caller can reach.
        for role in [ArgumentRole::Field, ArgumentRole::Timebase] {
            let served: Vec<_> = WRITE_CHECKS
                .iter()
                .filter(|(tag, _)| tag.serves(role))
                .filter(|(_, check)| {
                    matches!(
                        check,
                        WriteCheck::TimebaseTransformation | WriteCheck::InvertibleTransformation
                    )
                })
                .collect();
            assert_eq!(served.len(), 1, "one role must never serve both checks");
        }
        // The field-only entry is the one a `field` argument reaches. It is
        // unreachable from any artifact today — the only non-invertible
        // transformation is a sign flip between identical conventions, which
        // the loader normalises to `None` — so its position is pinned by the
        // list and its behaviour by the seam policy's own inversion tests.
        assert_eq!(
            WRITE_CHECKS.last().map(|(_, check)| *check),
            Some(WriteCheck::InvertibleTransformation)
        );
    }

    #[test]
    fn delete_order_unclaimed_precedes_the_stamp_check() {
        assert_delete_refusal(
            &record(NO_DEFAULT, ""),
            "ids_properties",
            "this path is unclaimed by the conversion map",
        );
        assert_delete_refusal(
            &record(CLAIMED, ""),
            "ids_properties",
            "this delete would remove the DD-version stamp while stored data remains",
        );
    }

    #[test]
    fn delete_order_the_stamp_check_precedes_the_shared_guard() {
        let record = record(CLAIMED, "");
        // `ids_properties` carries an unservable rule, so both claim it.
        assert_delete_refusal(
            &record,
            "ids_properties",
            "this delete would remove the DD-version stamp while stored data remains",
        );
        assert_delete_refusal(
            &record,
            "shape",
            "this path's container changed shape and cannot be served",
        );
    }

    #[test]
    fn delete_order_the_shared_guard_precedes_the_non_primary_check() {
        let record = record(CLAIMED, "");
        assert_delete_refusal(
            &record,
            "secondary",
            "this path has no safe conversion between DD versions",
        );
        assert_delete_refusal(
            &record,
            "other_secondary",
            "this path is a non-primary source and cannot delete a shared stored slot",
        );
    }

    #[test]
    fn delete_order_the_non_primary_check_precedes_the_no_stored_source_check() {
        // Cannot co-occur, for the reason the write seam's equivalent records.
        let record = record(CLAIMED, "");
        assert_delete_refusal(
            &record,
            "other_secondary",
            "this path is a non-primary source and cannot delete a shared stored slot",
        );
        assert_delete_refusal(&record, "missing", "this path has no stored source");
    }

    #[test]
    fn delete_order_the_no_stored_source_check_precedes_the_escaping_subtree_check() {
        // Cannot co-occur either: the subtree check reads a candidate's stored
        // spelling, and a resolution with no stored source has no candidate.
        let record = record(CLAIMED, "");
        let path = CString::new("missing").expect("fixture paths contain no NUL");
        let resolved = resolve(&record, path.as_ptr());
        let caller = caller_dd_path(&record, path.as_ptr());
        assert!(
            delete_subjects(&resolved, &caller)
                .iter()
                .all(|subject| subject.candidate().is_none())
        );
        assert_delete_refusal(&record, "missing", "this path has no stored source");
        // The escaping-subtree check firing on its own is pinned by
        // `delete_narrowing_admits_a_trivial_structure_and_refuses_an_escaping_one`.
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
