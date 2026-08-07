//! IMAS-Multiversion-DD-Loader — C ABI surface.
//!
//! This crate re-exports IMAS-Core's public C ABI verbatim and interposes on
//! the path-bearing entry points. The shared constants and `al_status_t` are
//! here, and the runtime-binding architecture (see `src/resolve.rs` and
//! `docs/adr/0001-runtime-binding-not-linking.md`) is proven end to end on
//! `al_context_info` plus the thirteen data-entry, action-lifecycle and
//! data-operation symbols below. Every other mirrored entry point, and all
//! DD path/version conversion, is still unimplemented.

// The mirrored ABI dictates the names; matching IMAS-Core exactly is the point.
#![allow(non_camel_case_types)]

use std::ffi::c_char;
use std::ffi::c_double;
use std::ffi::c_int;
use std::ffi::c_void;

mod dl;
mod resolve;

/// Length of `al_status_t::message`, mirroring IMAS-Core's `MAX_ERR_MSG_LEN`.
pub const MAX_ERR_MSG_LEN: usize = 256;

/// Maximum array rank accepted across the ABI, mirroring IMAS-Core's `MAXDIM`.
pub const MAXDIM: usize = 7;

/// Status returned by every ABI entry point. `code == 0` means success.
///
/// Note the conflicting convention one layer down: a backend's `read_data`
/// returns `0` for *not found* and `1` for success.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct al_status_t {
    pub code: c_int,
    pub message: [c_char; MAX_ERR_MSG_LEN],
}

impl Default for al_status_t {
    fn default() -> Self {
        Self {
            code: 0,
            message: [0; MAX_ERR_MSG_LEN],
        }
    }
}

/// Version of this shim, as a NUL-terminated static string.
///
/// Deliberately *not* named `getDDVersion` — that IMAS-Core call is dead and
/// returns the sentinel `"!!DEPRECATED!!"`.
#[unsafe(no_mangle)]
pub extern "C" fn imas_mvdd_loader_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// Reset `status` to success (`code == 0`, empty message).
///
/// # Safety
/// `status` must be non-null and point to a writable `al_status_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imas_mvdd_loader_status_clear(status: *mut al_status_t) {
    if status.is_null() {
        return;
    }
    unsafe { *status = al_status_t::default() };
}

/// Mirrors IMAS-Core's `al_context_info` exactly — same name, same
/// signature. Resolves IMAS-Core lazily on first call (see
/// `resolve::context_info`) and forwards every call to the real
/// implementation; a process that never calls this never requires
/// IMAS-Core to be present.
///
/// # Safety
/// `info` must be a valid, writable `*mut *mut c_char`, or null, matching
/// IMAS-Core's own contract for this function. On success the caller owns
/// `*info` and must free it, per IMAS-Core's documented contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_context_info(ctx: c_int, info: *mut *mut c_char) -> al_status_t {
    unsafe { resolve::context_info(ctx, info) }
}

/// Mirrors IMAS-Core's `al_begin_dataentry_action` exactly and forwards
/// unchanged. Opens a pulse addressed by `uri` and reports the resulting
/// context id in `*dectxID`.
///
/// # Safety
/// `uri` must be a valid, NUL-terminated C string. `dectxID` must be a
/// valid, writable `*mut c_int`, matching IMAS-Core's own contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_begin_dataentry_action(
    uri: *const c_char,
    mode: c_int,
    dectx_id: *mut c_int,
) -> al_status_t {
    unsafe { resolve::begin_dataentry_action(uri, mode, dectx_id) }
}

/// Mirrors IMAS-Core's `al_close_pulse` exactly and forwards unchanged.
#[unsafe(no_mangle)]
pub extern "C" fn al_close_pulse(pulse_ctx: c_int, mode: c_int) -> al_status_t {
    resolve::close_pulse(pulse_ctx, mode)
}

/// Mirrors IMAS-Core's `al_begin_global_action` exactly and forwards
/// unchanged. `dataobjectname` and `datapath` are seam arguments: this
/// ticket forwards them verbatim, DD path translation is future work.
///
/// # Safety
/// `dataobjectname` and `datapath` must be valid, NUL-terminated C
/// strings, or null where IMAS-Core's own contract allows it. `octxID`
/// must be a valid, writable `*mut c_int`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_begin_global_action(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    datapath: *const c_char,
    rwmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    unsafe { resolve::begin_global_action(pctx_id, dataobjectname, datapath, rwmode, octx_id) }
}

/// Mirrors IMAS-Core's `al_begin_slice_action` exactly and forwards
/// unchanged. `dataobjectname` is a seam argument: this ticket forwards it
/// verbatim, DD path translation is future work.
///
/// # Safety
/// `dataobjectname` must be a valid, NUL-terminated C string, or null
/// where IMAS-Core's own contract allows it. `octxID` must be a valid,
/// writable `*mut c_int`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_begin_slice_action(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    rwmode: c_int,
    time: c_double,
    interpmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    unsafe {
        resolve::begin_slice_action(pctx_id, dataobjectname, rwmode, time, interpmode, octx_id)
    }
}

/// Mirrors IMAS-Core's `al_begin_timerange_action` exactly and forwards
/// unchanged. `dataobjectname` is a seam argument: this ticket forwards it
/// verbatim, DD path translation is future work.
///
/// # Safety
/// `dataobjectname` must be a valid, NUL-terminated C string, or null
/// where IMAS-Core's own contract allows it. `dtime_buffer` and
/// `dtime_shape` must together describe a valid buffer, or be null/empty.
/// `octxID` must be a valid, writable `*mut c_int`.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_begin_timerange_action(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    rwmode: c_int,
    tmin: c_double,
    tmax: c_double,
    dtime_buffer: *const c_double,
    dtime_shape: *const c_int,
    interpmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    unsafe {
        resolve::begin_timerange_action(
            pctx_id,
            dataobjectname,
            rwmode,
            tmin,
            tmax,
            dtime_buffer,
            dtime_shape,
            interpmode,
            octx_id,
        )
    }
}

