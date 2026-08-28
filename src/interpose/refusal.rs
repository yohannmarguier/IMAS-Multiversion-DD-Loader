use std::ffi::{CStr, c_char, c_int};

use crate::al_status_t;
use crate::conversion::path_conversion;
#[cfg(test)]
use crate::registry::context_registry::MapCacheKey;
use crate::registry::context_registry::{ConversionRecord, REGISTRY};

/// `ptr` as a borrowed `&CStr`, or `None` if it is null.
///
/// # Safety
/// `ptr` must be a valid, NUL-terminated C string, or null.
pub(super) unsafe fn c_str_ref<'a>(ptr: *const c_char) -> Option<&'a CStr> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller's own contract requires `ptr`, when non-null, to be
    // a valid NUL-terminated C string.
    Some(unsafe { CStr::from_ptr(ptr) })
}

/// `ptr` as a borrowed `&str`, or `None` if it is null or not valid UTF-8.
pub(super) fn c_str_or_none<'a>(ptr: *const c_char) -> Option<&'a str> {
    // SAFETY: this function carries `c_str_ref`'s contract to its own
    // callers, who are the ones holding IMAS-Core's guarantee about `ptr`.
    unsafe { c_str_ref(ptr) }.and_then(|path| path.to_str().ok())
}

/// The raw HLI argument joined onto `record`'s own anchor, or `None` if the
/// argument itself is absent. Shared by its two callers, which want opposite
/// things from that `None`: `read_argument_path` falls back to the bare anchor,
/// because a loss entry always needs some path to name, while
/// `contextual_refusal` prefers a non-empty anchor and otherwise says so
/// explicitly rather than reporting a misleading one.
pub(super) fn joined_argument_path(
    record: &ConversionRecord,
    raw_path: *const c_char,
) -> Option<String> {
    c_str_or_none(raw_path)
        .filter(|path| !path.is_empty())
        .map(|path| path_conversion::join_hli_path(&record.resolved_path, path))
}

pub(super) fn read_argument_path(record: &ConversionRecord, raw_path: *const c_char) -> String {
    joined_argument_path(record, raw_path).unwrap_or_else(|| record.resolved_path.clone())
}

/// Formats a path-conversion refusal using the version pair retained by its
/// live context record. Read, write, and context-opening seams use this one
/// status boundary, so their caller-visible diagnostics cannot drift.
pub(super) fn context_path_refusal(
    record: &ConversionRecord,
    reason: &str,
    dd_path: &str,
) -> al_status_t {
    crate::path_conversion_refusal(reason, dd_path, &record.hli_version, &record.stored_version)
}

/// A refusal from a seam that holds a live conversion record but has no
/// resolved path to name — today the two arraystruct-open arguments, whose
/// own resolution already failed and so produced no stored spelling.
///
/// Issue #58 AC3 asks that *every* refusal message name the reason, the DD
/// path and both DD versions, and these seams used to emit the reason alone.
/// Not having resolved a path is no reason to withhold the rest: the record
/// that triggered the refusal carries both versions, and `raw_path` is the
/// caller's own argument, which is the spelling AC3 asks to see anyway.
///
/// A seam whose path argument is null or empty falls back to the context's
/// own resolved path, and says so plainly when there is no path at either
/// place rather than inventing one. That fallback outlives the delete seam
/// that motivated it: issue #64's blanket context-keyed delete refusal was
/// this function's original caller, and #129/#131 replaced it with real path
/// resolution, so `delete_data` now refuses through `context_path_refusal`
/// with a resolved spelling in hand.
pub(super) fn contextual_refusal(
    record: &ConversionRecord,
    reason: &str,
    raw_path: *const c_char,
) -> al_status_t {
    let dd_path = joined_argument_path(record, raw_path)
        .or_else(|| (!record.resolved_path.is_empty()).then(|| record.resolved_path.clone()))
        .unwrap_or_else(|| "(no path argument)".to_string());
    context_path_refusal(record, reason, &dd_path)
}

/// The live conversion record for `ctx_id`, or `None` — with the
/// conversion-disabled case answered before the registry's lock is taken.
///
/// Every seam keyed on a context ID goes through this rather than
/// [`ContextRegistry::lookup`] directly. A record exists only where
/// `open_occurrence` made one, which requires a latched HLI DD
/// version, and the latch is an `OnceLock` that can never fall back to unset —
/// so with no conversion basis the answer is `None` by construction, and
/// acquiring the registry's mutex to rediscover that is cost with no result. It
/// is per `al_read_data` call, on the path every non-converting HLI takes for
/// every field it reads: issue #56 AC5 asks for exactly this
/// ("Matching, unknown, unstamped, and conversion-disabled contexts bypass
/// registry lookup and rule resolution"), and the `begin_*` seams have always
/// short-circuited the same way — they call `hli_version::latched` because they
/// go on to use the version, while these seams only need to know whether one
/// exists.
///
/// The *unknown* and *matching* halves of that criterion still cost one lookup:
/// they are not knowable without asking the registry, and ADR 0003 budgets one
/// lookup for them by design.
pub(super) fn live_conversion_record(ctx_id: c_int) -> Option<ConversionRecord> {
    if !crate::version::hli_version::conversion_is_possible() {
        return None;
    }
    REGISTRY.lookup(ctx_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::known_artifacts;

    /// Issue #56 AC5: "Matching, unknown, unstamped, and conversion-disabled
    /// contexts bypass registry lookup and rule resolution." The
    /// conversion-disabled half is the one a seam can act on by itself, and
    /// this proves it acts on it *before* the registry rather than after.
    ///
    /// `hli_version`'s latch is deliberately never set in-process (its module
    /// comment explains why a unit test cannot set it), so
    /// `conversion_is_possible()` is false for the whole `cargo test` run.
    /// Registering a genuine root record and still getting `None` back is the
    /// observable proof: the record is unquestionably there, so a lookup that
    /// ran could not have missed it.
    #[test]
    fn a_data_path_seam_answers_before_the_registry_when_conversion_is_disabled() {
        // Far from the small IDs every other registry test uses, so this one
        // cannot collide with a concurrently running test in the same process.
        const CTX_ID: c_int = 0x5D00;
        let stored: crate::version::dd_version::DdVersion =
            "3.39.0".parse().expect("known release");
        let hli: crate::version::dd_version::DdVersion = "4.1.1".parse().expect("known release");
        let artifact = known_artifacts::lookup("equilibrium", &stored, &hli)
            .expect("the embedded equilibrium artifact serves this pair");
        let direction = artifact.direction_to_stored;
        assert!(REGISTRY.record_root(
            CTX_ID,
            String::new(),
            CTX_ID,
            MapCacheKey::new("equilibrium".to_string(), stored, hli),
            direction,
            || {
                crate::conversion::conversion_map::ConversionMap::load(artifact.xml)
                    .expect("embedded artifact must parse")
            },
        ));

        assert!(
            !crate::version::hli_version::conversion_is_possible(),
            "no unit test can latch an HLI DD version, so conversion is off here"
        );
        assert!(
            REGISTRY.lookup(CTX_ID).is_some(),
            "the record must really be in the registry for this test to prove anything"
        );
        assert!(
            live_conversion_record(CTX_ID).is_none(),
            "the seam must answer from the latch, without consulting the registry"
        );

        REGISTRY.remove(CTX_ID);
    }
}
