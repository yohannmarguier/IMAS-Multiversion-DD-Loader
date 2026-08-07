//! Runtime resolution of IMAS-Core.
//!
//! Proven end to end on `al_context_info` (issue #3), then extended to the
//! data-entry, action-lifecycle and data-operation symbols below (issue
//! #6): the shim carries no link-time dependency on IMAS-Core. On first use
//! it opens IMAS-Core with local symbol visibility and resolves each
//! function's address through that specific library handle, so the shim's
//! own globally visible exports are never in the lookup scope and can't
//! capture its outbound calls. See
//! `docs/adr/0001-runtime-binding-not-linking.md`.

use std::env;
use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::sync::OnceLock;

use crate::dl::Library;
use crate::{MAX_ERR_MSG_LEN, al_status_t};

/// Explicit override, honoured before the bare soname — see the ADR's
/// resolution order.
const CORE_LIBRARY_ENV_VAR: &str = "IMAS_CORE_LIBRARY";

/// Supported IMAS-Core release, sourced by `build.rs` from the repository's
/// `IMAS_CORE_VERSION` pin.
const BUILT_AGAINST_VERSION: &str = env!("IMAS_CORE_VERSION");

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

struct CoreBinding {
    // Kept alive for the process's lifetime: dropping it would unmap the
    // resolved function pointers below. Never read again once resolution
    // succeeds.
    _library: Library,
    context_info: ContextInfoFn,
    get_backend_id: GetBackendIdFn,
    build_uri_from_legacy_parameters: BuildUriFromLegacyParametersFn,
    const2str: StringLookupFn,
    err2str: StringLookupFn,
    get_al_version: VersionAccessorFn,
    get_dd_version: VersionAccessorFn,
    begin_dataentry_action: BeginDataentryActionFn,
    close_pulse: ClosePulseFn,
    begin_global_action: BeginGlobalActionFn,
    begin_slice_action: BeginSliceActionFn,
    begin_timerange_action: BeginTimerangeActionFn,
    begin_arraystruct_action: BeginArraystructActionFn,
    end_action: EndActionFn,
    read_data: ReadDataFn,
    write_data: WriteDataFn,
    delete_data: DeleteDataFn,
    iterate_over_arraystruct: IterateOverArraystructFn,
    get_occurrences: GetOccurrencesFn,
    list_filled_paths: ListFilledPathsFn,
    register_plugin: PluginNameFn,
    unregister_plugin: PluginNameFn,
    bind_plugin: BindPluginFn,
    unbind_plugin: BindPluginFn,
    bind_readback_plugins: PluginContextFn,
    unbind_readback_plugins: PluginContextFn,
    is_plugin_registered: IsPluginRegisteredFn,
    write_plugins_metadata: PluginContextFn,
    setvalue_parameter_plugin: SetvalueParameterPluginFn,
    setvalue_int_scalar_parameter_plugin: SetvalueIntScalarParameterPluginFn,
    setvalue_double_scalar_parameter_plugin: SetvalueDoubleScalarParameterPluginFn,
    plugin_begin_global_action: BeginGlobalActionFn,
    plugin_begin_slice_action: BeginSliceActionFn,
    plugin_begin_arraystruct_action: BeginArraystructActionFn,
    plugin_end_action: EndActionFn,
    plugin_read_data: ReadDataFn,
    plugin_write_data: WriteDataFn,
    // Retained with the binding so tolerated compatibility drift remains
    // recorded for the process lifetime after its diagnostic is emitted.
    _version_drift: Option<VersionDrift>,
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

// `al_status_t` is the ABI struct itself, not an internal error type boxed
// away for convenience — returning it by value from `Err` is the point.
#[allow(clippy::result_large_err)]
fn resolve() -> Result<CoreBinding, ResolutionError> {
    let path = library_path(env::var(CORE_LIBRARY_ENV_VAR).ok().as_deref());

    let library = Library::open(&path).map_err(|underlying| {
        failure(&format!(
            "failed to open IMAS-Core library '{path}': {underlying}"
        ))
    })?;

    // getALVersion is the bootstrap symbol: it must be resolved, and its
    // report checked, before any other IMAS-Core symbol is resolved at all
    // (see the ADR's Consequences) — a major mismatch means the ABI itself
    // may disagree, so nothing past this point can be trusted.
    let get_al_version: VersionAccessorFn =
        unsafe { resolve_symbol(&library, &path, "getALVersion") }?;
    let found_version = unsafe { get_al_version() };
    if found_version.is_null() {
        return Err(failure(&format!(
            "IMAS-Core library '{path}' returned a null pointer from 'getALVersion'"
        ))
        .into());
    }
    let found_version = unsafe { CStr::from_ptr(found_version) };
    let found_version_text = found_version.to_string_lossy();
    let version_drift = match check_major_version(BUILT_AGAINST_VERSION, &found_version_text) {
        Ok(version_drift) => version_drift,
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
    };
    if let Some(drift) = &version_drift {
        drift.record();
    }

    let context_info: ContextInfoFn =
        unsafe { resolve_symbol(&library, &path, "al_context_info") }?;
    let get_backend_id: GetBackendIdFn =
        unsafe { resolve_symbol(&library, &path, "al_get_backendID") }?;
    let build_uri_from_legacy_parameters: BuildUriFromLegacyParametersFn =
        unsafe { resolve_symbol(&library, &path, "al_build_uri_from_legacy_parameters") }?;
    let const2str: StringLookupFn = unsafe { resolve_symbol(&library, &path, "const2str") }?;
    let err2str: StringLookupFn = unsafe { resolve_symbol(&library, &path, "err2str") }?;
    let get_dd_version: VersionAccessorFn =
        unsafe { resolve_symbol(&library, &path, "getDDVersion") }?;
    let begin_dataentry_action: BeginDataentryActionFn =
        unsafe { resolve_symbol(&library, &path, "al_begin_dataentry_action") }?;
    let close_pulse: ClosePulseFn = unsafe { resolve_symbol(&library, &path, "al_close_pulse") }?;
    let begin_global_action: BeginGlobalActionFn =
        unsafe { resolve_symbol(&library, &path, "al_begin_global_action") }?;
    let begin_slice_action: BeginSliceActionFn =
        unsafe { resolve_symbol(&library, &path, "al_begin_slice_action") }?;
    let begin_timerange_action: BeginTimerangeActionFn =
        unsafe { resolve_symbol(&library, &path, "al_begin_timerange_action") }?;
    let begin_arraystruct_action: BeginArraystructActionFn =
        unsafe { resolve_symbol(&library, &path, "al_begin_arraystruct_action") }?;
    let end_action: EndActionFn = unsafe { resolve_symbol(&library, &path, "al_end_action") }?;
    let read_data: ReadDataFn = unsafe { resolve_symbol(&library, &path, "al_read_data") }?;
    let write_data: WriteDataFn = unsafe { resolve_symbol(&library, &path, "al_write_data") }?;
    let delete_data: DeleteDataFn = unsafe { resolve_symbol(&library, &path, "al_delete_data") }?;
    let iterate_over_arraystruct: IterateOverArraystructFn =
        unsafe { resolve_symbol(&library, &path, "al_iterate_over_arraystruct") }?;
    let get_occurrences: GetOccurrencesFn =
        unsafe { resolve_symbol(&library, &path, "al_get_occurrences") }?;
    let list_filled_paths: ListFilledPathsFn =
        unsafe { resolve_symbol(&library, &path, "al_list_filled_paths") }?;
    let register_plugin: PluginNameFn =
        unsafe { resolve_symbol(&library, &path, "al_register_plugin") }?;
    let unregister_plugin: PluginNameFn =
        unsafe { resolve_symbol(&library, &path, "al_unregister_plugin") }?;
    let bind_plugin: BindPluginFn = unsafe { resolve_symbol(&library, &path, "al_bind_plugin") }?;
    let unbind_plugin: BindPluginFn =
        unsafe { resolve_symbol(&library, &path, "al_unbind_plugin") }?;
    let bind_readback_plugins: PluginContextFn =
        unsafe { resolve_symbol(&library, &path, "al_bind_readback_plugins") }?;
    let unbind_readback_plugins: PluginContextFn =
        unsafe { resolve_symbol(&library, &path, "al_unbind_readback_plugins") }?;
    let is_plugin_registered: IsPluginRegisteredFn =
        unsafe { resolve_symbol(&library, &path, "al_is_plugin_registered") }?;
    let write_plugins_metadata: PluginContextFn =
        unsafe { resolve_symbol(&library, &path, "al_write_plugins_metadata") }?;
    let setvalue_parameter_plugin: SetvalueParameterPluginFn =
        unsafe { resolve_symbol(&library, &path, "al_setvalue_parameter_plugin") }?;
    let setvalue_int_scalar_parameter_plugin: SetvalueIntScalarParameterPluginFn =
        unsafe { resolve_symbol(&library, &path, "al_setvalue_int_scalar_parameter_plugin") }?;
    let setvalue_double_scalar_parameter_plugin: SetvalueDoubleScalarParameterPluginFn = unsafe {
        resolve_symbol(
            &library,
            &path,
            "al_setvalue_double_scalar_parameter_plugin",
        )
    }?;
    let plugin_begin_global_action: BeginGlobalActionFn =
        unsafe { resolve_symbol(&library, &path, "al_plugin_begin_global_action") }?;
    let plugin_begin_slice_action: BeginSliceActionFn =
        unsafe { resolve_symbol(&library, &path, "al_plugin_begin_slice_action") }?;
    let plugin_begin_arraystruct_action: BeginArraystructActionFn =
        unsafe { resolve_symbol(&library, &path, "al_plugin_begin_arraystruct_action") }?;
    let plugin_end_action: EndActionFn =
        unsafe { resolve_symbol(&library, &path, "al_plugin_end_action") }?;
    let plugin_read_data: ReadDataFn =
        unsafe { resolve_symbol(&library, &path, "al_plugin_read_data") }?;
    let plugin_write_data: WriteDataFn =
        unsafe { resolve_symbol(&library, &path, "al_plugin_write_data") }?;

    Ok(CoreBinding {
        _library: library,
        context_info,
        get_backend_id,
        build_uri_from_legacy_parameters,
        const2str,
        err2str,
        get_al_version,
        get_dd_version,
        begin_dataentry_action,
        close_pulse,
        begin_global_action,
        begin_slice_action,
        begin_timerange_action,
        begin_arraystruct_action,
        end_action,
        read_data,
        write_data,
        delete_data,
        iterate_over_arraystruct,
        get_occurrences,
        list_filled_paths,
        register_plugin,
        unregister_plugin,
        bind_plugin,
        unbind_plugin,
        bind_readback_plugins,
        unbind_readback_plugins,
        is_plugin_registered,
        write_plugins_metadata,
        setvalue_parameter_plugin,
        setvalue_int_scalar_parameter_plugin,
        setvalue_double_scalar_parameter_plugin,
        plugin_begin_global_action,
        plugin_begin_slice_action,
        plugin_begin_arraystruct_action,
        plugin_end_action,
        plugin_read_data,
        plugin_write_data,
        _version_drift: version_drift,
    })
}

pub(crate) unsafe fn get_backend_id(ctx: c_int, backend_id: *mut c_int) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.get_backend_id)(ctx, backend_id) },
        Err(status) => *status,
    }
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
    match core() {
        Ok(binding) => unsafe {
            (binding.build_uri_from_legacy_parameters)(
                backend_id, pulse, run, user, tokamak, version, options, uri,
            )
        },
        Err(status) => *status,
    }
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
const CHAR_DATA_ID: c_int = 50;
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

