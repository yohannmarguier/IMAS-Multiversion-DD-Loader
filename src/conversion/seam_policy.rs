//! The `al_read_data`/`al_plugin_read_data` read loop, the
//! `al_write_data`/`al_plugin_write_data` write decision, and the
//! `al_delete_data` delete loop (see ADR 0015).
//!
//! The three are deliberately not equal in weight. [`run_read`] owns the
//! candidate loop and its fidelity bookkeeping; [`run_write`] owns the
//! sentinel skip, shape validation, the shim-owned
//! copy and the unwritten-candidate list. [`run_delete`] owns candidate
//! iteration, continuation after a failure, and retention of the first
//! failure. The adapter injects the one Core-facing operation each loop
//! calls — the delete loop took two until ADR 0017 decision 2 retired its
//! presence probe — and formats that retained failure after the loop returns.
//!
//! Before this module existed, `read_data_impl` (`src/interpose.rs`) mixed
//! raw-pointer marshalling with the read-loop decisions ADR 0010, ADR 0012
//! and ADR 0014 make: which candidate to try next, whether a value
//! transformation applies, and what fidelity a caller's field/timebase
//! argument reached. Both of this loop's historical defects — issue #65 (a
//! short-circuit skipped the loss log for a single-candidate lossy read) and
//! issue #66 (a retained path dropped an arraystruct anchor prefix) — were
//! loop bookkeeping, not path resolution, and neither is reachable from
//! `cargo test` while the loop lives beside `unsafe` marshalling code.
//!
//! This module owns those decisions and nothing else: the read loop takes an
//! already-resolved [`path_conversion::ReadPath`] per argument, a buffer's
//! shape, and a reader closure; the write decision takes one
//! [`path_conversion::WritePath`] per argument, and the delete loop one
//! [`path_conversion::DeletePath`] plus an injected delete operation.
//! Their verdicts tell the
//! adapter what to forward or refuse. This module contains no `unsafe`, never
//! touches [`crate::registry::context_registry::REGISTRY`] or the HLI version
//! latch, and never calls into [`crate::core::dl`] — every raw pointer, every
//! registry lookup, and the ADR-0014/HLI-version gates ahead of them stay in
//! `src/interpose.rs`, the interposition layer ADR 0015 names.
//!
//! [`ReadVerdict`]'s `field`/`timebase` fidelities are mandatory struct
//! fields rather than a separately-returned loss list: every branch of
//! [`run_read`] must populate both or the crate does not compile, which is
//! what makes issue #65's defect (a return path that forgot to retain a
//! fidelity) a compile error instead of a silent gap, and collapses issue
//! #66's nine independent join call sites into the one place — the adapter's
//! single mechanical write after `run_read` returns — that ever writes to
//! the loss log.

use std::ffi::{CStr, c_int};

use crate::al_status_t;
use crate::conversion::conversion_map::{
    ConversionMap, Direction, Fidelity, TransformationDirection, ValueTransformation,
};
use crate::conversion::known_artifacts::{self, ArtifactMatch};
use crate::conversion::path_conversion::{
    self, DeletePath, ReadPath, TranslatedReadPath, WritePath,
};
use crate::conversion::read_outcome::EMPTY_DOUBLE;
use crate::version::dd_version::DdVersion;
use crate::version::version_stamp::StampOutcome;

/// The occurrence-cache write discovery asks its interposition adapter to
/// perform. This preserves the cached mismatch necessary for a later global
/// action's `datapath` without letting the adapter reconstruct a policy choice.
pub(crate) enum OccurrenceCacheEffect {
    Forget,
    RememberMismatch(DdVersion),
}

/// The effect discovery asks its interposition adapter to perform after an
/// occurrence-opening seam has succeeded. Policy drives the stamp read and
/// decides which ADR-0007/0009/0011 branch applies; it never touches the
/// registry or chooses an ABI end-action symbol itself.
pub(crate) enum DiscoveryDecision {
    /// The stored DD version differs from the HLI's and an embedded artifact
    /// can serve the IDS/version pair. The adapter records both the known
    /// mismatch and the root conversion context.
    RegisterRoot {
        stored: DdVersion,
        artifact: ArtifactMatch,
        occurrence_cache: OccurrenceCacheEffect,
    },
    /// No root conversion context is warranted. A mismatching `stored` value
    /// without an artifact is still returned so the adapter can preserve the
    /// occurrence cache it uses for a later global-action `datapath`.
    RegisterNothing {
        occurrence_cache: OccurrenceCacheEffect,
    },
    /// A present but malformed stamp refuses the successful open; the adapter
    /// clears any stale occurrence cache and ends that context through the
    /// same ABI family that opened it.
    RefuseAndEnd {
        reason: Box<al_status_t>,
        occurrence_cache: OccurrenceCacheEffect,
    },
}

/// Drives stored-DD-version discovery for one successfully opened IDS
/// occurrence. The reader is injected by the interposition adapter, just as
/// the read loop receives its Core reader: policy chooses when it runs and
/// returns the effect, while the adapter owns raw pointers, Core calls and
/// process-global state.
pub(crate) fn decide_occurrence_registration(
    ids_name: &str,
    hli: &DdVersion,
    read_stamp: impl FnOnce() -> StampOutcome,
) -> DiscoveryDecision {
    match read_stamp() {
        StampOutcome::Malformed(reason) => DiscoveryDecision::RefuseAndEnd {
            reason,
            occurrence_cache: OccurrenceCacheEffect::Forget,
        },
        StampOutcome::Unstamped => DiscoveryDecision::RegisterNothing {
            occurrence_cache: OccurrenceCacheEffect::Forget,
        },
        StampOutcome::Stored(stored) if stored == *hli => DiscoveryDecision::RegisterNothing {
            occurrence_cache: OccurrenceCacheEffect::Forget,
        },
        StampOutcome::Stored(stored) => match known_artifacts::lookup(ids_name, &stored, hli) {
            Some(artifact) => DiscoveryDecision::RegisterRoot {
                occurrence_cache: OccurrenceCacheEffect::RememberMismatch(stored.clone()),
                stored,
                artifact,
            },
            None => DiscoveryDecision::RegisterNothing {
                occurrence_cache: OccurrenceCacheEffect::RememberMismatch(stored),
            },
        },
    }
}

/// Decides whether a cached mismatching occurrence's global-action
/// `datapath` has one concrete stored-DD spelling. This is a sibling of
/// [`decide_occurrence_registration`], not part of it: this decision happens
/// before forwarding from a cached mismatch, while discovery decides after
/// forwarding from a freshly read stamp, so they have different inputs and
/// timing despite sharing the occurrence-opening seam.
pub(crate) fn decide_datapath_translation(
    map: &ConversionMap,
    direction: Direction,
    path: &str,
) -> Option<String> {
    let explanation = map.resolve(path, direction)?;
    path_conversion::datapath_translation(explanation.outcome)
}

