//! IMAS-Multiversion-DD-Loader — C ABI surface.
//!
//! This crate re-exports IMAS-Core's public C ABI verbatim and interposes on
//! the path-bearing entry points. The shared constants and `al_status_t` are
//! here, and the runtime-binding architecture (see `src/resolve.rs` and
//! `docs/adr/0001-runtime-binding-not-linking.md`) is proven end to end on
//! all 37 linkable exported IMAS-Core C symbols. DD path/version conversion
//! is wired for the data-entry and global-action seams (issue #53): the
//! process-wide HLI DD version latch (`src/hli_version.rs`, ADR 0005), the
//! context registry (`src/context_registry.rs`, ADR 0003), and DD-version
//! stamp discovery (`src/version_stamp.rs`, ADR 0007) together decide, per
//! `al_begin_global_action` open, whether an IDS occurrence's stored DD
//! version differs from the HLI's and registers a conversion record only
//! then. `al_begin_arraystruct_action` registers a child record when its own
//! context already carries one, and `al_read_data` resolves `field` through
//! that record's shared conversion map before forwarding the translated
//! field to IMAS-Core (issue #54) — proven bidirectionally against the
//! checked-in equilibrium fixture pair for an explicit `renamed`/identity
//! path with no value transformation and no `merged`/`split` candidates.
//! IMAS-Core's returned allocation is forwarded unchanged. A `merged`/`split`
//! read plan (issue #57), value-transform execution (issue #59), a renamed
//! AOS container (issue #61), independent `field`/`timebase` resolution
//! (issue #56), and slice/time-range discovery (issue #55) remain future
//! work under issue #43.

// The mirrored ABI dictates the names; matching IMAS-Core exactly is the point.
#![allow(non_camel_case_types)]

use std::ffi::c_char;
use std::ffi::c_double;
use std::ffi::c_int;
use std::ffi::c_void;

mod context_registry;
pub mod conversion_map;
mod dd_version;
mod dl;
mod hli_version;
mod known_artifacts;
mod read_outcome;
mod resolve;
mod version_stamp;

/// Length of `al_status_t::message`, mirroring IMAS-Core's `MAX_ERR_MSG_LEN`.
pub const MAX_ERR_MSG_LEN: usize = 256;

/// Maximum array rank accepted across the ABI, mirroring IMAS-Core's `MAXDIM`.
pub const MAXDIM: usize = 7;

/// Shim-owned refusal code (ADR 0010, ADR 0012): returned instead of an
/// IMAS-Core code whenever the shim itself refuses a call rather than
/// forwarding it. The shim reserves `-1000..=-1099` and allocates only this
/// value here; every other failure propagates IMAS-Core's own code unchanged.
pub const IMAS_MVDD_CONVERSION_ERROR: c_int = -1000;

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

/// Writes as much of `message` as fits in `buffer`, always leaving room for
/// the trailing NUL and never splitting a UTF-8 code point.
pub(crate) fn write_truncated(buffer: &mut [c_char; MAX_ERR_MSG_LEN], message: &str) {
    let capacity = MAX_ERR_MSG_LEN - 1; // always leave room for the NUL
    let mut len = message.len().min(capacity);
    while len > 0 && !message.is_char_boundary(len) {
        len -= 1;
    }
    for (slot, byte) in buffer.iter_mut().zip(message.as_bytes()[..len].iter()) {
        *slot = *byte as c_char;
    }
}

/// Builds a shim-originated refusal `al_status_t`: `IMAS_MVDD_CONVERSION_ERROR`
/// with a message prefixed `IMAS-MVDD:` (ADR 0010), truncated to fit the
/// fixed-size ABI buffer without ever panicking or overflowing.
pub(crate) fn conversion_refusal(reason: &str) -> al_status_t {
    let mut status = al_status_t {
        code: IMAS_MVDD_CONVERSION_ERROR,
        message: [0; MAX_ERR_MSG_LEN],
    };
    write_truncated(&mut status.message, &format!("IMAS-MVDD: {reason}"));
    status
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

/// Mirrors IMAS-Core's `al_get_backendID` exactly and forwards unchanged.
///
/// # Safety
/// `backend_id` must be a valid, writable `*mut c_int`, matching IMAS-Core's
/// own contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_get_backendID(ctx: c_int, backend_id: *mut c_int) -> al_status_t {
    unsafe { resolve::get_backend_id(ctx, backend_id) }
}

