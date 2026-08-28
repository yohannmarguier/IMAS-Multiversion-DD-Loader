//! The seams that forward unchanged.
//!
//! Two kinds, deliberately together. `al_get_occurrences`,
//! `al_list_filled_paths` and the plugin bind/unbind pair carry DD paths and
//! are left untranslated *by decision* (ADR 0002) — passthrough is their
//! policy, not the absence of one. The rest — utility accessors, plugin
//! registration, metadata and parameter setters — have no path to translate.
//! Neither kind touches the context registry.

use std::ffi::{c_char, c_double, c_int, c_void};

use super::reentry::ReentryGuard;
use crate::al_status_t;
use crate::core::core_binding::forward_status;

/// Forwards to IMAS-Core's real `al_context_info`, resolving IMAS-Core
/// lazily on first use.
///
/// # Safety
/// `info` must be a valid, writable `*mut *mut c_char`, or null, matching
/// IMAS-Core's own contract for this function.
pub(crate) unsafe fn context_info(ctx: c_int, info: *mut *mut c_char) -> al_status_t {
    forward_status!(context_info(ctx, info))
}

/// Forwards to IMAS-Core's real `al_close_pulse`, resolving IMAS-Core
/// lazily on first use.
pub(crate) fn close_pulse(pulse_ctx: c_int, mode: c_int) -> al_status_t {
    forward_status!(close_pulse(pulse_ctx, mode))
}

/// Forwards to IMAS-Core's real `al_iterate_over_arraystruct`, resolving
/// IMAS-Core lazily on first use.
pub(crate) fn iterate_over_arraystruct(aosctx: c_int, step: c_int) -> al_status_t {
    forward_status!(iterate_over_arraystruct(aosctx, step))
}

/// Forwards to IMAS-Core's real `al_get_occurrences`, resolving IMAS-Core
/// lazily on first use.
///
/// # Safety
/// `ids_name` must be a valid, NUL-terminated C string. `occurrences_list`
/// and `size` must be valid, writable pointers, matching IMAS-Core's own
/// contract for this function.
pub(crate) unsafe fn get_occurrences(
    pctx_id: c_int,
    ids_name: *const c_char,
    occurrences_list: *mut *mut c_int,
    size: *mut c_int,
) -> al_status_t {
    forward_status!(get_occurrences(pctx_id, ids_name, occurrences_list, size,))
}

/// Forwards to IMAS-Core's real `al_list_filled_paths`, resolving
/// IMAS-Core lazily on first use.
///
/// # Safety
/// `dataobjectname` must be a valid, NUL-terminated C string. `path_list`
/// and `size` must be valid, writable pointers, matching IMAS-Core's own
/// contract for this function.
pub(crate) unsafe fn list_filled_paths(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    path_list: *mut *mut *mut c_char,
    size: *mut c_int,
) -> al_status_t {
    forward_status!(list_filled_paths(pctx_id, dataobjectname, path_list, size,))
}

pub(crate) unsafe fn register_plugin(plugin_name: *const c_char) -> al_status_t {
    forward_status!(register_plugin(plugin_name))
}

pub(crate) unsafe fn unregister_plugin(plugin_name: *const c_char) -> al_status_t {
    forward_status!(unregister_plugin(plugin_name))
}

pub(crate) unsafe fn bind_plugin(
    field_path: *const c_char,
    plugin_name: *const c_char,
) -> al_status_t {
    forward_status!(bind_plugin(field_path, plugin_name))
}

pub(crate) unsafe fn unbind_plugin(
    field_path: *const c_char,
    plugin_name: *const c_char,
) -> al_status_t {
    forward_status!(unbind_plugin(field_path, plugin_name))
}

pub(crate) fn bind_readback_plugins(ctx_id: c_int) -> al_status_t {
    let (_reentry_guard, _already_entered) = ReentryGuard::enter();
    forward_status!(bind_readback_plugins(ctx_id))
}

pub(crate) fn unbind_readback_plugins(ctx_id: c_int) -> al_status_t {
    let (_reentry_guard, _already_entered) = ReentryGuard::enter();
    forward_status!(unbind_readback_plugins(ctx_id))
}

pub(crate) unsafe fn is_plugin_registered(
    plugin_name: *const c_char,
    is_registered: *mut bool,
) -> al_status_t {
    forward_status!(is_plugin_registered(plugin_name, is_registered))
}

pub(crate) fn write_plugins_metadata(ctx_id: c_int) -> al_status_t {
    let (_reentry_guard, _already_entered) = ReentryGuard::enter();
    forward_status!(write_plugins_metadata(ctx_id))
}

pub(crate) unsafe fn setvalue_parameter_plugin(
    parameter_name: *const c_char,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
    data: *mut c_void,
    plugin_name: *const c_char,
) -> al_status_t {
    forward_status!(setvalue_parameter_plugin(
        parameter_name,
        datatype,
        dim,
        size,
        data,
        plugin_name,
    ))
}

pub(crate) unsafe fn setvalue_int_scalar_parameter_plugin(
    parameter_name: *const c_char,
    parameter_value: c_int,
    plugin_name: *const c_char,
) -> al_status_t {
    forward_status!(setvalue_int_scalar_parameter_plugin(
        parameter_name,
        parameter_value,
        plugin_name,
    ))
}

pub(crate) unsafe fn setvalue_double_scalar_parameter_plugin(
    parameter_name: *const c_char,
    parameter_value: c_double,
    plugin_name: *const c_char,
) -> al_status_t {
    forward_status!(setvalue_double_scalar_parameter_plugin(
        parameter_name,
        parameter_value,
        plugin_name,
    ))
}