/// The datatype half of a buffer's shape, mapped by the adapter from
/// IMAS-Core's raw `datatype` argument before policy ever sees it. A value
/// transformation only ever applies to `DOUBLE_DATA` today (ADR 0010); every
/// other IMAS-Core datatype collapses to `Other`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BufferDataType {
    Double,
    Other,
}

/// A data buffer's element type and rank, known before any IMAS-Core call.
/// ADR 0010 requires a value-transformation refusal to happen before
/// `forward()` runs, so this — not a typed view onto the buffer, which can
/// only exist after IMAS-Core has written it — is everything policy sees
/// ahead of a [`Attempt::Data`] outcome.
pub(crate) struct BufferShape {
    pub(crate) datatype: BufferDataType,
    pub(crate) rank: c_int,
}

/// A safe, typed view onto a data buffer IMAS-Core has already written for
/// one attempt, built by the adapter only once that attempt's outcome is
/// [`Attempt::Data`] — so the ADR-0010 refusal above can never depend on it.
pub(crate) enum DataView<'a> {
    /// The datatype was `DOUBLE_DATA` and the returned shape was captured
    /// safely: `values` is exactly as long as the returned extents multiply
    /// out to.
    Double(&'a mut [f64]),
    /// The datatype was `DOUBLE_DATA` but the returned shape could not be
    /// captured safely — no extents were available for a nonscalar read, or
    /// the extent product overflowed what this platform can represent.
    InvalidShape(&'static str),
    /// The datatype was something other than `DOUBLE_DATA`.
    NotDouble,
}

/// A safe, read-only view of a caller-owned write buffer. Unlike [`DataView`]
/// this source is never changed: a write-side transformation allocates its
/// own vector and returns it in [`WriteVerdict`] (ADR 0018).
pub(crate) enum SourceView<'a> {
    Double(&'a [f64]),
    /// IMAS-Core's rank-zero empty sentinel, in any scalar datatype whose
    /// shape gate understands one. It forwards unchanged before the declared
    /// transformation's datatype gate (ADR 0018).
    UnsetScalar,
    InvalidShape(&'static str),
    NotDouble,
}

/// What one candidate attempt at IMAS-Core reported, already classified by
/// the adapter through [`crate::read_outcome::classify`] before policy ever
/// sees it: policy decides what to do next, never how to read the status or
/// data pointer that produced this.
pub(crate) enum Attempt<'a> {
    Failure(al_status_t),
    NotFound,
    Data(al_status_t, DataView<'a>),
}

/// One path-bearing read argument (`field` or `timebase`), reduced to what
/// the read loop needs: how the conversion map resolved it, the original
/// caller argument to forward when a candidate calls for exactly that, and
/// the DD path this argument is known by — already anchor-joined by the
/// adapter (the caller's raw argument onto the live context's own resolved
/// path), for refusal messages and the eventual loss-log entry alike.
pub(crate) struct ReadArgument<'a> {
    pub(crate) resolution: ReadPath,
    pub(crate) forward: Option<&'a CStr>,
    pub(crate) dd_path: String,
}

/// One path-bearing write argument reduced to the decision the write seam
/// needs: either the one safe stored spelling or a refusal, plus the original
/// C string for the ordinary forward case.
pub(crate) struct WriteArgument<'a> {
    pub(crate) resolution: WritePath,
    pub(crate) forward: Option<&'a CStr>,
    pub(crate) dd_path: String,
}

/// One delete path reduced to the decision the delete seam needs: either one
/// safe stored spelling or a refusal, plus the original C string for an empty
/// whole-DATAOBJECT delete.
pub(crate) struct DeleteArgument<'a> {
    pub(crate) resolution: DeletePath,
    pub(crate) forward: Option<&'a CStr>,
}

/// The write policy's complete answer. The adapter alone turns a successful
/// decision into an IMAS-Core call, so this layer never touches raw pointers
/// or process-global state.
pub(crate) enum WriteVerdict<'a> {
    Forward {
        field: Option<&'a CStr>,
        timebase: Option<&'a CStr>,
        /// `Some` is a transformed shim-owned copy, borrowed by the adapter
        /// for exactly one IMAS-Core call. `None` forwards caller storage.
        data: Option<Vec<f64>>,
        /// Every stored candidate that deliberately remains unwritten, named
        /// by its own complete stored-DD spelling rather than by the caller's
        /// path — the point of the entry is to say where a stale value may
        /// now be found. These are retained as potentially lossy only after
        /// IMAS-Core accepts the one precedence-1 write.
        unwritten_candidates: Vec<&'a str>,
    },
    Refusal {
        reason: String,
        dd_path: String,
    },
}

/// Resolves `field` and `timebase` independently, refusing the entire write
/// when either one cannot name one safe stored-DD path.
pub(crate) fn run_write<'a>(
    field: &'a WriteArgument<'a>,
    timebase: &'a WriteArgument<'a>,
    shape: BufferShape,
    source: SourceView<'a>,
) -> WriteVerdict<'a> {
    let field = match write_argument_path(field) {
        Ok(path) => path,
        Err(refusal) => return write_refusal(refusal),
    };
    let timebase = match write_argument_path(timebase) {
        Ok(path) => path,
        Err(refusal) => return write_refusal(refusal),
    };
    if matches!(source, SourceView::UnsetScalar) {
        // ADR 0018: IMAS-Core will skip this unset scalar, so it neither
        // writes the primary candidate nor discards a value from the others.
        return WriteVerdict::Forward {
            field: field.path,
            timebase: timebase.path,
            data: None,
            unwritten_candidates: Vec::new(),
        };
    }
    let transformation = field.value_transformation.clone();
    if let Err(reason) = validate_value_transformation(&transformation, &shape) {
        return WriteVerdict::Refusal {
            reason: reason.to_string(),
            dd_path: field.dd_path.to_string(),
        };
    }
    let data = match copy_value_transformation(&transformation, source) {
        Ok(data) => data,
        Err(reason) => {
            return WriteVerdict::Refusal {
                reason: reason.to_string(),
                dd_path: field.dd_path.to_string(),
            };
        }
    };
    WriteVerdict::Forward {
        field: field.path,
        timebase: timebase.path,
        data,
        unwritten_candidates: unwritten_candidate_paths(&field, &timebase),
    }
}

fn write_refusal<'a>((reason, dd_path): (&str, &str)) -> WriteVerdict<'a> {
    WriteVerdict::Refusal {
        reason: reason.to_string(),
        dd_path: dd_path.to_string(),
    }
}

struct ResolvedWriteArgument<'a> {
    path: Option<&'a CStr>,
    value_transformation: ValueTransformation,
    dd_path: &'a str,
    /// The complete stored-DD spelling of every candidate this write
    /// deliberately leaves alone — never the caller's own `dd_path`, because
    /// the stale value a reader of the stored occurrence may find lives under
    /// one of *these* names (ADR 0016 decision 4).
    unwritten_candidates: Vec<&'a str>,
}

