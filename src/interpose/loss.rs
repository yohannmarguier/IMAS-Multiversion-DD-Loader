//! The shim-owned `imas_mvdd_context_loss_*` exports.
//!
//! ADR 0012's reporting channel. `al_status_t` has no room for a partial
//! outcome, so loss is retained on the context and drained afterwards by
//! these three exports — the only symbols here that IMAS-Core does not also
//! define. They allocate nothing, resolve a child context to its root, and
//! report zero for an untracked context rather than refusing.

use std::ffi::{c_char, c_int};

use crate::al_status_t;
use crate::conversion::conversion_map::Fidelity;
use crate::loss::{LossOperation, fidelity_c_code};
use crate::loss_file::{self, LossFileEntry};
use crate::registry::context_registry::{ContextId, REGISTRY};

/// Retains one operation's loss against the root captured before the seam
/// called IMAS-Core. The loss module decides whether it is worth keeping.
pub(crate) fn retain_loss(
    root_id: ContextId,
    dd_path: String,
    fidelity: Fidelity,
    operation: LossOperation,
) {
    // The record snapshot and in-memory retention each take and release the
    // registry lock before the file module ever considers an I/O operation.
    let record = REGISTRY.lookup(root_id);
    REGISTRY.retain_loss_at_root(root_id, dd_path.clone(), fidelity, operation);
    if let Some(record) = record {
        let stored_version = record.stored_version.to_string();
        let hli_version = record.hli_version.to_string();
        loss_file::retain(LossFileEntry {
            uri: &record.pulse_uri,
            ids: &record.dataobjectname,
            stored_version: &stored_version,
            hli_version: &hli_version,
            operation,
            fidelity,
            path: &dd_path,
        });
    }
}

/// Implements `imas_mvdd_context_loss_count` (ADR 0012): reports the number
/// of loss-log entries retained on `ctx_id`'s root context without
/// allocating. Every untracked context — a data-entry pulse, an unrecorded
/// or already-ended id, or an operation whose stored and HLI DD versions
/// matched — reports `0` rather than a refusal, since none of them has ever
/// produced a loss entry.
///
/// # Safety
/// `count` must be a valid, writable `*mut c_int`, or null.
pub(crate) unsafe fn context_loss_count(ctx_id: c_int, count: *mut c_int) -> al_status_t {
    if count.is_null() {
        return crate::conversion_refusal(
            "imas_mvdd_context_loss_count requires a non-null count output",
        );
    }
    let n = REGISTRY.loss_count(ctx_id);
    // SAFETY: just checked non-null above.
    unsafe {
        *count = n as c_int;
    }
    al_status_t::default()
}

/// Implements `imas_mvdd_context_loss_at` (ADR 0012): copies the
/// `index`-th loss-log entry retained on `ctx_id`'s root context into
/// caller-owned storage, without allocating or publishing any internal
/// struct or pointer. Refuses — leaving every output untouched — for a null
/// output pointer, a negative index or buffer length, an out-of-range index
/// (which also covers every untracked context, whose count is always
/// zero), and a buffer too small to hold the path and its trailing NUL.
///
/// # Safety
/// `path_buf` must be a valid, writable buffer of at least `buf_len` bytes,
/// or null. `verdict` must be a valid, writable `*mut c_int`, or null.
pub(crate) unsafe fn context_loss_at(
    ctx_id: c_int,
    index: c_int,
    path_buf: *mut c_char,
    buf_len: c_int,
    verdict: *mut c_int,
) -> al_status_t {
    if verdict.is_null() {
        return crate::conversion_refusal(
            "imas_mvdd_context_loss_at requires a non-null verdict output",
        );
    }
    if path_buf.is_null() {
        return crate::conversion_refusal(
            "imas_mvdd_context_loss_at requires a non-null path buffer",
        );
    }
    let Ok(index) = usize::try_from(index) else {
        return crate::conversion_refusal("imas_mvdd_context_loss_at index must not be negative");
    };
    let Ok(buf_len) = usize::try_from(buf_len) else {
        return crate::conversion_refusal(
            "imas_mvdd_context_loss_at buffer length must not be negative",
        );
    };
    let Some(copy_result) = REGISTRY.with_loss_at(ctx_id, index, |path, fidelity, _| {
        if path.len() >= buf_len {
            return Err("imas_mvdd_context_loss_at buffer is too small for this path");
        }
        // SAFETY: `path_buf` is non-null and at least `buf_len` bytes long
        // per this function's safety contract, and `path.len() < buf_len`
        // leaves room for the trailing NUL written just past it.
        unsafe {
            std::ptr::copy_nonoverlapping(path.as_ptr().cast::<c_char>(), path_buf, path.len());
            *path_buf.add(path.len()) = 0;
            *verdict = fidelity_c_code(fidelity);
        }
        Ok(())
    }) else {
        return crate::conversion_refusal(
            "imas_mvdd_context_loss_at index is out of range for this context",
        );
    };
    if let Err(reason) = copy_result {
        return crate::conversion_refusal(reason);
    }
    al_status_t::default()
}

/// Implements `imas_mvdd_context_loss_operation_at` (ADR 0012): reports
/// which operation produced the `index`-th loss-log entry retained on
/// `ctx_id`'s root context, without allocating. Refuses — leaving the output
/// untouched — for a null output pointer, a negative index, or an
/// out-of-range index (which also covers an untracked context).
///
/// # Safety
/// `operation` must be a valid, writable `*mut c_int`, or null.
pub(crate) unsafe fn context_loss_operation_at(
    ctx_id: c_int,
    index: c_int,
    operation: *mut c_int,
) -> al_status_t {
    if operation.is_null() {
        return crate::conversion_refusal(
            "imas_mvdd_context_loss_operation_at requires a non-null operation output",
        );
    }
    let Ok(index) = usize::try_from(index) else {
        return crate::conversion_refusal(
            "imas_mvdd_context_loss_operation_at index must not be negative",
        );
    };
    let Some(()) = REGISTRY.with_loss_at(ctx_id, index, |_, _, entry_operation| {
        // SAFETY: `operation` is non-null and valid for writes per this
        // function's safety contract.
        unsafe {
            *operation = entry_operation.c_code();
        }
    }) else {
        return crate::conversion_refusal(
            "imas_mvdd_context_loss_operation_at index is out of range for this context",
        );
    };
    al_status_t::default()
}
