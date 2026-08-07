//! Runtime resolution of IMAS-Core.
//!
//! Proven end to end on one symbol, `al_context_info` (issue #3): the shim
//! carries no link-time dependency on IMAS-Core. On first use it opens
//! IMAS-Core with local symbol visibility and resolves each function's
//! address through that specific library handle, so the shim's own
//! globally visible exports are never in the lookup scope and can't
//! capture its outbound calls. See
//! `docs/adr/0001-runtime-binding-not-linking.md`.

use std::env;
use std::ffi::{CStr, c_char, c_int};
use std::sync::OnceLock;

use crate::dl::Library;
use crate::{MAX_ERR_MSG_LEN, al_status_t};

/// Explicit override, honoured before the bare soname — see the ADR's
/// resolution order.
const CORE_LIBRARY_ENV_VAR: &str = "IMAS_CORE_LIBRARY";

/// IMAS-Core version this shim was built against. Real acquisition-time
/// pinning (CMake fetching and threading through a specific tag) arrives
/// with the ITER-style acquisition work tracked in issue #1; this constant
/// stands in until that lands.
const BUILT_AGAINST_VERSION: &str = "1.0.0";

type ContextInfoFn = unsafe extern "C" fn(c_int, *mut *mut c_char) -> al_status_t;
type GetAlVersionFn = unsafe extern "C" fn() -> *const c_char;

struct CoreBinding {
    // Kept alive for the process's lifetime: dropping it would unmap
    // `context_info`. Never read again once resolution succeeds.
    _library: Library,
    context_info: ContextInfoFn,
}

static CORE: OnceLock<Result<CoreBinding, al_status_t>> = OnceLock::new();

/// Resolves IMAS-Core, once, memoized for the process's lifetime. A process
/// that never calls a mirrored ABI function never runs this.
fn core() -> &'static Result<CoreBinding, al_status_t> {
    CORE.get_or_init(resolve)
}

// `al_status_t` is the ABI struct itself, not an internal error type boxed
// away for convenience — returning it by value from `Err` is the point.
#[allow(clippy::result_large_err)]
fn resolve() -> Result<CoreBinding, al_status_t> {
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
    let get_al_version: GetAlVersionFn =
        unsafe { resolve_symbol(&library, &path, "getALVersion") }?;
    let found_version = unsafe { get_al_version() };
    if found_version.is_null() {
        return Err(failure(&format!(
            "IMAS-Core library '{path}' returned a null pointer from 'getALVersion'"
        )));
    }
    let found_version = unsafe { CStr::from_ptr(found_version) }
        .to_string_lossy()
        .into_owned();

    if let Err(detail) = check_major_version(BUILT_AGAINST_VERSION, &found_version) {
        return Err(failure(&detail));
    }

    let context_info: ContextInfoFn =
        unsafe { resolve_symbol(&library, &path, "al_context_info") }?;

    Ok(CoreBinding {
        _library: library,
        context_info,
    })
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
fn check_major_version(built_against: &str, found: &str) -> Result<(), String> {
    let built_major = major_component(built_against);
    let found_major = major_component(found);

    match (built_major, found_major) {
        (Some(b), Some(f)) if b == f => {
            if built_against != found {
                // No logging infrastructure exists yet; this is the
                // "recorded" half of "recorded and tolerated" until one
                // does.
                eprintln!(
                    "imas-mvdd-loader: tolerating IMAS-Core version drift (built against {built_against}, found {found})"
                );
            }
            Ok(())
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
        assert_eq!(check_major_version("4.1.1", "4.1.1"), Ok(()));
    }

    #[test]
    fn minor_and_patch_drift_is_tolerated() {
        assert_eq!(check_major_version("4.1.1", "4.2.0"), Ok(()));
        assert_eq!(check_major_version("1.0.0", "1.0.9"), Ok(()));
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