/// # Safety
/// The caller is responsible for `symbol_name` in `library` really having
/// signature `F`.
#[allow(clippy::result_large_err)]
unsafe fn resolve_symbol<F: Copy>(
    library: &Library,
    library_path: &str,
    symbol_name: &str,
) -> Result<F, al_status_t> {
    let address = unsafe { library.symbol(symbol_name) }.map_err(|underlying| {
        failure(&format!(
            "IMAS-Core library '{library_path}' has no '{symbol_name}': {underlying}"
        ))
    })?;
    if address.is_null() {
        return Err(failure(&format!(
            "IMAS-Core library '{library_path}' resolved '{symbol_name}' to a null address"
        )));
    }
    // SAFETY: forwarded to this function's own safety contract on `F`.
    Ok(unsafe { std::mem::transmute_copy(&address) })
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
    write_truncated(&mut status.message, &message);
    status
}

fn write_truncated(buffer: &mut [c_char; MAX_ERR_MSG_LEN], message: &str) {
    let capacity = MAX_ERR_MSG_LEN - 1; // always leave room for the NUL
    let mut len = message.len().min(capacity);
    while len > 0 && !message.is_char_boundary(len) {
        len -= 1;
    }
    for (slot, byte) in buffer.iter_mut().zip(message.as_bytes()[..len].iter()) {
        *slot = *byte as c_char;
    }
}

