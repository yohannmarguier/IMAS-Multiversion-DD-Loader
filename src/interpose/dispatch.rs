use std::ffi::{c_char, c_double, c_int, c_void};

use crate::al_status_t;
use crate::core::core_binding::forward_status;

/// Which real ABI symbol family a shared seam call forwards through: an
/// ordinary HLI call (`al_begin_global_action`, `al_read_data`, ...) or its
/// plugin-reentry twin (`al_plugin_begin_global_action`,
/// `al_plugin_read_data`, ...). One `CallFamily` value now replaces the
/// forward/end-on-refusal closure pair that used to be rebuilt at each of the
/// nine seam-specific call sites, differing only in which symbol it named (issue
/// #109).
///
/// Bound over exactly the six symbols the two families share —
/// `al_begin_timerange_action` and `al_delete_data` have no plugin twin at
/// all (`lib.rs`'s manifest never resolves one), so those two seams take no
/// `CallFamily` parameter at all rather than carry an always-unused half.
///
/// This does **not** make "a context opened through one family must be closed
/// through that same family" unrepresentable. The calling binary — not this
/// type — chooses the family, by which ABI symbol it calls first; no Rust
/// type constrains that choice, and nothing here stops someone writing
/// `CallFamily::ORDINARY` where `family` was meant. What this *does*
/// guarantee: a family mismatch requires bypassing the `family` parameter
/// threaded through this module's shared policy functions, and a reviewer
/// sees that bypass, because [`call_begin_global`], [`call_begin_slice`],
/// [`call_begin_arraystruct`], [`call_read`], [`call_write`] and
/// [`call_end`] are the only six places the choice between symbols is ever
/// made — not nine independently-miswirable closures rebuilding it.
///
/// `ORDINARY` and `PLUGIN` are not independently resolved values: each of the
/// six dispatch functions below match on `CallFamily` to pick one of a pair
/// [`crate::core::core_binding`]'s manifest already resolves together (e.g.
/// `begin_global_action`/`plugin_begin_global_action`, both
/// `BeginGlobalActionFn`). A `CallFamily` value never holds a raw symbol
/// pointer itself — it only *names* which half of that manifest applies —
/// because `CoreBinding` sits behind a lazily-resolved `OnceLock`
/// ([`crate::core::core_binding::core`]) and several of the nine call sites refuse
/// before ever forwarding at all (`begin_arraystruct_action_impl`'s argument
/// resolution, `write_data_impl`'s mismatch check): eagerly extracting a
/// resolved fn pointer at `CallFamily` construction time would attempt
/// IMAS-Core resolution on a path that used to skip it entirely. Naming the
/// family and resolving `core()` at the point of the actual forward — the
/// same lazy timing `forward_status!` already used — keeps both properties.
#[derive(Clone, Copy)]
pub(super) enum CallFamily {
    Ordinary,
    Plugin,
}

impl CallFamily {
    /// The ordinary HLI call family: `al_begin_global_action`, `al_read_data`
    /// and their four siblings.
    pub(super) const ORDINARY: Self = Self::Ordinary;
    /// The plugin-reentry call family: `al_plugin_begin_global_action`,
    /// `al_plugin_read_data` and their four siblings.
    pub(super) const PLUGIN: Self = Self::Plugin;
}

/// Forwards to `al_begin_global_action` or `al_plugin_begin_global_action`,
/// chosen by `family`.
pub(super) fn call_begin_global(
    family: CallFamily,
    pctx_id: c_int,
    dataobjectname: *const c_char,
    datapath: *const c_char,
    rwmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    match family {
        CallFamily::Ordinary => forward_status!(begin_global_action(
            pctx_id,
            dataobjectname,
            datapath,
            rwmode,
            octx_id,
        )),
        CallFamily::Plugin => forward_status!(plugin_begin_global_action(
            pctx_id,
            dataobjectname,
            datapath,
            rwmode,
            octx_id,
        )),
    }
}

/// Forwards to `al_begin_slice_action` or `al_plugin_begin_slice_action`,
/// chosen by `family`.
pub(super) fn call_begin_slice(
    family: CallFamily,
    pctx_id: c_int,
    dataobjectname: *const c_char,
    rwmode: c_int,
    time: c_double,
    interpmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    match family {
        CallFamily::Ordinary => forward_status!(begin_slice_action(
            pctx_id,
            dataobjectname,
            rwmode,
            time,
            interpmode,
            octx_id,
        )),
        CallFamily::Plugin => forward_status!(plugin_begin_slice_action(
            pctx_id,
            dataobjectname,
            rwmode,
            time,
            interpmode,
            octx_id,
        )),
    }
}

/// Forwards to `al_begin_arraystruct_action` or
/// `al_plugin_begin_arraystruct_action`, chosen by `family`.
pub(super) fn call_begin_arraystruct(
    family: CallFamily,
    ctx_id: c_int,
    path: *const c_char,
    timebase: *const c_char,
    size: *mut c_int,
    actx_id: *mut c_int,
) -> al_status_t {
    match family {
        CallFamily::Ordinary => {
            forward_status!(begin_arraystruct_action(
                ctx_id, path, timebase, size, actx_id
            ))
        }
        CallFamily::Plugin => forward_status!(plugin_begin_arraystruct_action(
            ctx_id, path, timebase, size, actx_id,
        )),
    }
}

/// Forwards to `al_read_data` or `al_plugin_read_data`, chosen by `family`.
#[allow(clippy::too_many_arguments)]
pub(super) fn call_read(
    family: CallFamily,
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    match family {
        CallFamily::Ordinary => forward_status!(read_data(
            ctx_id, field, timebase, data, datatype, dim, size
        )),
        CallFamily::Plugin => forward_status!(plugin_read_data(
            ctx_id, field, timebase, data, datatype, dim, size
        )),
    }
}

/// Forwards to `al_write_data` or `al_plugin_write_data`, chosen by `family`.
#[allow(clippy::too_many_arguments)]
pub(super) fn call_write(
    family: CallFamily,
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    match family {
        CallFamily::Ordinary => forward_status!(write_data(
            ctx_id, field, timebase, data, datatype, dim, size,
        )),
        CallFamily::Plugin => forward_status!(plugin_write_data(
            ctx_id, field, timebase, data, datatype, dim, size,
        )),
    }
}

/// Forwards to `al_end_action` or `al_plugin_end_action`, chosen by `family`.
pub(super) fn call_end(family: CallFamily, ctx_id: c_int) -> al_status_t {
    match family {
        CallFamily::Ordinary => forward_status!(end_action(ctx_id)),
        CallFamily::Plugin => forward_status!(plugin_end_action(ctx_id)),
    }
}
