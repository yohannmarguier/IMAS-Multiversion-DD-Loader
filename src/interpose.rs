//! The C-facing interposition layer: one module per seam family.
//!
//! This file is now nothing but the map. Every seam body lives in a submodule
//! below, and the `pub(crate) use` list is the whole surface [`crate`] reaches
//! through `use interpose as resolve;`:
//!
//! | Module | Seams it owns |
//! |---|---|
//! | [`occurrence`] | `al_begin_dataentry_action`, the global/slice/timerange/arraystruct opening seams and their plugin twins, `al_end_action` — plus stamp discovery, root and child registration, and the conversion-map cache they share |
//! | [`read`] | `al_read_data` / `al_plugin_read_data` |
//! | [`mod@write`] | `al_write_data` / `al_plugin_write_data` |
//! | [`delete`] | `al_delete_data` |
//! | [`loss`] | the shim-owned `imas_mvdd_context_loss_*` exports |
//! | [`passthrough`] | the seams ADR 0002 leaves untranslated, and the verbatim forwards |
//! | [`dispatch`], [`reentry`], [`refusal`] | the machinery the seam modules share: ABI-symbol dispatch by [`dispatch::CallFamily`], the ADR 0014 depth gate [`reentry::ReentryGuard`], and the one refusal formatter |
//!
//! **The binding is elsewhere.** How IMAS-Core is found, version-checked and
//! called lives in [`crate::core::core_binding`], which enforces ADR 0001 and makes
//! no conversion decision; every forward in the modules above reaches IMAS-Core
//! through that module's
//! [`forward_status!`](crate::core::core_binding::forward_status) macro.
//! **What an HLI argument means once it is claimed by the conversion map is
//! also elsewhere.** [`crate::conversion::path_conversion`] is the one place that
//! interprets [`crate::conversion::conversion_map::Outcome`] into a concrete stored path
//! or a read plan; the modules below supply it a live
//! [`ConversionRecord`](crate::registry::context_registry::ConversionRecord) and a
//! raw argument, and perform the ABI-facing effects its decisions require:
//! Core calls, registry access, raw-pointer marshalling and depth gating.
//! Four ADRs are enforced at this boundary:
//!
//! - ADR 0002 — which seams translate, which refuse, and which forward
//!   unchanged; stamp discovery and root registration at the opening seams.
//! - ADR 0010 — read-path value transformations: one per rule, executed in
//!   place after the read.
//! - ADR 0012 — the three-way read outcome and the refusal/loss reporting
//!   channel, via [`crate::conversion::read_outcome`] and the registry's loss log.
//! - ADR 0014 — a seam arriving beneath an in-flight one is forwarded
//!   untouched, by call depth managed by [`reentry::ReentryGuard`].
//!
//! Issue #101 split this layer from `core_binding` and `seam_policy`, and
//! issue #153 split the layer itself into the modules above — both in a series
//! rather than one unreviewable change. The layer used to be called `resolve`,
//! a name that conflated resolving IMAS-Core symbols with resolving DD paths;
//! it is now named for its role at the C boundary.

mod delete;
mod dispatch;
mod loss;
mod occurrence;
mod passthrough;
mod read;
mod reentry;
mod refusal;
mod write;

pub(crate) use delete::delete_data;
pub(crate) use loss::{context_loss_at, context_loss_count, context_loss_operation_at};
pub(crate) use occurrence::{
    begin_arraystruct_action, begin_dataentry_action, begin_global_action, begin_slice_action,
    begin_timerange_action, end_action, plugin_begin_arraystruct_action,
    plugin_begin_global_action, plugin_begin_slice_action, plugin_end_action,
};
pub(crate) use passthrough::{
    bind_plugin, bind_readback_plugins, close_pulse, context_info, get_occurrences,
    is_plugin_registered, iterate_over_arraystruct, list_filled_paths, register_plugin,
    setvalue_double_scalar_parameter_plugin, setvalue_int_scalar_parameter_plugin,
    setvalue_parameter_plugin, unbind_plugin, unbind_readback_plugins, unregister_plugin,
    write_plugins_metadata,
};
pub(crate) use read::{plugin_read_data, read_data};
pub(crate) use write::{plugin_write_data, write_data};