fn unwritten_candidate_paths<'a>(
    field: &ResolvedWriteArgument<'a>,
    timebase: &ResolvedWriteArgument<'a>,
) -> Vec<&'a str> {
    let mut paths =
        Vec::with_capacity(field.unwritten_candidates.len() + timebase.unwritten_candidates.len());
    paths.extend(field.unwritten_candidates.iter().copied());
    paths.extend(timebase.unwritten_candidates.iter().copied());
    paths
}

fn write_argument_path<'a>(
    argument: &'a WriteArgument<'a>,
) -> Result<ResolvedWriteArgument<'a>, (&'a str, &'a str)> {
    match &argument.resolution {
        WritePath::Forward => Ok(ResolvedWriteArgument {
            path: argument.forward,
            value_transformation: ValueTransformation::None,
            dd_path: &argument.dd_path,
            unwritten_candidates: Vec::new(),
        }),
        WritePath::Translated {
            path,
            value_transformation,
        } => Ok(ResolvedWriteArgument {
            path: Some(path.as_c_str()),
            value_transformation: value_transformation.clone(),
            dd_path: &argument.dd_path,
            unwritten_candidates: Vec::new(),
        }),
        WritePath::Candidates(candidates) => {
            let Some(primary) = candidates
                .iter()
                .position(|candidate| candidate.precedence == 1)
            else {
                return Err((
                    "this candidate plan has no precedence-1 source for a write",
                    &argument.dd_path,
                ));
            };
            Ok(ResolvedWriteArgument {
                path: Some(candidates[primary].path.as_c_str()),
                value_transformation: candidates[primary].value_transformation.clone(),
                dd_path: &argument.dd_path,
                // Every candidate but the one being written, named by its own
                // stored spelling. Indexing past the primary rather than
                // filtering on `precedence != 1` keeps this exactly the
                // complement of the slot chosen above.
                unwritten_candidates: candidates
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != primary)
                    .map(|(_, candidate)| candidate.stored_dd_path.as_str())
                    .collect(),
            })
        }
        WritePath::Refusal {
            reason, dd_path, ..
        } => Err((reason, dd_path)),
    }
}

/// Applies a write-side transformation to a copy the policy owns. Rank-zero
/// Scalar sentinels are returned before this function runs. A sentinel inside
/// an array remains a value and therefore is transformed with its neighbours,
/// matching the scope of IMAS-Core's own shape gate (ADR 0018).
///
/// This is the one place that reads [`TransformationDirection`]. The resolver
/// inverts the map's read-direction transformation before it reaches this
/// policy, and this remains its final assertion that only `ToStored` data can
/// be copied. A future resolver that forgets that inversion therefore fails
/// loudly instead of storing values with the sign the caller supplied.
fn copy_value_transformation(
    transformation: &ValueTransformation,
    source: SourceView<'_>,
) -> Result<Option<Vec<f64>>, &'static str> {
    match transformation {
        ValueTransformation::None => Ok(None),
        ValueTransformation::SignFlip { direction, .. }
            if *direction != TransformationDirection::ToStored =>
        {
            Err("this value transformation was not inverted for the write direction")
        }
        ValueTransformation::SignFlip { .. } => match source {
            SourceView::Double(values) => Ok(Some(values.iter().map(|value| -*value).collect())),
            SourceView::UnsetScalar => {
                debug_assert!(
                    false,
                    "scalar sentinels must return before transformation validation"
                );
                Ok(None)
            }
            SourceView::InvalidShape(reason) => Err(reason),
            SourceView::NotDouble => Err(
                "value-transform execution requires DOUBLE_DATA and a rank no greater than MAXDIM",
            ),
        },
    }
}

/// The first candidate failure retained while later candidates are still
/// attempted. The adapter owns the eventual ABI-message formatting.
///
/// A candidate has one operation now, so this names no operation. It used to
/// carry a `DeleteFailureOperation` because a candidate was probed and then
/// deleted, and a refusal had to say which of the two failed; ADR 0017
/// decision 2 retired the probe and that distinction with it.
pub(crate) struct DeleteFailure<'a> {
    pub(crate) status: al_status_t,
    pub(crate) path: &'a CStr,
}

/// The delete policy's complete answer. The injected closures alone perform
/// IMAS-Core calls, so this layer remains free of raw pointers and state.
// As with `SeamOutcome`, carrying IMAS-Core's fixed-size status by value keeps
// the verdict consistent with every other status path in this crate.
#[allow(clippy::large_enum_variant)]
pub(crate) enum DeleteVerdict<'a> {
    Forward { path: Option<&'a CStr> },
    Complete { failure: Option<DeleteFailure<'a>> },
    Refusal { reason: String, dd_path: String },
}

/// Runs one delete decision. An empty path deliberately forwards: it is
/// IMAS-Core's explicit whole-DATAOBJECT delete, the only legitimate route to
/// discard a mismatched occurrence before recreating it in the HLI DD. A
/// candidate plan deletes every path. Failures do not stop later candidates,
/// because no rollback exists and continuing leaves the least stale data
/// behind.
pub(crate) fn run_delete<'a>(
    argument: &'a DeleteArgument<'a>,
    mut delete: impl FnMut(&CStr) -> al_status_t,
) -> DeleteVerdict<'a> {
    match &argument.resolution {
        DeletePath::Forward => DeleteVerdict::Forward {
            path: argument.forward,
        },
        DeletePath::Translated(path) => DeleteVerdict::Forward {
            path: Some(path.as_c_str()),
        },
        DeletePath::Candidates(paths) => {
            let mut first_failure = None;
            // ADR 0017 decision 2: a candidate plan asserts absence only after
            // every possible stored source has been asked to delete itself, so
            // this visits all of them and never stops at the first.
            for path in paths {
                let status = delete(path);
                if status.code != 0 {
                    // ADR 0017 decision 3: later candidates still run, and the
                    // caller sees the first failure once they have completed.
                    first_failure.get_or_insert(DeleteFailure { status, path });
                }
            }
            DeleteVerdict::Complete {
                failure: first_failure,
            }
        }
        DeletePath::Refusal { reason, dd_path } => DeleteVerdict::Refusal {
            reason: reason.to_string(),
            dd_path: dd_path.to_string(),
        },
    }
}

/// One argument's read-loop fidelity verdict: the fidelity the loop actually
/// reached, alongside the DD path it would be retained against if that
/// fidelity is not [`Fidelity::Exact`]. `path` is always present — see this
/// module's doc comment for why that is load-bearing.
pub(crate) struct ArgumentFidelity {
    pub(crate) path: String,
    pub(crate) fidelity: Fidelity,
}

/// The field pair accepted by [`verdict`]. It is deliberately distinct from
/// [`TimebaseFidelity`] so field and timebase cannot be transposed at a call
/// site while still compiling.
struct FieldFidelity<'a> {
    path: &'a str,
    fidelity: Fidelity,
}

/// The timebase pair accepted by [`verdict`].
struct TimebaseFidelity<'a> {
    path: &'a str,
    fidelity: Fidelity,
}

