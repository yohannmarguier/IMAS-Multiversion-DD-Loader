//! Minimal POSIX `dlopen`/`dlsym` bindings.
//!
//! No `libloading` dependency: the only thing this project needs is
//! `dlopen` with the default, private (`RTLD_LOCAL`) symbol scope, which is
//! exactly what the platform gives you without asking — see
//! docs/adr/0001-runtime-binding-not-linking.md.

use std::ffi::{CStr, CString, c_char, c_int, c_void};

#[cfg_attr(target_os = "linux", link(name = "dl"))]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *mut c_char;
}

const RTLD_NOW: c_int = 2;
const RTLD_LOCAL: c_int = 0;

/// A resolved shared library. Deliberately has no `Drop` impl: this handle
/// is meant to live for the process's lifetime, and the function pointers
/// resolved from it must stay valid for as long as it's called through —
/// `dlclose`-ing it would be the bug, not leaking it.
pub(crate) struct Library {
    handle: *mut c_void,
}

// The handle is an opaque address used only to look up symbols and never
// mutated; concurrent `dlsym` calls against it are the same operation the
// dynamic loader itself performs concurrently for ordinary linked code.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}

impl Library {
    pub(crate) fn open(name: &str) -> Result<Self, String> {
        let name_c =
            CString::new(name).map_err(|_| format!("library name '{name}' contains a NUL byte"))?;
        clear_dlerror();
        let handle = unsafe { dlopen(name_c.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        if handle.is_null() {
            return Err(last_dlerror().unwrap_or_else(|| "dlopen failed".to_string()));
        }
        Ok(Self { handle })
    }

    /// # Safety
    /// The caller must not call through the returned pointer unless it
    /// actually names a function with the signature the caller assumes.
    pub(crate) unsafe fn symbol(&self, name: &str) -> Result<*mut c_void, String> {
        let name_c =
            CString::new(name).map_err(|_| format!("symbol name '{name}' contains a NUL byte"))?;
        clear_dlerror();
        let symbol = unsafe { dlsym(self.handle, name_c.as_ptr()) };
        if symbol.is_null()
            && let Some(err) = last_dlerror()
        {
            return Err(err);
        }
        Ok(symbol)
    }
}

fn clear_dlerror() {
    unsafe { dlerror() };
}

fn last_dlerror() -> Option<String> {
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
