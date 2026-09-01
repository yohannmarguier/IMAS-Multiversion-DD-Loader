//! The `al_read_data` seams.
//!
//! `al_read_data` and its `al_plugin_read_data` twin share one body: resolve
//! `field` and `timebase` to the stored spelling, try the candidate plan in
//! declared precedence order, classify the three-way read outcome (ADR 0012)
//! and apply any value transformation in place on the way back up.
//!
//! The loop itself is not here — [`crate::conversion::seam_policy::run_read`]
//! owns which candidate to try next and what fidelity an argument reached
//! (ADR 0015). This module supplies it the Core call and the raw buffers.

use std::ffi::{CStr, c_char, c_int, c_void};

use crate::al_status_t;
use crate::conversion::path_conversion;
use crate::conversion::read_outcome::{self, ReadOutcome};
use crate::conversion::seam_policy;
use crate::core::core_binding::DOUBLE_DATA_ID;
use crate::loss::LossOperation;
use crate::registry::context_registry::ConversionRecord;

use super::dispatch::{CallFamily, call_read};
use super::loss::retain_loss;
use super::reentry::ReentryGuard;
use super::refusal::{c_str_ref, context_path_refusal, live_conversion_record, read_argument_path};

/// Forwards to IMAS-Core's real `al_read_data`, resolving IMAS-Core lazily
/// on first use. See [`read_data_impl`] for the shared policy this and
/// [`plugin_read_data`] both carry out.
///
/// # Safety
/// `field` and `timebase` must be valid, NUL-terminated C strings, or null
/// where IMAS-Core's own contract allows it. `data` and `size` must be
/// valid, writable pointers, matching IMAS-Core's own contract for this
/// function.
pub(crate) unsafe fn read_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    // SAFETY: same contract as `read_data_impl`, already upheld by this
    // function's own `unsafe fn` contract.
    unsafe {
        read_data_impl(
            CallFamily::ORDINARY,
            ctx_id,
            field,
            timebase,
            data,
            datatype,
            dim,
            size,
        )
    }
}

/// Mirrors `read_data`'s policy exactly (issue #68): the same registry
/// snapshot, conversion-map resolution, merged/split candidate loop, value
/// transformation, and fidelity retention as an ordinary read — forwarded
/// through IMAS-Core's plugin reentry read symbol rather than its ordinary
/// twin, so a plugin re-entering the ABI gets the same translation an HLI
/// would.
///
/// # Safety
/// Same contract as [`read_data`].
pub(crate) unsafe fn plugin_read_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    // SAFETY: same contract as `read_data_impl`, already upheld by this
    // function's own `unsafe fn` contract.
    unsafe {
        read_data_impl(
            CallFamily::PLUGIN,
            ctx_id,
            field,
            timebase,
            data,
            datatype,
            dim,
            size,
        )
    }
}

/// The policy shared by `read_data` and `plugin_read_data` (issue #68,
/// consolidated onto [`CallFamily`] by issue #109).
///
/// When `ctx_id` names no live conversion record — no mismatch was ever
/// discovered, the occurrence matched or was unstamped, or the HLI DD
/// version is unset — this is a plain forward, unchanged from before issue
/// #54. The unset case is answered by [`live_conversion_record`] from the
/// version latch, without taking the registry's lock at all.
///
/// Otherwise this is marshalling and effect performance around
/// [`seam_policy::run_read`], which owns every decision — path resolution,
/// the merged/split candidate loop, the value transformation, and each
/// argument's retained fidelity (issue #107). This function resolves `field`
/// and `timebase` through the conversion map, builds the reader closure
/// `run_read` drives (classifying each attempt through
/// [`read_outcome::classify`] and handing back a safe [`seam_policy::DataView`]
/// only once IMAS-Core has actually written one), and turns the returned
/// [`seam_policy::ReadVerdict`] into an `al_status_t` plus the two loss-log
/// writes ADR 0012 asks for — the one place either ever happens now (issue
/// #66).
///
/// # Safety
/// `field` and `timebase` must be valid, NUL-terminated C strings, or null
/// where IMAS-Core's own contract allows it. `data` and `size` must be
/// valid, writable pointers, matching IMAS-Core's own contract for this
/// function.
#[allow(clippy::too_many_arguments)]
unsafe fn read_data_impl(
    family: CallFamily,
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    // A read that arrives while this thread is already inside a read seam was
    // not issued by the caller this shim converts for: it comes from
    // underneath the in-flight IMAS-Core call, carrying a path the shim has
    // already translated into the stored DD version. Converting it again is
    // wrong in every direction — it would resolve a stored path as if it were
    // an HLI one, apply a second value transformation, and retain a loss entry
    // for a read the caller never issued. Forward it exactly as received
    // (ADR 0014).
    let (_reentry_guard, already_entered) = ReentryGuard::enter();
    if already_entered {
        return call_read(family, ctx_id, field, timebase, data, datatype, dim, size);
    }
    let Some(record) = live_conversion_record(ctx_id) else {
        return call_read(family, ctx_id, field, timebase, data, datatype, dim, size);
    };

    let field_argument = seam_policy::ReadArgument {
        resolution: path_conversion::narrow_read_path(path_conversion::resolve(&record, field)),
        // SAFETY: this function's own contract requires `field` to be a
        // valid, NUL-terminated C string, or null.
        forward: unsafe { c_str_ref(field) },
        dd_path: read_argument_path(&record, field),
    };
    let timebase_argument = seam_policy::ReadArgument {
        resolution: path_conversion::narrow_read_path(path_conversion::resolve(&record, timebase)),
        // SAFETY: this function's own contract requires `timebase` to be a
        // valid, NUL-terminated C string, or null.
        forward: unsafe { c_str_ref(timebase) },
        dd_path: read_argument_path(&record, timebase),
    };
    let shape = seam_policy::BufferShape {
        datatype: if datatype == DOUBLE_DATA_ID {
            seam_policy::BufferDataType::Double
        } else {
            seam_policy::BufferDataType::Other
        },
        rank: dim,
    };

    let reader = |field_attempt: Option<&CStr>, timebase_attempt: Option<&CStr>| {
        let field_ptr = field_attempt.map_or(std::ptr::null(), CStr::as_ptr);
        let timebase_ptr = timebase_attempt.map_or(std::ptr::null(), CStr::as_ptr);
        let status = call_read(
            family,
            ctx_id,
            field_ptr,
            timebase_ptr,
            data,
            datatype,
            dim,
            size,
        );
        // SAFETY: `data` is valid and writable by `read_data_impl`'s own
        // safety contract, and the just-finished IMAS-Core call has
        // initialized it.
        let data_ptr = unsafe { *data };
        match read_outcome::classify(&status, data_ptr) {
            ReadOutcome::Failure => seam_policy::Attempt::Failure(status),
            ReadOutcome::NotFound => seam_policy::Attempt::NotFound,
            // SAFETY: `data`/`size` are valid per this function's own safety
            // contract, and `ReadOutcome::Data` establishes `data_ptr`
            // non-null and initialized by the just-finished IMAS-Core call.
            ReadOutcome::Data => seam_policy::Attempt::Data(status, unsafe {
                build_data_view(data_ptr, datatype, dim, size)
            }),
        }
    };

    let verdict = seam_policy::run_read(field_argument, timebase_argument, shape, reader);
    finish_read(&record, verdict, data)
}