/// Mirrors IMAS-Core's legacy URI builder exactly and forwards unchanged.
///
/// # Safety
/// Every string must be a valid, NUL-terminated C string, and `uri` must be
/// a valid, writable `*mut *mut c_char`, matching IMAS-Core's own contract.
#[allow(clippy::too_many_arguments)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_build_uri_from_legacy_parameters(
    backend_id: c_int,
    pulse: c_int,
    run: c_int,
    user: *const c_char,
    tokamak: *const c_char,
    version: *const c_char,
    options: *const c_char,
    uri: *mut *mut c_char,
) -> al_status_t {
    unsafe {
        resolve::build_uri_from_legacy_parameters(
            backend_id, pulse, run, user, tokamak, version, options, uri,
        )
    }
}

/// Mirrors IMAS-Core's constant-to-string helper exactly and forwards
/// unchanged. On a major-version mismatch, the shim supplies IMAS-Core's
/// pinned lookup table instead of querying the incompatible library further.
/// It returns null only if IMAS-Core could not be opened or bootstrapped.
#[unsafe(no_mangle)]
pub extern "C" fn const2str(id: c_int) -> *const c_char {
    resolve::const2str(id)
}

/// Mirrors IMAS-Core's error-code-to-string helper exactly and forwards
/// unchanged. On a major-version mismatch, the shim supplies IMAS-Core's
/// pinned lookup table instead of querying the incompatible library further.
/// It returns null only if IMAS-Core could not be opened or bootstrapped.
#[unsafe(no_mangle)]
pub extern "C" fn err2str(id: c_int) -> *const c_char {
    resolve::err2str(id)
}

/// Mirrors IMAS-Core's access-layer version accessor exactly and forwards
/// unchanged. On a major-version mismatch it returns the bootstrap version;
/// it returns null only if IMAS-Core could not be opened or bootstrapped.
#[unsafe(no_mangle)]
pub extern "C" fn getALVersion() -> *const c_char {
    resolve::get_al_version()
}

/// Mirrors IMAS-Core's deliberately deprecated DD-version accessor exactly.
/// Its sentinel `"!!DEPRECATED!!"` is forwarded rather than replaced with a
/// version inferred by the shim. On a major-version mismatch, the shim
/// returns that fixed sentinel without querying the incompatible library
/// further. It returns null only if IMAS-Core could not be opened or
/// bootstrapped.
#[unsafe(no_mangle)]
pub extern "C" fn getDDVersion() -> *const c_char {
    resolve::get_dd_version()
}

/// Shim-owned export (ADR 0005) — the `imas_mvdd_` prefix marks it as a
/// symbol this project defines rather than mirrors from IMAS-Core, and it is
/// listed explicitly on the export-drift check's owned-exports manifest
/// (`tests/owned_exports.def`). Reports the calling HLI's process-wide DD
/// version once, before any open. The value latches on first use for the
/// life of the process: an identical repeat is accepted, a conflicting
/// repeat is refused naming both versions, and the call is safe from any
/// thread. A version arriving after the process already latched to unset —
/// an earlier open with no setter call and no valid
/// `IMAS_MVDD_HLI_DD_VERSION` — is refused too. An invalid version string
/// fails immediately and never touches the latch.
///
/// # Safety
/// `version` must be a valid, NUL-terminated C string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imas_mvdd_set_hli_dd_version(version: *const c_char) -> al_status_t {
    unsafe { hli_version::set_from_c(version) }
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

/// Mirrors IMAS-Core's `al_begin_slice_action` exactly and applies the same
/// stored-version discovery and occurrence-registration rule as
/// `al_begin_global_action` (issue #55). `dataobjectname` is always
/// forwarded unchanged — IDS names are stable across DD versions.
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

/// Mirrors IMAS-Core's `al_begin_timerange_action` exactly and applies the
/// same stored-version discovery and occurrence-registration rule as
/// `al_begin_global_action` (issue #55). `dataobjectname` is always
/// forwarded unchanged — IDS names are stable across DD versions.
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
/// `path` and `timebase` verbatim — translating an AOS container that was
/// itself renamed between DD versions is future work (issue #61). When this
/// context already carries a conversion record, the opened `actxID` is
/// registered as its child (issue #54), so a later `al_read_data` on it can
/// translate its own relative fields.
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

// Deliberately no `al_begin_array_struct_action`: IMAS-Core exports the
// spelling above (without the second underscore) and has never declared or
// exported this compatibility alias. Adding it would make the shim's ABI
// surface differ from IMAS-Core's (issue #8).

/// Mirrors IMAS-Core's `al_end_action` exactly and forwards unchanged.
#[unsafe(no_mangle)]
pub extern "C" fn al_end_action(ctx_id: c_int) -> al_status_t {
    resolve::end_action(ctx_id)
}

