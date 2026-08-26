//! The interposition that carries out each IMAS-Core seam policy.
//!
//! **The binding is elsewhere.** How IMAS-Core is found, version-checked and
//! called lives in [`crate::core::core_binding`], which enforces ADR 0001 and makes
//! no conversion decision; every forward below reaches IMAS-Core through that
//! module's [`forward_status!`](crate::core::core_binding::forward_status) macro.
//! **What an HLI argument means once it is claimed by the conversion map is
//! also elsewhere.** [`crate::conversion::path_conversion`] is the one place that
//! interprets [`crate::conversion::conversion_map::Outcome`] into a concrete stored path
//! or a read plan; this file supplies it a live [`ConversionRecord`] and a
//! raw argument, and performs the ABI-facing effects its decisions require:
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
//!   untouched, by call depth (see [`SHIM_REENTRY_DEPTH`] and [`ReentryGuard`]).
//!
//! Issue #101 split this layer from `core_binding` and `seam_policy` in a
//! series rather than one unreviewable change. The layer used to be called
//! `resolve`, a name that conflated resolving IMAS-Core symbols with resolving
//! DD paths; it is now named for its role at the C boundary.

use std::cell::Cell;
use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};

use crate::conversion::conversion_map::{ConversionMap, Fidelity};
use crate::conversion::known_artifacts;
use crate::conversion::path_conversion::{self, ContextPathResolution};
use crate::conversion::read_outcome::{self, ReadOutcome};
use crate::conversion::seam_policy;
use crate::core::core_binding::{
    COMPLEX_DATA_ID, DOUBLE_DATA_ID, INTEGER_DATA_ID, READ_OP_ID, forward_status,
};
use crate::registry::context_registry::{ConversionRecord, MapCacheKey, REGISTRY};
use crate::version::version_stamp;
use crate::{al_status_t, write_truncated};

