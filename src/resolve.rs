//! Runtime resolution of IMAS-Core, and the conversion policy of every seam
//! resolved through it.
//!
//! **The binding.** Proven end to end on `al_context_info` (issue #3), then
//! extended to the data-entry, action-lifecycle and data-operation symbols
//! below (issue #6): the shim carries no link-time dependency on IMAS-Core. On
//! first use it opens IMAS-Core with local symbol visibility and resolves each
//! function's address through that specific library handle, so the shim's own
//! globally visible exports are never in the lookup scope and can't capture its
//! outbound calls. See `docs/adr/0001-runtime-binding-not-linking.md`.
//!
//! **The policy.** Each mirrored symbol's shim-side behaviour also lives here,
//! beside the binding it forwards through, so a reader of one seam sees both at
//! once. Four ADRs are enforced in this file, and a change to any of them lands
//! here:
//!
//! - ADR 0001 — the runtime binding above.
//! - ADR 0002 — which seams translate, which refuse, and which forward
//!   unchanged; stamp discovery and root registration at the opening seams.
//! - ADR 0010 — read-path value transformations: one per rule, executed in
//!   place after the read, once (see [`ReadNesting`]).
//! - ADR 0012 — the three-way read outcome and the refusal/loss reporting
//!   channel, via [`crate::read_outcome`] and the registry's loss log.
//!
//! That breadth is why the file is long, and it is a known tension rather than
//! an accident: the review's S-J6 finding labels it Divergent Change. Splitting
//! the binding from the policy is a real option, but the split has to be made
//! on purpose, with the seam list as it will be — not as a side effect of a
//! cleanup — so this module states what it owns instead of pretending to own
//! only the first paragraph.

use std::cell::Cell;
use std::env;
use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::sync::OnceLock;

use crate::context_registry::{MapCacheKey, REGISTRY};
use crate::conversion_map::{
    ConversionMap, Fidelity, Outcome, RefusalReason, Rel, ValueTransformation,
};
use crate::dl::Library;
use crate::known_artifacts;
use crate::read_outcome::{self, ReadOutcome};
use crate::version_stamp::{self, StampOutcome};
use crate::{MAX_ERR_MSG_LEN, MAXDIM, al_status_t};

/// Explicit override, honoured before the bare soname — see the ADR's
/// resolution order.
const CORE_LIBRARY_ENV_VAR: &str = "IMAS_CORE_LIBRARY";

/// Supported IMAS-Core release, sourced by `build.rs` from the repository's
/// `IMAS_CORE_VERSION` pin.
const BUILT_AGAINST_VERSION: &str = env!("IMAS_CORE_VERSION");

/// How deep this thread currently is inside the shim's own read seam, and
/// which returned buffer a value transformation has already been applied to at
/// that nesting.
///
/// This exists to enforce ADR 0010's "the shim therefore cannot apply a sign
/// change twice" in the one situation where it could: a read that re-enters the
/// shim's read seam. Two callers do that today — `version_stamp::discover`
/// reads the stamp through [`read_data`] while opening a context, and the
/// plugin reentry family calls back into the shim — so without a guard one
/// IMAS-Core buffer could be handed to `apply_value_transformation` on both the
/// inner and the outer call, silently negating a COCOS-flipped value back to
/// its stored sign. Recording *which* buffer was already transformed, rather
/// than merely that one was, is what lets a genuinely different buffer read on
/// the same thread still get its own flip.
///
/// The identity of "the same buffer" is its address, which is the part of this
/// mechanism that is a judgement call rather than a settled invariant: an
/// allocator is free to hand back an address a previous buffer has released,
/// and a required flip would then be skipped. Finding P8 of the read-path
/// review raises exactly that, and the question of whether ADR 0010 should
/// specify the mechanism at all, so treat the keying as open rather than
/// decided. What *is* relied on here: the outermost read clears the record on
/// the way out, so a remembered address never outlives the call stack that
/// observed it, and the state is thread-local, so two threads reading
/// concurrently cannot suppress each other's transformation.
#[derive(Copy, Clone)]
struct ReadNesting {
    depth: u32,
    transformed_buffer: Option<usize>,
}

thread_local! {
    static READ_NESTING: Cell<ReadNesting> = const {
        Cell::new(ReadNesting {
            depth: 0,
            transformed_buffer: None,
        })
    };
}

/// Tracks one read's nesting for as long as it is on the stack. Held by
/// [`read_data_impl`] for the whole of its body, so the depth it reports is the
/// number of shim reads between here and the HLI's own call.
struct ReadNestingGuard;

impl ReadNestingGuard {
    fn enter() -> Self {
        READ_NESTING.with(|state| {
            let nesting = state.get();
            state.set(ReadNesting {
                depth: nesting.depth + 1,
                ..nesting
            });
        });
        Self
    }
}

impl Drop for ReadNestingGuard {
    fn drop(&mut self) {
        READ_NESTING.with(|state| {
            let nesting = state.get();
            // Leaving the outermost read ends the call stack that observed the
            // buffer, so the address stops meaning anything and is dropped.
            let still_nested = nesting.depth > 1;
            state.set(ReadNesting {
                depth: nesting.depth - 1,
                transformed_buffer: still_nested.then_some(nesting.transformed_buffer).flatten(),
            });
        });
    }
}

type ContextInfoFn = unsafe extern "C" fn(c_int, *mut *mut c_char) -> al_status_t;
type VersionAccessorFn = unsafe extern "C" fn() -> *const c_char;
type GetBackendIdFn = unsafe extern "C" fn(c_int, *mut c_int) -> al_status_t;
type BuildUriFromLegacyParametersFn = unsafe extern "C" fn(
    c_int,
    c_int,
    c_int,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut *mut c_char,
) -> al_status_t;
type StringLookupFn = unsafe extern "C" fn(c_int) -> *const c_char;

type BeginDataentryActionFn = unsafe extern "C" fn(*const c_char, c_int, *mut c_int) -> al_status_t;
type ClosePulseFn = unsafe extern "C" fn(c_int, c_int) -> al_status_t;
type BeginGlobalActionFn =
    unsafe extern "C" fn(c_int, *const c_char, *const c_char, c_int, *mut c_int) -> al_status_t;
type BeginSliceActionFn =
    unsafe extern "C" fn(c_int, *const c_char, c_int, c_double, c_int, *mut c_int) -> al_status_t;
type BeginTimerangeActionFn = unsafe extern "C" fn(
    c_int,
    *const c_char,
    c_int,
    c_double,
    c_double,
    *const c_double,
    *const c_int,
    c_int,
    *mut c_int,
) -> al_status_t;
type BeginArraystructActionFn = unsafe extern "C" fn(
    c_int,
    *const c_char,
    *const c_char,
    *mut c_int,
    *mut c_int,
) -> al_status_t;
type EndActionFn = unsafe extern "C" fn(c_int) -> al_status_t;
type ReadDataFn = unsafe extern "C" fn(
    c_int,
    *const c_char,
    *const c_char,
    *mut *mut c_void,
    c_int,
    c_int,
    *mut c_int,
) -> al_status_t;
type WriteDataFn = unsafe extern "C" fn(
    c_int,
    *const c_char,
    *const c_char,
    *mut c_void,
    c_int,
    c_int,
    *mut c_int,
) -> al_status_t;
type DeleteDataFn = unsafe extern "C" fn(c_int, *const c_char) -> al_status_t;
type IterateOverArraystructFn = unsafe extern "C" fn(c_int, c_int) -> al_status_t;
type GetOccurrencesFn =
    unsafe extern "C" fn(c_int, *const c_char, *mut *mut c_int, *mut c_int) -> al_status_t;
type ListFilledPathsFn =
    unsafe extern "C" fn(c_int, *const c_char, *mut *mut *mut c_char, *mut c_int) -> al_status_t;
type PluginNameFn = unsafe extern "C" fn(*const c_char) -> al_status_t;
type BindPluginFn = unsafe extern "C" fn(*const c_char, *const c_char) -> al_status_t;
type PluginContextFn = unsafe extern "C" fn(c_int) -> al_status_t;
type IsPluginRegisteredFn = unsafe extern "C" fn(*const c_char, *mut bool) -> al_status_t;
type SetvalueParameterPluginFn = unsafe extern "C" fn(
    *const c_char,
    c_int,
    c_int,
    *mut c_int,
    *mut c_void,
    *const c_char,
) -> al_status_t;
type SetvalueIntScalarParameterPluginFn =
    unsafe extern "C" fn(*const c_char, c_int, *const c_char) -> al_status_t;
type SetvalueDoubleScalarParameterPluginFn =
    unsafe extern "C" fn(*const c_char, c_double, *const c_char) -> al_status_t;

// Declares the whole binding from one manifest: each entry names a field, the
// ABI signature it holds, and the IMAS-Core symbol it resolves from.
//
// Every symbol used to appear three times in this file — once as a
// `CoreBinding` field, once as a `let` in `resolve()`, and once more in the
// struct literal — so adding one export meant three coordinated edits in
// lockstep and a missed one was a compile error at best, a field bound to the
// wrong symbol at worst. The C side of the project already resolves this with
// a single X-macro manifest (`tests/abi_symbols.def`, and now
// `tests/abi_fallback_constants.def`); this is that idiom's Rust counterpart.
//
// `bootstrap` holds symbols resolved before the manifest runs. `getALVersion`
// is the only one: the ADR requires its version report to be checked before
// any other IMAS-Core symbol is resolved, so it is passed in already resolved
// rather than listed below.
macro_rules! core_binding {
    (
        bootstrap { $($bootstrap_field:ident: $bootstrap_type:ty,)* }
        resolved { $($field:ident: $field_type:ty = $symbol:literal,)* }
    ) => {
        struct CoreBinding {
            // Kept alive for the process's lifetime: dropping it would unmap
            // the resolved function pointers below. Never read again once
            // resolution succeeds.
            _library: Library,
            $($bootstrap_field: $bootstrap_type,)*
            $($field: $field_type,)*
        }

        impl CoreBinding {
            /// Resolves every manifest symbol from an already-opened library,
            /// failing on the first one the library does not provide.
            ///
            /// # Safety
            /// Each manifest entry must pair a symbol that really exists in
            /// the resolver's library with the signature that symbol really
            /// has.
            #[allow(clippy::result_large_err)]
            unsafe fn bind(
                resolver: SymbolResolver,
                $($bootstrap_field: $bootstrap_type,)*
            ) -> Result<Self, al_status_t> {
                Ok(Self {
                    $($field: unsafe { resolver.resolve($symbol) }?,)*
                    $($bootstrap_field,)*
                    // Initialised last on purpose: every field above borrows
                    // the resolver, and this field consumes it.
                    _library: resolver.into_library(),
                })
            }
        }
    };
}

