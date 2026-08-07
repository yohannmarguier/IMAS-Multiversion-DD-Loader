//! Runtime binding to IMAS-Core: the tracer bullet from issue #3.
//!
//! See docs/adr/0001-runtime-binding-not-linking.md. The shim never links
//! against IMAS-Core; it resolves it at runtime, once, into a private
//! symbol scope, and calls through the resolved function pointer.

use std::ffi::{CStr, c_char, c_int};
use std::sync::OnceLock;

use crate::{MAX_ERR_MSG_LEN, al_status_t};

/// A resolution failure never gets a specific taxonomy here — that's a
/// broader status-code design question (see CLAUDE.md). This just needs to
/// be reliably non-zero.
const RESOLUTION_FAILURE_CODE: std::ffi::c_int = -1;

/// Explicit override for the resolved library, checked before the bare
/// soname. Shares the crate's existing `imas_mvdd_loader_` symbol prefix
/// (see `imas_mvdd_loader_version` in lib.rs).
pub(crate) const CORE_LIBRARY_ENV_VAR: &str = "IMAS_MVDD_LOADER_CORE_LIBRARY";

/// Where to `dlopen` IMAS-Core from: the override if one was given, else a
/// bare soname resolved through the loader's normal search path. Never an
/// absolute, build-machine-specific path — nothing here is baked in at
/// compile time.
pub(crate) fn resolve_library_path(override_value: Option<&str>) -> String {
    match override_value {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => default_core_library_name(),
    }
}

#[cfg(target_os = "macos")]
fn default_core_library_name() -> String {
    "libal.dylib".to_string()
}

#[cfg(not(target_os = "macos"))]
fn default_core_library_name() -> String {
    "libal.so".to_string()
}

