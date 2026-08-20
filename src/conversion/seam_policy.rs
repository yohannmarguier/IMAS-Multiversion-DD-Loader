//! The `al_read_data`/`al_plugin_read_data` read loop (issue #107, part C of
//! the #101 series; see ADR 0015).
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
//! This module owns that loop and nothing else: it takes an already-resolved
//! [`path_conversion::ReadPath`] per argument, a buffer's shape, and a reader
//! closure the adapter injects, and returns a [`ReadVerdict`] the adapter
//! turns into an `al_status_t` and a pair of loss-log writes. It contains no
//! `unsafe`, never touches [`crate::registry::context_registry::REGISTRY`] or the HLI
//! version latch, and never calls into [`crate::core::dl`] — every raw pointer,
//! every registry lookup, and the two ADR-0014/HLI-version gates
//! ahead of it stay in `src/interpose.rs`, the interposition layer ADR 0015
//! names.
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
use crate::conversion::conversion_map::{Fidelity, ValueTransformation};
use crate::conversion::known_artifacts::{self, ArtifactMatch};
use crate::conversion::path_conversion::{self, ReadPath, TranslatedReadPath};
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

/// `EMPTY_DOUBLE`, IMAS-Core's sentinel for an absent value in an otherwise
/// populated `DOUBLE_DATA` array: never sign-flipped, so a caller can tell a
/// real zero from a hole in the data.
const EMPTY_DOUBLE: f64 = -9e40;

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

/// One argument's read-loop fidelity verdict: the fidelity the loop actually
/// reached, alongside the DD path it would be retained against if that
/// fidelity is not [`Fidelity::Exact`]. `path` is always present — see this
/// module's doc comment for why that is load-bearing.
pub(crate) struct ArgumentFidelity {
    pub(crate) path: String,
    pub(crate) fidelity: Fidelity,
}

/// What [`run_read`] decided to report back to the HLI, short of turning it
/// into an `al_status_t` — that formatting step needs the live
/// [`crate::context_registry::ConversionRecord`]'s DD versions, which this
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
/// [`validate_value_transformation`] runs before `reader` is ever called for
/// that pair (ADR 0010); a [`Attempt::Data`] result runs
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
        ReadPath::Translated(path) | ReadPath::Candidates(path) => Some(path),
        ReadPath::Refusal {
            reason,
            dd_path,
            fidelity,
        } => {
            return verdict(
                SeamOutcome::Refusal { reason, dd_path },
                &field_dd_path,
                fidelity,
                &timebase_dd_path,
                Fidelity::Exact,
            );
        }
        ReadPath::NoSource(fidelity) => {
            return verdict(
                SeamOutcome::NotFound,
                &field_dd_path,
                fidelity,
                &timebase_dd_path,
                Fidelity::Exact,
            );
        }
    };

    let timebase_translated = match timebase.resolution {
        ReadPath::Forward => None,
        ReadPath::Translated(path) | ReadPath::Candidates(path) => Some(path),
        ReadPath::Refusal {
            reason,
            dd_path,
            fidelity,
        } => {
            return verdict(
                SeamOutcome::Refusal { reason, dd_path },
                &field_dd_path,
                Fidelity::Exact,
                &timebase_dd_path,
                fidelity,
            );
        }
        ReadPath::NoSource(fidelity) => {
            return verdict(
                SeamOutcome::NotFound,
                &field_dd_path,
                Fidelity::Exact,
                &timebase_dd_path,
                fidelity,
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
        for timebase_attempt in &timebase_attempts {
            if let Err(reason) =
                validate_value_transformation(&field_attempt.value_transformation, &shape)
            {
                return verdict(
                    SeamOutcome::Refusal {
                        reason: reason.to_string(),
                        dd_path: field_dd_path.clone(),
                    },
                    &field_dd_path,
                    Fidelity::Unmappable,
                    &timebase_dd_path,
                    timebase_attempt.fidelity,
                );
            }
            match reader(field_attempt.path, timebase_attempt.path) {
                Attempt::Failure(status) => {
                    return verdict(
                        SeamOutcome::Data(status),
                        &field_dd_path,
                        field_attempt.fidelity,
                        &timebase_dd_path,
                        timebase_attempt.fidelity,
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
                            &field_dd_path,
                            Fidelity::Unmappable,
                            &timebase_dd_path,
                            timebase_attempt.fidelity,
                        );
                    }
                    return verdict(
                        SeamOutcome::Data(status),
                        &field_dd_path,
                        field_attempt.fidelity,
                        &timebase_dd_path,
                        timebase_attempt.fidelity,
                    );
                }
                Attempt::NotFound => {}
            }
        }
    }

    verdict(
        SeamOutcome::NotFound,
        &field_dd_path,
        path_conversion::translated_read_fidelity(field_translated.as_ref()),
        &timebase_dd_path,
        path_conversion::translated_read_fidelity(timebase_translated.as_ref()),
    )
}

/// Assembles one [`ReadVerdict`], the one place that pairs an outcome with
/// both arguments' fidelities — collapsing what would otherwise be the same
/// nested-struct literal at every one of `run_read`'s return points.
fn verdict(
    outcome: SeamOutcome,
    field_path: &str,
    field_fidelity: Fidelity,
    timebase_path: &str,
    timebase_fidelity: Fidelity,
) -> ReadVerdict {
    ReadVerdict {
        outcome,
        field: ArgumentFidelity {
            path: field_path.to_string(),
            fidelity: field_fidelity,
        },
        timebase: ArgumentFidelity {
            path: timebase_path.to_string(),
            fidelity: timebase_fidelity,
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
            resolution: ReadPath::Candidates(candidates),
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
}