/// Forwards to IMAS-Core's real `al_context_info`, resolving IMAS-Core
/// lazily on first use.
///
/// # Safety
/// `info` must be a valid, writable `*mut *mut c_char`, or null, matching
/// IMAS-Core's own contract for this function.
pub(crate) unsafe fn context_info(ctx: c_int, info: *mut *mut c_char) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.context_info)(ctx, info) },
        Err(status) => *status,
    }
}

/// Forwards to IMAS-Core's real `al_begin_dataentry_action`, resolving
/// IMAS-Core lazily on first use.
///
/// # Safety
/// `uri` must be a valid, NUL-terminated C string. `dectxID` must be a
/// valid, writable `*mut c_int`, matching IMAS-Core's own contract.
pub(crate) unsafe fn begin_dataentry_action(
    uri: *const c_char,
    mode: c_int,
    dectx_id: *mut c_int,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.begin_dataentry_action)(uri, mode, dectx_id) },
        Err(status) => *status,
    }
}

/// Forwards to IMAS-Core's real `al_close_pulse`, resolving IMAS-Core
/// lazily on first use.
pub(crate) fn close_pulse(pulse_ctx: c_int, mode: c_int) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.close_pulse)(pulse_ctx, mode) },
        Err(status) => *status,
    }
}

/// Forwards to IMAS-Core's real `al_begin_global_action`, resolving
/// IMAS-Core lazily on first use.
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
    match core() {
        Ok(binding) => unsafe {
            (binding.begin_global_action)(pctx_id, dataobjectname, datapath, rwmode, octx_id)
        },
        Err(status) => *status,
    }
}