/// IMAS-Core AL version this shim was built to call. CMake passes this from
/// `IMAS_MVDD_EXPECTED_AL_VERSION` to cargo-c, so a deployed shim gates the
/// library it was configured for rather than a source-code placeholder. The
/// fallback keeps direct `cargo test` usable with the recording stub.
pub(crate) const EXPECTED_AL_VERSION: &str = match option_env!("IMAS_MVDD_EXPECTED_AL_VERSION") {
    Some(version) => version,
    None => "1.0.0",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VersionOutcome {
    Compatible,
    ToleratedDrift,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VersionError {
    Malformed { version: String },
    MajorMismatch { expected: String, actual: String },
}

fn major_component(version: &str) -> Option<&str> {
    let major = version.split('.').next()?;
    if !major.is_empty() && major.bytes().all(|b| b.is_ascii_digit()) {
        Some(major)
    } else {
        None
    }
}

/// Compares the version this shim was built against with the version an
/// IMAS-Core it just resolved reports. Only `major` gates: a mismatch there
/// means the ABI itself may disagree, so resolution must fail; minor/patch
/// drift is recorded (by the caller) and tolerated.
pub(crate) fn check_version_compatibility(
    expected: &str,
    actual: &str,
) -> Result<VersionOutcome, VersionError> {
    let expected_major = major_component(expected).ok_or_else(|| VersionError::Malformed {
        version: expected.to_string(),
    })?;
    let actual_major = major_component(actual).ok_or_else(|| VersionError::Malformed {
        version: actual.to_string(),
    })?;

    if expected_major != actual_major {
        return Err(VersionError::MajorMismatch {
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }

    if expected == actual {
        Ok(VersionOutcome::Compatible)
    } else {
        Ok(VersionOutcome::ToleratedDrift)
    }
}

/// Wraps a failure detail with the one piece of advice every resolution
/// failure needs to carry: which environment variable overrides it. That
/// advice comes *first* — `detail` comes from `dlerror()`, which on some
/// platforms (macOS notably) lists every search path tried and can run well
/// past the ABI's 256-byte message buffer on its own, so whatever is placed
/// after it is exactly what truncation discards.
pub(crate) fn format_resolution_failure_message(detail: &str) -> String {
    format!("set {CORE_LIBRARY_ENV_VAR} to override IMAS-Core resolution; {detail}")
}

/// Shorthand for the failure shape every resolution step shares: wrap a
/// detail, format it, and build the `al_status_t` from it.
fn resolution_failure(detail: &str) -> al_status_t {
    error_status(&format_resolution_failure_message(detail))
}

pub(crate) fn format_major_mismatch_message(expected: &str, actual: &str) -> String {
    format!(
        "IMAS-Core major version mismatch: this shim was built against {expected}, \
         the resolved library reports {actual}"
    )
}

/// Builds a failure `al_status_t` from `message`, truncating at a UTF-8
/// character boundary if it doesn't fit the fixed-size ABI buffer — this
/// must never panic or overflow, however long `message` is.
pub(crate) fn error_status(message: &str) -> al_status_t {
    let mut status = al_status_t {
        code: RESOLUTION_FAILURE_CODE,
        ..al_status_t::default()
    };
    write_message(&mut status.message, message);
    status
}

fn write_message(buffer: &mut [std::ffi::c_char; MAX_ERR_MSG_LEN], message: &str) {
    let capacity = MAX_ERR_MSG_LEN - 1;
    let mut end = message.len().min(capacity);
    while end > 0 && !message.is_char_boundary(end) {
        end -= 1;
    }
    for (slot, byte) in buffer.iter_mut().zip(message.as_bytes()[..end].iter()) {
        *slot = *byte as std::ffi::c_char;
    }
    buffer[end] = 0;
}

#[cfg(unix)]
type ContextInfoFn = unsafe extern "C" fn(c_int, *mut *mut c_char) -> al_status_t;

#[cfg(unix)]
struct CoreBinding {
    // Never dlclose'd — see [`crate::dl::Library`]. Kept only to document
    // that this binding owns the resolved library for the process lifetime.
    _library: crate::dl::Library,
    context_info: ContextInfoFn,
}

// `context_info` is a plain function pointer into a library that is never
// unloaded; calling through it concurrently is exactly what an ordinary
// linked call would do.
#[cfg(unix)]
unsafe impl Sync for CoreBinding {}

#[cfg(unix)]
static CORE: OnceLock<Result<CoreBinding, al_status_t>> = OnceLock::new();

#[cfg(unix)]
fn resolve() -> &'static Result<CoreBinding, al_status_t> {
    CORE.get_or_init(resolve_uncached)
}

// `al_status_t` is the ABI struct itself, not an internal error type wrapped
// for convenience — returning it by value from `Err` is the point, not an
// oversight to box away.
#[cfg(unix)]
#[allow(clippy::result_large_err)]
fn resolve_uncached() -> Result<CoreBinding, al_status_t> {
    let override_value = std::env::var(CORE_LIBRARY_ENV_VAR).ok();
    let library_path = resolve_library_path(override_value.as_deref());

    let library = crate::dl::Library::open(&library_path).map_err(|underlying| {
        resolution_failure(&format!(
            "failed to open IMAS-Core library '{library_path}': {underlying}"
        ))
    })?;

    let get_al_version: unsafe extern "C" fn() -> *const c_char =
        unsafe { resolve_symbol(&library, &library_path, "getALVersion")? };
    let version_ptr = unsafe { get_al_version() };
    if version_ptr.is_null() {
        return Err(resolution_failure(
            "IMAS-Core's getALVersion() returned a null string",
        ));
    }
    let actual_version = unsafe { CStr::from_ptr(version_ptr) }
        .to_string_lossy()
        .into_owned();

    match check_version_compatibility(EXPECTED_AL_VERSION, &actual_version) {
        Ok(VersionOutcome::Compatible) => {}
        Ok(VersionOutcome::ToleratedDrift) => {
            eprintln!(
                "imas-mvdd-loader: IMAS-Core reports version {actual_version}, this shim was \
                 built against {EXPECTED_AL_VERSION} — minor/patch drift tolerated"
            );
        }
        Err(VersionError::MajorMismatch { expected, actual }) => {
            return Err(error_status(&format_major_mismatch_message(
                &expected, &actual,
            )));
        }
        Err(VersionError::Malformed { version }) => {
            return Err(resolution_failure(&format!(
                "IMAS-Core reported an unparsable version string '{version}'"
            )));
        }
    }

    let context_info: ContextInfoFn =
        unsafe { resolve_symbol(&library, &library_path, "al_context_info")? };

    Ok(CoreBinding {
        _library: library,
        context_info,
    })
}

/// # Safety
/// The caller must know that `symbol_name` in `library` really has the
/// signature `F`.
#[cfg(unix)]
#[allow(clippy::result_large_err)]
unsafe fn resolve_symbol<F: Copy>(
    library: &crate::dl::Library,
    library_path: &str,
    symbol_name: &str,
) -> Result<F, al_status_t> {
    let raw = unsafe { library.symbol(symbol_name) }.map_err(|underlying| {
        resolution_failure(&format!(
            "IMAS-Core library '{library_path}' has no symbol '{symbol_name}': {underlying}"
        ))
    })?;
    if raw.is_null() {
        return Err(resolution_failure(&format!(
            "IMAS-Core library '{library_path}' resolved '{symbol_name}' to a null address"
        )));
    }
    // SAFETY: forwarded to the caller's own safety contract on `F`.
    Ok(unsafe { std::mem::transmute_copy(&raw) })
}

/// Mirrors IMAS-Core's `al_context_info` exactly — same name, same
/// signature. Resolves IMAS-Core once, on first call, into a private
/// symbol scope, then forwards every call to the resolved implementation.
/// A process that never calls this (or any other mirrored entry point)
/// never requires IMAS-Core to be present.
#[cfg(unix)]
#[unsafe(no_mangle)]
pub extern "C" fn al_context_info(ctx: c_int, info: *mut *mut c_char) -> al_status_t {
    match resolve() {
        Ok(binding) => unsafe { (binding.context_info)(ctx, info) },
        Err(status) => *status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn override_value_is_used_verbatim() {
        assert_eq!(
            resolve_library_path(Some("/opt/iter/lib/libal.so")),
            "/opt/iter/lib/libal.so"
        );
    }

    #[test]
    fn absent_override_falls_back_to_a_bare_soname() {
        let path = resolve_library_path(None);
        assert!(
            !path.contains('/'),
            "default library name must be a bare soname, not a path: {path}"
        );
    }

    #[test]
    fn empty_override_is_treated_as_absent() {
        assert_eq!(resolve_library_path(Some("")), resolve_library_path(None));
    }

    #[test]
    fn identical_versions_are_compatible() {
        assert_eq!(
            check_version_compatibility("4.1.1", "4.1.1"),
            Ok(VersionOutcome::Compatible)
        );
    }

    #[test]
    fn same_major_different_minor_or_patch_is_tolerated_drift() {
        assert_eq!(
            check_version_compatibility("4.1.1", "4.2.0"),
            Ok(VersionOutcome::ToleratedDrift)
        );
    }

    #[test]
    fn different_major_is_a_hard_mismatch() {
        assert_eq!(
            check_version_compatibility("4.1.1", "3.22.0"),
            Err(VersionError::MajorMismatch {
                expected: "4.1.1".to_string(),
                actual: "3.22.0".to_string(),
            })
        );
    }

    #[test]
    fn unparsable_version_is_rejected_without_panicking() {
        assert_eq!(
            check_version_compatibility("4.1.1", "not-a-version"),
            Err(VersionError::Malformed {
                version: "not-a-version".to_string(),
            })
        );
    }

    #[test]
    fn resolution_failure_message_names_the_failure_and_the_override_variable() {
        let message =
            format_resolution_failure_message("failed to open IMAS-Core library 'libal.so': boom");
        assert!(message.contains("boom"));
        assert!(message.contains(CORE_LIBRARY_ENV_VAR));
    }

    #[test]
    fn override_variable_survives_truncation_even_with_a_verbose_underlying_error() {
        // Real dlerror() text (notably on macOS) can list every search path
        // tried and run well past 256 bytes on its own — the override
        // variable must not be the part that gets cut off.
        let verbose_underlying = "tried: ".to_string() + &"/some/very/long/search/path ".repeat(20);
        let message = format_resolution_failure_message(&verbose_underlying);
        let status = error_status(&message);
        let truncated = unsafe { CStr::from_ptr(status.message.as_ptr()) }
            .to_str()
            .unwrap();
        assert!(
            truncated.contains(CORE_LIBRARY_ENV_VAR),
            "override variable was truncated away: {truncated}"
        );
    }

    #[test]
    fn major_mismatch_message_names_both_versions() {
        let message = format_major_mismatch_message("4.1.1", "3.22.0");
        assert!(message.contains("4.1.1"));
        assert!(message.contains("3.22.0"));
    }

    #[test]
    fn error_status_carries_a_nonzero_code_and_the_message() {
        let status = error_status("something went wrong");
        assert_ne!(status.code, 0);
        let message = unsafe { CStr::from_ptr(status.message.as_ptr()) };
        assert_eq!(message.to_str().unwrap(), "something went wrong");
    }

    #[test]
    fn error_status_truncates_an_overlong_message_instead_of_overflowing() {
        let long_message = "x".repeat(1000);
        let status = error_status(&long_message);
        let message = unsafe { CStr::from_ptr(status.message.as_ptr()) };
        assert_eq!(message.to_bytes().len(), MAX_ERR_MSG_LEN - 1);
    }
}