core_binding! {
    bootstrap {
        get_al_version: VersionAccessorFn,
    }
    resolved {
        context_info: ContextInfoFn = "al_context_info",
        get_backend_id: GetBackendIdFn = "al_get_backendID",
        build_uri_from_legacy_parameters: BuildUriFromLegacyParametersFn =
            "al_build_uri_from_legacy_parameters",
        const2str: StringLookupFn = "const2str",
        err2str: StringLookupFn = "err2str",
        get_dd_version: VersionAccessorFn = "getDDVersion",
        begin_dataentry_action: BeginDataentryActionFn = "al_begin_dataentry_action",
        close_pulse: ClosePulseFn = "al_close_pulse",
        begin_global_action: BeginGlobalActionFn = "al_begin_global_action",
        begin_slice_action: BeginSliceActionFn = "al_begin_slice_action",
        begin_timerange_action: BeginTimerangeActionFn = "al_begin_timerange_action",
        begin_arraystruct_action: BeginArraystructActionFn = "al_begin_arraystruct_action",
        end_action: EndActionFn = "al_end_action",
        read_data: ReadDataFn = "al_read_data",
        write_data: WriteDataFn = "al_write_data",
        delete_data: DeleteDataFn = "al_delete_data",
        iterate_over_arraystruct: IterateOverArraystructFn = "al_iterate_over_arraystruct",
        get_occurrences: GetOccurrencesFn = "al_get_occurrences",
        list_filled_paths: ListFilledPathsFn = "al_list_filled_paths",
        register_plugin: PluginNameFn = "al_register_plugin",
        unregister_plugin: PluginNameFn = "al_unregister_plugin",
        bind_plugin: BindPluginFn = "al_bind_plugin",
        unbind_plugin: BindPluginFn = "al_unbind_plugin",
        bind_readback_plugins: PluginContextFn = "al_bind_readback_plugins",
        unbind_readback_plugins: PluginContextFn = "al_unbind_readback_plugins",
        is_plugin_registered: IsPluginRegisteredFn = "al_is_plugin_registered",
        write_plugins_metadata: PluginContextFn = "al_write_plugins_metadata",
        setvalue_parameter_plugin: SetvalueParameterPluginFn = "al_setvalue_parameter_plugin",
        setvalue_int_scalar_parameter_plugin: SetvalueIntScalarParameterPluginFn =
            "al_setvalue_int_scalar_parameter_plugin",
        setvalue_double_scalar_parameter_plugin: SetvalueDoubleScalarParameterPluginFn =
            "al_setvalue_double_scalar_parameter_plugin",
        plugin_begin_global_action: BeginGlobalActionFn = "al_plugin_begin_global_action",
        plugin_begin_slice_action: BeginSliceActionFn = "al_plugin_begin_slice_action",
        plugin_begin_arraystruct_action: BeginArraystructActionFn =
            "al_plugin_begin_arraystruct_action",
        plugin_end_action: EndActionFn = "al_plugin_end_action",
        plugin_read_data: ReadDataFn = "al_plugin_read_data",
        plugin_write_data: WriteDataFn = "al_plugin_write_data",
    }
}

#[derive(Debug, PartialEq, Eq)]
struct VersionDrift {
    built_against: String,
    found: String,
}

impl VersionDrift {
    fn record(&self) {
        eprintln!(
            "imas-mvdd-loader: tolerating IMAS-Core version drift (built against {}, found {})",
            self.built_against, self.found
        );
    }
}

enum ResolutionError {
    Unavailable(al_status_t),
    VersionMismatch {
        status: al_status_t,
        detected_version: CString,
    },
}

impl ResolutionError {
    fn status(&self) -> &al_status_t {
        match self {
            Self::Unavailable(status) | Self::VersionMismatch { status, .. } => status,
        }
    }
}

impl From<al_status_t> for ResolutionError {
    fn from(status: al_status_t) -> Self {
        Self::Unavailable(status)
    }
}

static CORE: OnceLock<Result<CoreBinding, ResolutionError>> = OnceLock::new();

/// Resolves IMAS-Core, once, memoized for the process's lifetime. A process
/// that never calls a mirrored ABI function never runs this.
fn resolution() -> &'static Result<CoreBinding, ResolutionError> {
    CORE.get_or_init(resolve)
}

fn core() -> Result<&'static CoreBinding, &'static al_status_t> {
    resolution().as_ref().map_err(ResolutionError::status)
}

// Every status-returning ABI function reports an unresolvable IMAS-Core the
// same way, so that plumbing lives here rather than in all 37 exports.
//
// The named forwarders below exist for that reason alone. They are *not* all
// prospective conversion seams: CONTEXT.md reserves "seam" for an ABI entry
// point carrying a DD path or IDS name, which is 16 of the 37 (CLAUDE.md
// tabulates them). The rest are plain forwards with nothing to interpose on,
// and calling them seams-in-waiting made the word mean "function", which is
// how seven genuine seams came to carry no marking at all.
macro_rules! forward_status {
    ($function:ident($($argument:expr),* $(,)?)) => {
        match core() {
            Ok(binding) => unsafe { (binding.$function)($($argument),*) },
            Err(status) => *status,
        }
    };
}

// `al_status_t` is the ABI struct itself, not an internal error type boxed
// away for convenience — returning it by value from `Err` is the point.
#[allow(clippy::result_large_err)]
fn resolve() -> Result<CoreBinding, ResolutionError> {
    let resolver =
        SymbolResolver::open(library_path(env::var(CORE_LIBRARY_ENV_VAR).ok().as_deref()))?;

    // getALVersion is the bootstrap symbol: it must be resolved, and its
    // report checked, before any other IMAS-Core symbol is resolved at all
    // (see the ADR's Consequences) — a major mismatch means the ABI itself
    // may disagree, so nothing past this point can be trusted.
    let get_al_version: VersionAccessorFn = unsafe { resolver.resolve("getALVersion") }?;
    let found_version = unsafe { get_al_version() };
    if found_version.is_null() {
        return Err(failure(&format!(
            "IMAS-Core library '{}' returned a null pointer from 'getALVersion'",
            resolver.path()
        ))
        .into());
    }
    let found_version = unsafe { CStr::from_ptr(found_version) };
    let found_version_text = found_version.to_string_lossy();
    // Tolerated drift is a one-off diagnostic, not state: nothing downstream
    // branches on it, so it is reported here and not carried any further.
    match check_major_version(BUILT_AGAINST_VERSION, &found_version_text) {
        Ok(Some(drift)) => drift.record(),
        Ok(None) => {}
        Err(detail) => {
            return Err(ResolutionError::VersionMismatch {
                status: failure(&detail),
                // A CStr cannot contain interior NUL bytes, so copying its
                // contents into a CString is infallible. Retaining the copy
                // keeps getALVersion's mismatch result valid indefinitely.
                detected_version: CString::new(found_version.to_bytes())
                    .expect("CStr values cannot contain interior NUL bytes"),
            });
        }
    }

    // SAFETY: every manifest entry pairs an IMAS-Core symbol name with the
    // signature transcribed for it above, and the drift check
    // (tests/real_core_abi_contract.h) holds that transcription to IMAS-Core's
    // real header.
    Ok(unsafe { CoreBinding::bind(resolver, get_al_version) }?)
}