/// Mirrors IMAS-Core's `al_begin_arraystruct_action` exactly and forwards
/// unchanged. `path` and `timebase` are seam arguments: this ticket
/// forwards them verbatim, DD path translation is future work.
///
/// # Safety
/// `path` and `timebase` must be valid, NUL-terminated C strings, or null
/// where IMAS-Core's own contract allows it. `size` and `actxID` must be
/// valid, writable `*mut c_int`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_begin_arraystruct_action(
    ctx_id: c_int,
    path: *const c_char,
    timebase: *const c_char,
    size: *mut c_int,
    actx_id: *mut c_int,
) -> al_status_t {
    unsafe { resolve::begin_arraystruct_action(ctx_id, path, timebase, size, actx_id) }
}

/// Mirrors IMAS-Core's `al_end_action` exactly and forwards unchanged.
#[unsafe(no_mangle)]
pub extern "C" fn al_end_action(ctx_id: c_int) -> al_status_t {
    resolve::end_action(ctx_id)
}

/// Mirrors IMAS-Core's `al_read_data` exactly and forwards unchanged.
/// `field` and `timebase` are seam arguments: this ticket forwards them
/// verbatim, DD path translation is future work.
///
/// # Safety
/// `field` and `timebase` must be valid, NUL-terminated C strings, or null
/// where IMAS-Core's own contract allows it. `data` and `size` must be
/// valid, writable pointers, matching IMAS-Core's own contract for this
/// function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_read_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    unsafe { resolve::read_data(ctx_id, field, timebase, data, datatype, dim, size) }
}

/// Mirrors IMAS-Core's `al_write_data` exactly and forwards unchanged.
/// `field` and `timebase` are seam arguments: this ticket forwards them
/// verbatim, DD path translation is future work.
///
/// # Safety
/// `field` and `timebase` must be valid, NUL-terminated C strings, or null
/// where IMAS-Core's own contract allows it. `data` and `size` must be
/// valid pointers, matching IMAS-Core's own contract for this function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_write_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    unsafe { resolve::write_data(ctx_id, field, timebase, data, datatype, dim, size) }
}

/// Mirrors IMAS-Core's `al_delete_data` exactly and forwards unchanged.
/// `path` is a seam argument: this ticket forwards it verbatim, DD path
/// translation is future work.
///
/// # Safety
/// `path` must be a valid, NUL-terminated C string, or null where
/// IMAS-Core's own contract allows it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_delete_data(ctx: c_int, path: *const c_char) -> al_status_t {
    unsafe { resolve::delete_data(ctx, path) }
}

/// Mirrors IMAS-Core's `al_iterate_over_arraystruct` exactly and forwards
/// unchanged.
#[unsafe(no_mangle)]
pub extern "C" fn al_iterate_over_arraystruct(aosctx: c_int, step: c_int) -> al_status_t {
    resolve::iterate_over_arraystruct(aosctx, step)
}

/// Mirrors IMAS-Core's `al_get_occurrences` exactly and forwards
/// unchanged. `ids_name` is a seam argument: this ticket forwards it
/// verbatim, DD path translation is future work.
///
/// # Safety
/// `ids_name` must be a valid, NUL-terminated C string. `occurrences_list`
/// and `size` must be valid, writable pointers, matching IMAS-Core's own
/// contract for this function. Whether the caller must free
/// `*occurrences_list` is not stated in IMAS-Core's own documentation
/// (flagged, unresolved, in the functionality inventory) — this function
/// forwards it unexamined either way.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_get_occurrences(
    pctx_id: c_int,
    ids_name: *const c_char,
    occurrences_list: *mut *mut c_int,
    size: *mut c_int,
) -> al_status_t {
    unsafe { resolve::get_occurrences(pctx_id, ids_name, occurrences_list, size) }
}

/// Mirrors IMAS-Core's `al_list_filled_paths` exactly and forwards
/// unchanged. `dataobjectname` is a seam argument on the way down, and the
/// returned `*path_list` is the main up-conversion seam — this ticket
/// forwards both verbatim, DD path translation is future work.
///
/// # Safety
/// `dataobjectname` must be a valid, NUL-terminated C string. `path_list`
/// and `size` must be valid, writable pointers, matching IMAS-Core's own
/// contract for this function. On success the caller owns `*path_list` and
/// every string in it, per IMAS-Core's documented contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_list_filled_paths(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    path_list: *mut *mut *mut c_char,
    size: *mut c_int,
) -> al_status_t {
    unsafe { resolve::list_filled_paths(pctx_id, dataobjectname, path_list, size) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn version_matches_the_package() {
        let version = unsafe { CStr::from_ptr(imas_mvdd_loader_version()) };
        assert_eq!(version.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn status_clear_zeroes_a_dirty_status() {
        let mut status = al_status_t {
            code: 42,
            message: [b'x' as c_char; MAX_ERR_MSG_LEN],
        };
        unsafe { imas_mvdd_loader_status_clear(&raw mut status) };
        assert_eq!(status.code, 0);
        assert!(status.message.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn status_clear_tolerates_null() {
        unsafe { imas_mvdd_loader_status_clear(std::ptr::null_mut()) };
    }
}
