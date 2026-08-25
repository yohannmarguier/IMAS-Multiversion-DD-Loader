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
//! A **scalar** read is the one exception, and it is an exception in the ABI
//! rather than in this module's design. For `dim == 0` IMAS-Core does not own
//! the buffer: `Lowlevel::setValue` copies the stored value *into* `*data`
//! and frees its own allocation, and `Lowlevel::setDefaultValue` writes the
//! datatype's EMPTY sentinel into `*data` when the field is absent — both
//! dereference `*data` unconditionally, so a caller that passes a null
//! pointer for a scalar read crashes IMAS-Core rather than being told
//! not-found. A scalar therefore never returns a null pointer, and absence
//! has to be read off the *value*. [`classify_scalar_double`] is the one
//! place that does so.
#![allow(dead_code)]

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
/// see this module's doc comment. Not valid for a `dim == 0` read: use
/// [`classify_scalar_double`], which this module's doc comment explains.
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
/// value is absent. Two seams need it and for opposite reasons — a scalar
/// read has no other way to report not-found, and a value transformation
/// must leave the sentinel alone so a caller can still tell a real zero from
/// a hole in an array — so it is defined once, here, alongside the outcome
/// it decides.
pub(crate) const EMPTY_DOUBLE: f64 = -9e40;

/// Classifies one scalar (`dim == 0`) `DOUBLE_DATA` read. `data` is the
/// pointer IMAS-Core returned and `value` is what it left in the caller's own
/// buffer.
///
/// This delegates to [`classify`] and then adds the one signal a scalar read
/// has and no other read does: the pointer is still the caller's own, so a
/// scalar cannot report absence by returning null, and IMAS-Core reports it
/// by writing [`EMPTY_DOUBLE`] into the buffer instead. Both channels are
/// consulted rather than only the sentinel, because a layer below IMAS-Core
/// is free to answer through the null pointer as well, and either answer
/// means the same thing: nothing is stored there.
///
/// A stored value that genuinely equals the sentinel is indistinguishable
/// from an absent one. That ambiguity is IMAS-Core's, not this function's:
/// the scalar ABI provides no further channel to disambiguate it.
pub(crate) fn classify_scalar_double(
    status: &al_status_t,
    data: *const c_void,
    value: f64,
) -> ReadOutcome {
    match classify(status, data) {
        ReadOutcome::Data if value == EMPTY_DOUBLE => ReadOutcome::NotFound,
        outcome => outcome,
    }
}

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

    /// The scalar ABI's own convention: absence is a sentinel value, not a
    /// null pointer, because IMAS-Core dereferences the caller's pointer
    /// unconditionally for `dim == 0` (see this module's doc comment).
    #[test]
    fn a_scalar_read_reports_not_found_through_either_channel() {
        let mut caller_owned = 0.0f64;
        let owned = (&raw mut caller_owned).cast::<c_void>().cast_const();

        // IMAS-Core's own scalar channel: the pointer comes back unchanged
        // and the sentinel is in the caller's buffer.
        assert_eq!(
            classify_scalar_double(&al_status_t::default(), owned, EMPTY_DOUBLE),
            ReadOutcome::NotFound
        );
        // The pointer channel every non-scalar read uses still answers.
        assert_eq!(
            classify_scalar_double(&al_status_t::default(), std::ptr::null(), 0.0),
            ReadOutcome::NotFound
        );
        // A real zero is data, not absence.
        assert_eq!(
            classify_scalar_double(&al_status_t::default(), owned, 0.0),
            ReadOutcome::Data
        );
        assert_eq!(
            classify_scalar_double(&al_status_t::default(), owned, -7.5),
            ReadOutcome::Data
        );
        // A failure outranks both, exactly as it does in `classify`.
        assert_eq!(
            classify_scalar_double(&failure_status(), owned, 1.0),
            ReadOutcome::Failure
        );
        assert_eq!(
            classify_scalar_double(&failure_status(), owned, EMPTY_DOUBLE),
            ReadOutcome::Failure
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