pub(crate) unsafe fn get_backend_id(ctx: c_int, backend_id: *mut c_int) -> al_status_t {
    forward_status!(get_backend_id(ctx, backend_id))
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn build_uri_from_legacy_parameters(
    backend_id: c_int,
    pulse: c_int,
    run: c_int,
    user: *const c_char,
    tokamak: *const c_char,
    version: *const c_char,
    options: *const c_char,
    uri: *mut *mut c_char,
) -> al_status_t {
    forward_status!(build_uri_from_legacy_parameters(
        backend_id, pulse, run, user, tokamak, version, options, uri,
    ))
}

pub(crate) fn const2str(id: c_int) -> *const c_char {
    match resolution() {
        Ok(binding) => unsafe { (binding.const2str)(id) },
        // The ADR deliberately keeps these diagnostics useful when the
        // mismatched library's ABI cannot safely be queried further.
        Err(ResolutionError::VersionMismatch { .. }) => fallback_const2str(id),
        Err(ResolutionError::Unavailable(_)) => std::ptr::null(),
    }
}

pub(crate) fn err2str(id: c_int) -> *const c_char {
    match resolution() {
        Ok(binding) => unsafe { (binding.err2str)(id) },
        Err(ResolutionError::VersionMismatch { .. }) => fallback_err2str(id),
        Err(ResolutionError::Unavailable(_)) => std::ptr::null(),
    }
}

pub(crate) fn get_al_version() -> *const c_char {
    match resolution() {
        Ok(binding) => unsafe { (binding.get_al_version)() },
        Err(ResolutionError::VersionMismatch {
            detected_version, ..
        }) => detected_version.as_ptr(),
        Err(ResolutionError::Unavailable(_)) => std::ptr::null(),
    }
}

pub(crate) fn get_dd_version() -> *const c_char {
    match resolution() {
        Ok(binding) => unsafe { (binding.get_dd_version)() },
        Err(ResolutionError::VersionMismatch { .. }) => static_c_str(b"!!DEPRECATED!!\0"),
        Err(ResolutionError::Unavailable(_)) => std::ptr::null(),
    }
}

// Version-pinned values from IMAS-Core's al_const.h. TIMERANGE_OP and
// FLEXBUFFERS_BACKEND deliberately have no entries: upstream's const2str map
// omits them too (see the functionality inventory).
const NO_BACKEND_ID: c_int = 10;
const ASCII_BACKEND_ID: c_int = 11;
const MDSPLUS_BACKEND_ID: c_int = 12;
const HDF5_BACKEND_ID: c_int = 13;
const MEMORY_BACKEND_ID: c_int = 14;
const UDA_BACKEND_ID: c_int = 15;
const GLOBAL_OP_ID: c_int = 20;
const SLICE_OP_ID: c_int = 21;
const READ_OP_ID: c_int = 30;
const WRITE_OP_ID: c_int = 31;
const REPLACE_OP_ID: c_int = 32;
const UNDEFINED_INTERP_ID: c_int = 0;
const CLOSEST_INTERP_ID: c_int = 1;
const PREVIOUS_INTERP_ID: c_int = 2;
const LINEAR_INTERP_ID: c_int = 3;
const UNDEFINED_TIME_ID: c_int = -999;
const OPEN_PULSE_ID: c_int = 40;
const FORCE_OPEN_PULSE_ID: c_int = 41;
const CREATE_PULSE_ID: c_int = 42;
const FORCE_CREATE_PULSE_ID: c_int = 43;
const CLOSE_PULSE_ID: c_int = 44;
const ERASE_PULSE_ID: c_int = 45;
pub(crate) const CHAR_DATA_ID: c_int = 50;
const INTEGER_DATA_ID: c_int = 51;
const DOUBLE_DATA_ID: c_int = 52;
const COMPLEX_DATA_ID: c_int = 53;
const ASCII_SERIALIZER_PROTOCOL_ID: c_int = 60;
const FLEXBUFFERS_SERIALIZER_PROTOCOL_ID: c_int = 61;
const UNKNOWN_ERR_ID: c_int = -1;
const CONTEXT_ERR_ID: c_int = -2;
const BACKEND_ERR_ID: c_int = -3;
const LOWLEVEL_ERR_ID: c_int = -4;

fn fallback_const2str(id: c_int) -> *const c_char {
    let name = match id {
        NO_BACKEND_ID => b"NO_BACKEND\0".as_slice(),
        ASCII_BACKEND_ID => b"ASCII_BACKEND\0".as_slice(),
        MDSPLUS_BACKEND_ID => b"MDSPLUS_BACKEND\0".as_slice(),
        HDF5_BACKEND_ID => b"HDF5_BACKEND\0".as_slice(),
        MEMORY_BACKEND_ID => b"MEMORY_BACKEND\0".as_slice(),
        UDA_BACKEND_ID => b"UDA_BACKEND\0".as_slice(),
        GLOBAL_OP_ID => b"GLOBAL_OP\0".as_slice(),
        SLICE_OP_ID => b"SLICE_OP\0".as_slice(),
        READ_OP_ID => b"READ_OP\0".as_slice(),
        WRITE_OP_ID => b"WRITE_OP\0".as_slice(),
        REPLACE_OP_ID => b"REPLACE_OP\0".as_slice(),
        UNDEFINED_INTERP_ID => b"UNDEFINED_INTERP\0".as_slice(),
        CLOSEST_INTERP_ID => b"CLOSEST_INTERP\0".as_slice(),
        PREVIOUS_INTERP_ID => b"PREVIOUS_INTERP\0".as_slice(),
        LINEAR_INTERP_ID => b"LINEAR_INTERP\0".as_slice(),
        UNDEFINED_TIME_ID => b"UNDEFINED_TIME\0".as_slice(),
        OPEN_PULSE_ID => b"OPEN_PULSE\0".as_slice(),
        FORCE_OPEN_PULSE_ID => b"FORCE_OPEN_PULSE\0".as_slice(),
        CREATE_PULSE_ID => b"CREATE_PULSE\0".as_slice(),
        FORCE_CREATE_PULSE_ID => b"FORCE_CREATE_PULSE\0".as_slice(),
        CLOSE_PULSE_ID => b"CLOSE_PULSE\0".as_slice(),
        ERASE_PULSE_ID => b"ERASE_PULSE\0".as_slice(),
        CHAR_DATA_ID => b"CHAR_DATA\0".as_slice(),
        INTEGER_DATA_ID => b"INTEGER_DATA\0".as_slice(),
        DOUBLE_DATA_ID => b"DOUBLE_DATA\0".as_slice(),
        COMPLEX_DATA_ID => b"COMPLEX_DATA\0".as_slice(),
        ASCII_SERIALIZER_PROTOCOL_ID => b"ASCII_SERIALIZER_PROTOCOL\0".as_slice(),
        FLEXBUFFERS_SERIALIZER_PROTOCOL_ID => b"FLEXBUFFERS_SERIALIZER_PROTOCOL\0".as_slice(),
        _ => b"\0".as_slice(),
    };
    static_c_str(name)
}

fn fallback_err2str(id: c_int) -> *const c_char {
    let name = match id {
        UNKNOWN_ERR_ID => b"UNKNOWN_ERR\0".as_slice(),
        CONTEXT_ERR_ID => b"CONTEXT_ERR\0".as_slice(),
        BACKEND_ERR_ID => b"BACKEND_ERR\0".as_slice(),
        LOWLEVEL_ERR_ID => b"LOWLEVEL_ERR\0".as_slice(),
        _ => b"\0".as_slice(),
    };
    static_c_str(name)
}

fn static_c_str(value: &'static [u8]) -> *const c_char {
    value.as_ptr().cast()
}

/// The explicit override if set, else a bare soname resolved through the
/// loader's normal search path. Never an absolute, build-machine-specific
/// path — nothing here is baked in at compile time.
fn library_path(override_value: Option<&str>) -> String {
    match override_value {
        Some(value) => value.to_string(),
        None => bare_soname(),
    }
}

fn bare_soname() -> String {
    format!(
        "{}al{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )
}

/// An opened IMAS-Core together with the path it was opened from.
///
/// The two are never useful apart: the path's only job after `dlopen` is to
/// name the library in the resolution-failure messages, so every lookup needs
/// both and passing them as a pair invited them to drift out of step.
struct SymbolResolver {
    library: Library,
    path: String,
}

impl SymbolResolver {
    /// Opens `path` into a private symbol scope, keeping it for diagnostics.
    #[allow(clippy::result_large_err)]
    fn open(path: String) -> Result<Self, al_status_t> {
        let library = Library::open(&path).map_err(|underlying| {
            failure(&format!(
                "failed to open IMAS-Core library '{path}': {underlying}"
            ))
        })?;
        Ok(Self { library, path })
    }

    /// The path this library was opened from.
    fn path(&self) -> &str {
        &self.path
    }

    /// Surrenders the opened library, which must outlive every function
    /// pointer resolved from it.
    fn into_library(self) -> Library {
        self.library
    }

    /// # Safety
    /// The caller is responsible for `symbol_name` in this library really
    /// having signature `F`.
    #[allow(clippy::result_large_err)]
    unsafe fn resolve<F: Copy>(&self, symbol_name: &str) -> Result<F, al_status_t> {
        let path = &self.path;
        let address = unsafe { self.library.symbol(symbol_name) }.map_err(|underlying| {
            failure(&format!(
                "IMAS-Core library '{path}' has no '{symbol_name}': {underlying}"
            ))
        })?;
        if address.is_null() {
            return Err(failure(&format!(
                "IMAS-Core library '{path}' resolved '{symbol_name}' to a null address"
            )));
        }
        // SAFETY: forwarded to this method's own safety contract on `F`.
        Ok(unsafe { std::mem::transmute_copy(&address) })
    }
}

/// Compares the version this shim was built against with the version a
/// resolved IMAS-Core reports. Only `major` gates: a mismatch there means
/// the ABI itself may disagree, so resolution must fail; minor/patch drift
/// is tolerated.
fn check_major_version(built_against: &str, found: &str) -> Result<Option<VersionDrift>, String> {
    let built_major = major_component(built_against);
    let found_major = major_component(found);

    match (built_major, found_major) {
        (Some(b), Some(f)) if b == f => {
            if built_against != found {
                return Ok(Some(VersionDrift {
                    built_against: built_against.to_string(),
                    found: found.to_string(),
                }));
            }
            Ok(None)
        }
        (Some(_), Some(_)) => Err(format!(
            "IMAS-Core major version mismatch: shim built against {built_against}, found {found}"
        )),
        _ => Err(format!(
            "could not compare IMAS-Core versions: shim built against '{built_against}', found '{found}'"
        )),
    }
}

fn major_component(version: &str) -> Option<&str> {
    let major = version.split('.').next()?;
    (!major.is_empty() && major.bytes().all(|b| b.is_ascii_digit())).then_some(major)
}

/// Builds a failure `al_status_t`: a non-zero code and a message naming
/// both the override variable and the underlying problem, truncated to fit
/// the fixed-size ABI buffer without ever panicking or overflowing.
///
/// The override variable is named *first*: the underlying detail can be
/// arbitrarily long (macOS's `dlerror()` text lists every search path
/// tried and can run well past 256 bytes on its own), and whatever comes
/// after that detail is exactly what truncation would discard.
fn failure(detail: &str) -> al_status_t {
    let message = format!("override with ${CORE_LIBRARY_ENV_VAR} if this is wrong; {detail}");
    let mut status = al_status_t {
        code: -1,
        message: [0; MAX_ERR_MSG_LEN],
    };
    crate::write_truncated(&mut status.message, &message);
    status
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
    if let Err(reason) = crate::hli_version::resolve_for_open() {
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
/// policy (issue #53) when the HLI DD version is latched:
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
/// also ended first, so a refusal here never leaks it. An absent stamp, or
/// one that matches the HLI DD version, registers nothing (ADR 0007): the
/// occurrence is presumed to match. A present, valid, *mismatched* stamp
/// registers the root context, but only when an artifact actually covers
/// this IDS and version pair (ADR 0011 decision 1) — otherwise this is
/// treated exactly like an unknown context, passthrough with no record.
///
/// When the HLI DD version is unset, this is a plain forward with none of
/// the above: no stamp read, no registry lookup, no rule resolution.
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
    let forward = |effective_datapath| {
        forward_status!(begin_global_action(
            pctx_id,
            dataobjectname,
            effective_datapath,
            rwmode,
            octx_id,
        ))
    };
    let end_on_refusal = |ctx| forward_status!(end_action(ctx));
    // SAFETY: same contract as `begin_global_action_impl`, already upheld by
    // this function's own `unsafe fn` contract.
    unsafe {
        begin_global_action_impl(
            pctx_id,
            dataobjectname,
            datapath,
            octx_id,
            forward,
            end_on_refusal,
        )
    }
}

/// The policy shared by `begin_global_action` and `plugin_begin_global_action`
/// (issue #67): the occurrence-cache `datapath` translation on the way in and
/// the stored-version discovery/root-registration rule on the way out,
/// factored out of both so only the forwarded ABI symbol and the matching
/// end-action twin differ between the ordinary and plugin reentry seams.
/// `forward` is called with the effective (possibly translated) `datapath`
/// exactly once, whether or not the HLI DD version is latched.
///
/// # Safety
/// Same contract as [`begin_global_action`]: `dataobjectname` and `datapath`
/// must be valid, NUL-terminated C strings, or null where IMAS-Core's own
/// contract allows it, and `octx_id` must be a valid, writable `*mut c_int`
/// once `forward` reports success.
unsafe fn begin_global_action_impl(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    datapath: *const c_char,
    octx_id: *mut c_int,
    forward: impl FnOnce(*const c_char) -> al_status_t,
    end_on_refusal: impl FnOnce(c_int) -> al_status_t,
) -> al_status_t {
    let Some(hli) = crate::hli_version::current() else {
        return forward(datapath);
    };

    let dataobjectname_str = c_str_or_none(dataobjectname);
    let ids_name = dataobjectname_str.map(ids_name_from);

    let mut translated_datapath: Option<CString> = None;
    if let (Some(dataobjectname_str), Some(ids_name)) = (dataobjectname_str, ids_name)
        && let Some(stored) = REGISTRY.known_stored_version(pctx_id, dataobjectname_str)
        && stored != hli
    {
        translated_datapath = translate_down(ids_name, &stored, &hli, c_str_or_none(datapath));
    }
    let effective_datapath = translated_datapath
        .as_deref()
        .map(CStr::as_ptr)
        .unwrap_or(datapath);

    let status = forward(effective_datapath);
    if status.code != 0 {
        return status;
    }

    // SAFETY: IMAS-Core's own contract requires `octx_id` to be a valid,
    // writable pointer, already relied on by the forwarded call above.
    let opened_octx_id = unsafe { *octx_id };
    discover_and_register_occurrence(
        pctx_id,
        dataobjectname_str,
        ids_name,
        opened_octx_id,
        &hli,
        status,
        end_on_refusal,
    )
}

/// The stored-version discovery, classification and root-registration rule
/// shared by `al_begin_global_action`, `al_begin_slice_action`,
/// `al_begin_timerange_action` (ADR 0002, issue #53, issue #55) and their
/// `al_plugin_*` reentry twins (issue #67) — every operation-context seam
/// that opens a whole IDS occurrence, once the real open has already
/// succeeded and the HLI DD version is latched.
///
/// `dataobjectname_str`/`ids_name` are `None` when the occurrence identity
/// isn't usable (null or non-UTF-8 `dataobjectname`): the open itself
/// already succeeded against real IMAS-Core, but discovery and registration
/// need a valid `dataobjectname` to key on, so this is a no-op passthrough
/// in that case.
///
/// Otherwise the occurrence's DD-version stamp is read immediately (before
/// the caller returns to the HLI) and classified through the one
/// read-outcome classifier ([`crate::read_outcome`]). A present, malformed
/// stamp is a hard refusal — the just-opened IMAS-Core context is also
/// ended first via `end_on_refusal`, so a refusal here never leaks it; the
/// caller supplies its own matching end-action symbol (`al_end_action` for
/// an ordinary open, `al_plugin_end_action` for a plugin reentry open) since
/// a context opened through one family is closed through that same family.
/// An absent stamp, or one that matches the HLI DD version, registers
/// nothing (ADR 0007): the occurrence is presumed to match. A present,
/// valid, *mismatched* stamp registers the root context, but only when an
/// artifact actually covers this IDS and version pair (ADR 0011 decision 1)
/// — otherwise this is treated exactly like an unknown context, passthrough
/// with no record.
fn discover_and_register_occurrence(
    pctx_id: c_int,
    dataobjectname_str: Option<&str>,
    ids_name: Option<&str>,
    opened_ctx_id: c_int,
    hli: &crate::dd_version::DdVersion,
    status: al_status_t,
    end_on_refusal: impl FnOnce(c_int) -> al_status_t,
) -> al_status_t {
    let (Some(dataobjectname_str), Some(ids_name)) = (dataobjectname_str, ids_name) else {
        return status;
    };

    match version_stamp::discover(opened_ctx_id) {
        StampOutcome::Malformed(refusal) => {
            // A prior open may have cached a mismatch for this occurrence,
            // but this read gives no usable version to justify retaining it.
            // Never translate a later `datapath` from stale discovery state.
            REGISTRY.forget_occurrence_version(pctx_id, dataobjectname_str);
            // The open already succeeded against real IMAS-Core; a refusal
            // from here on must not leak that context, since the HLI — told
            // this open failed — will never call the matching end-action
            // itself.
            let _ = end_on_refusal(opened_ctx_id);
            *refusal
        }
        StampOutcome::Unstamped => {
            // An absent or failed discovery read means this occurrence is no
            // longer known to differ from the HLI DD version. Clear any
            // earlier mismatch before a future open chooses its `datapath`.
            REGISTRY.forget_occurrence_version(pctx_id, dataobjectname_str);
            status
        }
        StampOutcome::Stored(stored) => {
            if stored == *hli {
                REGISTRY.forget_occurrence_version(pctx_id, dataobjectname_str);
            } else {
                REGISTRY.remember_mismatched_occurrence(
                    pctx_id,
                    dataobjectname_str.to_string(),
                    stored.clone(),
                );
                if let Some(artifact) = known_artifacts::lookup(ids_name, &stored, hli) {
                    let key = map_cache_key(ids_name, &stored, hli);
                    let direction = artifact.direction_to_stored;
                    // A global/slice/time-range action opens the whole IDS
                    // occurrence, not one field: the record's resolved path
                    // is the occurrence's own root, empty because a relative
                    // `al_read_data` `field` under this context is resolved
                    // against it directly, with no IDS-name segment to skip
                    // (ADR 0002, ADR 0003). This is unrelated to `datapath`,
                    // which stays near-inert (CLAUDE.md) and never feeds
                    // this field.
                    REGISTRY.record_root(
                        opened_ctx_id,
                        String::new(),
                        pctx_id,
                        key,
                        direction,
                        || load_artifact(&artifact),
                    );
                }
            }
            status
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
    stored: &crate::dd_version::DdVersion,
    hli: &crate::dd_version::DdVersion,
    path: Option<&str>,
) -> Option<CString> {
    let path = path.filter(|p| !p.is_empty())?;
    let artifact = known_artifacts::lookup(ids, stored, hli)?;
    let key = map_cache_key(ids, stored, hli);
    let map = REGISTRY.get_or_create_map(key, || load_artifact(&artifact));
    let explanation = map.resolve(path, artifact.direction_to_stored)?;
    match explanation.outcome {
        // `datapath` is near-inert (CLAUDE.md): only a concrete resolved
        // path is a basis to translate it at all. A no-source or refusal
        // outcome here is not this seam's call to make — forward unchanged
        // and let the eventual `al_read_data` on this occurrence be the one
        // that refuses or reports absence.
        Outcome::Path { resolved_path, .. } => CString::new(resolved_path).ok(),
        Outcome::NoSource | Outcome::Refusal(_) => None,
    }
}

/// The `(IDS name, stored DD version, HLI DD version)` cache key both the
/// datapath-translation and root-registration call sites look their shared
/// conversion map up under.
fn map_cache_key(
    ids: &str,
    stored: &crate::dd_version::DdVersion,
    hli: &crate::dd_version::DdVersion,
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
/// IMAS-Core lazily on first use, and applies the same stored-version
/// discovery and occurrence-registration rule as `begin_global_action`
/// (ADR 0002, issue #55) when the HLI DD version is latched. `dataobjectname`
/// (the IDS name, plus occurrence) is always forwarded unchanged — a slice
/// action carries no `datapath` argument, so there is nothing to translate
/// on the way in.
///
/// When the HLI DD version is unset, this is a plain forward with no stamp
/// read, no registry lookup, no rule resolution.
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
    let forward = || {
        forward_status!(begin_slice_action(
            pctx_id,
            dataobjectname,
            rwmode,
            time,
            interpmode,
            octx_id,
        ))
    };
    let end_on_refusal = |ctx| forward_status!(end_action(ctx));
    // SAFETY: same contract as `begin_occurrence_action_impl`, already upheld by
    // this function's own `unsafe fn` contract.
    unsafe {
        begin_occurrence_action_impl(pctx_id, dataobjectname, octx_id, forward, end_on_refusal)
    }
}

/// The policy shared by every occurrence-opening seam whose only path-bearing
/// argument is the IDS name: `begin_slice_action` and
/// `plugin_begin_slice_action` (issue #67), and `begin_timerange_action`. The
/// stored-version discovery and root-registration rule is factored out of all
/// three so only the forwarded ABI symbol and the matching end-action twin
/// differ between them. Because none of them carries a `datapath` argument to
/// translate, `forward` takes no arguments, unlike
/// [`begin_global_action_impl`]'s.
///
/// # Safety
/// Same contract as [`begin_slice_action`]: `dataobjectname` must be a valid,
/// NUL-terminated C string, or null where IMAS-Core's own contract allows
/// it, and `octx_id` must be a valid, writable `*mut c_int` once `forward`
/// reports success.
unsafe fn begin_occurrence_action_impl(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    octx_id: *mut c_int,
    forward: impl FnOnce() -> al_status_t,
    end_on_refusal: impl FnOnce(c_int) -> al_status_t,
) -> al_status_t {
    let Some(hli) = crate::hli_version::current() else {
        return forward();
    };

    let dataobjectname_str = c_str_or_none(dataobjectname);
    let ids_name = dataobjectname_str.map(ids_name_from);

    let status = forward();
    if status.code != 0 {
        return status;
    }

    // SAFETY: IMAS-Core's own contract requires `octx_id` to be a valid,
    // writable pointer, already relied on by the forwarded call above.
    let opened_octx_id = unsafe { *octx_id };
    discover_and_register_occurrence(
        pctx_id,
        dataobjectname_str,
        ids_name,
        opened_octx_id,
        &hli,
        status,
        end_on_refusal,
    )
}

/// Forwards to IMAS-Core's real `al_begin_timerange_action`, resolving
/// IMAS-Core lazily on first use, and applies the same stored-version
/// discovery and occurrence-registration rule as `begin_global_action`
/// (ADR 0002, issue #55) when the HLI DD version is latched. `dataobjectname`
/// (the IDS name, plus occurrence) is always forwarded unchanged — a
/// time-range action carries no `datapath` argument, so there is nothing to
/// translate on the way in.
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
    let end_on_refusal = |ctx| forward_status!(end_action(ctx));

    // SAFETY: same contract as `begin_occurrence_action_impl`, already upheld by
    // this function's own safety contract.
    unsafe {
        begin_occurrence_action_impl(pctx_id, dataobjectname, octx_id, forward, end_on_refusal)
    }
}

/// Forwards to IMAS-Core's real `al_begin_arraystruct_action`, resolving
/// `path` and `timebase` from a mismatched parent's HLI-DD spelling to its
/// stored-DD spelling before IMAS-Core is called. Relative arguments resolve
/// below the parent context; absolute arguments resolve from the IDS root.
///
/// A refusal or a path with no stored source stops before IMAS-Core is called.
/// When the real open succeeds, the resulting `actxID` is registered as a
/// child of `ctx_id` using the HLI-DD spelling, so later reads can resolve
/// their own relative fields through the same conversion map.
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
    let forward = |p, t| forward_status!(begin_arraystruct_action(ctx_id, p, t, size, actx_id));
    // SAFETY: same contract as `begin_arraystruct_action_impl`, already
    // upheld by this function's own `unsafe fn` contract.
    unsafe { begin_arraystruct_action_impl(ctx_id, path, timebase, actx_id, forward) }
}

/// The policy shared by `begin_arraystruct_action` and
/// `plugin_begin_arraystruct_action` (issue #67): the `path`/`timebase`
/// resolution against the parent's conversion record and the child-record
/// registration on success, factored out of both so only the forwarded ABI
/// symbol differs between the ordinary and plugin reentry seams. `forward`
/// is called with the effective (possibly translated) `path`/`timebase`
/// exactly once.
///
/// # Safety
/// Same contract as [`begin_arraystruct_action`]: `path` and `timebase` must
/// be valid, NUL-terminated C strings, or null where IMAS-Core's own
/// contract allows it, and `actx_id` must be a valid, writable `*mut c_int`
/// once `forward` reports success.
unsafe fn begin_arraystruct_action_impl(
    ctx_id: c_int,
    path: *const c_char,
    timebase: *const c_char,
    actx_id: *mut c_int,
    forward: impl FnOnce(*const c_char, *const c_char) -> al_status_t,
) -> al_status_t {
    let Some(parent) = REGISTRY.lookup(ctx_id) else {
        return forward(path, timebase);
    };

    let translated_path = match resolve_arraystruct_argument(&parent, path, "path") {
        Ok(path) => path,
        Err(message) => return crate::conversion_refusal(&message),
    };
    let translated_timebase = match resolve_arraystruct_argument(&parent, timebase, "timebase") {
        Ok(path) => path,
        Err(message) => return crate::conversion_refusal(&message),
    };

    let status = forward(
        translated_path.as_deref().map(CStr::as_ptr).unwrap_or(path),
        translated_timebase
            .as_deref()
            .map(CStr::as_ptr)
            .unwrap_or(timebase),
    );
    if status.code == 0 {
        let resolved_path = join_hli_path(
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
/// lazily on first use. On success, removes only `ctx_id`'s own registry
/// record, if any (ADR 0002, ADR 0003) — a parent context never owns a
/// child context's lifetime, and an unrecorded or already-plain `ctx_id`
/// removal is a harmless no-op.
pub(crate) fn end_action(ctx_id: c_int) -> al_status_t {
    let status = forward_status!(end_action(ctx_id));
    if status.code == 0 {
        REGISTRY.remove(ctx_id);
    }
    status
}

/// Forwards to IMAS-Core's real `al_read_data`, resolving IMAS-Core lazily
/// on first use. See [`read_data_impl`] for the shared policy.
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
    let forward = |field: *const c_char, timebase: *const c_char| {
        forward_status!(read_data(
            ctx_id, field, timebase, data, datatype, dim, size
        ))
    };
    // SAFETY: same contract as `read_data_impl`, already upheld by this
    // function's own `unsafe fn` contract.
    unsafe { read_data_impl(ctx_id, field, timebase, data, datatype, dim, size, forward) }
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
    let forward = |field: *const c_char, timebase: *const c_char| {
        forward_status!(plugin_read_data(
            ctx_id, field, timebase, data, datatype, dim, size
        ))
    };
    // SAFETY: same contract as `read_data_impl`, already upheld by this
    // function's own `unsafe fn` contract.
    unsafe { read_data_impl(ctx_id, field, timebase, data, datatype, dim, size, forward) }
}

/// The policy shared by `read_data` and `plugin_read_data` (issue #68).
///
/// When `ctx_id` names no live conversion record — no mismatch was ever
/// discovered, the occurrence matched or was unstamped, or the HLI DD
/// version is unset — this is a plain forward, unchanged from before issue
/// #54. Otherwise `field` is resolved through the record's conversion map,
/// in the direction that reaches the stored DD spelling, before IMAS-Core is
/// called:
///
/// - An explicit refusal is a shim-owned [`al_status_t`] refusal — IMAS-Core
///   is never called.
/// - A `merged`/`split` plan is tried in declared precedence order until one
///   candidate returns data. A winning field transformation runs in place
///   before the buffer reaches the HLI.
/// - No claimed source on the stored side returns success with a null data
///   pointer, matching IMAS-Core's own not-found convention, without calling
///   IMAS-Core at all.
/// - Otherwise the translated field reaches IMAS-Core through `forward` and
///   its returned allocation is forwarded to the HLI exactly as received:
///   the shim neither substitutes nor frees it.
///
/// `field` and `timebase` are resolved independently through the same
/// version pair. A no-source result for either means this read cannot find
/// data in the stored representation, so the seam returns the normal
/// success-with-null result without calling IMAS-Core.
///
/// # Safety
/// `field` and `timebase` must be valid, NUL-terminated C strings, or null
/// where IMAS-Core's own contract allows it. `data` and `size` must be
/// valid, writable pointers, matching IMAS-Core's own contract for this
/// function. `forward` must call through to the matching real IMAS-Core
/// read symbol with the given (possibly translated) field/timebase and
/// this function's own `data`/`datatype`/`dim`/`size`.
#[allow(clippy::too_many_arguments)]
unsafe fn read_data_impl(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
    forward: impl Fn(*const c_char, *const c_char) -> al_status_t,
) -> al_status_t {
    let _nesting_guard = ReadNestingGuard::enter();
    let Some(record) = REGISTRY.lookup(ctx_id) else {
        return forward(field, timebase);
    };

    let translated_field = match resolve_read_path(&record, field) {
        ReadPath::Forward => None,
        ReadPath::Translated(path) | ReadPath::Candidates(path) => Some(path),
        ReadPath::Refusal {
            reason,
            dd_path,
            fidelity,
        } => {
            retain_read_fidelity(&record, field, fidelity);
            return read_refusal(&record, &reason, &dd_path);
        }
        ReadPath::NoSource(fidelity) => {
            retain_read_fidelity(&record, field, fidelity);
            return no_source_read(data);
        }
    };
    let translated_timebase = match resolve_read_path(&record, timebase) {
        ReadPath::Forward => None,
        ReadPath::Translated(path) | ReadPath::Candidates(path) => Some(path),
        ReadPath::Refusal {
            reason,
            dd_path,
            fidelity,
        } => {
            retain_read_fidelity(&record, timebase, fidelity);
            return read_refusal(&record, &reason, &dd_path);
        }
        ReadPath::NoSource(fidelity) => {
            retain_read_fidelity(&record, timebase, fidelity);
            return no_source_read(data);
        }
    };

    // Every translated field/timebase — a single translated path or a
    // merged/split candidate plan — is tried through this one loop rather
    // than short-circuiting a "simple" single-path case through a bare
    // forward: a short-circuit here previously skipped `retain_read_fidelity`
    // for a plain non-exact `renamed`/`moved` rule, so a single-candidate
    // Lossy read never reached the loss log (ADR 0012, issue #65).
    let field_attempts = translated_field.as_ref().map_or_else(
        || vec![ReadAttempt::forward(field)],
        TranslatedReadPath::attempts,
    );
    let timebase_attempts = translated_timebase.as_ref().map_or_else(
        || vec![ReadAttempt::forward(timebase)],
        TranslatedReadPath::attempts,
    );
    let field_dd_path = read_argument_path(&record, field);
    for field_attempt in &field_attempts {
        for timebase_attempt in &timebase_attempts {
            if let Err(reason) =
                validate_value_transformation(&field_attempt.value_transformation, datatype, dim)
            {
                retain_read_fidelities(
                    &record,
                    field,
                    Fidelity::Unmappable,
                    timebase,
                    timebase_attempt.fidelity,
                );
                return read_refusal(&record, reason, &field_dd_path);
            }
            let status = forward(field_attempt.path, timebase_attempt.path);
            // SAFETY: `data` is valid and writable by `read_data_impl`'s own
            // safety contract, and the just-finished IMAS-Core call has
            // initialized it.
            match read_outcome::classify(&status, unsafe { *data }) {
                ReadOutcome::Failure => {
                    retain_read_fidelities(
                        &record,
                        field,
                        field_attempt.fidelity,
                        timebase,
                        timebase_attempt.fidelity,
                    );
                    return status;
                }
                ReadOutcome::Data => {
                    // ADR 0010: this buffer's one transformation applies once.
                    // A reentrant read (see `ReadNesting`) can reach the same
                    // buffer twice, so claim it before transforming it.
                    let should_transform = field_attempt.value_transformation
                        != ValueTransformation::None
                        && READ_NESTING.with(|state| {
                            let nesting = state.get();
                            let buffer = unsafe { *data } as usize;
                            if nesting.transformed_buffer == Some(buffer) {
                                false
                            } else {
                                state.set(ReadNesting {
                                    transformed_buffer: Some(buffer),
                                    ..nesting
                                });
                                true
                            }
                        });
                    if should_transform
                        && let Err(reason) = apply_value_transformation(
                            &field_attempt.value_transformation,
                            unsafe { *data },
                            datatype,
                            dim,
                            size,
                        )
                    {
                        retain_read_fidelities(
                            &record,
                            field,
                            Fidelity::Unmappable,
                            timebase,
                            timebase_attempt.fidelity,
                        );
                        return read_refusal(&record, reason, &field_dd_path);
                    }
                    retain_read_fidelities(
                        &record,
                        field,
                        field_attempt.fidelity,
                        timebase,
                        timebase_attempt.fidelity,
                    );
                    return status;
                }
                ReadOutcome::NotFound => {}
            }
        }
    }
    retain_read_fidelities(
        &record,
        field,
        translated_read_fidelity(translated_field.as_ref()),
        timebase,
        translated_read_fidelity(translated_timebase.as_ref()),
    );
    no_source_read(data)
}

const EMPTY_DOUBLE: f64 = -9e40;

fn validate_value_transformation(
    transformation: &ValueTransformation,
    datatype: c_int,
    dim: c_int,
) -> Result<(), &'static str> {
    match transformation {
        ValueTransformation::None => Ok(()),
        ValueTransformation::SignFlip { .. }
            if datatype == DOUBLE_DATA_ID && (0..=MAXDIM as c_int).contains(&dim) =>
        {
            Ok(())
        }
        ValueTransformation::SignFlip { .. } => {
            Err("value-transform execution requires DOUBLE_DATA and a rank no greater than MAXDIM")
        }
    }
}

fn apply_value_transformation(
    transformation: &ValueTransformation,
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> Result<(), &'static str> {
    match transformation {
        ValueTransformation::None => Ok(()),
        ValueTransformation::SignFlip { .. } => {
            validate_value_transformation(transformation, datatype, dim)?;
            let element_count = if dim == 0 {
                1
            } else {
                if size.is_null() {
                    return Err("value-transform execution needs array dimensions");
                }
                // SAFETY: the ABI requires one initialized extent per rank
                // after a successful IMAS-Core array read.
                unsafe { std::slice::from_raw_parts(size, dim as usize) }
                    .iter()
                    .try_fold(1usize, |count, &extent| {
                        usize::try_from(extent)
                            .ok()
                            .and_then(|extent| count.checked_mul(extent))
                    })
                    .ok_or("value-transform execution received an invalid array shape")?
            };
            // SAFETY: ReadOutcome::Data establishes non-null data, and the
            // validated datatype and returned shape describe this buffer.
            let values =
                unsafe { std::slice::from_raw_parts_mut(data.cast::<f64>(), element_count) };
            for value in values {
                if *value != EMPTY_DOUBLE {
                    *value = -*value;
                }
            }
            Ok(())
        }
    }
}

/// The raw HLI argument joined onto `record`'s own anchor, or `None` if the
/// argument itself is absent. Shared by `read_argument_path`, which falls
/// back to the bare anchor for a display path, and `retain_read_fidelity`,
/// which skips logging outright when there was no argument to join.
fn joined_argument_path(
    record: &crate::context_registry::ConversionRecord,
    raw_path: *const c_char,
) -> Option<String> {
    c_str_or_none(raw_path)
        .filter(|path| !path.is_empty())
        .map(|path| join_hli_path(&record.resolved_path, path))
}

/// Retains one non-exact outcome on `ctx_id`'s root loss log, keyed by the
/// complete DD path as the HLI requested it — `record.resolved_path` joined
/// with `raw_path` — never the raw argument alone. Under a root context
/// `resolved_path` is empty and the join is a no-op, but under an arraystruct
/// child it restores the anchor a relative argument was implicitly addressed
/// against (issue #66), matching the path already used for refusal messages
/// (`read_argument_path`).
fn retain_read_fidelity(
    record: &crate::context_registry::ConversionRecord,
    raw_path: *const c_char,
    fidelity: Fidelity,
) {
    if fidelity != Fidelity::Exact
        && let Some(path) = joined_argument_path(record, raw_path)
    {
        REGISTRY.record_read_loss_at_root(record.root_id, path, fidelity);
    }
}

fn retain_read_fidelities(
    record: &crate::context_registry::ConversionRecord,
    field: *const c_char,
    field_fidelity: Fidelity,
    timebase: *const c_char,
    timebase_fidelity: Fidelity,
) {
    retain_read_fidelity(record, field, field_fidelity);
    retain_read_fidelity(record, timebase, timebase_fidelity);
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
    let Some(copy_result) = REGISTRY.with_loss_at(ctx_id, index, |path, fidelity| {
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

fn read_argument_path(
    record: &crate::context_registry::ConversionRecord,
    raw_path: *const c_char,
) -> String {
    joined_argument_path(record, raw_path).unwrap_or_else(|| record.resolved_path.clone())
}

/// Formats a path-conversion refusal using the version pair retained by its
/// live context record. Both `field` and `timebase` resolve through this one
/// status boundary, so their caller-visible diagnostics cannot drift.
fn read_refusal(
    record: &crate::context_registry::ConversionRecord,
    reason: &str,
    dd_path: &str,
) -> al_status_t {
    crate::read_conversion_refusal(reason, dd_path, &record.hli_version, &record.stored_version)
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

/// The result of resolving one path-bearing context argument against a
/// mismatched conversion record.
enum ContextPathResolution {
    /// No usable caller path or no matching rule/default: forward it unchanged.
    Forward,
    /// A concrete stored-DD spelling for IMAS-Core to receive.
    Translated(CString),
    /// The artifact says no stored counterpart exists.
    NoSource,
    /// The artifact or this seam deliberately declines to serve the path.
    Refusal(String),
}

/// The richer path result used only by `al_read_data`: an ordered candidate
/// plan retains each candidate's fidelity and value transformation until a
/// stored source actually returns data.
enum ReadPath {
    Forward,
    Translated(TranslatedReadPath),
    Candidates(TranslatedReadPath),
    NoSource(Fidelity),
    Refusal {
        reason: String,
        dd_path: String,
        fidelity: Fidelity,
    },
}

struct TranslatedReadPath {
    paths: Vec<ResolvedReadPath>,
}

struct ResolvedReadPath {
    path: CString,
    fidelity: Fidelity,
    value_transformation: ValueTransformation,
}

struct ReadAttempt {
    path: *const c_char,
    fidelity: Fidelity,
    value_transformation: ValueTransformation,
}

impl ReadAttempt {
    fn forward(path: *const c_char) -> Self {
        Self {
            path,
            fidelity: Fidelity::Exact,
            value_transformation: ValueTransformation::None,
        }
    }
}

impl TranslatedReadPath {
    fn attempts(&self) -> Vec<ReadAttempt> {
        self.paths
            .iter()
            .map(|path| ReadAttempt {
                path: path.path.as_ptr(),
                fidelity: path.fidelity,
                value_transformation: path.value_transformation.clone(),
            })
            .collect()
    }
}

fn translated_read_fidelity(path: Option<&TranslatedReadPath>) -> Fidelity {
    path.and_then(|path| path.paths.first())
        .map_or(Fidelity::Exact, |path| path.fidelity)
}

/// A conversion-map outcome narrowed to what a path-bearing ABI argument can
/// pass to IMAS-Core: one concrete stored spelling, no source, or a refusal.
/// Merged/split plans and value transformations deliberately have no single
/// spelling for these seams to pass through yet.
enum ConcreteStoredPath {
    Path(String),
    NoSource,
    Refusal(String),
}

/// Resolves one arraystruct argument. Unlike a data read, a nonempty path
/// which the map does not claim cannot safely be forwarded: the new context's
/// stored anchor would be unknown, so the seam refuses before IMAS-Core opens
/// it.
fn resolve_arraystruct_argument(
    record: &crate::context_registry::ConversionRecord,
    raw: *const c_char,
    label: &str,
) -> Result<Option<CString>, String> {
    match resolve_context_path(record, raw) {
        ContextPathResolution::Translated(path) => Ok(Some(path)),
        ContextPathResolution::Refusal(reason) => Err(reason),
        ContextPathResolution::NoSource => Err(format!("arraystruct {label} has no stored source")),
        ContextPathResolution::Forward => {
            match c_str_or_none(raw).filter(|path| !path.is_empty()) {
                Some(_) => Err(format!(
                    "arraystruct {label} is unclaimed by the conversion map"
                )),
                None => Ok(None),
            }
        }
    }
}

/// One path-bearing ABI argument that the conversion map claims, in the form
/// both path resolvers need before they can differ: whether the caller spelled
/// it absolutely, its absolute HLI-DD spelling, and the rule that explains it.
struct ClaimedArgument {
    is_absolute: bool,
    hli_absolute: String,
    explanation: crate::conversion_map::RuleExplanation,
}

/// The preamble [`resolve_context_path`] and [`resolve_read_path`] share.
/// `None` means forward the argument unchanged, for either of the two reasons
/// that verdict can arise: there is no usable path to translate, or no rule
/// claims the one there is. The embedded artifact has an identity default, but
/// a future artifact may not, and an absent rule or default is never
/// permission to invent a stored spelling.
fn claimed_argument(
    record: &crate::context_registry::ConversionRecord,
    raw: *const c_char,
) -> Option<ClaimedArgument> {
    let raw = c_str_or_none(raw).filter(|path| !path.is_empty())?;
    let is_absolute = raw.starts_with('/');
    let hli_absolute = join_hli_path(&record.resolved_path, raw);
    let explanation = record
        .map
        .resolve(&hli_absolute, record.direction_to_stored)?;
    Some(ClaimedArgument {
        is_absolute,
        hli_absolute,
        explanation,
    })
}

/// Resolves one path-bearing context argument independently, preserving the
/// caller's relative-vs-absolute spelling after conversion has selected the
/// stored-DD path. `al_read_data` and `al_begin_arraystruct_action` share this
/// policy.
fn resolve_context_path(
    record: &crate::context_registry::ConversionRecord,
    raw: *const c_char,
) -> ContextPathResolution {
    let Some(argument) = claimed_argument(record, raw) else {
        return ContextPathResolution::Forward;
    };
    let ClaimedArgument {
        is_absolute,
        explanation,
        ..
    } = argument;

    match concrete_stored_path(explanation.outcome) {
        ConcreteStoredPath::NoSource => ContextPathResolution::NoSource,
        ConcreteStoredPath::Refusal(reason) => ContextPathResolution::Refusal(reason),
        ConcreteStoredPath::Path(resolved_path) => {
            match stored_c_path(record, &resolved_path, is_absolute) {
                Ok(path) => ContextPathResolution::Translated(path),
                Err(reason) => ContextPathResolution::Refusal(reason),
            }
        }
    }
}

/// Resolves one read argument. Unlike `resolve_context_path`, this preserves
/// merged/split candidates and their transformations so the read seam can
/// execute the plan without making them appear as one concrete AOS path.
fn resolve_read_path(
    record: &crate::context_registry::ConversionRecord,
    raw: *const c_char,
) -> ReadPath {
    let Some(argument) = claimed_argument(record, raw) else {
        return ReadPath::Forward;
    };
    let ClaimedArgument {
        is_absolute,
        hli_absolute,
        explanation,
    } = argument;

    let fidelity = read_fidelity(explanation.fidelity, explanation.rel);
    match explanation.outcome {
        Outcome::Refusal(reason) => ReadPath::Refusal {
            reason: refusal_reason_message(reason),
            dd_path: hli_absolute,
            fidelity: Fidelity::Unmappable,
        },
        Outcome::NoSource => ReadPath::NoSource(fidelity),
        Outcome::Path {
            resolved_path,
            value_transformation,
            candidates,
        } if candidates.is_empty() => translated_read_component(
            record,
            &resolved_path,
            is_absolute,
            fidelity,
            value_transformation,
        )
        .map(|path| ReadPath::Translated(TranslatedReadPath { paths: vec![path] }))
        .unwrap_or_else(|reason| ReadPath::Refusal {
            reason,
            dd_path: hli_absolute,
            fidelity: Fidelity::Unmappable,
        }),
        Outcome::Path { candidates, .. } => candidates
            .into_iter()
            .map(|candidate| {
                translated_read_component(
                    record,
                    &candidate.path,
                    is_absolute,
                    fidelity,
                    candidate.value_transformation,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|paths| ReadPath::Candidates(TranslatedReadPath { paths }))
            .unwrap_or_else(|reason| ReadPath::Refusal {
                reason,
                dd_path: hli_absolute,
                fidelity: Fidelity::Unmappable,
            }),
    }
}

/// Distinguishes a conditional merged/split conversion from an unconditional
/// lossy conversion only where a read exposes the verdict to the caller. The
/// conversion map retains its literal `lossy` declaration; ADR 0008 assigns
/// the potential-loss meaning from the selected rule kind.
fn read_fidelity(fidelity: Fidelity, rel: Option<Rel>) -> Fidelity {
    match (fidelity, rel) {
        (Fidelity::Lossy, Some(Rel::Merged | Rel::Split)) => Fidelity::PotentiallyLossy,
        _ => fidelity,
    }
}

fn translated_read_component(
    record: &crate::context_registry::ConversionRecord,
    resolved_path: &str,
    is_absolute: bool,
    fidelity: Fidelity,
    value_transformation: ValueTransformation,
) -> Result<ResolvedReadPath, String> {
    stored_c_path(record, resolved_path, is_absolute).map(|path| ResolvedReadPath {
        path,
        fidelity,
        value_transformation,
    })
}

/// Turns a resolved stored-DD path into the exact spelling IMAS-Core must
/// receive: absolute when the caller spelled its argument absolutely,
/// otherwise stripped back to this context's own stored anchor. Both
/// path-bearing seams — [`resolve_context_path`] for an arraystruct open and
/// [`translated_read_component`] for a read — decide that spelling here, so
/// the two cannot drift apart and the two refusals it can produce are worded
/// once rather than twice.
fn stored_c_path(
    record: &crate::context_registry::ConversionRecord,
    resolved_path: &str,
    is_absolute: bool,
) -> Result<CString, String> {
    let translated = if is_absolute {
        format!("/{resolved_path}")
    } else {
        let anchor = stored_anchor(record)?;
        strip_anchor(&anchor, resolved_path).ok_or_else(|| {
            "translated path does not lie beneath this context's stored anchor".to_string()
        })?
    };
    CString::new(translated)
        .map_err(|_| "translated field contains an interior NUL byte".to_string())
}

/// Resolves the context's HLI-DD anchor to its stored-DD spelling. A child
/// record deliberately retains its HLI-DD anchor, so a renamed AOS container
/// must be converted here before a relative Core argument can be formed.
fn stored_anchor(record: &crate::context_registry::ConversionRecord) -> Result<String, String> {
    if record.resolved_path.is_empty() {
        return Ok(String::new());
    }
    let Some(explanation) = record
        .map
        .resolve(&record.resolved_path, record.direction_to_stored)
    else {
        return Err("context anchor has no stored-DD conversion rule".to_string());
    };
    match concrete_stored_path(explanation.outcome) {
        ConcreteStoredPath::Path(path) => Ok(path),
        ConcreteStoredPath::NoSource => Err("context anchor has no stored source".to_string()),
        ConcreteStoredPath::Refusal(message) => Err(message),
    }
}

fn concrete_stored_path(outcome: Outcome) -> ConcreteStoredPath {
    match outcome {
        Outcome::Refusal(reason) => ConcreteStoredPath::Refusal(refusal_reason_message(reason)),
        Outcome::NoSource => ConcreteStoredPath::NoSource,
        Outcome::Path {
            resolved_path: _,
            value_transformation: _,
            candidates,
        } if !candidates.is_empty() => ConcreteStoredPath::Refusal(
            "resolving a merged/split path is not yet implemented (issue #57)".to_string(),
        ),
        Outcome::Path {
            resolved_path: _,
            value_transformation,
            candidates: _,
        } if value_transformation != ValueTransformation::None => ConcreteStoredPath::Refusal(
            "value-transform execution is not yet implemented (issue #59)".to_string(),
        ),
        Outcome::Path { resolved_path, .. } => ConcreteStoredPath::Path(resolved_path),
    }
}

/// Joins `anchor` (a context's own resolved path, in the HLI's own DD
/// spelling) with a relative `raw` path argument, or resolves `raw` from the
/// IDS root when it is absolute (a leading `/`) — the same relative-vs-
/// absolute rule every path/field argument follows (CLAUDE.md).
fn join_hli_path(anchor: &str, raw: &str) -> String {
    match raw.strip_prefix('/') {
        Some(root_relative) => root_relative.to_string(),
        None if anchor.is_empty() => raw.to_string(),
        None => format!("{anchor}/{raw}"),
    }
}

/// The portion of `resolved_path` (a full path in the stored DD's spelling)
/// past `anchor` (this context's own stored-DD path) — the field IMAS-Core
/// actually expects relative to `ctx_id`.
fn strip_anchor(anchor: &str, resolved_path: &str) -> Option<String> {
    if anchor.is_empty() {
        return Some(resolved_path.to_string());
    }
    resolved_path
        .strip_prefix(anchor)
        .and_then(|rest| rest.strip_prefix('/'))
        .map(str::to_string)
}

/// A short, stable description of one [`RefusalReason`]. Full refusal-
/// message formatting (naming the DD path and both versions, with
/// CLAUDE.md's fixed truncation order) is issue #58's contract; this seam
/// only needs `conversion_refusal`'s existing `IMAS-MVDD:`-prefixed wrapper.
fn refusal_reason_message(reason: RefusalReason) -> String {
    match reason {
        RefusalReason::UnservableRetype => {
            "this path's container changed shape and cannot be served".to_string()
        }
        RefusalReason::UnitRedefinition => {
            "this path's unit was redefined and cannot be converted".to_string()
        }
        RefusalReason::Unmappable => {
            "this path has no safe conversion between DD versions".to_string()
        }
    }
}

/// A short, stable refusal message for a write seam whose `ctx_id`
/// carries a live conversion record (ADR 0002: "If known versions differ,
/// return failure without calling IMAS-Core"). Unlike the read path, this is
/// a blanket refusal keyed only on the context, never on `field`/`path`
/// content — write-path translation is not introduced by this seam.
fn mismatched_context_write_refusal(function_name: &str) -> String {
    format!("{function_name} refuses on a context with a known DD version mismatch")
}

/// Forwards to IMAS-Core's real `al_write_data`, resolving IMAS-Core
/// lazily on first use.
///
/// When `ctx_id` names a live conversion record — a known mismatched root,
/// or a child context that inherited one — this refuses before IMAS-Core is
/// called, leaving `data` and `size` untouched. Matching, unknown,
/// unstamped, and conversion-disabled contexts carry no record and forward
/// unchanged.
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
    if REGISTRY.lookup(ctx_id).is_some() {
        return crate::conversion_refusal(&mismatched_context_write_refusal("al_write_data"));
    }
    forward_status!(write_data(
        ctx_id, field, timebase, data, datatype, dim, size,
    ))
}

/// Forwards to IMAS-Core's real `al_delete_data`, resolving IMAS-Core
/// lazily on first use.
///
/// Follows the same rule as [`write_data`]: a live conversion record on
/// `ctx_id` refuses before IMAS-Core is called; otherwise this forwards
/// unchanged.
///
/// # Safety
/// `path` must be a valid, NUL-terminated C string, or null where
/// IMAS-Core's own contract allows it.
pub(crate) unsafe fn delete_data(ctx: c_int, path: *const c_char) -> al_status_t {
    if REGISTRY.lookup(ctx).is_some() {
        return crate::conversion_refusal(&mismatched_context_write_refusal("al_delete_data"));
    }
    forward_status!(delete_data(ctx, path))
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
    forward_status!(bind_readback_plugins(ctx_id))
}

pub(crate) fn unbind_readback_plugins(ctx_id: c_int) -> al_status_t {
    forward_status!(unbind_readback_plugins(ctx_id))
}

pub(crate) unsafe fn is_plugin_registered(
    plugin_name: *const c_char,
    is_registered: *mut bool,
) -> al_status_t {
    forward_status!(is_plugin_registered(plugin_name, is_registered))
}

pub(crate) fn write_plugins_metadata(ctx_id: c_int) -> al_status_t {
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

/// Mirrors `begin_global_action`'s policy exactly (issue #67): the same
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
    let forward = |effective_datapath| {
        forward_status!(plugin_begin_global_action(
            pctx_id,
            dataobjectname,
            effective_datapath,
            rwmode,
            octx_id,
        ))
    };
    let end_on_refusal = |ctx| forward_status!(plugin_end_action(ctx));
    // SAFETY: same contract as `begin_global_action_impl`, already upheld by
    // this function's own `unsafe fn` contract.
    unsafe {
        begin_global_action_impl(
            pctx_id,
            dataobjectname,
            datapath,
            octx_id,
            forward,
            end_on_refusal,
        )
    }
}

/// Mirrors `begin_slice_action`'s policy exactly (issue #67): the same
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
    let forward = || {
        forward_status!(plugin_begin_slice_action(
            pctx_id,
            dataobjectname,
            rwmode,
            time,
            interpmode,
            octx_id,
        ))
    };
    let end_on_refusal = |ctx| forward_status!(plugin_end_action(ctx));
    // SAFETY: same contract as `begin_occurrence_action_impl`, already upheld by
    // this function's own `unsafe fn` contract.
    unsafe {
        begin_occurrence_action_impl(pctx_id, dataobjectname, octx_id, forward, end_on_refusal)
    }
}

/// Mirrors `begin_arraystruct_action`'s policy exactly (issue #67): the same
/// `path`/`timebase` resolution against the parent's conversion record and
/// the same child-record registration on success, forwarded through
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
    let forward =
        |p, t| forward_status!(plugin_begin_arraystruct_action(ctx_id, p, t, size, actx_id));
    // SAFETY: same contract as `begin_arraystruct_action_impl`, already
    // upheld by this function's own `unsafe fn` contract.
    unsafe { begin_arraystruct_action_impl(ctx_id, path, timebase, actx_id, forward) }
}

/// Mirrors `end_action`'s policy exactly (issue #67): removes only `ctx_id`'s
/// own registry record, if any, and only once IMAS-Core's own
/// `al_plugin_end_action` reports success — a refused close leaves the
/// record intact, matching `end_action`'s rule for `al_end_action`.
pub(crate) fn plugin_end_action(ctx_id: c_int) -> al_status_t {
    let status = forward_status!(plugin_end_action(ctx_id));
    if status.code == 0 {
        REGISTRY.remove(ctx_id);
    }
    status
}

/// Follows the same rule as [`write_data`] (issue #64), forwarded through
/// IMAS-Core's plugin reentry write symbol rather than its ordinary twin: a
/// live conversion record on `ctx_id` refuses before IMAS-Core is called;
/// otherwise this forwards unchanged. No path translation is introduced for
/// writes, ordinary or plugin.
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
    if REGISTRY.lookup(ctx_id).is_some() {
        return crate::conversion_refusal(&mismatched_context_write_refusal(
            "al_plugin_write_data",
        ));
    }
    forward_status!(plugin_write_data(
        ctx_id, field, timebase, data, datatype, dim, size,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_hli_path_appends_a_relative_path_under_a_nonempty_anchor() {
        assert_eq!(
            join_hli_path("time_slice", "global_quantities/beta_tor_norm"),
            "time_slice/global_quantities/beta_tor_norm"
        );
    }

    #[test]
    fn join_hli_path_uses_the_relative_path_verbatim_under_an_empty_anchor() {
        assert_eq!(join_hli_path("", "time_slice"), "time_slice");
    }

    #[test]
    fn join_hli_path_resolves_an_absolute_path_from_the_ids_root_ignoring_the_anchor() {
        assert_eq!(
            join_hli_path("time_slice", "/ids_properties/comment"),
            "ids_properties/comment"
        );
    }

    #[test]
    fn strip_anchor_returns_the_resolved_path_verbatim_under_an_empty_anchor() {
        assert_eq!(
            strip_anchor("", "time_slice/global_quantities/beta_normal"),
            Some("time_slice/global_quantities/beta_normal".to_string())
        );
    }

    #[test]
    fn strip_anchor_removes_a_matching_anchor_prefix() {
        assert_eq!(
            strip_anchor("time_slice", "time_slice/global_quantities/beta_normal"),
            Some("global_quantities/beta_normal".to_string())
        );
    }

    #[test]
    fn strip_anchor_fails_when_the_resolved_path_does_not_start_with_the_anchor() {
        assert_eq!(
            strip_anchor("time_slice", "grids_ggd/grid/space/coordinates_type"),
            None
        );
    }

    #[test]
    fn override_value_is_used_verbatim() {
        assert_eq!(
            library_path(Some("/opt/iter/lib/libal.so")),
            "/opt/iter/lib/libal.so"
        );
    }

    #[test]
    fn absent_override_falls_back_to_a_bare_soname() {
        let path = library_path(None);
        assert!(
            !path.contains('/'),
            "default library name must be a bare soname, not a path: {path}"
        );
        assert!(path.starts_with(std::env::consts::DLL_PREFIX));
        assert!(path.ends_with(std::env::consts::DLL_SUFFIX));
        assert!(path.contains("al"));
    }

    #[test]
    fn identical_versions_agree() {
        assert_eq!(check_major_version("4.1.1", "4.1.1"), Ok(None));
    }

    #[test]
    fn minor_and_patch_drift_is_tolerated() {
        assert_eq!(
            check_major_version("4.1.1", "4.2.0"),
            Ok(Some(VersionDrift {
                built_against: "4.1.1".to_string(),
                found: "4.2.0".to_string(),
            }))
        );
        assert!(check_major_version("1.0.0", "1.0.9").is_ok_and(|drift| drift.is_some()));
    }

    #[test]
    fn major_version_mismatch_names_both_versions() {
        let error = check_major_version("4.1.1", "3.22.0").unwrap_err();
        assert!(error.contains("4.1.1"));
        assert!(error.contains("3.22.0"));
    }

    #[test]
    fn unparsable_versions_are_rejected_without_panicking() {
        assert!(check_major_version("4.1.1", "not-a-version").is_err());
        assert!(check_major_version("not-a-version", "4.1.1").is_err());
        assert!(check_major_version("", "4.1.1").is_err());
    }

    #[test]
    fn failure_status_carries_a_nonzero_code_naming_the_override_variable_and_the_detail() {
        let status = failure("boom");
        assert_ne!(status.code, 0);
        let message = unsafe { CStr::from_ptr(status.message.as_ptr()) }
            .to_str()
            .unwrap();
        assert!(message.contains("boom"));
        assert!(message.contains(CORE_LIBRARY_ENV_VAR));
    }

    #[test]
    fn failure_status_truncates_an_overlong_detail_without_losing_the_override_variable() {
        // Real dlerror() text (notably on macOS) can list every search path
        // tried and run well past 256 bytes on its own. The override
        // variable must survive truncation regardless.
        let verbose_detail = "tried: ".to_string() + &"/some/very/long/search/path ".repeat(20);
        let status = failure(&verbose_detail);
        assert_eq!(status.message[MAX_ERR_MSG_LEN - 1], 0);
        let message = unsafe { CStr::from_ptr(status.message.as_ptr()) }
            .to_str()
            .unwrap();
        assert!(
            message.contains(CORE_LIBRARY_ENV_VAR),
            "override variable was truncated away: {message}"
        );
    }

    #[test]
    fn failure_status_truncates_at_a_utf8_char_boundary_instead_of_panicking() {
        let long_message = "é".repeat(200); // 2 bytes each — straddles byte 255
        let status = failure(&long_message);
        let message = unsafe { CStr::from_ptr(status.message.as_ptr()) };
        assert!(message.to_str().is_ok());
    }
}
