//! Minimal POSIX `dlopen`/`dlsym`/`dlerror` bindings.
//!
//! Hand-rolled instead of a crate dependency: the shim needs exactly three
//! functions, and this project only ever deploys to the ITER cluster
//! (Linux) or runs locally on macOS for development — POSIX `dlfcn.h` is
//! available on both, and no Windows target exists. See
//! `docs/adr/0001-runtime-binding-not-linking.md` for why the shim resolves
//! IMAS-Core this way at all.

use std::ffi::{CStr, CString, c_char, c_int, c_void};

#[cfg_attr(target_os = "linux", link(name = "dl"))]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *mut c_char;
}

// RTLD_NOW is 2 on both Linux and macOS, but RTLD_LOCAL is not: it is the
// *absence* of RTLD_GLOBAL (0) on Linux, while on macOS it is its own bit
// (0x4) — leaving it unset there does not mean "local", it means dyld picks
// its own historical default. Getting this wrong on macOS would silently
// drop the private-scope property the runtime-binding design relies on
// (see the ADR), so it is not left to a single shared constant.
#[cfg(target_os = "macos")]
const RTLD_NOW_LOCAL: c_int = 0x2 | 0x4;
#[cfg(not(target_os = "macos"))]
const RTLD_NOW_LOCAL: c_int = 0x2;

/// A shared library opened into a private (`RTLD_LOCAL`) symbol scope, so
/// its exports never enter the process-wide symbol table.
///
/// Deliberately has no `Drop` impl: the handle is meant to live for the
/// process's lifetime, and every function pointer resolved from it must
/// stay valid for as long as it is called through. `dlclose`-ing it would
/// be the bug, not leaking it.
pub(crate) struct Library {
    handle: *mut c_void,
}

// The handle is an opaque address, fixed after `open`, used only to look up
// symbols. Concurrent `dlsym` calls against it are exactly what the dynamic
// loader itself performs for ordinary linked code, so sharing it across
// threads is sound.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}

impl Library {
    /// Opens `name` (a bare soname or an explicit path) into a private
    /// symbol scope. Returns the loader's own error text on failure.
    pub(crate) fn open(name: &str) -> Result<Self, String> {
        let name =
            CString::new(name).map_err(|_| format!("library name '{name}' contains a NUL byte"))?;
        clear_error();
        let handle = unsafe { dlopen(name.as_ptr(), RTLD_NOW_LOCAL) };
        if handle.is_null() {
            return Err(last_error().unwrap_or_else(|| "dlopen failed".to_string()));
        }
        Ok(Self { handle })
    }

    /// Resolves `name` to a raw address within this library.
    ///
    /// # Safety
    /// The caller is responsible for knowing that `name` really has the
    /// signature it goes on to interpret this address as.
    pub(crate) unsafe fn symbol(&self, name: &str) -> Result<*mut c_void, String> {
        let name =
            CString::new(name).map_err(|_| format!("symbol name '{name}' contains a NUL byte"))?;
        clear_error();
        let address = unsafe { dlsym(self.handle, name.as_ptr()) };
        // POSIX's documented pattern: a symbol legitimately valued NULL is
        // indistinguishable from a failed lookup except by consulting
        // dlerror() again, so that — not `address.is_null()` — is the
        // authoritative failure signal.
        if let Some(error) = last_error() {
            return Err(error);
        }
        Ok(address)
    }
}

fn clear_error() {
    unsafe { dlerror() };
}

fn last_error() -> Option<String> {
    let message = unsafe { dlerror() };
    if message.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}
