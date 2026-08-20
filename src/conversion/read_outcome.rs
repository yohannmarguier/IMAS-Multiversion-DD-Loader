//! The one read-outcome classifier (issue #53, ADR 0012 decision 3).
//!
//! `al_read_data` packs three outcomes into `al_status_t` plus the returned
//! data pointer: failure (`code != 0`), not-found (`code == 0` with a null
//! data pointer), and data. This is the one shim function that turns that
//! pair into a [`ReadOutcome`]; every seam that needs the distinction (the
//! `merged` precedence loop, the value-transform gate, DD-version-stamp
//! discovery) consumes this result instead of comparing the data pointer to
//! null itself (CONTEXT.md's "read outcome").
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
/// see this module's doc comment.
pub(crate) fn classify(status: &al_status_t, data: *const c_void) -> ReadOutcome {
    if status.code != 0 {
        ReadOutcome::Failure
    } else if data.is_null() {
        ReadOutcome::NotFound
    } else {
        ReadOutcome::Data
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