/// Mirrors IMAS-Core's `al_read_data` exactly. When `ctxID` carries no live
/// conversion record, this is a plain forward, unchanged from before issue
/// #54. Otherwise `field` is resolved through the record's conversion map
/// and translated to the stored spelling before IMAS-Core is called — for an
/// explicit `renamed`/identity outcome with no value transformation and no
/// `merged`/`split` candidates; every other outcome refuses rather than
/// guess (see `src/resolve.rs`). `timebase` is always forwarded unchanged
/// (issue #56). IMAS-Core's returned allocation is forwarded exactly as
/// received: the shim neither substitutes nor frees it.
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

// Plugin registration and configuration deliberately forward without guard
// rails. In particular, IMAS-Core's three parameter setters null-dereference
// on an unregistered plugin name; fixing that here would make the shim's ABI
// behaviour differ from the Core it mirrors (issue #7).

/// Mirrors IMAS-Core's `al_register_plugin` exactly and forwards unchanged.
/// IMAS-Core's failed plugin-library `dlopen` can proceed to a crash in an
/// `NDEBUG` build; that upstream defect is preserved by forwarding unchanged
/// (issue #7).
///
/// # Safety
/// `plugin_name` must be a valid, NUL-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_register_plugin(plugin_name: *const c_char) -> al_status_t {
    unsafe { resolve::register_plugin(plugin_name) }
}

/// Mirrors IMAS-Core's `al_unregister_plugin` exactly and forwards unchanged.
/// IMAS-Core only removes a currently-bound plugin; the shim preserves that
/// upstream defect by forwarding without intervention (issue #7).
///
/// # Safety
/// `plugin_name` must be a valid, NUL-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_unregister_plugin(plugin_name: *const c_char) -> al_status_t {
    unsafe { resolve::unregister_plugin(plugin_name) }
}

/// Mirrors IMAS-Core's `al_bind_plugin` exactly and forwards unchanged.
/// `field_path` is a seam argument: this ticket forwards it verbatim, DD path
/// translation is future work.
///
/// # Safety
/// `field_path` and `plugin_name` must be valid, NUL-terminated C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_bind_plugin(
    field_path: *const c_char,
    plugin_name: *const c_char,
) -> al_status_t {
    unsafe { resolve::bind_plugin(field_path, plugin_name) }
}

/// Mirrors IMAS-Core's `al_unbind_plugin` exactly and forwards unchanged.
/// `field_path` is a seam argument: this ticket forwards it verbatim, DD path
/// translation is future work.
/// Its silent no-op for an unbound path is an upstream behaviour preserved by
/// this thin forwarding layer (issue #7).
///
/// # Safety
/// `field_path` and `plugin_name` must be valid, NUL-terminated C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_unbind_plugin(
    field_path: *const c_char,
    plugin_name: *const c_char,
) -> al_status_t {
    unsafe { resolve::unbind_plugin(field_path, plugin_name) }
}

/// Mirrors IMAS-Core's `al_bind_readback_plugins` exactly and forwards unchanged.
#[unsafe(no_mangle)]
pub extern "C" fn al_bind_readback_plugins(ctx_id: c_int) -> al_status_t {
    resolve::bind_readback_plugins(ctx_id)
}

/// Mirrors IMAS-Core's `al_unbind_readback_plugins` exactly and forwards unchanged.
#[unsafe(no_mangle)]
pub extern "C" fn al_unbind_readback_plugins(ctx_id: c_int) -> al_status_t {
    resolve::unbind_readback_plugins(ctx_id)
}

/// Mirrors IMAS-Core's `al_is_plugin_registered` exactly and forwards unchanged.
///
/// # Safety
/// `plugin_name` must be a valid, NUL-terminated C string and `is_registered`
/// must be a valid, writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_is_plugin_registered(
    plugin_name: *const c_char,
    is_registered: *mut bool,
) -> al_status_t {
    unsafe { resolve::is_plugin_registered(plugin_name, is_registered) }
}

/// Mirrors IMAS-Core's `al_write_plugins_metadata` exactly and forwards unchanged.
#[unsafe(no_mangle)]
pub extern "C" fn al_write_plugins_metadata(ctx_id: c_int) -> al_status_t {
    resolve::write_plugins_metadata(ctx_id)
}

/// Mirrors IMAS-Core's generic plugin parameter setter exactly and forwards unchanged.
///
/// # Safety
/// All pointers must meet IMAS-Core's parameter-setter contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_setvalue_parameter_plugin(
    parameter_name: *const c_char,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
    data: *mut c_void,
    plugin_name: *const c_char,
) -> al_status_t {
    unsafe {
        resolve::setvalue_parameter_plugin(parameter_name, datatype, dim, size, data, plugin_name)
    }
}

