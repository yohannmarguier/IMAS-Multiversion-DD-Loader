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
use crate::conversion::read_outcome::{
    self, EMPTY_CHAR, EMPTY_COMPLEX, EMPTY_DOUBLE, EMPTY_INT, ReadOutcome,
};
use crate::conversion::seam_policy;
use crate::core::core_binding::{CHAR_DATA_ID, COMPLEX_DATA_ID, DOUBLE_DATA_ID, INTEGER_DATA_ID};
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
    // SAFETY: `data` and `size` are valid and writable, and `datatype`/`dim`
    // describe them, by this function's own safety contract.
    unsafe { finish_read(&record, verdict, data, datatype, dim, size) }
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
///
/// # Safety
/// `data` and `size` must satisfy `read_data_impl`'s own contract, and
/// `datatype`/`dim` must describe the buffer they address — [`no_source_read`]
/// writes through both.
unsafe fn finish_read(
    record: &ConversionRecord,
    verdict: seam_policy::ReadVerdict,
    data: *mut *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    record_argument_loss(record, &verdict.field);
    record_argument_loss(record, &verdict.timebase);
    match verdict.outcome {
        seam_policy::SeamOutcome::Data(status) => status,
        // SAFETY: `data`, `size`, `datatype` and `dim` are this function's
        // own contract, forwarded unchanged from `read_data_impl`.
        seam_policy::SeamOutcome::NotFound => unsafe { no_source_read(data, datatype, dim, size) },
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
/// has no stored source, leaving the caller's buffers exactly as IMAS-Core
/// would have left them for an absent field.
///
/// This mirrors `Lowlevel::setDefaultValue`, and the rank branch is the whole
/// point of doing so. For `dim > 0` IMAS-Core owns the buffer: absence is a
/// null `*data` plus every returned extent set to zero. For `dim == 0` the
/// *caller* owns the buffer and the pointer comes back unchanged, so absence
/// has no channel other than the datatype's EMPTY sentinel written into it
/// (CONTEXT.md's "read outcome"). Nulling the pointer there instead would
/// hand a scalar caller whatever its buffer already held — indistinguishable
/// from a real measurement, and a regression against not installing the shim
/// at all.
///
/// A datatype IMAS-Core does not know is the one case that diverges: Core
/// throws, while this leaves the buffer untouched and reports success. The
/// four datatypes are the entire ABI (`CHAR_DATA` through `COMPLEX_DATA`) and
/// the caller had to name one of them to get this far, so the arm is
/// unreachable rather than lenient.
///
/// # Safety
/// `data` must be a valid, writable pointer. When `dim == 0`, `*data` must
/// address a caller-owned scalar of the type `datatype` names. When
/// `dim > 0`, `size` must address at least `dim` writable `c_int`s. Both are
/// the public `al_read_data` contract, forwarded unchanged.
unsafe fn no_source_read(
    data: *mut *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    if dim == 0 {
        // SAFETY: `data` is valid and writable by this function's contract.
        let scalar = unsafe { *data };
        if !scalar.is_null() {
            // SAFETY: `dim == 0` makes `*data` a caller-owned scalar of the
            // type `datatype` names, per this function's contract. The null
            // guard above is the one place this is gentler than IMAS-Core,
            // which dereferences unconditionally and so crashes; a shim that
            // crashed the process here would be strictly worse, and it can
            // mask nothing, because IMAS-Core is never reached on this path.
            unsafe {
                match datatype {
                    CHAR_DATA_ID => *scalar.cast::<c_char>() = EMPTY_CHAR,
                    INTEGER_DATA_ID => *scalar.cast::<c_int>() = EMPTY_INT,
                    DOUBLE_DATA_ID => *scalar.cast::<f64>() = EMPTY_DOUBLE,
                    COMPLEX_DATA_ID => {
                        scalar.cast::<f64>().copy_from(EMPTY_COMPLEX.as_ptr(), 2);
                    }
                    _ => {}
                }
            }
        }
        return al_status_t::default();
    }
    // SAFETY: `data` is valid and writable, and `size` addresses at least
    // `dim` `c_int`s, both by this function's contract.
    unsafe {
        *data = std::ptr::null_mut();
        if !size.is_null() {
            std::slice::from_raw_parts_mut(size, dim.max(0) as usize).fill(0);
        }
    }
    al_status_t::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Calls [`no_source_read`] the way `al_read_data` would for a scalar:
    /// `data` points at the caller's own pointer, which points at the
    /// caller's own storage.
    fn scalar_no_source(storage: *mut c_void, datatype: c_int) -> al_status_t {
        let mut data = storage;
        // SAFETY: `data` is a live local, `*data` addresses caller storage of
        // the type `datatype` names, and `dim == 0` never touches `size`.
        unsafe { no_source_read(&raw mut data, datatype, 0, std::ptr::null_mut()) }
    }

    #[test]
    fn an_absent_double_scalar_receives_the_empty_double_sentinel() {
        let mut value = 3.5f64;
        let status = scalar_no_source((&raw mut value).cast(), DOUBLE_DATA_ID);
        assert_eq!(status.code, 0);
        assert_eq!(value, EMPTY_DOUBLE);
    }

    #[test]
    fn an_absent_integer_scalar_receives_the_empty_int_sentinel() {
        let mut value: c_int = 7;
        let status = scalar_no_source((&raw mut value).cast(), INTEGER_DATA_ID);
        assert_eq!(status.code, 0);
        assert_eq!(value, EMPTY_INT);
    }

    #[test]
    fn an_absent_char_scalar_receives_the_empty_char_sentinel() {
        let mut value: c_char = b'x' as c_char;
        let status = scalar_no_source((&raw mut value).cast(), CHAR_DATA_ID);
        assert_eq!(status.code, 0);
        assert_eq!(value, EMPTY_CHAR);
    }

    #[test]
    fn an_absent_complex_scalar_receives_both_halves_of_the_sentinel() {
        let mut value = [1.5f64, 2.5f64];
        let status = scalar_no_source(value.as_mut_ptr().cast(), COMPLEX_DATA_ID);
        assert_eq!(status.code, 0);
        assert_eq!(value, EMPTY_COMPLEX);
    }

    /// The scalar branch must not null the caller's pointer: IMAS-Core leaves
    /// it alone for `dim == 0`, and the caller may well read through it again.
    #[test]
    fn an_absent_scalar_leaves_the_caller_pointer_addressing_its_own_buffer() {
        let mut value = 3.5f64;
        let mut data: *mut c_void = (&raw mut value).cast();
        // SAFETY: as `scalar_no_source`, whose body this repeats to keep the
        // pointer observable after the call.
        let status =
            unsafe { no_source_read(&raw mut data, DOUBLE_DATA_ID, 0, std::ptr::null_mut()) };
        assert_eq!(status.code, 0);
        assert_eq!(data, (&raw mut value).cast::<c_void>());
    }

    /// A caller that passes a null scalar buffer crashes IMAS-Core; the shim
    /// declines to crash and reports the same success it would otherwise.
    #[test]
    fn an_absent_scalar_with_no_caller_buffer_is_reported_without_a_write() {
        let status = scalar_no_source(std::ptr::null_mut(), DOUBLE_DATA_ID);
        assert_eq!(status.code, 0);
    }

    /// The nonscalar half of `Lowlevel::setDefaultValue`: a null pointer *and*
    /// every returned extent zeroed. The extents are the half the shim used to
    /// skip, leaving a caller reading a stale shape off a null buffer.
    #[test]
    fn an_absent_array_nulls_the_pointer_and_zeroes_every_returned_extent() {
        let mut value = 3.5f64;
        let mut data: *mut c_void = (&raw mut value).cast();
        let mut size: [c_int; 3] = [11, 22, 33];
        // SAFETY: `data` is a live local and `size` has at least `dim == 2`
        // writable elements.
        let status = unsafe { no_source_read(&raw mut data, DOUBLE_DATA_ID, 2, size.as_mut_ptr()) };
        assert_eq!(status.code, 0);
        assert!(data.is_null());
        assert_eq!(size, [0, 0, 33]);
        assert_eq!(value, 3.5f64, "an array read must not touch caller storage");
    }
}