/// Forwards to IMAS-Core's real `al_begin_slice_action`, resolving
/// IMAS-Core lazily on first use.
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
    match core() {
        Ok(binding) => unsafe {
            (binding.begin_slice_action)(pctx_id, dataobjectname, rwmode, time, interpmode, octx_id)
        },
        Err(status) => *status,
    }
}

/// Forwards to IMAS-Core's real `al_begin_timerange_action`, resolving
/// IMAS-Core lazily on first use.
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
    match core() {
        Ok(binding) => unsafe {
            (binding.begin_timerange_action)(
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
        },
        Err(status) => *status,
    }
}

/// Forwards to IMAS-Core's real `al_begin_arraystruct_action`, resolving
/// IMAS-Core lazily on first use.
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
    match core() {
        Ok(binding) => unsafe {
            (binding.begin_arraystruct_action)(ctx_id, path, timebase, size, actx_id)
        },
        Err(status) => *status,
    }
}

/// Forwards to IMAS-Core's real `al_end_action`, resolving IMAS-Core
/// lazily on first use.
pub(crate) fn end_action(ctx_id: c_int) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.end_action)(ctx_id) },
        Err(status) => *status,
    }
}

/// Forwards to IMAS-Core's real `al_read_data`, resolving IMAS-Core lazily
/// on first use.
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
    match core() {
        Ok(binding) => unsafe {
            (binding.read_data)(ctx_id, field, timebase, data, datatype, dim, size)
        },
        Err(status) => *status,
    }
}

/// Forwards to IMAS-Core's real `al_write_data`, resolving IMAS-Core
/// lazily on first use.
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
    match core() {
        Ok(binding) => unsafe {
            (binding.write_data)(ctx_id, field, timebase, data, datatype, dim, size)
        },
        Err(status) => *status,
    }
}

/// Forwards to IMAS-Core's real `al_delete_data`, resolving IMAS-Core
/// lazily on first use.
///
/// # Safety
/// `path` must be a valid, NUL-terminated C string, or null where
/// IMAS-Core's own contract allows it.
pub(crate) unsafe fn delete_data(ctx: c_int, path: *const c_char) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.delete_data)(ctx, path) },
        Err(status) => *status,
    }
}

