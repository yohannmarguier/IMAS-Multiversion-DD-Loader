//! The one read-outcome classifier (issue #53, ADR 0012 decision 3).
//!
//! `al_read_data` packs three outcomes into `al_status_t` plus the returned
//! data pointer: failure (`code != 0`), not-found (`code == 0` with a null
//! data pointer), and data. This is the one shim function that turns that
//! pair into a [`ReadOutcome`]; every seam that needs the distinction (the
//! `merged` precedence loop, the value-transform gate, DD-version-stamp
//! discovery) consumes this result instead of comparing the data pointer to
//! null itself (CONTEXT.md's "read outcome").
//!
//! A **scalar** read cannot be classified this way at all, and that is a
//! property of the ABI rather than of this module. For `dim == 0` IMAS-Core
//! does not own the buffer: `Lowlevel::setValue` copies the stored value
//! *into* `*data` and frees its own allocation, and
//! `Lowlevel::setDefaultValue` writes the datatype's EMPTY sentinel into
//! `*data` when the field is absent — both dereference `*data`
//! unconditionally, so a caller that passes a null pointer for a scalar read
//! crashes IMAS-Core rather than being told not-found. A scalar therefore
//! never returns a null pointer, and absence has to be read off the *value*
//! against [`EMPTY_DOUBLE`].
//!
//! No seam *reads* a sentinel today. The one that did was the delete
//! fan-out's presence probe, removed with issue #138 because it read through
//! the caller's context (ADR 0017 decision 2), and its classifier went with
//! it rather than staying behind as an untested-in-production helper. A
//! future scalar reader needs both channels — the sentinel *and* the status
//! — since a layer below IMAS-Core may still answer through the pointer.
//!
//! The read seam does *write* one. When the artifact says a path has no
//! stored source the shim answers not-found without calling IMAS-Core at
//! all, so nothing else is left to fill the caller's scalar buffer; the
//! interposition layer mirrors `Lowlevel::setDefaultValue` there, out of the
//! sentinel set below.

use std::ffi::{c_char, c_int, c_void};

use crate::al_status_t;

/// Which of the three things one `al_read_data` call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadOutcome {
    /// `code != 0`: IMAS-Core (or the shim) failed the read outright.
    Failure,
    /// `code == 0` with a null data pointer: the field is genuinely absent.
    NotFound,
    /// `code == 0` with a non-null data pointer: data was returned.
    Data,
}

/// Classifies one `al_read_data` outcome from its status and returned data
/// pointer. Nothing else in the shim may compare a data pointer to null —
/// see this module's doc comment. Not valid for a `dim == 0` read, where
/// absence arrives as a sentinel value instead; this module's doc comment
/// explains why, and no seam currently performs one.
pub(crate) fn classify(status: &al_status_t, data: *const c_void) -> ReadOutcome {
    if status.code != 0 {
        ReadOutcome::Failure
    } else if data.is_null() {
        ReadOutcome::NotFound
    } else {
        ReadOutcome::Data
    }
}

/// IMAS-Core's EMPTY sentinels — the values `Lowlevel::setDefaultValue`
/// writes into a caller-owned scalar buffer where a field is absent, and the
/// values `Lowlevel::data_has_non_zero_shape` reads back to recognise an
/// unset scalar on the way down. They are mirrored here, in one place,
/// because both directions need the same four numbers: a read that decides
/// not-found without calling IMAS-Core has to write them (the caller has no
/// other channel for absence at `dim == 0`), and a write has to leave them
/// alone so a value transformation cannot fabricate a measurement out of a
/// hole (ADR 0018). CONTEXT.md's "read outcome" entry states the rule these
/// satisfy: one definition serves both.
///
/// Transcribed from IMAS-Core's `src/al_lowlevel.cpp`:
///
/// ```text
/// const char                 Lowlevel::EMPTY_CHAR    = '\0';
/// const int                  Lowlevel::EMPTY_INT     = -999999999;
/// const double               Lowlevel::EMPTY_DOUBLE  = -9.0E40;
/// const std::complex<double> Lowlevel::EMPTY_COMPLEX = {-9.0E40, -9.0E40};
/// ```
/// Spelled as the NUL byte rather than as `0` on purpose: unlike the other
/// three this is not a distinctive magic number, just a terminator, which is
/// why the write side's emptiness check deliberately leaves `CHAR_DATA` out.
pub(crate) const EMPTY_CHAR: c_char = b'\0' as c_char;
/// See [`EMPTY_CHAR`].
pub(crate) const EMPTY_INT: c_int = -999_999_999;
/// See [`EMPTY_CHAR`].
pub(crate) const EMPTY_DOUBLE: f64 = -9e40;
/// See [`EMPTY_CHAR`]. IMAS-Core's C ABI lays a `COMPLEX_DATA` value out as
/// consecutive real and imaginary `double`s, matching its `complex_t` HDF5
/// bridge, so the sentinel is the pair rather than one value.
pub(crate) const EMPTY_COMPLEX: [f64; 2] = [EMPTY_DOUBLE, EMPTY_DOUBLE];

#[cfg(test)]
mod tests {
    use super::*;

    fn failure_status() -> al_status_t {
        al_status_t {
            code: -1,
            ..al_status_t::default()
        }
    }

    #[test]
    fn a_nonzero_code_is_failure_regardless_of_the_data_pointer() {
        assert_eq!(
            classify(&failure_status(), std::ptr::null()),
            ReadOutcome::Failure
        );
        let mut sentinel = 0u8;
        assert_eq!(
            classify(&failure_status(), &mut sentinel as *mut u8 as *const c_void),
            ReadOutcome::Failure
        );
    }

    #[test]
    fn a_successful_status_with_a_null_pointer_is_not_found() {
        assert_eq!(
            classify(&al_status_t::default(), std::ptr::null()),
            ReadOutcome::NotFound
        );
    }

    #[test]
    fn a_successful_status_with_a_non_null_pointer_is_data() {
        let mut sentinel = 0u8;
        assert_eq!(
            classify(
                &al_status_t::default(),
                &mut sentinel as *mut u8 as *const c_void
            ),
            ReadOutcome::Data
        );
    }
}