/// Builds the safe, typed view [`seam_policy::run_read`] applies a value
/// transformation through, from a data buffer IMAS-Core has just written.
/// Only ever called on a [`ReadOutcome::Data`] outcome, per `read_data_impl`'s
/// own reader closure.
///
/// # Safety
/// `data_ptr` must be non-null and, when `datatype == DOUBLE_DATA_ID`, must
/// point to an initialized array of `DOUBLE_DATA` elements whose extents
/// `size` describes for a rank-`dim` read (or a single `f64` when `dim ==
/// 0`), matching IMAS-Core's own contract for a successful `al_read_data`.
unsafe fn build_data_view<'a>(
    data_ptr: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> seam_policy::DataView<'a> {
    if datatype != DOUBLE_DATA_ID {
        return seam_policy::DataView::NotDouble;
    }
    let element_count = if dim == 0 {
        Ok(1usize)
    } else if size.is_null() {
        Err("value-transform execution needs array dimensions")
    } else {
        // SAFETY: the ABI requires one initialized extent per rank after a
        // successful IMAS-Core array read.
        unsafe { std::slice::from_raw_parts(size, dim as usize) }
            .iter()
            .try_fold(1usize, |count, &extent| {
                usize::try_from(extent)
                    .ok()
                    .and_then(|extent| count.checked_mul(extent))
            })
            .ok_or("value-transform execution received an invalid array shape")
    };
    match element_count {
        Ok(count) => {
            // SAFETY: the caller's own contract requires `data_ptr` to point
            // to an initialized `DOUBLE_DATA` buffer of exactly this shape.
            let values = unsafe { std::slice::from_raw_parts_mut(data_ptr.cast::<f64>(), count) };
            seam_policy::DataView::Double(values)
        }
        Err(reason) => seam_policy::DataView::InvalidShape(reason),
    }
}

/// Turns a [`seam_policy::ReadVerdict`] into the `al_status_t` `read_data_impl`
/// returns, writing both arguments' retained fidelities to `record`'s root
/// loss log first. This is the one call site that ever writes to the loss
/// log for a read (issue #66): `seam_policy::ReadVerdict::field`/`timebase`
/// are mandatory, so there is no return path left that could reach this
/// point without both to write.
fn finish_read(
    record: &ConversionRecord,
    verdict: seam_policy::ReadVerdict,
    data: *mut *mut c_void,
) -> al_status_t {
    record_argument_loss(record, &verdict.field);
    record_argument_loss(record, &verdict.timebase);
    match verdict.outcome {
        seam_policy::SeamOutcome::Data(status) => status,
        seam_policy::SeamOutcome::NotFound => no_source_read(data),
        seam_policy::SeamOutcome::Refusal { reason, dd_path } => {
            context_path_refusal(record, &reason, &dd_path)
        }
    }
}

/// Retains one argument's fidelity on `record`'s root loss log — skipping
/// exact-fidelity operations, which are never logged (ADR 0012).
fn record_argument_loss(record: &ConversionRecord, argument: &seam_policy::ArgumentFidelity) {
    retain_loss(
        record,
        argument.path.clone(),
        argument.fidelity,
        LossOperation::Read,
    );
}

/// Returns the C ABI's normal not-found outcome for a path the artifact says
/// has no stored source. The caller owns `data`'s validity by the public
/// `al_read_data` contract.
fn no_source_read(data: *mut *mut c_void) -> al_status_t {
    // SAFETY: forwarded from `read_data`, whose safety contract requires a
    // valid, writable data pointer.
    unsafe {
        *data = std::ptr::null_mut();
    }
    al_status_t::default()
}
