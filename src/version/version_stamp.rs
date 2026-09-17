//! DD-version stamp discovery (issue #53, ADR 0007, ADR 0009).
//!
//! Immediately after `al_begin_global_action` opens successfully, the shim
//! reads `ids_properties/version_put/data_dictionary` — `CHAR_DATA` at
//! `dim == 1` — through a reader the interposition adapter injects, but
//! deliberately *not* through the converting wrapper around it: this read
//! decides whether conversion applies to the occurrence at all, so it cannot
//! be subject to it.
//! The outcome is classified with the one read-outcome classifier
//! ([`crate::conversion::read_outcome`]). IMAS-Core allocates this buffer and,
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
use crate::conversion::read_outcome::{self, ReadOutcome};
use crate::version::dd_version::DdVersion;

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

/// Classifies a discovery read from the read result and the bytes reported by
/// IMAS-Core. This is intentionally separate from allocation ownership and
/// argument marshalling so the boundary between an absent stamp and present,
/// malformed metadata can be tested from ordinary Rust values.
fn classify_discovery_read(
    outcome: ReadOutcome,
    bytes: &[u8],
    reported_extent: c_int,
) -> StampOutcome {
    match outcome {
        ReadOutcome::Failure | ReadOutcome::NotFound => StampOutcome::Unstamped,
        ReadOutcome::Data => {
            // A non-positive extent reports no stamp bytes. `>= 0` would be
            // equivalent here: both zero-length slices decode as malformed;
            // retain `> 0` to document the positive-byte contract explicitly.
            let len = if reported_extent > 0 {
                reported_extent as usize
            } else {
                0
            };
            match bytes.get(..len).and_then(decode) {
                Some(version) => StampOutcome::Stored(version),
                None => StampOutcome::Malformed(Box::new(crate::conversion_refusal(
                    "malformed DD-version stamp at 'ids_properties/version_put/data_dictionary'",
                ))),
            }
        }
    }
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
        crate::core::core_binding::CHAR_DATA_ID,
        1,
        &mut size,
    );

    let outcome = read_outcome::classify(&status, data.cast_const());
    match outcome {
        ReadOutcome::Failure | ReadOutcome::NotFound => classify_discovery_read(outcome, &[], size),
        ReadOutcome::Data => {
            let len = usize::try_from(size).unwrap_or(0);
            // SAFETY: IMAS-Core reported `size` bytes at `data` for this
            // CHAR_DATA, dim == 1 read; `data` is non-null (this arm of the
            // classifier guarantees it) and IMAS-Core-allocated.
            let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len) };
            let outcome = classify_discovery_read(outcome, bytes, size);
            // Freed exactly once, on every path through this arm — malformed
            // or valid — since this buffer never reaches the HLI.
            unsafe { free(data) };
            outcome
        }
    }
}

unsafe extern "C" {
    fn free(ptr: *mut c_void);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    fn assert_unstamped(outcome: StampOutcome) {
        assert!(matches!(outcome, StampOutcome::Unstamped));
    }

    fn assert_stored(outcome: StampOutcome, expected: &str) {
        match outcome {
            StampOutcome::Stored(actual) => assert_eq!(actual, expected.parse().unwrap()),
            StampOutcome::Unstamped | StampOutcome::Malformed(_) => {
                panic!("expected stored DD version {expected}")
            }
        }
    }

    fn assert_malformed(outcome: StampOutcome) {
        match outcome {
            StampOutcome::Malformed(status) => {
                assert_eq!(status.code, crate::IMAS_MVDD_CONVERSION_ERROR);
                assert_eq!(
                    unsafe { CStr::from_ptr(status.message.as_ptr()) }
                        .to_str()
                        .unwrap(),
                    "IMAS-MVDD: malformed DD-version stamp at 'ids_properties/version_put/data_dictionary'"
                );
            }
            StampOutcome::Unstamped | StampOutcome::Stored(_) => {
                panic!("expected malformed DD-version stamp")
            }
        }
    }

    #[test]
    fn failed_or_absent_reads_are_unstamped_but_present_bad_bytes_are_malformed() {
        assert_unstamped(classify_discovery_read(ReadOutcome::Failure, b"4.1.1", 5));
        assert_unstamped(classify_discovery_read(ReadOutcome::NotFound, b"4.1.1", 5));
        assert_malformed(classify_discovery_read(
            ReadOutcome::Data,
            b"not-a-version",
            13,
        ));
        assert_malformed(classify_discovery_read(ReadOutcome::Data, b"4.1.2", 5));
        assert_malformed(classify_discovery_read(ReadOutcome::Data, &[0xff, 0xfe], 2));
    }

    #[test]
    fn a_present_stamp_decodes_only_its_positive_reported_extent() {
        assert_stored(
            classify_discovery_read(ReadOutcome::Data, b"4.1.1\0unreported", 5),
            "4.1.1",
        );
        assert_malformed(classify_discovery_read(
            ReadOutcome::Data,
            b"4.1.1\0unreported",
            6,
        ));
    }

    #[test]
    fn a_present_stamp_with_zero_or_negative_extent_is_malformed() {
        assert_malformed(classify_discovery_read(ReadOutcome::Data, b"", 0));
        assert_malformed(classify_discovery_read(ReadOutcome::Data, b"4.1.1", 0));
        assert_malformed(classify_discovery_read(ReadOutcome::Data, b"4.1.1", -1));
    }

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