/// What [`run_read`] decided to report back to the HLI, short of turning it
/// into an `al_status_t` — that formatting step needs the live
/// [`crate::registry::context_registry::ConversionRecord`]'s DD versions, which this
/// module never touches.
// `al_status_t` is a 260-byte fixed-size ABI struct (`MAX_ERR_MSG_LEN`) that
// the crate already copies by value throughout rather than boxing (see
// `core_binding.rs`'s matching `#[allow(clippy::result_large_err)]`); doing
// the same here keeps this type consistent with how a status is carried
// everywhere else.
#[allow(clippy::large_enum_variant)]
pub(crate) enum SeamOutcome {
    /// Forward this status to the caller exactly as received — a successful
    /// read (any value transformation has already been applied in place) or
    /// an IMAS-Core failure the loop stopped trying further candidates for.
    Data(al_status_t),
    /// No candidate returned data: the normal success-with-null result,
    /// without a further IMAS-Core call.
    NotFound,
    /// A shim-owned refusal — a declared `unmappable` rule, or a value
    /// transformation this buffer's shape cannot carry out.
    Refusal { reason: String, dd_path: String },
}

/// The read loop's complete answer for one `al_read_data`/`al_plugin_read_data`
/// call: what to report, and at what fidelity each of the two arguments was
/// actually served.
pub(crate) struct ReadVerdict {
    pub(crate) outcome: SeamOutcome,
    pub(crate) field: ArgumentFidelity,
    pub(crate) timebase: ArgumentFidelity,
}

/// One candidate this loop can try: a stored-DD path to forward (or `None`
/// to forward the original caller argument unchanged), the fidelity retained
/// if this is the candidate that ends up serving the read, and the value
/// transformation to apply to a successful result.
struct ReadAttempt<'a> {
    path: Option<&'a CStr>,
    fidelity: Fidelity,
    value_transformation: ValueTransformation,
}

impl<'a> ReadAttempt<'a> {
    /// The one attempt a `Forward`-resolved argument ever tries: the
    /// caller's own argument, unmodified, at `Fidelity::Exact` with no value
    /// transformation.
    fn forward(original: Option<&'a CStr>) -> Self {
        Self {
            path: original,
            fidelity: Fidelity::Exact,
            value_transformation: ValueTransformation::None,
        }
    }
}

impl TranslatedReadPath {
    /// Turns each resolved candidate into a forwarding attempt. `pub(crate)`
    /// on [`TranslatedReadPath`]'s own fields is what lets this live here
    /// rather than in `path_conversion.rs`: that module constructs and
    /// orders candidates but never turns them into something IMAS-Core can
    /// be called with, which is this loop's job alone.
    fn attempts(&self) -> Vec<ReadAttempt<'_>> {
        self.paths
            .iter()
            .map(|path| ReadAttempt {
                path: Some(path.path.as_c_str()),
                fidelity: path.fidelity,
                value_transformation: path.value_transformation.clone(),
            })
            .collect()
    }
}

/// ADR 0010's gate: a value transformation must refuse before `forward()` is
/// ever called, using only the buffer's declared shape — never a value it
/// has not been handed yet.
fn validate_value_transformation(
    transformation: &ValueTransformation,
    shape: &BufferShape,
) -> Result<(), &'static str> {
    match transformation {
        ValueTransformation::None => Ok(()),
        ValueTransformation::SignFlip { .. }
            if shape.datatype == BufferDataType::Double
                && (0..=crate::MAXDIM as c_int).contains(&shape.rank) =>
        {
            Ok(())
        }
        ValueTransformation::SignFlip { .. } => {
            Err("value-transform execution requires DOUBLE_DATA and a rank no greater than MAXDIM")
        }
    }
}

/// Executes a value transformation on a buffer IMAS-Core has already
/// written. A [`DataView`] that could not be captured safely refuses here
/// rather than at [`validate_value_transformation`] time, because whether the
/// buffer's actual returned shape is usable is only known after IMAS-Core has
/// returned it.
fn apply_value_transformation(
    transformation: &ValueTransformation,
    view: &mut DataView,
) -> Result<(), &'static str> {
    match transformation {
        ValueTransformation::None => Ok(()),
        ValueTransformation::SignFlip { .. } => match view {
            DataView::Double(values) => {
                for value in values.iter_mut() {
                    if *value != EMPTY_DOUBLE {
                        *value = -*value;
                    }
                }
                Ok(())
            }
            DataView::InvalidShape(reason) => Err(reason),
            DataView::NotDouble => Err(
                "value-transform execution requires DOUBLE_DATA and a rank no greater than MAXDIM",
            ),
        },
    }
}

