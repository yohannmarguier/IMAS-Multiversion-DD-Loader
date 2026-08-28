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
//! No seam does that today. The one that did was the delete fan-out's
//! presence probe, removed with issue #138 because it read through the
//! caller's context (ADR 0017 decision 2), and its classifier went with it
//! rather than staying behind as an untested-in-production helper. A future
//! scalar reader needs both channels — the sentinel *and* the status — since
//! a layer below IMAS-Core may still answer through the pointer.

use std::ffi::c_void;

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

/// IMAS-Core's `EMPTY_DOUBLE`: the sentinel it writes where a `DOUBLE_DATA`
/// value is absent. The write path needs it so a value transformation leaves
/// the sentinel alone, letting a caller still tell a real zero from a hole in
/// an array (ADR 0018), and any future scalar read would need it to report
/// not-found at all — so it is defined once, here, alongside the outcome it
/// decides.
pub(crate) const EMPTY_DOUBLE: f64 = -9e40;

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
