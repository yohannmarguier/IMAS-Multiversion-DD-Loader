//! The occurrence-opening and context-closing seams.
//!
//! One cluster of the C ABI, held together by a single question: *which
//! stored DD version does this occurrence hold, and does this process convert
//! against it?* `al_begin_global_action` and its slice, timerange and
//! arraystruct siblings all open a context; `al_end_action` closes one. The
//! stamp discovery (ADR 0007), the root and child registrations (ADR 0003)
//! and the conversion-map cache that serves them are all here because they
//! are only ever reached through an opening seam.
//!
//! The decisions themselves are not: `seam_policy::decide_occurrence_registration`
//! and `decide_datapath_translation` are pure functions in
//! [`crate::conversion::seam_policy`] (ADR 0015). What this module owns is the
//! ABI-facing half — the Core calls, the raw-pointer marshalling, the
//! registry writes, and the depth gating each decision requires.

use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::sync::Arc;

use crate::al_status_t;
use crate::conversion::conversion_map::ConversionMap;
use crate::conversion::known_artifacts;
use crate::conversion::path_conversion::{self, ContextPathResolution};
use crate::conversion::seam_policy;
use crate::core::core_binding::{READ_OP_ID, forward_status};
use crate::registry::context_registry::{MapCacheKey, REGISTRY};
use crate::version::version_stamp;

use super::dispatch::{
    CallFamily, call_begin_arraystruct, call_begin_global, call_begin_slice, call_end,
};
use super::reentry::ReentryGuard;
use super::refusal::{c_str_or_none, contextual_refusal, live_conversion_record};

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
/// one read-outcome classifier ([`crate::conversion::read_outcome`]). A present,
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
        && let Some(raw_path) = datapath
            .and_then(c_str_or_none)
            .filter(|path| !path.is_empty())
        && let Some(artifact) = known_artifacts::lookup(ids_name, &stored, &hli)
    {
        let map = resolve_conversion_map(ids_name, &stored, &hli, &artifact);
        translated_datapath =
            seam_policy::decide_datapath_translation(&map, artifact.direction_to_stored, raw_path)
                .and_then(|path| CString::new(path).ok());
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

/// Resolves the cached conversion map for a global-action `datapath`.
///
/// This does not fold into [`crate::registry::context_registry::ContextRegistry::record_root`]:
/// a global action needs the same map before its forward call, whereas root
/// registration happens only after a successful occurrence open. Keeping the
/// cache lookup separate also preserves `record_root`'s focused registry API.
fn resolve_conversion_map(
    ids: &str,
    stored: &crate::version::dd_version::DdVersion,
    hli: &crate::version::dd_version::DdVersion,
    artifact: &known_artifacts::ArtifactMatch,
) -> Arc<ConversionMap> {
    let key = map_cache_key(ids, stored, hli);
    REGISTRY.get_or_create_map(key, || load_artifact(artifact))
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

/// Resolves one arraystruct argument. Unlike a data read, a nonempty path
/// which the map does not claim cannot safely be forwarded: the new context's
/// stored anchor would be unknown, so the seam refuses before IMAS-Core opens
/// it.
fn resolve_arraystruct_argument(
    record: &crate::registry::context_registry::ConversionRecord,
    raw: *const c_char,
    label: &str,
) -> Result<Option<CString>, String> {
    match path_conversion::narrow_context_path(path_conversion::resolve(record, raw)) {
        ContextPathResolution::Translated(path) => Ok(Some(path)),
        ContextPathResolution::Refusal(reason) => Err(reason),
        ContextPathResolution::NoSource => Err(format!("arraystruct {label} has no stored source")),
        ContextPathResolution::Unclaimed => Err(format!(
            "arraystruct {label} is unclaimed by the conversion map"
        )),
        ContextPathResolution::Forward => Ok(None),
    }
}