thread_local! {
    /// How many guarded shim seams this thread is currently inside (ADR 0014).
    /// Only ever read through [`ReentryGuard`]; a thread-local rather than a
    /// global because the depth describes one call stack, and ADR 0003 already
    /// puts concurrent use of a single IMAS-Core context out of scope.
    static SHIM_REENTRY_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Raises the thread's shim-seam depth for as long as a guarded seam is on the
/// stack, so a call that arrives *underneath* an in-flight IMAS-Core call can
/// recognise itself as reentrant (ADR 0014). The guard wraps the forwarded
/// call too, not just any conversion policy around it — the reentrant call
/// happens inside that call.
struct ReentryGuard;

impl ReentryGuard {
    /// Enters a guarded seam, reporting whether one was already in flight on this
    /// thread.
    fn enter() -> (Self, bool) {
        let already_entered = SHIM_REENTRY_DEPTH.with(|depth| {
            let entered = depth.get();
            depth.set(entered + 1);
            entered > 0
        });
        (Self, already_entered)
    }
}

impl Drop for ReentryGuard {
    fn drop(&mut self) {
        SHIM_REENTRY_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Which real ABI symbol family a shared seam call forwards through: an
/// ordinary HLI call (`al_begin_global_action`, `al_read_data`, ...) or its
/// plugin-reentry twin (`al_plugin_begin_global_action`,
/// `al_plugin_read_data`, ...). One `CallFamily` value now replaces the
/// forward/end-on-refusal closure pair that used to be rebuilt at each of the
/// nine call sites below, differing only in which symbol it named (issue
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
/// six dispatch functions above matches on `CallFamily` to pick one of a pair
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
enum CallFamily {
    Ordinary,
    Plugin,
}

impl CallFamily {
    /// The ordinary HLI call family: `al_begin_global_action`, `al_read_data`
    /// and their four siblings.
    const ORDINARY: Self = Self::Ordinary;
    /// The plugin-reentry call family: `al_plugin_begin_global_action`,
    /// `al_plugin_read_data` and their four siblings.
    const PLUGIN: Self = Self::Plugin;
}

/// Forwards to `al_begin_global_action` or `al_plugin_begin_global_action`,
/// chosen by `family`.
fn call_begin_global(
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
fn call_begin_slice(
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
fn call_begin_arraystruct(
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
        CallFamily::Plugin => {
            forward_status!(plugin_begin_arraystruct_action(
                ctx_id, path, timebase, size, actx_id
            ))
        }
    }
}

/// Forwards to `al_read_data` or `al_plugin_read_data`, chosen by `family`.
#[allow(clippy::too_many_arguments)]
fn call_read(
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

/// Calls IMAS-Core's ordinary read symbol without applying conversion policy.
/// Internal readers enter the reentry guard so an IMAS-Core callback knows the
/// path in flight is already in the stored DD spelling.
#[allow(clippy::too_many_arguments)]
unsafe fn read_data_unconverted(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    let (_reentry_guard, _already_entered) = ReentryGuard::enter();
    forward_status!(read_data(
        ctx_id, field, timebase, data, datatype, dim, size
    ))
}

/// Forwards to `al_write_data` or `al_plugin_write_data`, chosen by `family`.
#[allow(clippy::too_many_arguments)]
fn call_write(
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
fn call_end(family: CallFamily, ctx_id: c_int) -> al_status_t {
    match family {
        CallFamily::Ordinary => forward_status!(end_action(ctx_id)),
        CallFamily::Plugin => forward_status!(plugin_end_action(ctx_id)),
    }
}

/// The result of one occurrence-opening adapter call. A malformed stamp is
/// deliberately returned rather than ended inside policy: only the ABI
/// wrapper knows which action family opened the context and therefore which
/// end-action symbol must close it.
enum OpenOccurrenceResult {
    Status(al_status_t),
    RefuseAndEnd {
        opened_ctx_id: c_int,
        status: al_status_t,
    },
}

/// Forwards to IMAS-Core's real `al_context_info`, resolving IMAS-Core
/// lazily on first use.
///
/// # Safety
/// `info` must be a valid, writable `*mut *mut c_char`, or null, matching
/// IMAS-Core's own contract for this function.
pub(crate) unsafe fn context_info(ctx: c_int, info: *mut *mut c_char) -> al_status_t {
    forward_status!(context_info(ctx, info))
}

/// Forwards to IMAS-Core's real `al_begin_dataentry_action`, resolving
/// IMAS-Core lazily on first use.
///
/// Opening a pulse is the earliest action any HLI performs, so this is
/// where the process-wide HLI DD version latch resolves for the first time
/// if the setter was never called (ADR 0005): the environment variable or
/// the unset state settles here, atomically, for the rest of the process.
/// An invalid environment value refuses the call before IMAS-Core is ever
/// reached.
///
/// `uri` and `mode` are forwarded unchanged in every case (ADR 0002: this
/// seam has no DD version of its own to translate against). On success the
/// resulting pulse context is registered in the context registry so that
/// operation records opened beneath it can carry its ID as their pulse
/// context ID (issue #53); a failed open registers nothing.
///
/// # Safety
/// `uri` must be a valid, NUL-terminated C string. `dectxID` must be a
/// valid, writable `*mut c_int`, matching IMAS-Core's own contract.
pub(crate) unsafe fn begin_dataentry_action(
    uri: *const c_char,
    mode: c_int,
    dectx_id: *mut c_int,
) -> al_status_t {
    if let Err(reason) = crate::version::hli_version::resolve_for_open() {
        return crate::conversion_refusal(&reason);
    }
    let status = forward_status!(begin_dataentry_action(uri, mode, dectx_id));
    if status.code == 0 {
        // SAFETY: IMAS-Core's own contract already relied on above requires
        // `dectx_id` to be a valid, writable pointer.
        let ctx_id = unsafe { *dectx_id };
        REGISTRY.record_dataentry(ctx_id);
    }
    status
}

/// Forwards to IMAS-Core's real `al_close_pulse`, resolving IMAS-Core
/// lazily on first use.
pub(crate) fn close_pulse(pulse_ctx: c_int, mode: c_int) -> al_status_t {
    forward_status!(close_pulse(pulse_ctx, mode))
}

/// Forwards to IMAS-Core's real `al_begin_global_action`, resolving
/// IMAS-Core lazily on first use, and applies ADR 0002's global-action seam
/// policy (issue #53) when the HLI DD version is latched. See
/// [`begin_global_action_seam`] for the shared policy this and
/// [`plugin_begin_global_action`] both carry out.
///
/// # Safety
/// `dataobjectname` and `datapath` must be valid, NUL-terminated C strings,
/// or null where IMAS-Core's own contract allows it. `octxID` must be a
/// valid, writable `*mut c_int`.
pub(crate) unsafe fn begin_global_action(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    datapath: *const c_char,
    rwmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    // SAFETY: same contract as `begin_global_action_seam`, already upheld by
    // this function's own `unsafe fn` contract.
    unsafe {
        begin_global_action_seam(
            CallFamily::ORDINARY,
            pctx_id,
            dataobjectname,
            datapath,
            rwmode,
            octx_id,
        )
    }
}

/// Mirrors [`begin_global_action`]'s policy exactly (issue #67): the same
/// occurrence-cache `datapath` translation on the way in, forwarded through
/// `al_plugin_begin_global_action` rather than `al_begin_global_action`, and
/// the same stored-version discovery and root-registration rule on success —
/// cleaned up through `al_plugin_end_action` rather than `al_end_action` on a
/// malformed-stamp refusal, since a context this seam opened must be closed
/// through its own reentry family.
///
/// # Safety
/// Same contract as [`begin_global_action`].
pub(crate) unsafe fn plugin_begin_global_action(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    datapath: *const c_char,
    rwmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    // SAFETY: same contract as `begin_global_action_seam`, already upheld by
    // this function's own `unsafe fn` contract.
    unsafe {
        begin_global_action_seam(
            CallFamily::PLUGIN,
            pctx_id,
            dataobjectname,
            datapath,
            rwmode,
            octx_id,
        )
    }
}

/// The policy shared by `begin_global_action` and `plugin_begin_global_action`
/// (issue #67, consolidated onto [`CallFamily`] by issue #109):
///
/// `dataobjectname` (the IDS name, plus occurrence) is always forwarded
/// unchanged — IDS names are stable across DD versions. `datapath` is
/// translated only when an *earlier* open of this same occurrence under this
/// pulse already found a stored-version mismatch this project has an
/// artifact for; on an occurrence's first use (or once found to match, or
/// found unstamped) it is forwarded unchanged, since the version that would
/// justify translating it is not yet known at the point IMAS-Core must be
/// called.
///
/// Once the real open succeeds, the occurrence's DD-version stamp is read
/// immediately (before this returns to the HLI) and classified through the
/// one read-outcome classifier ([`crate::read_outcome`]). A present,
/// malformed stamp is a hard refusal — the just-opened IMAS-Core context is
/// also ended first, through `family`'s own end-action symbol, so a refusal
/// here never leaks it. An absent stamp, or one that matches the HLI DD
/// version, registers nothing (ADR 0007): the occurrence is presumed to
/// match. A present, valid, *mismatched* stamp registers the root context,
/// but only when an artifact actually covers this IDS and version pair (ADR
/// 0011 decision 1) — otherwise this is treated exactly like an unknown
/// context, passthrough with no record.
///
/// When the HLI DD version is unset, this is a plain forward with none of
/// the above: no stamp read, no registry lookup, no rule resolution.
///
/// # Safety
/// Same contract as [`begin_global_action`], plus `octx_id` must be a valid,
/// writable `*mut c_int`.
unsafe fn begin_global_action_seam(
    family: CallFamily,
    pctx_id: c_int,
    dataobjectname: *const c_char,
    datapath: *const c_char,
    rwmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    let forward = |effective_datapath: Option<*const c_char>| {
        call_begin_global(
            family,
            pctx_id,
            dataobjectname,
            effective_datapath.expect("global action has datapath"),
            rwmode,
            octx_id,
        )
    };
    // SAFETY: same contract as `open_occurrence`, already upheld by
    // this function's own `unsafe fn` contract.
    match unsafe {
        open_occurrence(
            pctx_id,
            dataobjectname,
            Some(datapath),
            rwmode,
            octx_id,
            forward,
        )
    } {
        OpenOccurrenceResult::Status(status) => status,
        OpenOccurrenceResult::RefuseAndEnd {
            opened_ctx_id,
            status,
        } => {
            let _ = call_end(family, opened_ctx_id);
            status
        }
    }
}

/// The one interposition adapter shared by all occurrence-opening seams. It
/// optionally translates a global-action `datapath`, injects the raw stamp
/// reader the policy drives, and applies the registry effects the policy
/// returns. `forward` receives the effective `datapath`, or `None` for slice
/// and time-range actions, exactly once.
///
/// # Safety
/// Same contract as [`begin_global_action`]: `dataobjectname` and `datapath`
/// must be valid, NUL-terminated C strings, or null where IMAS-Core's own
/// contract allows it, and `octx_id` must be a valid, writable `*mut c_int`
/// once `forward` reports success.
unsafe fn open_occurrence(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    datapath: Option<*const c_char>,
    rwmode: c_int,
    octx_id: *mut c_int,
    forward: impl FnOnce(Option<*const c_char>) -> al_status_t,
) -> OpenOccurrenceResult {
    let Some(hli) = crate::version::hli_version::latched() else {
        return OpenOccurrenceResult::Status(forward(datapath));
    };

    let dataobjectname_str = c_str_or_none(dataobjectname);
    let ids_name = dataobjectname_str.map(ids_name_from);

    let mut translated_datapath: Option<CString> = None;
    if let (Some(dataobjectname_str), Some(ids_name)) = (dataobjectname_str, ids_name)
        && let Some(stored) = REGISTRY.known_stored_version(pctx_id, dataobjectname_str)
        && stored != hli
    {
        translated_datapath =
            translate_down(ids_name, &stored, &hli, datapath.and_then(c_str_or_none));
    }
    let effective_datapath = datapath.map(|original| {
        translated_datapath
            .as_deref()
            .map(CStr::as_ptr)
            .unwrap_or(original)
    });

    // A context the caller did not open for reading cannot be trusted to
    // answer the stamp question, so ask through one of our own instead --
    // and ask before the caller's own context exists, not after (ADR 0020).
    let probed_stamp = if rwmode != READ_OP_ID && ids_name.is_some() {
        // SAFETY: `dataobjectname` is non-null and a valid, NUL-terminated C
        // string -- `ids_name` was derived from it above and is `Some` on
        // exactly that condition.
        Some(unsafe { probe_stamp_through_a_read_context(pctx_id, dataobjectname) })
    } else {
        None
    };

    let status = forward(effective_datapath);
    if status.code != 0 {
        return OpenOccurrenceResult::Status(status);
    }

    // SAFETY: IMAS-Core's own contract requires `octx_id` to be a valid,
    // writable pointer, already relied on by the forwarded call above.
    let opened_octx_id = unsafe { *octx_id };
    let (Some(dataobjectname_str), Some(ids_name)) = (dataobjectname_str, ids_name) else {
        return OpenOccurrenceResult::Status(status);
    };
    let decision = seam_policy::decide_occurrence_registration(ids_name, &hli, || {
        probed_stamp.unwrap_or_else(|| discover_stamp(opened_octx_id))
    });
    apply_discovery_decision(
        pctx_id,
        dataobjectname_str,
        opened_octx_id,
        &hli,
        status,
        decision,
    )
}

/// Reads and classifies an occurrence's DD-version stamp through a shim-owned
/// context, opened and closed purely for discovery, for a seam whose caller
/// opened theirs under something other than `READ_OP` (ADR 0020).
///
/// Real IMAS-Core's HDF5 backend initializes the *reader's* per-IDS group only
/// under `READ_OP`; a `WRITE_OP` open initializes the writer's. The stamp read
/// this seam issues through the caller's own context therefore comes back
/// not-found rather than failing, ADR 0007 presumes the occurrence matches,
/// and every write through that context is an untranslated forward. A probe
/// asks the same question through a context that has a reader.
///
/// Three properties of the probe are deliberate:
///
/// - **It runs before the caller's own context exists.** Ending a `READ_OP`
///   context closes the pulse's per-IDS file handle (`HDF5Reader::
///   close_file_handler` sets the shared `opened_IDS_files` entry to `-1`),
///   which a caller's still-open write context would be holding. Probing
///   first, then forwarding, means the caller's own open re-establishes that
///   handle for itself.
/// - **It opens and closes through the plugin call family.** IMAS-Core's
///   `al_begin_global_action` is `al_plugin_begin_global_action` plus plugin
///   registration and binding; a context no HLI will ever see needs neither,
///   and a probe issued from inside a plugin callback must not re-enter the
///   plugin machinery that called it. The two are a matched open/close pair,
///   so this obeys the same family rule every other seam does.
/// - **Every failure is [`version_stamp::StampOutcome::Unstamped`].** A probe
///   that cannot be opened -- a backend that refuses a read-mode open, or an
///   occurrence that does not exist yet, which is the ordinary case for a
///   writer -- says nothing about the stored DD version, and ADR 0007 already
///   presumes a match in exactly that situation.
///
/// # Safety
/// `dataobjectname` must be a valid, NUL-terminated C string.
unsafe fn probe_stamp_through_a_read_context(
    pctx_id: c_int,
    dataobjectname: *const c_char,
) -> version_stamp::StampOutcome {
    let mut probe_ctx_id: c_int = 0;
    let status = call_begin_global(
        CallFamily::PLUGIN,
        pctx_id,
        dataobjectname,
        c"".as_ptr(),
        READ_OP_ID,
        &mut probe_ctx_id,
    );
    if status.code != 0 {
        return version_stamp::StampOutcome::Unstamped;
    }
    let outcome = discover_stamp(probe_ctx_id);
    // A probe that will not close leaves one IMAS-Core context behind, and
    // there is nothing better to do with that: the caller asked to open an
    // occurrence, not to clean up after the shim's own bookkeeping, so failing
    // their open over it would turn a leak into a denied open. ADR 0020
    // records the trade.
    let _ = call_end(CallFamily::PLUGIN, probe_ctx_id);
    outcome
}

/// Reads and classifies the DD-version stamp through `ctx_id`, whether that is
/// a context the caller opened or one the probe above opened for itself. The
/// injected reader is the real `al_read_data` with none of the conversion
/// policy an HLI-issued read carries (ADR 0014's closing section): this read
/// is what *decides* whether conversion applies, so it cannot be subject to
/// it.
fn discover_stamp(ctx_id: c_int) -> version_stamp::StampOutcome {
    version_stamp::discover(
        ctx_id,
        |ctx, field, timebase, data, datatype, dim, size| unsafe {
            read_data_unconverted(ctx, field, timebase, data, datatype, dim, size)
        },
    )
}

/// Performs the process-global effects a discovery decision returned after a
/// successful occurrence open. A malformed stamp clears the occurrence cache
/// and asks the wrapper to end its just-opened context through its matching
/// ABI family; an absent or matching stamp clears the cache; a mismatch
/// records its stored version and, when covered by an artifact, the root.
fn apply_discovery_decision(
    pctx_id: c_int,
    dataobjectname: &str,
    opened_ctx_id: c_int,
    hli: &crate::version::dd_version::DdVersion,
    status: al_status_t,
    decision: seam_policy::DiscoveryDecision,
) -> OpenOccurrenceResult {
    match decision {
        seam_policy::DiscoveryDecision::RefuseAndEnd {
            reason,
            occurrence_cache,
        } => {
            // A prior open may have cached a mismatch for this occurrence,
            // but this read gives no usable version to justify retaining it.
            // Never translate a later `datapath` from stale discovery state.
            apply_occurrence_cache_effect(pctx_id, dataobjectname, occurrence_cache);
            OpenOccurrenceResult::RefuseAndEnd {
                opened_ctx_id,
                status: *reason,
            }
        }
        seam_policy::DiscoveryDecision::RegisterNothing { occurrence_cache } => {
            apply_occurrence_cache_effect(pctx_id, dataobjectname, occurrence_cache);
            OpenOccurrenceResult::Status(status)
        }
        seam_policy::DiscoveryDecision::RegisterRoot {
            stored,
            artifact,
            occurrence_cache,
        } => {
            apply_occurrence_cache_effect(pctx_id, dataobjectname, occurrence_cache);
            let ids_name = ids_name_from(dataobjectname);
            let key = map_cache_key(ids_name, &stored, hli);
            let direction = artifact.direction_to_stored;
            // A global/slice/time-range action opens the whole IDS
            // occurrence, not one field: the record's resolved path is the
            // occurrence's own root, empty because a relative read resolves
            // against it directly (ADR 0002, ADR 0003).
            REGISTRY.record_root(
                opened_ctx_id,
                String::new(),
                pctx_id,
                key,
                direction,
                || load_artifact(&artifact),
            );
            OpenOccurrenceResult::Status(status)
        }
    }
}

/// Mechanically performs the occurrence-cache effect a seam-policy decision
/// returned. The policy chooses the write; this adapter only supplies the
/// pulse and occurrence identities the registry API requires.
fn apply_occurrence_cache_effect(
    pctx_id: c_int,
    dataobjectname: &str,
    effect: seam_policy::OccurrenceCacheEffect,
) {
    match effect {
        seam_policy::OccurrenceCacheEffect::Forget => {
            REGISTRY.forget_occurrence_version(pctx_id, dataobjectname);
        }
        seam_policy::OccurrenceCacheEffect::RememberMismatch(stored) => {
            REGISTRY.remember_mismatched_occurrence(pctx_id, dataobjectname.to_string(), stored);
        }
    }
}

/// The IDS name portion of a `dataobjectname` such as `"equilibrium"` or
/// `"equilibrium/3"` — occurrence numbers do not affect which conversion
/// map applies.
fn ids_name_from(dataobjectname: &str) -> &str {
    dataobjectname.split('/').next().unwrap_or(dataobjectname)
}

/// `ptr` as a borrowed `&str`, or `None` if it is null or not valid UTF-8.
fn c_str_or_none<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller's own contract requires `ptr`, when non-null, to be
    // a valid NUL-terminated C string.
    unsafe { CStr::from_ptr(ptr) }.to_str().ok()
}

/// Translates `path` from the HLI's own DD spelling to `stored`'s spelling
/// via the artifact this project has embedded for `(ids, stored, hli)`, if
/// any. Returns `None` — forward unchanged — when there is no such artifact,
/// `path` is absent or empty (nothing to translate), or no rule in the
/// artifact claims `path` at all: none of these is a basis to invent a
/// translation (ADR 0011).
fn translate_down(
    ids: &str,
    stored: &crate::version::dd_version::DdVersion,
    hli: &crate::version::dd_version::DdVersion,
    path: Option<&str>,
) -> Option<CString> {
    let path = path.filter(|p| !p.is_empty())?;
    let artifact = known_artifacts::lookup(ids, stored, hli)?;
    let key = map_cache_key(ids, stored, hli);
    let map = REGISTRY.get_or_create_map(key, || load_artifact(&artifact));
    let explanation = map.resolve(path, artifact.direction_to_stored)?;
    path_conversion::datapath_translation(explanation.outcome)
        .and_then(|resolved_path| CString::new(resolved_path).ok())
}

/// The `(IDS name, stored DD version, HLI DD version)` cache key both the
/// datapath-translation and root-registration call sites look their shared
/// conversion map up under.
fn map_cache_key(
    ids: &str,
    stored: &crate::version::dd_version::DdVersion,
    hli: &crate::version::dd_version::DdVersion,
) -> MapCacheKey {
    MapCacheKey::new(ids.to_string(), stored.clone(), hli.clone())
}

/// Parses the one embedded conversion-map artifact `artifact` names. Used
/// only as a `get_or_create_map`/`record_root` cache-miss closure, so this
/// runs at most once per `(IDS, stored, HLI)` key for as long as some record
/// still references the resulting map.
fn load_artifact(artifact: &known_artifacts::ArtifactMatch) -> ConversionMap {
    ConversionMap::load(artifact.xml).expect("embedded artifact must parse")
}

/// Forwards to IMAS-Core's real `al_begin_slice_action`, resolving
/// IMAS-Core lazily on first use. See [`begin_slice_action_seam`] for the
/// shared policy this and [`plugin_begin_slice_action`] both carry out.
///
/// # Safety
/// `dataobjectname` must be a valid, NUL-terminated C string, or null where
/// IMAS-Core's own contract allows it. `octxID` must be a valid, writable
/// `*mut c_int`.
pub(crate) unsafe fn begin_slice_action(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    rwmode: c_int,
    time: c_double,
    interpmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    // SAFETY: same contract as `begin_slice_action_seam`, already upheld by
    // this function's own `unsafe fn` contract.
    unsafe {
        begin_slice_action_seam(
            CallFamily::ORDINARY,
            pctx_id,
            dataobjectname,
            rwmode,
            time,
            interpmode,
            octx_id,
        )
    }
}

/// Mirrors [`begin_slice_action`]'s policy exactly (issue #67): the same
/// stored-version discovery and root-registration rule, forwarded through
/// `al_plugin_begin_slice_action` rather than `al_begin_slice_action` and
/// cleaned up through `al_plugin_end_action` on a malformed-stamp refusal.
///
/// # Safety
/// Same contract as [`begin_slice_action`].
pub(crate) unsafe fn plugin_begin_slice_action(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    rwmode: c_int,
    time: c_double,
    interpmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    // SAFETY: same contract as `begin_slice_action_seam`, already upheld by
    // this function's own `unsafe fn` contract.
    unsafe {
        begin_slice_action_seam(
            CallFamily::PLUGIN,
            pctx_id,
            dataobjectname,
            rwmode,
            time,
            interpmode,
            octx_id,
        )
    }
}

/// The policy shared by `begin_slice_action` and `plugin_begin_slice_action`
/// (issue #67, consolidated onto [`CallFamily`] by issue #109): the same
/// stored-version discovery and occurrence-registration rule as
/// [`begin_global_action_seam`] (ADR 0002, issue #55) when the HLI DD version
/// is latched. `dataobjectname` (the IDS name, plus occurrence) is always
/// forwarded unchanged — a slice action carries no `datapath` argument, so
/// there is nothing to translate on the way in.
///
/// When the HLI DD version is unset, this is a plain forward with no stamp
/// read, no registry lookup, no rule resolution.
///
/// # Safety
/// Same contract as [`begin_slice_action`].
unsafe fn begin_slice_action_seam(
    family: CallFamily,
    pctx_id: c_int,
    dataobjectname: *const c_char,
    rwmode: c_int,
    time: c_double,
    interpmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    let forward = |_: Option<*const c_char>| {
        call_begin_slice(
            family,
            pctx_id,
            dataobjectname,
            rwmode,
            time,
            interpmode,
            octx_id,
        )
    };
    // SAFETY: same contract as `open_occurrence`, already upheld by
    // this function's own `unsafe fn` contract.
    match unsafe { open_occurrence(pctx_id, dataobjectname, None, rwmode, octx_id, forward) } {
        OpenOccurrenceResult::Status(status) => status,
        OpenOccurrenceResult::RefuseAndEnd {
            opened_ctx_id,
            status,
        } => {
            let _ = call_end(family, opened_ctx_id);
            status
        }
    }
}

/// Forwards to IMAS-Core's real `al_begin_timerange_action`, resolving
/// IMAS-Core lazily on first use, and applies the same stored-version
/// discovery and occurrence-registration rule as `begin_global_action`
/// (ADR 0002, issue #55) when the HLI DD version is latched. `dataobjectname`
/// (the IDS name, plus occurrence) is always forwarded unchanged — a
/// time-range action carries no `datapath` argument, so there is nothing to
/// translate on the way in.
///
/// Unlike its five siblings, this seam takes no [`CallFamily`] parameter:
/// `al_plugin_begin_timerange_action` has a header/impl signature mismatch
/// upstream and is unlinkable (CLAUDE.md), so there is no plugin twin to
/// choose between and no family for this seam to carry (issue #109 AC2).
///
/// When the HLI DD version is unset, this is a plain forward with no stamp
/// read, no registry lookup, no rule resolution.
///
/// # Safety
/// `dataobjectname` must be a valid, NUL-terminated C string, or null where
/// IMAS-Core's own contract allows it. `dtime_buffer` and `dtime_shape`
/// must together describe a valid buffer, or be null/empty. `octxID` must
/// be a valid, writable `*mut c_int`.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn begin_timerange_action(
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
    let forward = || {
        forward_status!(begin_timerange_action(
            pctx_id,
            dataobjectname,
            rwmode,
            tmin,
            tmax,
            dtime_buffer,
            dtime_shape,
            interpmode,
            octx_id,
        ))
    };
    // SAFETY: same contract as `open_occurrence`, already upheld by
    // this function's own safety contract.
    match unsafe {
        open_occurrence(pctx_id, dataobjectname, None, rwmode, octx_id, |_| {
            forward()
        })
    } {
        OpenOccurrenceResult::Status(status) => status,
        OpenOccurrenceResult::RefuseAndEnd {
            opened_ctx_id,
            status,
        } => {
            let _ = forward_status!(end_action(opened_ctx_id));
            status
        }
    }
}

/// Forwards to IMAS-Core's real `al_begin_arraystruct_action`, resolving
/// `path` and `timebase` from a mismatched parent's HLI-DD spelling to its
/// stored-DD spelling before IMAS-Core is called. See
/// [`begin_arraystruct_action_impl`] for the shared policy this and
/// [`plugin_begin_arraystruct_action`] both carry out.
///
/// # Safety
/// `path` and `timebase` must be valid, NUL-terminated C strings, or null
/// where IMAS-Core's own contract allows it. `size` and `actxID` must be
/// valid, writable `*mut c_int`.
pub(crate) unsafe fn begin_arraystruct_action(
    ctx_id: c_int,
    path: *const c_char,
    timebase: *const c_char,
    size: *mut c_int,
    actx_id: *mut c_int,
) -> al_status_t {
    // SAFETY: same contract as `begin_arraystruct_action_impl`, already
    // upheld by this function's own `unsafe fn` contract.
    unsafe {
        begin_arraystruct_action_impl(CallFamily::ORDINARY, ctx_id, path, timebase, size, actx_id)
    }
}

/// Mirrors [`begin_arraystruct_action`]'s policy exactly (issue #67): the
/// same `path`/`timebase` resolution against the parent's conversion record
/// and the same child-record registration on success, forwarded through
/// `al_plugin_begin_arraystruct_action` rather than
/// `al_begin_arraystruct_action`.
///
/// # Safety
/// Same contract as [`begin_arraystruct_action`].
pub(crate) unsafe fn plugin_begin_arraystruct_action(
    ctx_id: c_int,
    path: *const c_char,
    timebase: *const c_char,
    size: *mut c_int,
    actx_id: *mut c_int,
) -> al_status_t {
    // SAFETY: same contract as `begin_arraystruct_action_impl`, already
    // upheld by this function's own `unsafe fn` contract.
    unsafe {
        begin_arraystruct_action_impl(CallFamily::PLUGIN, ctx_id, path, timebase, size, actx_id)
    }
}

/// The policy shared by `begin_arraystruct_action` and
/// `plugin_begin_arraystruct_action` (issue #67, consolidated onto
/// [`CallFamily`] by issue #109): the `path`/`timebase` resolution against
/// the parent's conversion record and the child-record registration on
/// success, factored out of both so only `family` differs between the
/// ordinary and plugin reentry seams.
///
/// # Safety
/// Same contract as [`begin_arraystruct_action`]: `path` and `timebase` must
/// be valid, NUL-terminated C strings, or null where IMAS-Core's own
/// contract allows it, and `actx_id` must be a valid, writable `*mut c_int`
/// once IMAS-Core reports success.
unsafe fn begin_arraystruct_action_impl(
    family: CallFamily,
    ctx_id: c_int,
    path: *const c_char,
    timebase: *const c_char,
    size: *mut c_int,
    actx_id: *mut c_int,
) -> al_status_t {
    let Some(parent) = live_conversion_record(ctx_id) else {
        return call_begin_arraystruct(family, ctx_id, path, timebase, size, actx_id);
    };

    let translated_path = match resolve_arraystruct_argument(&parent, path, "path") {
        Ok(path) => path,
        Err(message) => return contextual_refusal(&parent, &message, path),
    };
    let translated_timebase = match resolve_arraystruct_argument(&parent, timebase, "timebase") {
        Ok(resolved) => resolved,
        Err(message) => return contextual_refusal(&parent, &message, timebase),
    };

    let status = call_begin_arraystruct(
        family,
        ctx_id,
        translated_path.as_deref().map(CStr::as_ptr).unwrap_or(path),
        translated_timebase
            .as_deref()
            .map(CStr::as_ptr)
            .unwrap_or(timebase),
        size,
        actx_id,
    );
    if status.code == 0 {
        let resolved_path = path_conversion::join_hli_path(
            &parent.resolved_path,
            c_str_or_none(path).unwrap_or_default(),
        );
        // SAFETY: IMAS-Core's own contract, already relied on by the
        // forwarded call above, requires `actx_id` to be a valid, writable
        // pointer on success.
        let opened_actx_id = unsafe { *actx_id };
        REGISTRY.record_child(opened_actx_id, ctx_id, resolved_path);
    }
    status
}

/// Forwards to IMAS-Core's real `al_end_action`, resolving IMAS-Core
/// lazily on first use. See [`end_action_impl`] for the shared policy this
/// and [`plugin_end_action`] both carry out.
pub(crate) fn end_action(ctx_id: c_int) -> al_status_t {
    end_action_impl(CallFamily::ORDINARY, ctx_id)
}

/// Mirrors [`end_action`]'s policy exactly (issue #67): removes only
/// `ctx_id`'s own registry record, if any, and only once IMAS-Core's own
/// `al_plugin_end_action` reports success — a refused close leaves the
/// record intact, matching `end_action`'s rule for `al_end_action`.
pub(crate) fn plugin_end_action(ctx_id: c_int) -> al_status_t {
    end_action_impl(CallFamily::PLUGIN, ctx_id)
}

/// The policy shared by `end_action` and `plugin_end_action` (issue #67,
/// consolidated onto [`CallFamily`] by issue #109). On success, removes only
/// `ctx_id`'s own registry record, if any (ADR 0002, ADR 0003) — a parent
/// context never owns a child context's lifetime, and an unrecorded or
/// already-plain `ctx_id` removal is a harmless no-op.
fn end_action_impl(family: CallFamily, ctx_id: c_int) -> al_status_t {
    let status = call_end(family, ctx_id);
    if status.code == 0 {
        REGISTRY.remove(ctx_id);
    }
    status
}

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
        resolution: path_conversion::resolve_read_path(&record, field),
        // SAFETY: this function's own contract requires `field` to be a
        // valid, NUL-terminated C string, or null.
        forward: unsafe { c_str_ref(field) },
        dd_path: read_argument_path(&record, field),
    };
    let timebase_argument = seam_policy::ReadArgument {
        resolution: path_conversion::resolve_read_path(&record, timebase),
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

/// `ptr` as a borrowed `&CStr`, or `None` if it is null.
///
/// # Safety
/// `ptr` must be a valid, NUL-terminated C string, or null.
unsafe fn c_str_ref<'a>(ptr: *const c_char) -> Option<&'a CStr> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller's own contract requires `ptr`, when non-null, to be
    // a valid NUL-terminated C string.
    Some(unsafe { CStr::from_ptr(ptr) })
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

/// Builds the read-only source view a write-side transformation can copy.
/// This never modifies caller storage; invalid raw shape metadata becomes a
/// policy refusal before an IMAS-Core write is attempted.
///
/// # Safety
/// `data`, when non-null, must point to the caller-owned buffer described by
/// `datatype`, `dim`, and `size`, matching IMAS-Core's write ABI contract.
unsafe fn build_source_view<'a>(
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> seam_policy::SourceView<'a> {
    if unsafe { is_empty_scalar(data, datatype, dim) } {
        return seam_policy::SourceView::UnsetScalar;
    }
    if datatype != DOUBLE_DATA_ID {
        return seam_policy::SourceView::NotDouble;
    }
    let element_count = if dim == 0 {
        Ok(1usize)
    } else if !(0..=crate::MAXDIM as c_int).contains(&dim) {
        Err("value-transform execution received an invalid array shape")
    } else if size.is_null() {
        Err("value-transform execution needs array dimensions")
    } else {
        // SAFETY: the ABI requires one initialized extent per write rank.
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
        Ok(_) if data.is_null() => {
            seam_policy::SourceView::InvalidShape("value-transform execution needs a data buffer")
        }
        Ok(count) => {
            // SAFETY: the caller's write ABI contract supplies an initialized
            // DOUBLE_DATA buffer of exactly this shape.
            let values = unsafe { std::slice::from_raw_parts(data.cast::<f64>(), count) };
            seam_policy::SourceView::Double(values)
        }
        Err(reason) => seam_policy::SourceView::InvalidShape(reason),
    }
}

/// Whether a scalar is one of IMAS-Core's own unset sentinels. This mirrors
/// the rank-zero half of `Lowlevel::data_has_non_zero_shape`: forwarding the
/// original bytes preserves Core's silent skip instead of letting a COCOS
/// flip fabricate a measurement (ADR 0018).
///
/// # Safety
/// When non-null, `data` must point to the scalar representation declared by
/// `datatype`. IMAS-Core's C ABI represents `COMPLEX_DATA` as consecutive
/// real and imaginary `double` values, matching its `complex_t` HDF5 bridge.
unsafe fn is_empty_scalar(data: *mut c_void, datatype: c_int, dim: c_int) -> bool {
    const EMPTY_INT: c_int = -999_999_999;
    const EMPTY_DOUBLE: f64 = -9e40;
    if dim != 0 || data.is_null() {
        return false;
    }
    match datatype {
        INTEGER_DATA_ID => unsafe { *data.cast::<c_int>() == EMPTY_INT },
        DOUBLE_DATA_ID => unsafe { *data.cast::<f64>() == EMPTY_DOUBLE },
        COMPLEX_DATA_ID => {
            let values = unsafe { std::slice::from_raw_parts(data.cast::<f64>(), 2) };
            values == [EMPTY_DOUBLE, EMPTY_DOUBLE]
        }
        _ => false,
    }
}

/// Turns a [`seam_policy::ReadVerdict`] into the `al_status_t` `read_data_impl`
/// returns, writing both arguments' retained fidelities to `record`'s root
/// loss log first. This is the one call site that ever writes to the loss
/// log for a read (issue #66): `seam_policy::ReadVerdict::field`/`timebase`
/// are mandatory, so there is no return path left that could reach this
/// point without both to write.
fn finish_read(
    record: &crate::registry::context_registry::ConversionRecord,
    verdict: seam_policy::ReadVerdict,
    data: *mut *mut c_void,
) -> al_status_t {
    record_argument_loss(record.root_id, &verdict.field);
    record_argument_loss(record.root_id, &verdict.timebase);
    match verdict.outcome {
        seam_policy::SeamOutcome::Data(status) => status,
        seam_policy::SeamOutcome::NotFound => no_source_read(data),
        seam_policy::SeamOutcome::Refusal { reason, dd_path } => {
            context_path_refusal(record, &reason, &dd_path)
        }
    }
}

/// Retains one argument's fidelity on `root_id`'s loss log — skipping
/// [`Fidelity::Exact`], which is never logged (ADR 0012).
fn record_argument_loss(
    root_id: crate::registry::context_registry::ContextId,
    argument: &seam_policy::ArgumentFidelity,
) {
    if argument.fidelity != Fidelity::Exact {
        REGISTRY.record_read_loss_at_root(root_id, argument.path.clone(), argument.fidelity);
    }
}

/// The raw HLI argument joined onto `record`'s own anchor, or `None` if the
/// argument itself is absent. Shared by its two callers, which want opposite
/// things from that `None`: `read_argument_path` falls back to the bare anchor,
/// because a loss entry always needs some path to name, while
/// `contextual_refusal` prefers a non-empty anchor and otherwise says so
/// explicitly rather than reporting a misleading one.
fn joined_argument_path(
    record: &crate::registry::context_registry::ConversionRecord,
    raw_path: *const c_char,
) -> Option<String> {
    c_str_or_none(raw_path)
        .filter(|path| !path.is_empty())
        .map(|path| path_conversion::join_hli_path(&record.resolved_path, path))
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
            *verdict = fidelity_verdict_code(fidelity);
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
            *operation = loss_operation_code(entry_operation);
        }
    }) else {
        return crate::conversion_refusal(
            "imas_mvdd_context_loss_operation_at index is out of range for this context",
        );
    };
    al_status_t::default()
}