/// The read loop shared by `al_read_data` and `al_plugin_read_data` (issue
/// #68), extracted whole from what was `read_data_impl` (issue #107).
///
/// `field` and `timebase` are resolved independently: an early
/// [`ReadPath::Refusal`] or [`ReadPath::NoSource`] on either one ends the
/// call immediately, before the other argument's candidates — if it has
/// any — are ever tried. When that early return is driven by one argument,
/// the other is reported at `Fidelity::Exact` regardless of its own
/// resolution: this loop never evaluates it far enough to know otherwise,
/// matching the pre-#107 behaviour exactly (only the argument that actually
/// triggered the return was ever retained).
///
/// Once both arguments clear that gate, every combination of a field
/// candidate and a timebase candidate is tried in declared precedence order
/// until one field candidate returns data — a single `Translated` path and a
/// `Forward` argument both present as a one-candidate list, so there is only
/// one loop, not a special case for the non-candidate path (issue #65).
/// [`validate_value_transformation`] runs once for each field candidate before
/// `reader` is ever called for any of its timebase candidates (ADR 0010); a [`Attempt::Data`] result runs
/// [`apply_value_transformation`] in place before this returns.
pub(crate) fn run_read<'a>(
    field: ReadArgument<'a>,
    timebase: ReadArgument<'a>,
    shape: BufferShape,
    mut reader: impl FnMut(Option<&CStr>, Option<&CStr>) -> Attempt<'a>,
) -> ReadVerdict {
    let field_forward = field.forward;
    let field_dd_path = field.dd_path;
    let timebase_forward = timebase.forward;
    let timebase_dd_path = timebase.dd_path;

    let field_translated = match field.resolution {
        ReadPath::Forward => None,
        ReadPath::Translated(path) => Some(path),
        ReadPath::Refusal {
            reason,
            dd_path,
            fidelity,
        } => {
            return verdict(
                SeamOutcome::Refusal { reason, dd_path },
                FieldFidelity {
                    path: &field_dd_path,
                    fidelity,
                },
                TimebaseFidelity {
                    path: &timebase_dd_path,
                    fidelity: Fidelity::Exact,
                },
            );
        }
        ReadPath::NoSource(fidelity) => {
            return verdict(
                SeamOutcome::NotFound,
                FieldFidelity {
                    path: &field_dd_path,
                    fidelity,
                },
                TimebaseFidelity {
                    path: &timebase_dd_path,
                    fidelity: Fidelity::Exact,
                },
            );
        }
    };

    let timebase_translated = match timebase.resolution {
        ReadPath::Forward => None,
        ReadPath::Translated(path) => Some(path),
        ReadPath::Refusal {
            reason,
            dd_path,
            fidelity,
        } => {
            return verdict(
                SeamOutcome::Refusal { reason, dd_path },
                FieldFidelity {
                    path: &field_dd_path,
                    fidelity: Fidelity::Exact,
                },
                TimebaseFidelity {
                    path: &timebase_dd_path,
                    fidelity,
                },
            );
        }
        ReadPath::NoSource(fidelity) => {
            return verdict(
                SeamOutcome::NotFound,
                FieldFidelity {
                    path: &field_dd_path,
                    fidelity: Fidelity::Exact,
                },
                TimebaseFidelity {
                    path: &timebase_dd_path,
                    fidelity,
                },
            );
        }
    };

    let field_attempts = field_translated.as_ref().map_or_else(
        || vec![ReadAttempt::forward(field_forward)],
        TranslatedReadPath::attempts,
    );
    let timebase_attempts = timebase_translated.as_ref().map_or_else(
        || vec![ReadAttempt::forward(timebase_forward)],
        TranslatedReadPath::attempts,
    );

    for field_attempt in &field_attempts {
        if let Err(reason) =
            validate_value_transformation(&field_attempt.value_transformation, &shape)
        {
            return verdict(
                SeamOutcome::Refusal {
                    reason: reason.to_string(),
                    dd_path: field_dd_path.clone(),
                },
                FieldFidelity {
                    path: &field_dd_path,
                    fidelity: Fidelity::Unmappable,
                },
                TimebaseFidelity {
                    path: &timebase_dd_path,
                    fidelity: timebase_attempts[0].fidelity,
                },
            );
        }
        for timebase_attempt in &timebase_attempts {
            match reader(field_attempt.path, timebase_attempt.path) {
                Attempt::Failure(status) => {
                    return verdict(
                        SeamOutcome::Data(status),
                        FieldFidelity {
                            path: &field_dd_path,
                            fidelity: field_attempt.fidelity,
                        },
                        TimebaseFidelity {
                            path: &timebase_dd_path,
                            fidelity: timebase_attempt.fidelity,
                        },
                    );
                }
                Attempt::Data(status, mut view) => {
                    if let Err(reason) =
                        apply_value_transformation(&field_attempt.value_transformation, &mut view)
                    {
                        return verdict(
                            SeamOutcome::Refusal {
                                reason: reason.to_string(),
                                dd_path: field_dd_path.clone(),
                            },
                            FieldFidelity {
                                path: &field_dd_path,
                                fidelity: Fidelity::Unmappable,
                            },
                            TimebaseFidelity {
                                path: &timebase_dd_path,
                                fidelity: timebase_attempt.fidelity,
                            },
                        );
                    }
                    return verdict(
                        SeamOutcome::Data(status),
                        FieldFidelity {
                            path: &field_dd_path,
                            fidelity: field_attempt.fidelity,
                        },
                        TimebaseFidelity {
                            path: &timebase_dd_path,
                            fidelity: timebase_attempt.fidelity,
                        },
                    );
                }
                Attempt::NotFound => {}
            }
        }
    }

    verdict(
        SeamOutcome::NotFound,
        FieldFidelity {
            path: &field_dd_path,
            fidelity: path_conversion::translated_read_fidelity(field_translated.as_ref()),
        },
        TimebaseFidelity {
            path: &timebase_dd_path,
            fidelity: path_conversion::translated_read_fidelity(timebase_translated.as_ref()),
        },
    )
}

