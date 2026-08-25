//! What stored path an HLI argument means, and at what fidelity.
//!
//! Before this module existed, [`crate::conversion::conversion_map::Outcome`] was
//! interpreted at three independent sites in `src/interpose.rs`, each deriving
//! a different subset of its meaning: `translate_down` derived a `CString`
//! or nothing, the read seam derived a [`ReadPath`] with fidelity and
//! candidates, and the context-opening seams derived one concrete spelling,
//! no-source, or a refusal. This module is the one place that answers the
//! question instead, so no consumer re-derives the enum.
//!
//! It knows nothing about seams, attempts, loops or IMAS-Core: it takes a
//! live [`ConversionRecord`] and a raw HLI argument, and answers either "what
//! stored path does this mean" ([`resolve_context_path`] for a context-open,
//! [`resolve_write_path`] for one safe write spelling), or "what stored read
//! plan does this mean" ([`resolve_read_path`], for the one seam —
//! `al_read_data` — that can try several candidates and apply a value
//! transformation).
//!
//! Issue #101 (part B); see ADR 0015 for the layering this belongs to.

use std::ffi::{CStr, CString, c_char};

use crate::conversion::conversion_map::{
    Fidelity, Outcome, RefusalReason, Rel, ValueTransformation,
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
    Candidates(TranslatedReadPath),
    NoSource(Fidelity),
    Refusal {
        reason: String,
        dd_path: String,
        fidelity: Fidelity,
    },
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
    /// The supplied HLI-DD path cannot safely be written through this seam.
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

/// A conversion-map outcome narrowed to what a structure-path ABI argument can
/// pass to IMAS-Core: one concrete stored spelling, no source, or a refusal.
/// A merged/split plan and a value transformation are both things only a data
/// read can carry out — a candidate plan needs somewhere to try each candidate
/// in turn, and a transformation needs a buffer to apply itself to. Neither
/// reduces to the single stored spelling these seams must hand IMAS-Core.
enum ConcreteStoredPath {
    Path(String),
    NoSource,
    Refusal(String),
}

/// One path-bearing ABI argument that the conversion map claims, in the form
/// both path resolvers need before they can differ: whether the caller spelled
/// it absolutely, its absolute HLI-DD spelling, and the rule that explains it.
struct ClaimedArgument {
    is_absolute: bool,
    hli_absolute: String,
    explanation: crate::conversion::conversion_map::RuleExplanation,
}

/// What one path-bearing ABI argument amounts to, before
/// [`resolve_context_path`] and [`resolve_read_path`] differ on what to do
/// with it.
///
/// The two reasons an argument yields no rule are kept apart here on purpose.
/// They used to share one `None`, which forced every caller to re-derive the
/// distinction from `raw` after the fact — the arraystruct seam did, and the
/// read seam did not, which is how an unclaimed read path came to be
/// forwarded to IMAS-Core.
enum ReadArgument {
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

/// The preamble [`resolve_context_path`] and [`resolve_read_path`] share.
fn claimed_argument(record: &ConversionRecord, raw: *const c_char) -> ReadArgument {
    let Some(raw) = c_str_or_none(raw).filter(|path| !path.is_empty()) else {
        return ReadArgument::Absent;
    };
    let is_absolute = raw.starts_with('/');
    let hli_absolute = join_hli_path(&record.resolved_path, raw);
    let Some(explanation) = record
        .map
        .resolve(&hli_absolute, record.direction_to_stored)
    else {
        return ReadArgument::Unclaimed;
    };
    ReadArgument::Claimed(ClaimedArgument {
        is_absolute,
        hli_absolute,
        explanation,
    })
}

/// Resolves one path-bearing context argument independently, preserving the
/// caller's relative-vs-absolute spelling after conversion has selected the
/// stored-DD path. `al_read_data` and `al_begin_arraystruct_action` share this
/// policy.
pub(crate) fn resolve_context_path(
    record: &ConversionRecord,
    raw: *const c_char,
) -> ContextPathResolution {
    let argument = match claimed_argument(record, raw) {
        ReadArgument::Absent => return ContextPathResolution::Forward,
        ReadArgument::Unclaimed => return ContextPathResolution::Unclaimed,
        ReadArgument::Claimed(argument) => argument,
    };
    let ClaimedArgument {
        is_absolute,
        explanation,
        ..
    } = argument;

    match concrete_stored_path(explanation.outcome) {
        ConcreteStoredPath::NoSource => ContextPathResolution::NoSource,
        ConcreteStoredPath::Refusal(reason) => ContextPathResolution::Refusal(reason),
        ConcreteStoredPath::Path(resolved_path) => {
            match stored_c_path(record, &resolved_path, is_absolute) {
                Ok(path) => ContextPathResolution::Translated(path),
                Err(reason) => ContextPathResolution::Refusal(reason),
            }
        }
    }
}

/// Resolves one read argument. Unlike `resolve_context_path`, this preserves
/// merged/split candidates and their transformations so the read seam can
/// execute the plan without making them appear as one concrete AOS path.
pub(crate) fn resolve_read_path(record: &ConversionRecord, raw: *const c_char) -> ReadPath {
    let argument = match claimed_argument(record, raw) {
        ReadArgument::Absent => return ReadPath::Forward,
        // User story 47: "a path that no rule claims a source for [must]
        // return not-found (code == 0, null data), so that an unclaimed path
        // is never silently forwarded under the wrong DD spelling". Forwarding
        // it would hand IMAS-Core an HLI-DD spelling against stored data of a
        // different DD version, on the one code path that knows no rule
        // vouches for it. The verdict is retained as unmappable, so a caller
        // draining the loss log sees why the read came back empty.
        ReadArgument::Unclaimed => return ReadPath::NoSource(Fidelity::Unmappable),
        ReadArgument::Claimed(argument) => argument,
    };
    let ClaimedArgument {
        is_absolute,
        hli_absolute,
        explanation,
    } = argument;

    let fidelity = read_fidelity(explanation.fidelity, explanation.rel);
    match explanation.outcome {
        Outcome::Refusal(reason) => ReadPath::Refusal {
            reason: refusal_reason_message(reason),
            dd_path: hli_absolute,
            fidelity: Fidelity::Unmappable,
        },
        Outcome::NoSource => ReadPath::NoSource(fidelity),
        Outcome::Path {
            resolved_path,
            value_transformation,
            candidates,
        } if candidates.is_empty() => translated_read_component(
            record,
            &resolved_path,
            is_absolute,
            fidelity,
            value_transformation,
        )
        .map(|path| ReadPath::Translated(TranslatedReadPath { paths: vec![path] }))
        .unwrap_or_else(|reason| ReadPath::Refusal {
            reason,
            dd_path: hli_absolute,
            fidelity: Fidelity::Unmappable,
        }),
        Outcome::Path { candidates, .. } => candidates
            .into_iter()
            .map(|candidate| {
                translated_read_component(
                    record,
                    &candidate.path,
                    is_absolute,
                    fidelity,
                    candidate.value_transformation,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|paths| ReadPath::Candidates(TranslatedReadPath { paths }))
            .unwrap_or_else(|reason| ReadPath::Refusal {
                reason,
                dd_path: hli_absolute,
                fidelity: Fidelity::Unmappable,
            }),
    }
}

/// Resolves one write argument to the only stored-DD spelling this ticket can
/// safely write: one path and no candidate plan. The resolved value
/// transformation still points from stored data to the HLI (because maps
/// serve reads); `run_write` inverts it before it copies caller data.
pub(crate) fn resolve_write_path(record: &ConversionRecord, raw: *const c_char) -> WritePath {
    let argument = match claimed_argument(record, raw) {
        ReadArgument::Absent => return WritePath::Forward,
        ReadArgument::Unclaimed => {
            let dd_path = c_str_or_none(raw)
                .filter(|path| !path.is_empty())
                .map(|path| join_hli_path(&record.resolved_path, path))
                .unwrap_or_else(|| record.resolved_path.clone());
            return WritePath::Refusal {
                reason: "this path is unclaimed by the conversion map".to_string(),
                dd_path,
            };
        }
        ReadArgument::Claimed(argument) => argument,
    };
    let ClaimedArgument {
        is_absolute,
        hli_absolute,
        explanation,
    } = argument;

    // A write through a mismatch must never rewrite the occurrence's DD
    // version stamp: the rest of this call can only translate one field, not
    // migrate the whole occurrence into the HLI DD version. The two sibling
    // `version_put` fields describe the writing library rather than the DD
    // and therefore continue through ordinary resolution below.
    if hli_absolute == "ids_properties/version_put/data_dictionary" {
        return WritePath::Refusal {
            reason: "the DD-version stamp is immutable under a version mismatch".to_string(),
            dd_path: hli_absolute,
        };
    }

    // A `merged`/`split` rule needs the later write policy that selects a
    // precedence-1 source and reports the candidates it left untouched. This
    // first write slice only serves identity, renamed, and moved paths.
    if matches!(explanation.rel, Some(Rel::Merged | Rel::Split)) {
        return WritePath::Refusal {
            reason: "this path is served by several stored candidates, and this write cannot choose one safely"
                .to_string(),
            dd_path: hli_absolute,
        };
    }

    match explanation.outcome {
        Outcome::Refusal(reason) => WritePath::Refusal {
            reason: refusal_reason_message(reason),
            dd_path: hli_absolute,
        },
        Outcome::NoSource => WritePath::Refusal {
            reason: "this path has no stored source".to_string(),
            dd_path: hli_absolute,
        },
        Outcome::Path { candidates, .. } if !candidates.is_empty() => WritePath::Refusal {
            reason: "this path is served by several stored candidates, and this write cannot choose one safely"
                .to_string(),
            dd_path: hli_absolute,
        },
        Outcome::Path {
            resolved_path,
            value_transformation,
            ..
        } => match stored_c_path(record, &resolved_path, is_absolute) {
            Ok(path) => WritePath::Translated {
                path,
                value_transformation,
            },
            Err(reason) => WritePath::Refusal {
                reason,
                dd_path: hli_absolute,
            },
        },
    }
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

fn translated_read_component(
    record: &ConversionRecord,
    resolved_path: &str,
    is_absolute: bool,
    fidelity: Fidelity,
    value_transformation: ValueTransformation,
) -> Result<ResolvedReadPath, String> {
    stored_c_path(record, resolved_path, is_absolute).map(|path| ResolvedReadPath {
        path,
        fidelity,
        value_transformation,
    })
}

/// Turns a resolved stored-DD path into the exact spelling IMAS-Core must
/// receive: absolute when the caller spelled its argument absolutely,
/// otherwise stripped back to this context's own stored anchor. Both
/// path-bearing seams — [`resolve_context_path`] for an arraystruct open and
/// [`translated_read_component`] for a read — decide that spelling here, so
/// the two cannot drift apart and the two refusals it can produce are worded
/// once rather than twice.
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
    match concrete_stored_path(explanation.outcome) {
        ConcreteStoredPath::Path(path) => Ok(path),
        ConcreteStoredPath::NoSource => Err("context anchor has no stored source".to_string()),
        ConcreteStoredPath::Refusal(message) => Err(message),
    }
}

/// Narrows a resolved rule to the one stored spelling a structure-path
/// argument can carry, stated here as the two rules a structure path obeys
/// that [`crate::conversion_map`] itself does not state:
///
/// 1. A structure path may not carry an ordered candidate plan — only a data
///    read resolves both cases (it tries a candidate plan in order and flips
///    signs in the returned buffer), but an AOS container path and a context
///    anchor are resolved *before* any data exists, so serving either here
///    would mean picking one candidate arbitrarily.
/// 2. A structure path may not carry a value transformation — a
///    transformation needs a buffer to apply itself to, and none exists yet
///    at open time, so serving it here would mean dropping the transformation
///    silently.
///
/// Neither is a property of the conversion data itself — the same `merged`
/// rule serves a read (which can try candidates) and would refuse an
/// arraystruct open on the identical resolved path — so `conversion_map.rs`
/// cannot state them: they depend on what the *consumer* can do with the
/// outcome, not on what the rule declares. This function is where that
/// consumer-side narrowing lives instead.
fn concrete_stored_path(outcome: Outcome) -> ConcreteStoredPath {
    match outcome {
        Outcome::Refusal(reason) => ConcreteStoredPath::Refusal(refusal_reason_message(reason)),
        Outcome::NoSource => ConcreteStoredPath::NoSource,
        Outcome::Path {
            resolved_path: _,
            value_transformation: _,
            candidates,
        } if !candidates.is_empty() => ConcreteStoredPath::Refusal(
            "this path is served by several stored candidates, and only a data read can try them \
             in turn"
                .to_string(),
        ),
        Outcome::Path {
            resolved_path: _,
            value_transformation,
            candidates: _,
        } if value_transformation != ValueTransformation::None => ConcreteStoredPath::Refusal(
            "this path needs a value transformation, which only a data read can apply".to_string(),
        ),
        Outcome::Path { resolved_path, .. } => ConcreteStoredPath::Path(resolved_path),
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
    use crate::conversion::conversion_map::ConversionMap;
    use crate::registry::context_registry::{MapCacheKey, REGISTRY};
    use std::ffi::c_int;

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
        // Far from the small IDs every other registry test uses, so this one
        // cannot collide with a concurrently running test in the same process.
        const CTX_ID: c_int = 0x5D01;
        // A distinct context ID is not enough. `record_root` obtains its map
        // through the registry's `(ids, stored, hli)` cache, so while any other
        // record on the real `("equilibrium", 3.39.0, 4.1.1)` pair is live —
        // `a_data_path_seam_answers_before_the_registry_when_conversion_is_disabled`
        // (`src/interpose.rs`) registers exactly that — the closure below never
        // runs and this test resolves through the *approved* map instead. That
        // map carries `<default rel="identical"/>`, which claims every path,
        // so the unclaimed branch this test exists to reach vanishes and the
        // test fails on whichever interleaving wins. The IDS half of the key
        // is what keeps the fixture map unshareable; it deliberately names no
        // real IDS.
        const FIXTURE_IDS: &str = "equilibrium-no-document-default-fixture";
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
        let stored: crate::version::dd_version::DdVersion =
            "3.39.0".parse().expect("known release");
        let hli: crate::version::dd_version::DdVersion = "4.1.1".parse().expect("known release");
        assert!(REGISTRY.record_root(
            CTX_ID,
            String::new(),
            CTX_ID,
            MapCacheKey::new(FIXTURE_IDS.to_string(), stored, hli),
            crate::conversion::conversion_map::Direction::Forward,
            || ConversionMap::load(NO_DEFAULT_ARTIFACT).expect("fixture artifact must load"),
        ));
        let record = REGISTRY
            .lookup(CTX_ID)
            .expect("the root record was just registered");

        // The rule the fixture does carry still resolves, so an unclaimed
        // verdict below cannot be the whole map failing to match anything.
        // Asserting the *stored spelling* rather than just "it translated"
        // also pins that this record really resolves through the fixture: the
        // approved map would rename nothing and hand back `claimed` itself
        // through its identity default.
        let claimed = CString::new("claimed").expect("no interior NUL");
        match resolve_read_path(&record, claimed.as_ptr()) {
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
        match resolve_read_path(&record, unclaimed.as_ptr()) {
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
                resolve_read_path(&record, empty.as_ptr()),
                ReadPath::Forward
            ),
            "an empty argument is absent, not unclaimed"
        );
        assert!(
            matches!(
                resolve_read_path(&record, std::ptr::null()),
                ReadPath::Forward
            ),
            "a null argument is absent, not unclaimed"
        );

        REGISTRY.remove(CTX_ID);
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
