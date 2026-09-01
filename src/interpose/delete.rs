//! The `al_delete_data` seam.
//!
//! A write asserts a value where a delete asserts an absence, so where a
//! write must not fan out a delete must: ADR 0017 calls Core for *every*
//! candidate in the plan, with no presence probe. The first nonzero status is
//! retained while later candidates are still attempted, which is why
//! [`candidate_failure`] renames the failure after the stored candidate that
//! produced it — the caller's own path does not identify it.

use std::ffi::{CStr, c_char, c_int};

use crate::conversion::conversion_map::Fidelity;
use crate::conversion::path_conversion;
use crate::conversion::seam_policy;
use crate::core::core_binding::forward_status;
use crate::loss::LossOperation;
use crate::{al_status_t, write_truncated};

use super::loss::retain_loss;
use super::reentry::ReentryGuard;
use super::refusal::{c_str_ref, context_path_refusal, live_conversion_record};

/// Forwards to IMAS-Core's real `al_delete_data`, resolving IMAS-Core
/// lazily on first use.
///
/// A live conversion record resolves a nonempty `path` to one safe stored-DD
/// spelling. The empty path deliberately forwards unchanged: IMAS-Core reads
/// it as an explicit whole-DATAOBJECT delete, leaving no foreign-version data
/// behind for a later unstamped open to mistake for HLI-version data. Unlike
/// [`super::write_data`], this seam takes no [`super::dispatch::CallFamily`] parameter:
/// `al_delete_data` has no plugin twin at all (issue #109 AC2).
///
/// # Safety
/// `path` must be a valid, NUL-terminated C string, or null where
/// IMAS-Core's own contract allows it.
pub(crate) unsafe fn delete_data(ctx: c_int, path: *const c_char) -> al_status_t {
    let (_reentry_guard, already_entered) = ReentryGuard::enter();
    if already_entered {
        return forward_status!(delete_data(ctx, path));
    }
    let Some(record) = live_conversion_record(ctx) else {
        return forward_status!(delete_data(ctx, path));
    };

    let argument = seam_policy::DeleteArgument {
        resolution: path_conversion::narrow_delete_path(
            &record,
            path,
            path_conversion::resolve(&record, path),
        ),
        // SAFETY: this function's contract requires `path` to be a valid,
        // NUL-terminated C string, or null.
        forward: unsafe { c_str_ref(path) },
    };
    let delete = |path: &CStr| forward_status!(delete_data(ctx, path.as_ptr()));
    match seam_policy::run_delete(&argument, delete) {
        seam_policy::DeleteVerdict::Forward { path } => {
            forward_status!(delete_data(
                ctx,
                path.map_or(std::ptr::null(), CStr::as_ptr)
            ))
        }
        seam_policy::DeleteVerdict::Complete {
            failure,
            visited_candidates,
        } => {
            if visited_candidates.len() > 1 {
                for path in visited_candidates {
                    retain_loss(
                        &record,
                        path.to_string_lossy().into_owned(),
                        Fidelity::PotentiallyLossy,
                        LossOperation::Delete,
                    );
                }
            }
            failure.map_or_else(al_status_t::default, |failure| {
                candidate_failure(failure.status, failure.path)
            })
        }
        seam_policy::DeleteVerdict::Refusal { reason, dd_path } => {
            retain_loss(
                &record,
                dd_path.clone(),
                Fidelity::Unmappable,
                LossOperation::Delete,
            );
            context_path_refusal(&record, &reason, &dd_path)
        }
    }
}

/// Keeps IMAS-Core's failure code while naming the stored candidate whose
/// delete failed, which the caller's own path does not identify: one HLI path
/// fans out over several stored ones.
fn candidate_failure(mut status: al_status_t, path: &CStr) -> al_status_t {
    status.message = [0; crate::MAX_ERR_MSG_LEN];
    write_truncated(
        &mut status.message,
        &format!(
            "IMAS-MVDD: delete failed for stored candidate {}",
            path.to_string_lossy()
        ),
    );
    status
}