/// Assembles one [`ReadVerdict`], the one place that pairs an outcome with
/// both arguments' fidelities — collapsing what would otherwise be the same
/// nested-struct literal at every one of `run_read`'s return points.
fn verdict(
    outcome: SeamOutcome,
    field: FieldFidelity<'_>,
    timebase: TimebaseFidelity<'_>,
) -> ReadVerdict {
    ReadVerdict {
        outcome,
        field: ArgumentFidelity {
            path: field.path.to_string(),
            fidelity: field.fidelity,
        },
        timebase: ArgumentFidelity {
            path: timebase.path.to_string(),
            fidelity: timebase.fidelity,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::conversion_map::{ConversionMap, Direction, Outcome};
    use crate::version::dd_version::DdVersion;
    use crate::version::version_stamp::StampOutcome;
    use path_conversion::ResolvedReadPath;
    use std::cell::RefCell;
    use std::ffi::CString;

    /// Review finding S-J1 / ADR 0016 decision 7: the write-side executor
    /// reads `TransformationDirection`, so a transformation that was never
    /// inverted cannot silently execute.
    ///
    /// The transformation is not hand-built — `CocosConvention` has no public
    /// constructor, and hand-building one would prove less anyway. It comes
    /// out of a loaded artifact exactly as conversion-map resolution produces
    /// it, which is always `ToHli`, because resolution serves reads. Handing
    /// that straight to the write executor is precisely the mistake a future
    /// write path can make. The resolver is responsible for inversion now, so
    /// this remains a direct test of the copy-side assertion rather than a
    /// `run_write` scenario.
    #[test]
    fn a_write_side_transformation_that_was_not_inverted_refuses() {
        const ARTIFACT: &str = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="identity-flipped" rel="renamed" left="flipped" right="flipped">
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
        let map = crate::conversion::conversion_map::ConversionMap::load(ARTIFACT)
            .expect("fixture artifact must load");
        let explanation = map
            .resolve(
                "flipped",
                crate::conversion::conversion_map::Direction::Forward,
            )
            .expect("the fixture rule claims its own path");
        let crate::conversion::conversion_map::Outcome::Path {
            value_transformation,
            ..
        } = explanation.outcome
        else {
            panic!("the fixture path resolves to a concrete stored path")
        };
        assert!(
            matches!(value_transformation, ValueTransformation::SignFlip { .. }),
            "the fixture must declare a flip, or this proves nothing"
        );

        let values = [1.0f64, -2.0];
        assert_eq!(
            copy_value_transformation(&value_transformation, SourceView::Double(&values)),
            Err("this value transformation was not inverted for the write direction")
        );

        // Inverted, the very same transformation executes and negates a copy,
        // leaving the caller's own buffer alone (ADR 0018).
        let inverted = value_transformation
            .inverse()
            .expect("a flip between differing conventions inverts");
        assert_eq!(
            copy_value_transformation(&inverted, SourceView::Double(&values)),
            Ok(Some(vec![-1.0, 2.0]))
        );
        assert_eq!(values, [1.0f64, -2.0]);
    }

    /// A real [`ValueTransformation::SignFlip`], obtained by loading a tiny
    /// fixture artifact and resolving its one declared flip path — the only
    /// way to construct one from outside `conversion_map.rs`, whose
    /// `CocosConvention` tuple field is private to that module.
    fn sign_flip_transformation() -> ValueTransformation {
        let xml = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <default rel="identical"/>
              <transforms>
                <cocos from="11" to="17">
                  <flip path="flipped"/>
                </cocos>
              </transforms>
            </ids-map>
        "#;
        let map = ConversionMap::load(xml).expect("fixture artifact must load");
        let explanation = map
            .resolve("flipped", Direction::Forward)
            .expect("default-identical path must resolve");
        match explanation.outcome {
            Outcome::Path {
                value_transformation,
                ..
            } => value_transformation,
            other => panic!("expected a resolved path, got {other:?}"),
        }
    }

    fn resolved(
        path: &str,
        fidelity: Fidelity,
        value_transformation: ValueTransformation,
    ) -> ResolvedReadPath {
        ResolvedReadPath {
            path: CString::new(path).expect("no interior NUL"),
            fidelity,
            value_transformation,
        }
    }

    fn version(input: &str) -> DdVersion {
        input
            .parse()
            .expect("fixture DD version must be recognised")
    }

    fn discover(stamp: StampOutcome) -> DiscoveryDecision {
        decide_occurrence_registration("equilibrium", &version("4.1.1"), || stamp)
    }

    /// Issue #108 AC5: discovery is a seam-policy decision, so every
    /// ADR-0007/0009/0011 outcome is selectable in-process, without the
    /// process-wide HLI latch, the registry, or a loaded IMAS-Core library.
    #[test]
    fn discovery_returns_the_registration_effect_for_each_stamp_outcome() {
        assert!(matches!(
            discover(StampOutcome::Stored(version("4.1.1"))),
            DiscoveryDecision::RegisterNothing {
                occurrence_cache: OccurrenceCacheEffect::Forget
            }
        ));
        assert!(matches!(
            discover(StampOutcome::Unstamped),
            DiscoveryDecision::RegisterNothing {
                occurrence_cache: OccurrenceCacheEffect::Forget
            }
        ));
        assert!(matches!(
            discover(StampOutcome::Malformed(Box::new(
                crate::conversion_refusal("bad stamp")
            ))),
            DiscoveryDecision::RefuseAndEnd {
                occurrence_cache: OccurrenceCacheEffect::Forget,
                ..
            }
        ));
        assert!(matches!(
            decide_occurrence_registration("core_profiles", &version("4.1.1"), || {
                StampOutcome::Stored(version("3.39.0"))
            }),
            DiscoveryDecision::RegisterNothing {
                occurrence_cache: OccurrenceCacheEffect::RememberMismatch(stored)
            } if stored == version("3.39.0")
        ));
        assert!(matches!(
            discover(StampOutcome::Stored(version("3.39.0"))),
            DiscoveryDecision::RegisterRoot {
                stored,
                occurrence_cache: OccurrenceCacheEffect::RememberMismatch(cache_stored),
                ..
            } if stored == version("3.39.0") && cache_stored == version("3.39.0")
        ));
    }

    const DATAPATH_FIXTURE: &str = include_str!("../../docs/3.39.0--4.1.1.xml");
    const UNCLAIMED_DATAPATH_FIXTURE: &str = r#"
        <ids-map ids="equilibrium" format-version="1">
          <side id="left" dd="3.39.0" cocos="11"/>
          <side id="right" dd="4.1.1" cocos="17"/>
          <rules/>
        </ids-map>
    "#;

    fn datapath_map() -> ConversionMap {
        ConversionMap::load(DATAPATH_FIXTURE).expect("fixture artifact must load")
    }

    #[test]
    fn datapath_translation_leaves_an_unclaimed_path_to_the_adapter() {
        let map = ConversionMap::load(UNCLAIMED_DATAPATH_FIXTURE)
            .expect("unclaimed-path fixture artifact must load");
        assert_eq!(
            decide_datapath_translation(&map, Direction::Forward, "not/in/the/map"),
            None
        );
    }

    #[test]
    fn datapath_translation_resolves_a_claimed_rename() {
        assert_eq!(
            decide_datapath_translation(
                &datapath_map(),
                Direction::Forward,
                "time_slice/global_quantities/beta_normal",
            ),
            Some("time_slice/global_quantities/beta_tor_norm".to_string())
        );
    }

    #[test]
    fn datapath_translation_leaves_no_source_rules_to_the_adapter() {
        let map = datapath_map();
        assert_eq!(
            decide_datapath_translation(&map, Direction::Forward, "time_slice/boundary/lcfs"),
            None
        );
        assert_eq!(
            decide_datapath_translation(&map, Direction::Reverse, "time_slice/contour_tree"),
            None
        );
    }

    #[test]
    fn datapath_translation_leaves_a_retyped_rule_to_the_read_seam() {
        assert_eq!(
            decide_datapath_translation(
                &datapath_map(),
                Direction::Forward,
                "grids_ggd/grid/space/coordinates_type",
            ),
            None
        );
    }

    #[test]
    fn datapath_translation_keeps_a_merged_rules_resolved_path() {
        assert_eq!(
            decide_datapath_translation(
                &datapath_map(),
                Direction::Forward,
                "time_slice/constraints/j_tor",
            ),
            Some("time_slice/constraints/j_phi".to_string())
        );
    }

    /// The deliverable test (issue #107 AC3): a `merged` field's second
    /// candidate is the one that actually holds data, in a plain `cargo
    /// test` unit test with no C, no stub and no latch. This is exactly
    /// issue #65's defect made unwritable: every return point of `run_read`
    /// must populate a field fidelity, so a short-circuit that forgot to
    /// would not compile, and it also pins ADR 0010's ordering (the
    /// transformation runs only after data is found) and ADR 0012's bucket
    /// (a merged rule's loss is `PotentiallyLossy`, never bare `Lossy`).
    #[test]
    fn a_merged_fields_second_candidate_serves_the_read_with_its_own_sign_flip() {
        let candidates = TranslatedReadPath {
            paths: vec![
                resolved(
                    "time_slice/profiles_2d/b_field_tor",
                    Fidelity::PotentiallyLossy,
                    ValueTransformation::None,
                ),
                resolved(
                    "time_slice/profiles_2d/b_field_phi",
                    Fidelity::PotentiallyLossy,
                    sign_flip_transformation(),
                ),
            ],
        };
        let field = ReadArgument {
            resolution: ReadPath::Translated(candidates),
            forward: None,
            dd_path: "profiles_2d/b_tor".to_string(),
        };
        let timebase = ReadArgument {
            resolution: ReadPath::Forward,
            forward: None,
            dd_path: String::new(),
        };
        let shape = BufferShape {
            datatype: BufferDataType::Double,
            rank: 1,
        };

        let mut buffer = [10.0_f64];
        let seen_fields: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let mut remaining_buffer = Some(&mut buffer[..]);
        let reader = |field: Option<&CStr>, _timebase: Option<&CStr>| {
            seen_fields.borrow_mut().push(
                field
                    .expect("both candidates are translated")
                    .to_str()
                    .unwrap()
                    .to_string(),
            );
            if seen_fields.borrow().len() == 1 {
                Attempt::NotFound
            } else {
                Attempt::Data(
                    al_status_t::default(),
                    DataView::Double(remaining_buffer.take().expect("only one Data outcome")),
                )
            }
        };

        let verdict = run_read(field, timebase, shape, reader);

        assert_eq!(
            seen_fields.into_inner(),
            vec![
                "time_slice/profiles_2d/b_field_tor".to_string(),
                "time_slice/profiles_2d/b_field_phi".to_string(),
            ],
            "the loop must try the first candidate before the second names the read"
        );
        assert_eq!(
            buffer[0], -10.0,
            "the second candidate's sign flip must be applied"
        );
        assert!(
            matches!(verdict.outcome, SeamOutcome::Data(status) if status.code == 0),
            "a candidate returning data must be reported as a successful read"
        );
        assert_eq!(
            verdict.field.fidelity,
            Fidelity::PotentiallyLossy,
            "a merged candidate's own declared fidelity must be retained"
        );
    }

    /// Issue #107 AC4: ADR 0010's ordering, proved directly — an unsupported
    /// datatype refuses before the reader closure is ever invoked.
    #[test]
    fn an_unsupported_datatype_refuses_without_calling_the_reader() {
        let field = ReadArgument {
            resolution: ReadPath::Translated(TranslatedReadPath {
                paths: vec![resolved(
                    "time_slice/boundary/psi",
                    Fidelity::Exact,
                    sign_flip_transformation(),
                )],
            }),
            forward: None,
            dd_path: "time_slice/boundary/psi".to_string(),
        };
        let timebase = ReadArgument {
            resolution: ReadPath::Forward,
            forward: None,
            dd_path: String::new(),
        };
        let shape = BufferShape {
            datatype: BufferDataType::Other,
            rank: 0,
        };

        let calls = RefCell::new(0u32);
        let reader = |_field: Option<&CStr>, _timebase: Option<&CStr>| {
            *calls.borrow_mut() += 1;
            Attempt::NotFound
        };

        let verdict = run_read(field, timebase, shape, reader);

        assert_eq!(*calls.borrow(), 0, "the reader must never be called");
        match verdict.outcome {
            SeamOutcome::Refusal { dd_path, .. } => {
                assert_eq!(dd_path, "time_slice/boundary/psi");
            }
            _ => panic!("an unsupported datatype must refuse"),
        }
        assert_eq!(verdict.field.fidelity, Fidelity::Unmappable);
    }

    #[test]
    fn field_transform_validation_uses_the_first_timebase_candidate_fidelity() {
        let field = ReadArgument {
            resolution: ReadPath::Translated(TranslatedReadPath {
                paths: vec![resolved(
                    "field",
                    Fidelity::Exact,
                    sign_flip_transformation(),
                )],
            }),
            forward: None,
            dd_path: "field".to_string(),
        };
        let timebase = ReadArgument {
            resolution: ReadPath::Translated(TranslatedReadPath {
                paths: vec![
                    resolved(
                        "first_timebase",
                        Fidelity::PotentiallyLossy,
                        ValueTransformation::None,
                    ),
                    resolved(
                        "second_timebase",
                        Fidelity::Lossy,
                        ValueTransformation::None,
                    ),
                ],
            }),
            forward: None,
            dd_path: "timebase".to_string(),
        };

        let verdict = run_read(
            field,
            timebase,
            BufferShape {
                datatype: BufferDataType::Other,
                rank: 0,
            },
            |_, _| panic!("validation must refuse before the reader runs"),
        );

        assert_eq!(verdict.field.fidelity, Fidelity::Unmappable);
        assert_eq!(verdict.timebase.fidelity, Fidelity::PotentiallyLossy);
    }

    /// Issue #107 AC5, the issue-#66 shape: a relative field resolved
    /// beneath a child record's anchor must retain the complete anchor-joined
    /// DD path, not the bare relative argument the caller actually passed.
    /// The adapter (`src/interpose.rs`) is the one that performs that join —
    /// this proves `run_read` never re-derives or truncates it once given,
    /// which is what makes issue #66's defect (nine independent join call
    /// sites, one of which used the unjoined argument) impossible to
    /// reintroduce: there is now exactly one path this loop ever reports.
    #[test]
    fn a_relative_field_under_a_child_record_retains_the_anchor_joined_path() {
        let field = ReadArgument {
            resolution: ReadPath::Translated(TranslatedReadPath {
                paths: vec![resolved(
                    "time_slice/boundary_separatrix/gap/r",
                    Fidelity::Lossy,
                    ValueTransformation::None,
                )],
            }),
            forward: None,
            // The anchor ("time_slice") already joined onto the caller's own
            // relative argument ("boundary_separatrix/gap/r") by the adapter,
            // exactly as `read_argument_path` does in `src/interpose.rs`.
            dd_path: "time_slice/boundary_separatrix/gap/r".to_string(),
        };
        let timebase = ReadArgument {
            resolution: ReadPath::Forward,
            forward: None,
            dd_path: String::new(),
        };
        let shape = BufferShape {
            datatype: BufferDataType::Double,
            rank: 0,
        };

        let mut buffer = [1.0_f64];
        let mut remaining_buffer = Some(&mut buffer[..]);
        let reader = |_field: Option<&CStr>, _timebase: Option<&CStr>| {
            Attempt::Data(
                al_status_t::default(),
                DataView::Double(remaining_buffer.take().expect("called once")),
            )
        };

        let verdict = run_read(field, timebase, shape, reader);

        assert_eq!(
            verdict.field.path, "time_slice/boundary_separatrix/gap/r",
            "the complete anchor-joined path must survive into the verdict"
        );
        assert_eq!(verdict.field.fidelity, Fidelity::Lossy);
    }

    fn write_argument(path: &str, transformation: ValueTransformation) -> WriteArgument<'static> {
        WriteArgument {
            resolution: WritePath::Translated {
                path: CString::new(path).expect("fixture paths contain no NUL"),
                value_transformation: transformation,
            },
            forward: None,
            dd_path: path.to_string(),
        }
    }

    fn plain_timebase() -> WriteArgument<'static> {
        WriteArgument {
            resolution: WritePath::Forward,
            forward: None,
            dd_path: String::new(),
        }
    }

    #[test]
    fn a_write_sign_flip_copies_and_transforms_the_source() {
        let field = write_argument(
            "time_slice/constraints/flux_loop/measured",
            sign_flip_transformation()
                .inverse()
                .expect("a flip between differing conventions inverts"),
        );
        let timebase = plain_timebase();
        let caller_values = [1.25, -2.5, 3.75];

        let verdict = run_write(
            &field,
            &timebase,
            BufferShape {
                datatype: BufferDataType::Double,
                rank: 1,
            },
            SourceView::Double(&caller_values),
        );

        assert_eq!(
            caller_values,
            [1.25, -2.5, 3.75],
            "policy must only read caller storage"
        );
        match verdict {
            WriteVerdict::Forward {
                data: Some(values), ..
            } => {
                assert_eq!(values, [-1.25, 2.5, -3.75]);
            }
            WriteVerdict::Forward { data: None, .. } => {
                panic!("a COCOS write must carry a shim-owned transformed copy")
            }
            WriteVerdict::Refusal { reason, .. } => {
                panic!("the sign flip must be writable: {reason}")
            }
        }
    }

    #[test]
    fn a_write_copy_refuses_a_transformation_that_did_not_point_to_stored_data() {
        use crate::conversion::conversion_map::TransformationDirection;
        let same_convention = match sign_flip_transformation() {
            ValueTransformation::SignFlip { from_cocos, .. } => from_cocos,
            ValueTransformation::None => panic!("fixture must declare a COCOS sign flip"),
        };
        let field = write_argument(
            "time_slice/constraints/flux_loop/measured",
            ValueTransformation::SignFlip {
                from_cocos: same_convention.clone(),
                to_cocos: same_convention,
                direction: TransformationDirection::ToHli,
            },
        );
        let timebase = plain_timebase();
        let caller_values = [1.25];

        let verdict = run_write(
            &field,
            &timebase,
            BufferShape {
                datatype: BufferDataType::Double,
                rank: 0,
            },
            SourceView::Double(&caller_values),
        );

        assert!(matches!(
            verdict,
            WriteVerdict::Refusal { ref reason, .. }
                if reason == "this value transformation was not inverted for the write direction"
        ));
        assert_eq!(caller_values, [1.25]);
    }

    #[test]
    fn a_refused_field_is_reported_ahead_of_a_refused_timebase() {
        // The two arguments are resolved independently and `field` is resolved
        // first, so a caller who supplied a bad field hears about the field.
        // Which check inside the resolver rejected each argument does not
        // reorder them against each other: `field` is the argument the write
        // is about, and the ordered lists pin the order *within* one argument.
        let field = WriteArgument {
            resolution: WritePath::Refusal {
                reason: "field transform is not invertible".to_string(),
                dd_path: "field".to_string(),
            },
            forward: None,
            dd_path: "field".to_string(),
        };
        let timebase = WriteArgument {
            resolution: WritePath::Refusal {
                reason: "timebase needs a transformation".to_string(),
                dd_path: "timebase".to_string(),
            },
            forward: None,
            dd_path: "timebase".to_string(),
        };

        let verdict = run_write(
            &field,
            &timebase,
            BufferShape {
                datatype: BufferDataType::Double,
                rank: 0,
            },
            SourceView::Double(&[1.25]),
        );

        assert!(matches!(
            verdict,
            WriteVerdict::Refusal { ref reason, ref dd_path }
                if reason == "field transform is not invertible" && dd_path == "field"
        ));
    }

    #[test]
    fn an_unsupported_write_shape_refuses_before_it_can_copy() {
        let field = write_argument(
            "time_slice/constraints/flux_loop/measured",
            sign_flip_transformation(),
        );
        let timebase = plain_timebase();
        let caller_values = [1.25];

        for shape in [
            BufferShape {
                datatype: BufferDataType::Other,
                rank: 0,
            },
            BufferShape {
                datatype: BufferDataType::Double,
                rank: crate::MAXDIM as c_int + 1,
            },
        ] {
            let verdict = run_write(&field, &timebase, shape, SourceView::Double(&caller_values));
            assert!(matches!(verdict, WriteVerdict::Refusal { .. }));
        }
        assert_eq!(caller_values, [1.25]);
    }

    #[test]
    fn an_unset_scalar_forwards_without_a_transformed_copy() {
        let field = write_argument(
            "time_slice/constraints/flux_loop/measured",
            sign_flip_transformation(),
        );
        let timebase = plain_timebase();
        let verdict = run_write(
            &field,
            &timebase,
            BufferShape {
                datatype: BufferDataType::Double,
                rank: 0,
            },
            SourceView::UnsetScalar,
        );

        assert!(matches!(verdict, WriteVerdict::Forward { data: None, .. }));
    }

    fn delete_argument(paths: &[&str]) -> DeleteArgument<'static> {
        DeleteArgument {
            resolution: DeletePath::Candidates(
                paths
                    .iter()
                    .map(|path| CString::new(*path).expect("fixture path contains no NUL"))
                    .collect(),
            ),
            forward: None,
        }
    }

    #[test]
    fn a_delete_candidate_plan_visits_every_candidate_in_order() {
        let argument = delete_argument(&["first", "second", "third"]);
        let mut visited = Vec::new();

        let verdict = run_delete(&argument, |path| {
            visited.push(path.to_str().expect("fixture paths are ASCII").to_string());
            al_status_t::default()
        });

        assert_eq!(visited, ["first", "second", "third"]);
        assert!(matches!(verdict, DeleteVerdict::Complete { failure: None }));
    }

    #[test]
    fn a_delete_candidate_plan_reports_its_first_failure_after_visiting_later_candidates() {
        let argument = delete_argument(&["first", "second", "third"]);
        let mut visited = Vec::new();

        let verdict = run_delete(&argument, |path| {
            let path = path.to_str().expect("fixture paths are ASCII");
            visited.push(path.to_string());
            al_status_t {
                code: if path == "first" {
                    7
                } else if path == "second" {
                    9
                } else {
                    0
                },
                ..al_status_t::default()
            }
        });

        assert_eq!(visited, ["first", "second", "third"]);
        match verdict {
            DeleteVerdict::Complete {
                failure: Some(failure),
            } => {
                assert_eq!(failure.status.code, 7);
                assert_eq!(
                    failure.path.to_str().expect("fixture paths are ASCII"),
                    "first"
                );
            }
            DeleteVerdict::Complete { failure: None } => {
                panic!("the first failure must be retained")
            }
            DeleteVerdict::Forward { .. } | DeleteVerdict::Refusal { .. } => {
                panic!("a candidate plan must complete in seam policy")
            }
        }
    }

    #[test]
    fn one_translated_delete_path_forwards_once_for_the_c_facing_layer() {
        let argument = DeleteArgument {
            resolution: DeletePath::Translated(CString::new("one").expect("no interior NUL")),
            forward: None,
        };

        let verdict = run_delete(&argument, |_| {
            panic!("the policy must not call Core for one path")
        });

        match verdict {
            DeleteVerdict::Forward { path: Some(path) } => {
                assert_eq!(path.to_str().expect("fixture paths are ASCII"), "one");
            }
            DeleteVerdict::Forward { path: None } => panic!("the translated path must be retained"),
            DeleteVerdict::Complete { .. } | DeleteVerdict::Refusal { .. } => {
                panic!("one translated path must forward to the C-facing layer")
            }
        }
    }
}
