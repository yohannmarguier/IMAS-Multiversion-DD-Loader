//! DD-version stamp discovery (issue #53, ADR 0007, ADR 0009).
//!
//! Immediately after `al_begin_global_action` opens successfully, the shim
//! reads `ids_properties/version_put/data_dictionary` — `CHAR_DATA` at
//! `dim == 1` — through a reader the interposition adapter injects, but
//! deliberately *not* through the converting wrapper around it: this read
//! decides whether conversion applies to the occurrence at all, so it cannot
//! be subject to it.
//! The outcome is classified with the one read-outcome classifier
//! ([`crate::read_outcome`]). IMAS-Core allocates this buffer and,
//! because this read is entirely shim-internal (the HLI never sees it), the
//! shim frees it itself exactly once — the ordinary "HLI frees it" ownership
//! contract (ADR 0006) does not apply here, since there is no HLI-visible
//! buffer to hand back. This is the shim's only `free` call, and ADR 0010
//! records it as the one deliberate exception to its own rule that the shim
//! never frees an IMAS-Core allocation, so the exception is auditable there
//! rather than resting on this comment alone.
//!
//! The stamp is decoded from the bytes IMAS-Core reported via `size`, never
//! by scanning for a NUL terminator: a malloc'd CHAR_DATA buffer carries no
//! guarantee of a NUL byte anywhere within its bounds.

use std::ffi::{c_char, c_int, c_void};

use crate::al_status_t;
use crate::dd_version::DdVersion;
use crate::read_outcome::{self, ReadOutcome};

/// `ids_properties/version_put/data_dictionary`, NUL-terminated for the FFI
/// call (the trailing byte here has nothing to do with how the *returned*
/// stamp is decoded, which never scans for one).
const VERSION_STAMP_FIELD: &[u8] = b"ids_properties/version_put/data_dictionary\0";

/// The classified result of one DD-version-stamp discovery read.
pub(crate) enum StampOutcome {
    /// The stamp is absent, or the discovery read itself failed. ADR 0007
    /// treats these identically: a mismatch is asserted only from a present,
    /// valid stamp, never inferred from a failure.
    Unstamped,
    /// A present, valid stamp naming the occurrence's stored DD version.
    Stored(DdVersion),
    /// A present stamp that failed to decode as UTF-8 or parse as a DD
    /// version — a hard refusal (ADR 0009), distinct from the absent case.
    /// Boxed since `al_status_t`'s 256-byte message would otherwise make
    /// every `StampOutcome` pay for the rarest, failure-only variant.
    Malformed(Box<al_status_t>),
}

/// Decodes a CHAR_DATA stamp buffer into a [`DdVersion`], or `None` if it is
/// not valid UTF-8 or not a recognised DD-version spelling. Pure and
/// allocation-free so it is directly unit-testable without any pointer.
pub(crate) fn decode(bytes: &[u8]) -> Option<DdVersion> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// Reads and classifies the DD-version stamp for the occurrence just opened
/// at `octx_id`. The interposition adapter supplies `read`, which has the
/// ordinary IMAS-Core `al_read_data` shape and forwards without any conversion
/// policy an HLI-issued read carries.
pub(crate) fn discover(
    octx_id: c_int,
    read: impl FnOnce(
        c_int,
        *const c_char,
        *const c_char,
        *mut *mut c_void,
        c_int,
        c_int,
        *mut c_int,
    ) -> al_status_t,
) -> StampOutcome {
    let field = VERSION_STAMP_FIELD.as_ptr().cast::<c_char>();
    let mut data: *mut c_void = std::ptr::null_mut();
    let mut size: c_int = 0;
    // `field` is a valid NUL-terminated C string for the duration of the
    // call, while `data` and `size` are writable local out-parameters.
    let status = read(
        octx_id,
        field,
        c"".as_ptr(),
        &mut data,
        crate::core_binding::CHAR_DATA_ID,
        1,
        &mut size,
    );

    match read_outcome::classify(&status, data.cast_const()) {
        ReadOutcome::Failure | ReadOutcome::NotFound => StampOutcome::Unstamped,
        ReadOutcome::Data => {
            let len = if size > 0 { size as usize } else { 0 };
            // SAFETY: IMAS-Core reported `size` bytes at `data` for this
            // CHAR_DATA, dim == 1 read; `data` is non-null (this arm of the
            // classifier guarantees it) and IMAS-Core-allocated.
            let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len) };
            let decoded = decode(bytes);
            // Freed exactly once, on every path through this arm — malformed
            // or valid — since this buffer never reaches the HLI.
            unsafe { free(data) };
            match decoded {
                Some(version) => StampOutcome::Stored(version),
                None => StampOutcome::Malformed(Box::new(crate::conversion_refusal(
                    "malformed DD-version stamp at 'ids_properties/version_put/data_dictionary'",
                ))),
            }
        }
    }
}

unsafe extern "C" {
    fn free(ptr: *mut c_void);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_release_stamp_decodes() {
        assert_eq!(decode(b"4.1.1"), Some("4.1.1".parse().unwrap()));
    }

    #[test]
    fn a_development_stamp_decodes() {
        assert_eq!(
            decode(b"4.1.1-47-g8eaa5f1"),
            Some("4.1.1-47-g8eaa5f1".parse().unwrap())
        );
    }

    #[test]
    fn non_utf8_bytes_do_not_decode() {
        assert_eq!(decode(&[0xff, 0xfe, 0xfd]), None);
    }

    #[test]
    fn garbage_text_does_not_decode() {
        assert_eq!(decode(b"not-a-version"), None);
    }

    #[test]
    fn an_unknown_release_does_not_decode() {
        assert_eq!(decode(b"4.1.2"), None);
    }

    #[test]
    fn empty_bytes_do_not_decode() {
        assert_eq!(decode(b""), None);
    }

    #[test]
    fn a_trailing_nul_is_not_stripped_and_does_not_decode() {
        // Proves decoding never assumes/relies on NUL handling: a stamp
        // buffer that happens to carry a trailing NUL within its reported
        // size is not silently trimmed into validity.
        assert_eq!(decode(b"4.1.1\0"), None);
    }
}
