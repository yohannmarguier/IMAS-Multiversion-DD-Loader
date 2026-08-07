//! IMAS-Multiversion-DD-Loader — C ABI surface.
//!
//! This crate re-exports IMAS-Core's public C ABI verbatim and interposes on
//! the path-bearing entry points. The shared constants, `al_status_t`, and
//! the runtime-binding architecture (`src/binding.rs`) are proven end to end
//! on `al_context_info`; every other mirrored entry point is still
//! unimplemented.

// The mirrored ABI dictates the names; matching IMAS-Core exactly is the point.
#![allow(non_camel_case_types)]

use std::ffi::c_char;
use std::ffi::c_int;

mod binding;
#[cfg(unix)]
mod dl;

/// Length of `al_status_t::message`, mirroring IMAS-Core's `MAX_ERR_MSG_LEN`.
pub const MAX_ERR_MSG_LEN: usize = 256;

/// Maximum array rank accepted across the ABI, mirroring IMAS-Core's `MAXDIM`.
pub const MAXDIM: usize = 7;

/// Status returned by every ABI entry point. `code == 0` means success.
///
/// Note the conflicting convention one layer down: a backend's `read_data`
/// returns `0` for *not found* and `1` for success.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct al_status_t {
    pub code: c_int,
    pub message: [c_char; MAX_ERR_MSG_LEN],
}

impl Default for al_status_t {
    fn default() -> Self {
        Self {
            code: 0,
            message: [0; MAX_ERR_MSG_LEN],
        }
    }
}

/// Version of this shim, as a NUL-terminated static string.
///
/// Deliberately *not* named `getDDVersion` — that IMAS-Core call is dead and
/// returns the sentinel `"!!DEPRECATED!!"`.
#[unsafe(no_mangle)]
pub extern "C" fn imas_mvdd_loader_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// Reset `status` to success (`code == 0`, empty message).
///
/// # Safety
/// `status` must be non-null and point to a writable `al_status_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn imas_mvdd_loader_status_clear(status: *mut al_status_t) {
    if status.is_null() {
        return;
    }
    unsafe { *status = al_status_t::default() };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn version_matches_the_package() {
        let version = unsafe { CStr::from_ptr(imas_mvdd_loader_version()) };
        assert_eq!(version.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn status_clear_zeroes_a_dirty_status() {
        let mut status = al_status_t {
            code: 42,
            message: [b'x' as c_char; MAX_ERR_MSG_LEN],
        };
        unsafe { imas_mvdd_loader_status_clear(&raw mut status) };
        assert_eq!(status.code, 0);
        assert!(status.message.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn status_clear_tolerates_null() {
        unsafe { imas_mvdd_loader_status_clear(std::ptr::null_mut()) };
    }
}