/// Mirrors IMAS-Core's integer plugin parameter setter exactly and forwards unchanged.
///
/// # Safety
/// `parameter_name` and `plugin_name` must be valid, NUL-terminated C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_setvalue_int_scalar_parameter_plugin(
    parameter_name: *const c_char,
    parameter_value: c_int,
    plugin_name: *const c_char,
) -> al_status_t {
    unsafe {
        resolve::setvalue_int_scalar_parameter_plugin(parameter_name, parameter_value, plugin_name)
    }
}

/// Mirrors IMAS-Core's double plugin parameter setter exactly and forwards unchanged.
///
/// # Safety
/// `parameter_name` and `plugin_name` must be valid, NUL-terminated C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_setvalue_double_scalar_parameter_plugin(
    parameter_name: *const c_char,
    parameter_value: c_double,
    plugin_name: *const c_char,
) -> al_status_t {
    unsafe {
        resolve::setvalue_double_scalar_parameter_plugin(
            parameter_name,
            parameter_value,
            plugin_name,
        )
    }
}

// Deliberately no `al_plugin_begin_timerange_action`: IMAS-Core's public
// header declares a plain-C symbol with `const double *dtime_shape`, but its
// implementation takes `const int *` and therefore exports only a C++-mangled
// symbol. Exporting it here would let a plugin compile against the shim yet
// fail to link against real IMAS-Core (issue #7).

// The plugin reentry twins below carry the same path arguments as their
// non-`al_plugin_` counterparts (CLAUDE.md's seam table), so each is a seam
// too — a plugin re-entering the ABI must get the same translation an HLI
// does, or the two would disagree about which DD version a path is written in.

/// Mirrors IMAS-Core's plugin reentry global-action function exactly.
/// `dataobjectname` and `datapath` are seam arguments: this ticket forwards
/// them verbatim, DD path translation is future work.
///
/// # Safety
/// String and output pointers must meet IMAS-Core's action-lifecycle contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_plugin_begin_global_action(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    datapath: *const c_char,
    rwmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    unsafe {
        resolve::plugin_begin_global_action(pctx_id, dataobjectname, datapath, rwmode, octx_id)
    }
}

/// Mirrors IMAS-Core's plugin reentry slice-action function exactly.
/// `dataobjectname` is a seam argument: this ticket forwards it verbatim, DD
/// path translation is future work.
///
/// # Safety
/// String and output pointers must meet IMAS-Core's action-lifecycle contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_plugin_begin_slice_action(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    rwmode: c_int,
    time: c_double,
    interpmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    unsafe {
        resolve::plugin_begin_slice_action(
            pctx_id,
            dataobjectname,
            rwmode,
            time,
            interpmode,
            octx_id,
        )
    }
}

/// Mirrors IMAS-Core's plugin reentry arraystruct-action function exactly.
/// `path` and `timebase` are seam arguments: this ticket forwards them
/// verbatim, DD path translation is future work.
///
/// # Safety
/// String and output pointers must meet IMAS-Core's action-lifecycle contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_plugin_begin_arraystruct_action(
    ctx_id: c_int,
    path: *const c_char,
    timebase: *const c_char,
    size: *mut c_int,
    actx_id: *mut c_int,
) -> al_status_t {
    unsafe { resolve::plugin_begin_arraystruct_action(ctx_id, path, timebase, size, actx_id) }
}

/// Mirrors IMAS-Core's plugin reentry end-action function exactly.
#[unsafe(no_mangle)]
pub extern "C" fn al_plugin_end_action(ctx_id: c_int) -> al_status_t {
    resolve::plugin_end_action(ctx_id)
}

/// Mirrors IMAS-Core's plugin reentry read-data function exactly.
/// `field` and `timebase` are seam arguments: this ticket forwards them
/// verbatim, DD path translation is future work.
///
/// # Safety
/// All pointers must meet IMAS-Core's data-access contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_plugin_read_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    unsafe { resolve::plugin_read_data(ctx_id, field, timebase, data, datatype, dim, size) }
}

/// Mirrors IMAS-Core's plugin reentry write-data function exactly.
/// `field` and `timebase` are seam arguments: this ticket forwards them
/// verbatim, DD path translation is future work.
///
/// # Safety
/// All pointers must meet IMAS-Core's data-access contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn al_plugin_write_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    unsafe { resolve::plugin_write_data(ctx_id, field, timebase, data, datatype, dim, size) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_default_is_success() {
        assert_eq!(al_status_t::default().code, 0);
    }
}
