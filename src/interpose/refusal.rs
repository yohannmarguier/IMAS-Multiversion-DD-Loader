//! The refusal formatter and the raw-argument marshalling around it.
//!
//! Every seam that can refuse names the same four things — reason, DD path,
//! HLI version and stored version — from [`context_path_refusal`], so no
//! caller-visible diagnostic drifts from its siblings. Naming the DD path
//! means first turning a raw `*const c_char` into an anchor-joined one, which
//! is why the pointer helpers live beside the formatter that consumes them.
//!
//! [`live_conversion_record`] is the gate the data-path seams share: it
//! answers from the ADR 0005 latch before touching the registry, so a process
//! that cannot convert pays no lock.

use std::ffi::{CStr, c_char, c_int};

use crate::al_status_t;
use crate::conversion::conversion_map::Fidelity;
use crate::conversion::path_conversion;
use crate::loss::LossOperation;
use crate::registry::context_registry::{ConversionRecord, REGISTRY};
#[cfg(test)]
use crate::registry::context_registry::{MapCacheKey, RootRegistration};

use super::loss::retain_loss;

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
///
/// Retains an `UNMAPPABLE` loss before formatting the refusal (issue #178):
/// this is the arraystruct-open seam's only refusal path, and until now it
/// was the one shim-decided refusal that never reached the loss log, while a
/// refused write and a refused delete both already do. It is logged as a read
/// loss (`LossOperation::Read`) because that is what the caller was
/// ultimately prevented from doing — opening a context exists to read or
/// write through it, and every reachable refusal here (issue #178's
/// candidate-plan case included) is refusing to *read*, never a write in
/// flight.
pub(super) fn contextual_refusal(
    record: &ConversionRecord,
    reason: &str,
    raw_path: *const c_char,
) -> al_status_t {
    let dd_path = joined_argument_path(record, raw_path)
        .or_else(|| (!record.resolved_path.is_empty()).then(|| record.resolved_path.clone()))
        .unwrap_or_else(|| "(no path argument)".to_string());
    retain_loss(
        record,
        dd_path.clone(),
        Fidelity::Unmappable,
        LossOperation::Read,
    );
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
    conversion_record_if_enabled(
        crate::version::hli_version::conversion_is_possible(),
        || REGISTRY.lookup(ctx_id),
    )
}

/// The gate's decision as a value, so both halves of it can be proven without
/// touching the process-wide latch: the registry is consulted only when
/// conversion is possible, and never before that answer is known.
fn conversion_record_if_enabled(
    conversion_is_possible: bool,
    lookup: impl FnOnce() -> Option<ConversionRecord>,
) -> Option<ConversionRecord> {
    conversion_is_possible.then(lookup).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::known_artifacts;
    use crate::interpose::occurrence::load_artifact;
    use std::ffi::CString;

    fn register_equilibrium_root(ctx_id: c_int, resolved_path: &str) -> ConversionRecord {
        let stored: crate::version::dd_version::DdVersion =
            "3.39.0".parse().expect("known release");
        let hli: crate::version::dd_version::DdVersion = "4.1.1".parse().expect("known release");
        let artifact = known_artifacts::lookup("equilibrium", &stored, &hli)
            .expect("the embedded equilibrium artifact serves this pair");
        assert!(REGISTRY.record_root(
            RootRegistration {
                ctx_id,
                resolved_path: resolved_path.to_string(),
                pulse_ctx_id: ctx_id,
                dataobjectname: "equilibrium".to_string(),
                key: MapCacheKey::new("equilibrium".to_string(), stored, hli),
                direction_to_stored: artifact.direction_to_stored,
                opened_read_op: true,
            },
            || load_artifact(&artifact),
        ));
        REGISTRY
            .lookup(ctx_id)
            .expect("the root just registered must be live")
    }

    /// Issue #56 AC5: "Matching, unknown, unstamped, and conversion-disabled
    /// contexts bypass registry lookup and rule resolution." The
    /// conversion-disabled half is the one a seam can act on by itself, and
    /// this proves it acts on it *before* the registry rather than after.
    ///
    /// The isolated latch decision enters as a value, so this test can prove
    /// the hot-path short-circuit without mutating the process-wide latch.
    #[test]
    fn a_data_path_seam_answers_before_the_registry_when_conversion_is_disabled() {
        assert!(
            conversion_record_if_enabled(false, || {
                panic!("a conversion-disabled seam must not query the registry")
            })
            .is_none(),
            "the seam must answer from the latch, without consulting the registry"
        );
    }

    /// The other half of the same gate: once conversion is possible the seam
    /// reports exactly what the registry holds, and invents nothing for a
    /// context that was never registered. The public entry point over the
    /// process-wide latch stays the business of the process-isolated C
    /// scenarios (ADR 0005).
    #[test]
    fn a_data_path_seam_sees_only_a_registered_record_when_conversion_is_enabled() {
        const REGISTERED_CTX_ID: c_int = 0x5D01;
        const UNREGISTERED_CTX_ID: c_int = 0x5D02;
        register_equilibrium_root(REGISTERED_CTX_ID, "time_slice");

        let record = conversion_record_if_enabled(true, || REGISTRY.lookup(REGISTERED_CTX_ID))
            .expect("an enabled seam must see the registered conversion record");
        assert_eq!(record.resolved_path, "time_slice");
        assert!(
            conversion_record_if_enabled(true, || REGISTRY.lookup(UNREGISTERED_CTX_ID)).is_none(),
            "an enabled seam must not invent a record for an unregistered context"
        );

        REGISTRY.remove(REGISTERED_CTX_ID);
    }

    #[test]
    fn a_contextual_refusal_uses_its_context_path_and_public_status() {
        const CTX_ID: c_int = 0x5D80;
        let record = register_equilibrium_root(CTX_ID, "time_slice");
        let empty_path = CString::new("").expect("empty C string");
        let status = contextual_refusal(
            &record,
            "arraystruct path has no stored source",
            empty_path.as_ptr(),
        );

        assert_eq!(status.code, crate::IMAS_MVDD_CONVERSION_ERROR);
        // SAFETY: contextual_refusal creates its status through the shim's
        // NUL-terminating public refusal formatter.
        let message = unsafe { std::ffi::CStr::from_ptr(status.message.as_ptr()) }
            .to_str()
            .expect("the public refusal is valid UTF-8");
        assert_eq!(
            message,
            "IMAS-MVDD: arraystruct path has no stored source; DD path: time_slice; \
             HLI DD version: 4.1.1; stored DD version: 3.39.0"
        );
        assert_eq!(REGISTRY.loss_count(CTX_ID), 1);

        REGISTRY.remove(CTX_ID);
    }
}