fn fidelity_verdict_code(fidelity: Fidelity) -> c_int {
    match fidelity {
        Fidelity::Exact => {
            unreachable!("the loss log never retains an exact-fidelity read (ADR 0012)")
        }
        Fidelity::PotentiallyLossy => crate::IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY,
        Fidelity::Lossy => crate::IMAS_MVDD_FIDELITY_LOSSY,
        Fidelity::Unmappable => crate::IMAS_MVDD_FIDELITY_UNMAPPABLE,
    }
}

fn loss_operation_code(operation: crate::registry::context_registry::LossOperation) -> c_int {
    match operation {
        crate::registry::context_registry::LossOperation::Read => {
            crate::IMAS_MVDD_LOSS_OPERATION_READ
        }
        crate::registry::context_registry::LossOperation::Write => {
            crate::IMAS_MVDD_LOSS_OPERATION_WRITE
        }
    }
}

fn read_argument_path(
    record: &crate::registry::context_registry::ConversionRecord,
    raw_path: *const c_char,
) -> String {
    joined_argument_path(record, raw_path).unwrap_or_else(|| record.resolved_path.clone())
}

/// Formats a path-conversion refusal using the version pair retained by its
/// live context record. Read, write, and context-opening seams use this one
/// status boundary, so their caller-visible diagnostics cannot drift.
fn context_path_refusal(
    record: &crate::registry::context_registry::ConversionRecord,
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
fn contextual_refusal(
    record: &crate::registry::context_registry::ConversionRecord,
    reason: &str,
    raw_path: *const c_char,
) -> al_status_t {
    let dd_path = joined_argument_path(record, raw_path)
        .or_else(|| (!record.resolved_path.is_empty()).then(|| record.resolved_path.clone()))
        .unwrap_or_else(|| "(no path argument)".to_string());
    context_path_refusal(record, reason, &dd_path)
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

/// Resolves one arraystruct argument. Unlike a data read, a nonempty path
/// which the map does not claim cannot safely be forwarded: the new context's
/// stored anchor would be unknown, so the seam refuses before IMAS-Core opens
/// it.
fn resolve_arraystruct_argument(
    record: &crate::registry::context_registry::ConversionRecord,
    raw: *const c_char,
    label: &str,
) -> Result<Option<CString>, String> {
    match path_conversion::resolve_context_path(record, raw) {
        ContextPathResolution::Translated(path) => Ok(Some(path)),
        ContextPathResolution::Refusal(reason) => Err(reason),
        ContextPathResolution::NoSource => Err(format!("arraystruct {label} has no stored source")),
        ContextPathResolution::Unclaimed => Err(format!(
            "arraystruct {label} is unclaimed by the conversion map"
        )),
        ContextPathResolution::Forward => Ok(None),
    }
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
fn live_conversion_record(ctx_id: c_int) -> Option<ConversionRecord> {
    if !crate::version::hli_version::conversion_is_possible() {
        return None;
    }
    REGISTRY.lookup(ctx_id)
}

/// Forwards to IMAS-Core's real `al_write_data`, resolving IMAS-Core
/// lazily on first use. See [`write_data_impl`] for the shared policy this
/// and [`plugin_write_data`] both carry out.
///
/// # Safety
/// `field` and `timebase` must be valid, NUL-terminated C strings, or null
/// where IMAS-Core's own contract allows it. `data` and `size` must be
/// valid pointers, matching IMAS-Core's own contract for this function.
pub(crate) unsafe fn write_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    write_data_impl(
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

/// Follows the same policy as [`write_data`], forwarded through IMAS-Core's
/// plugin reentry write symbol rather than its ordinary twin.
///
/// # Safety
/// Same contract as [`write_data`].
pub(crate) unsafe fn plugin_write_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    write_data_impl(
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

/// The policy shared by `write_data` and `plugin_write_data` (issue #125,
/// consolidated onto [`CallFamily`] by issue #109).
///
/// A live conversion record resolves `field` and `timebase` independently;
/// the policy forwards only when both name one safe stored-DD path. Matching,
/// unknown, unstamped, and conversion-disabled contexts carry no record and
/// forward unchanged.
#[allow(clippy::too_many_arguments)]
fn write_data_impl(
    family: CallFamily,
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    let (_reentry_guard, already_entered) = ReentryGuard::enter();
    if already_entered {
        return call_write(family, ctx_id, field, timebase, data, datatype, dim, size);
    }
    let Some(record) = live_conversion_record(ctx_id) else {
        return call_write(family, ctx_id, field, timebase, data, datatype, dim, size);
    };

    let field_argument = seam_policy::WriteArgument {
        resolution: path_conversion::resolve_write_path(&record, field),
        // SAFETY: this function's contract requires `field` to be a valid,
        // NUL-terminated C string, or null.
        forward: unsafe { c_str_ref(field) },
        dd_path: read_argument_path(&record, field),
    };
    let timebase_argument = seam_policy::WriteArgument {
        resolution: path_conversion::resolve_write_path(&record, timebase),
        // SAFETY: this function's contract requires `timebase` to be a valid,
        // NUL-terminated C string, or null.
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
    // SAFETY: `write_data_impl` has the same pointer contract as
    // `build_source_view`; it borrows the caller buffer only long enough for
    // the policy to build its owned transformed copy.
    let source = unsafe { build_source_view(data, datatype, dim, size) };
    match seam_policy::run_write(&field_argument, &timebase_argument, shape, source) {
        seam_policy::WriteVerdict::Forward {
            field,
            timebase,
            data: transformed_data,
            unwritten_candidates,
        } => {
            let forward_data = transformed_data
                .as_ref()
                .map_or(data, |values| values.as_ptr().cast_mut().cast::<c_void>());
            let status = call_write(
                family,
                ctx_id,
                field.map_or(std::ptr::null(), CStr::as_ptr),
                timebase.map_or(std::ptr::null(), CStr::as_ptr),
                forward_data,
                datatype,
                dim,
                size,
            );
            if status.code == 0 {
                retain_unwritten_candidates(&record, &unwritten_candidates);
            }
            status
        }
        seam_policy::WriteVerdict::Refusal { reason, dd_path } => {
            finish_write_refusal(&record, &reason, &dd_path)
        }
    }
}

/// Records the candidates a successful write deliberately left alone.
///
/// This is the write path's only fidelity verdict, and it is deliberately not
/// the one the artifact declares. Every `merged` rule in the shipped artifact
/// declares `lossy` — ADR 0008's *certain* bucket — but that declaration is a
/// statement about a **read**, where two stored spellings may disagree and the
/// reader cannot tell which it got. A write puts one value into one slot, so
/// what it risks is only that some other reader later finds a stale value
/// under a spelling this write did not touch: unverified, hence
/// `PotentiallyLossy` (ADR 0016 decision 12).
///
/// Together with `finish_write_refusal`'s `Fidelity::Unmappable`, these are
/// the only two fidelities the write seam can produce, which is what makes
/// `Fidelity::Lossy` unreachable from a write. That claim is pinned by
/// `a_declared_lossy_candidate_plan_still_retains_a_potential_loss` rather
/// than left to a reader to derive from these two literals (ADR 0011).
fn retain_unwritten_candidates(record: &ConversionRecord, unwritten: &[&str]) {
    for dd_path in unwritten {
        REGISTRY.record_write_loss_at_root(
            record.root_id,
            (*dd_path).to_string(),
            Fidelity::PotentiallyLossy,
        );
    }
}

/// Turns a write-policy refusal into the two caller-visible consequences the
/// write seam owes: a root-owned `WRITE` loss and the formatted conversion
/// refusal. The path was already resolved against the live record, so both
/// effects use that same complete HLI-DD spelling.
fn finish_write_refusal(record: &ConversionRecord, reason: &str, dd_path: &str) -> al_status_t {
    REGISTRY.record_write_loss_at_root(record.root_id, dd_path.to_string(), Fidelity::Unmappable);
    context_path_refusal(record, reason, dd_path)
}

/// Forwards to IMAS-Core's real `al_delete_data`, resolving IMAS-Core
/// lazily on first use.
///
/// A live conversion record resolves a nonempty `path` to one safe stored-DD
/// spelling. The empty path deliberately forwards unchanged: IMAS-Core reads
/// it as an explicit whole-DATAOBJECT delete, leaving no foreign-version data
/// behind for a later unstamped open to mistake for HLI-version data. Unlike
/// [`write_data`], this seam takes no [`CallFamily`] parameter:
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
        resolution: path_conversion::resolve_delete_path(&record, path),
        // SAFETY: this function's contract requires `path` to be a valid,
        // NUL-terminated C string, or null.
        forward: unsafe { c_str_ref(path) },
    };
    match seam_policy::run_delete(&argument) {
        seam_policy::DeleteVerdict::Forward { path } => {
            forward_status!(delete_data(
                ctx,
                path.map_or(std::ptr::null(), CStr::as_ptr)
            ))
        }
        seam_policy::DeleteVerdict::FanOut { paths } => unsafe { delete_candidates(ctx, paths) },
        seam_policy::DeleteVerdict::Refusal { reason, dd_path } => {
            context_path_refusal(&record, &reason, &dd_path)
        }
    }
}

/// Deletes every stored candidate a multi-source HLI path can resolve to.
/// Each candidate is probed first because `al_delete_data` cannot represent
/// not-found. A failure does not stop later candidates: without a rollback,
/// continuing leaves the least stale data behind.
///
/// The probe is a `DOUBLE_DATA` scalar read, and both halves of IMAS-Core's
/// scalar ABI apply to it: the buffer is the *caller's* — IMAS-Core
/// dereferences `*data` unconditionally for `dim == 0`, so passing a null
/// pointer crashes it rather than returning not-found — and absence comes
/// back as the `EMPTY_DOUBLE` sentinel written into that buffer, never as a
/// null pointer. `probed` is therefore stack-owned and never freed, and the
/// outcome is classified with
/// [`read_outcome::classify_scalar_double`](crate::conversion::read_outcome::classify_scalar_double)
/// rather than the pointer-based classifier every non-scalar read uses.
///
/// A layer below IMAS-Core that ignored the scalar contract and returned an
/// allocation of its own would leak it here. That is the right way round:
/// freeing a pointer that under the contract is the caller's own stack slot is
/// a crash, and a leak in a layer that broke the contract is not.
unsafe fn delete_candidates(ctx: c_int, paths: &[CString]) -> al_status_t {
    let mut first_failure = None;

    for path in paths {
        // One fixed probe shape for every candidate, whatever its real DD type
        // and rank are. That is a known unsoundness, not an assumption that
        // every candidate is a scalar: ADR 0017 decision 2 records why it
        // cannot be fixed or verified before issue #138, and issue #138 owns
        // it.
        let mut probed = 0.0f64;
        let mut data: *mut c_void = (&raw mut probed).cast();
        let probe = unsafe {
            read_data_unconverted(
                ctx,
                path.as_ptr(),
                c"".as_ptr(),
                &mut data,
                DOUBLE_DATA_ID,
                0,
                std::ptr::null_mut(),
            )
        };
        match read_outcome::classify_scalar_double(&probe, data.cast_const(), probed) {
            ReadOutcome::Failure => {
                first_failure.get_or_insert_with(|| candidate_failure(probe, "probe", path));
            }
            ReadOutcome::NotFound => {}
            ReadOutcome::Data => {
                let deletion = forward_status!(delete_data(ctx, path.as_ptr()));
                if deletion.code != 0 {
                    first_failure
                        .get_or_insert_with(|| candidate_failure(deletion, "delete", path));
                }
            }
        }
    }

    first_failure.unwrap_or_default()
}

/// Keeps IMAS-Core's failure code while saying which half of the delete
/// fan-out failed. A failed probe and a failed deletion point callers at
/// different backend operations, so their messages must stay distinct.
fn candidate_failure(mut status: al_status_t, operation: &str, path: &CStr) -> al_status_t {
    status.message = [0; crate::MAX_ERR_MSG_LEN];
    write_truncated(
        &mut status.message,
        &format!(
            "IMAS-MVDD: {operation} failed for stored candidate {}",
            path.to_string_lossy()
        ),
    );
    status
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::conversion_map::Direction;
    use crate::conversion::path_conversion::WritePath;

    /// Issue #56 AC5: "Matching, unknown, unstamped, and conversion-disabled
    /// contexts bypass registry lookup and rule resolution." The
    /// conversion-disabled half is the one a seam can act on by itself, and
    /// this proves it acts on it *before* the registry rather than after.
    ///
    /// `hli_version`'s latch is deliberately never set in-process (its module
    /// comment explains why a unit test cannot set it), so
    /// `conversion_is_possible()` is false for the whole `cargo test` run.
    /// Registering a genuine root record and still getting `None` back is the
    /// observable proof: the record is unquestionably there, so a lookup that
    /// ran could not have missed it.
    #[test]
    fn a_data_path_seam_answers_before_the_registry_when_conversion_is_disabled() {
        // Far from the small IDs every other registry test uses, so this one
        // cannot collide with a concurrently running test in the same process.
        const CTX_ID: c_int = 0x5D00;
        let stored: crate::version::dd_version::DdVersion =
            "3.39.0".parse().expect("known release");
        let hli: crate::version::dd_version::DdVersion = "4.1.1".parse().expect("known release");
        let artifact = known_artifacts::lookup("equilibrium", &stored, &hli)
            .expect("the embedded equilibrium artifact serves this pair");
        let direction = artifact.direction_to_stored;
        assert!(REGISTRY.record_root(
            CTX_ID,
            String::new(),
            CTX_ID,
            MapCacheKey::new("equilibrium".to_string(), stored, hli),
            direction,
            || load_artifact(&artifact),
        ));

        assert!(
            !crate::version::hli_version::conversion_is_possible(),
            "no unit test can latch an HLI DD version, so conversion is off here"
        );
        assert!(
            REGISTRY.lookup(CTX_ID).is_some(),
            "the record must really be in the registry for this test to prove anything"
        );
        assert!(
            live_conversion_record(CTX_ID).is_none(),
            "the seam must answer from the latch, without consulting the registry"
        );

        REGISTRY.remove(CTX_ID);
    }

    #[test]
    fn a_declared_unmappable_write_refusal_carries_its_message_and_write_loss() {
        const CTX_ID: c_int = 0x5D03;
        const FIXTURE_IDS: &str = "equilibrium-unmappable-write-seam-fixture";
        const ARTIFACT: &str = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="declared-impossible" rel="renamed" left="impossible" right="stored">
                  <fidelity forward="unmappable" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let stored = "4.1.1".parse().expect("known release");
        let hli = "3.39.0".parse().expect("known release");
        assert!(REGISTRY.record_root(
            CTX_ID,
            String::new(),
            CTX_ID,
            MapCacheKey::new(FIXTURE_IDS.to_string(), stored, hli),
            Direction::Forward,
            || ConversionMap::load(ARTIFACT).expect("fixture artifact must load"),
        ));
        let record = REGISTRY
            .lookup(CTX_ID)
            .expect("the root record was just registered");
        let path = CString::new("impossible").expect("fixture path contains no NUL");
        let (reason, dd_path) = match path_conversion::resolve_write_path(&record, path.as_ptr()) {
            WritePath::Refusal { reason, dd_path } => (reason, dd_path),
            WritePath::Forward | WritePath::Translated { .. } | WritePath::Candidates(_) => {
                panic!("a declared-unmappable write must refuse")
            }
        };

        let status = finish_write_refusal(&record, &reason, &dd_path);
        assert_eq!(status.code, crate::IMAS_MVDD_CONVERSION_ERROR);
        let message = unsafe { CStr::from_ptr(status.message.as_ptr()) }
            .to_str()
            .expect("refusal message is UTF-8");
        assert_eq!(
            message,
            "IMAS-MVDD: this path has no safe conversion between DD versions; DD path: impossible; \
             HLI DD version: 3.39.0; stored DD version: 4.1.1"
        );
        assert_eq!(REGISTRY.loss_count(CTX_ID), 1);
        assert_eq!(
            REGISTRY.with_loss_at(CTX_ID, 0, |path, fidelity, operation| {
                (path.to_string(), fidelity, operation)
            }),
            Some((
                "impossible".to_string(),
                Fidelity::Unmappable,
                crate::registry::context_registry::LossOperation::Write,
            ))
        );

        REGISTRY.remove(CTX_ID);
    }

    /// Issue #128 / ADR 0016 decision 12: the write path produces no
    /// `Fidelity::Lossy` verdict at all.
    ///
    /// The fixture declares its `merged` rule `lossy` in the direction under
    /// test, which is the one input that could make the certain bucket
    /// reachable — every `merged` rule in the shipped artifact declares
    /// exactly that. The write seam must still record `PotentiallyLossy`,
    /// because the declared fidelity describes a read: it is certain that two
    /// stored spellings may disagree when *read*, and merely possible that
    /// some later reader finds the stale one after a write put its value in
    /// the primary slot.
    ///
    /// If this ever records `Lossy`, the write path has grown a producer for a
    /// verdict that has never had coverage — add real coverage for it rather
    /// than relaxing this assertion (ADR 0011).
    #[test]
    fn a_declared_lossy_candidate_plan_still_retains_a_potential_loss() {
        const CTX_ID: c_int = 0x5D05;
        const FIXTURE_IDS: &str = "equilibrium-write-lossy-candidate-fixture";
        const ARTIFACT: &str = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="fold-two" rel="merged" right="folded">
                  <from left="primary" precedence="1"/>
                  <from left="secondary" precedence="2"/>
                  <fidelity forward="exact" reverse="lossy"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        // A `merged` rule offers its candidate plan on the side that folds —
        // the HLI asks for the one canonical name and several stored names can
        // serve it — so this record travels reverse: a 4.1.1 HLI over a
        // 3.39.0 occurrence, which is also the direction the fixture declares
        // `lossy`.
        let stored = "3.39.0".parse().expect("known release");
        let hli = "4.1.1".parse().expect("known release");
        assert!(REGISTRY.record_root(
            CTX_ID,
            String::new(),
            CTX_ID,
            MapCacheKey::new(FIXTURE_IDS.to_string(), stored, hli),
            Direction::Reverse,
            || ConversionMap::load(ARTIFACT).expect("fixture artifact must load"),
        ));
        let record = REGISTRY
            .lookup(CTX_ID)
            .expect("the root record was just registered");

        let field = CString::new("folded").expect("fixture path contains no NUL");
        let resolution = path_conversion::resolve_write_path(&record, field.as_ptr());
        assert!(
            matches!(resolution, WritePath::Candidates(_)),
            "the fixture must resolve to a candidate plan, or this proves nothing"
        );
        let field_argument = seam_policy::WriteArgument {
            resolution,
            forward: None,
            dd_path: "folded".to_string(),
        };
        let timebase_argument = seam_policy::WriteArgument {
            resolution: WritePath::Forward,
            forward: None,
            dd_path: String::new(),
        };
        let values = [1.0f64];
        let verdict = seam_policy::run_write(
            &field_argument,
            &timebase_argument,
            seam_policy::BufferShape {
                datatype: seam_policy::BufferDataType::Double,
                rank: 1,
            },
            seam_policy::SourceView::Double(&values),
        );
        let seam_policy::WriteVerdict::Forward {
            unwritten_candidates,
            ..
        } = verdict
        else {
            panic!("a precedence-1 write over a candidate plan must forward")
        };
        assert_eq!(unwritten_candidates, vec!["secondary"]);

        retain_unwritten_candidates(&record, &unwritten_candidates);
        assert_eq!(REGISTRY.loss_count(CTX_ID), 1);
        assert_eq!(
            REGISTRY.with_loss_at(CTX_ID, 0, |path, fidelity, operation| {
                (path.to_string(), fidelity, operation)
            }),
            Some((
                "secondary".to_string(),
                Fidelity::PotentiallyLossy,
                crate::registry::context_registry::LossOperation::Write,
            )),
            "the write seam recorded something other than one PotentiallyLossy entry \
             for a rule the artifact declares certainly lossy"
        );

        REGISTRY.remove(CTX_ID);
    }
}