/// Forwards to IMAS-Core's real `al_iterate_over_arraystruct`, resolving
/// IMAS-Core lazily on first use.
pub(crate) fn iterate_over_arraystruct(aosctx: c_int, step: c_int) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.iterate_over_arraystruct)(aosctx, step) },
        Err(status) => *status,
    }
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
    match core() {
        Ok(binding) => unsafe {
            (binding.get_occurrences)(pctx_id, ids_name, occurrences_list, size)
        },
        Err(status) => *status,
    }
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
    match core() {
        Ok(binding) => unsafe {
            (binding.list_filled_paths)(pctx_id, dataobjectname, path_list, size)
        },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn register_plugin(plugin_name: *const c_char) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.register_plugin)(plugin_name) },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn unregister_plugin(plugin_name: *const c_char) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.unregister_plugin)(plugin_name) },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn bind_plugin(
    field_path: *const c_char,
    plugin_name: *const c_char,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.bind_plugin)(field_path, plugin_name) },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn unbind_plugin(
    field_path: *const c_char,
    plugin_name: *const c_char,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.unbind_plugin)(field_path, plugin_name) },
        Err(status) => *status,
    }
}

pub(crate) fn bind_readback_plugins(ctx_id: c_int) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.bind_readback_plugins)(ctx_id) },
        Err(status) => *status,
    }
}

pub(crate) fn unbind_readback_plugins(ctx_id: c_int) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.unbind_readback_plugins)(ctx_id) },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn is_plugin_registered(
    plugin_name: *const c_char,
    is_registered: *mut bool,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.is_plugin_registered)(plugin_name, is_registered) },
        Err(status) => *status,
    }
}

pub(crate) fn write_plugins_metadata(ctx_id: c_int) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.write_plugins_metadata)(ctx_id) },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn setvalue_parameter_plugin(
    parameter_name: *const c_char,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
    data: *mut c_void,
    plugin_name: *const c_char,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe {
            (binding.setvalue_parameter_plugin)(
                parameter_name,
                datatype,
                dim,
                size,
                data,
                plugin_name,
            )
        },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn setvalue_int_scalar_parameter_plugin(
    parameter_name: *const c_char,
    parameter_value: c_int,
    plugin_name: *const c_char,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe {
            (binding.setvalue_int_scalar_parameter_plugin)(
                parameter_name,
                parameter_value,
                plugin_name,
            )
        },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn setvalue_double_scalar_parameter_plugin(
    parameter_name: *const c_char,
    parameter_value: c_double,
    plugin_name: *const c_char,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe {
            (binding.setvalue_double_scalar_parameter_plugin)(
                parameter_name,
                parameter_value,
                plugin_name,
            )
        },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn plugin_begin_global_action(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    datapath: *const c_char,
    rwmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe {
            (binding.plugin_begin_global_action)(pctx_id, dataobjectname, datapath, rwmode, octx_id)
        },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn plugin_begin_slice_action(
    pctx_id: c_int,
    dataobjectname: *const c_char,
    rwmode: c_int,
    time: c_double,
    interpmode: c_int,
    octx_id: *mut c_int,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe {
            (binding.plugin_begin_slice_action)(
                pctx_id,
                dataobjectname,
                rwmode,
                time,
                interpmode,
                octx_id,
            )
        },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn plugin_begin_arraystruct_action(
    ctx_id: c_int,
    path: *const c_char,
    timebase: *const c_char,
    size: *mut c_int,
    actx_id: *mut c_int,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe {
            (binding.plugin_begin_arraystruct_action)(ctx_id, path, timebase, size, actx_id)
        },
        Err(status) => *status,
    }
}

pub(crate) fn plugin_end_action(ctx_id: c_int) -> al_status_t {
    match core() {
        Ok(binding) => unsafe { (binding.plugin_end_action)(ctx_id) },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn plugin_read_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe {
            (binding.plugin_read_data)(ctx_id, field, timebase, data, datatype, dim, size)
        },
        Err(status) => *status,
    }
}

pub(crate) unsafe fn plugin_write_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    match core() {
        Ok(binding) => unsafe {
            (binding.plugin_write_data)(ctx_id, field, timebase, data, datatype, dim, size)
        },
        Err(status) => *status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
