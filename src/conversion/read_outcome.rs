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
//! [`classify_scalar`] is the seam that does read it off the value. It exists
//! because the `merged` precedence loop could not otherwise advance past a
//! scalar candidate: an absent one comes back `code == 0` with the caller's
//! own non-null pointer, which [`classify`] can only call `Data`, so the loop
//! stopped at precedence 1 and handed the caller the sentinel even where a
//! later candidate held the value. It uses both channels — the sentinel *and*
//! the status — since a layer below IMAS-Core may still answer through the
//! pointer.
//!
//! The read seam also *writes* one. When the artifact says a path has no
//! stored source the shim answers not-found without calling IMAS-Core at
//! all, so nothing else is left to fill the caller's scalar buffer; the
//! interposition layer mirrors `Lowlevel::setDefaultValue` there, out of the
//! sentinel set below.
//!
//! An earlier scalar classifier existed and was removed: the delete fan-out's
//! presence probe went with issue #138, because it read through the *caller's*
//! context (ADR 0017 decision 2), rather than staying behind as an
//! untested-in-production helper.

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
/// absence arrives as a sentinel value instead: use [`classify_scalar`].
pub(crate) fn classify(status: &al_status_t, data: *const c_void) -> ReadOutcome {
    if status.code != 0 {
        ReadOutcome::Failure
    } else if data.is_null() {
        ReadOutcome::NotFound
    } else {
        ReadOutcome::Data
    }
}

/// Classifies one `dim == 0` `al_read_data` outcome, where absence is a
/// sentinel *value* in the caller's own buffer rather than a null pointer.
///
/// Both channels are consulted, in the order the ABI makes them meaningful:
/// [`classify`] first, so a nonzero status is still a failure and a layer
/// below IMAS-Core that does answer through the pointer is still heard, and
/// only then the sentinel. `is_double` gates the value test because
/// [`EMPTY_DOUBLE`] is the one sentinel this shim currently reads back: a
/// scalar of any other datatype keeps [`classify`]'s answer, which is what
/// the artifact needs today, since its only scalar `merged` rules are
/// `DOUBLE_DATA` (ADR 0011 — no rule for a case the shipped artifact cannot
/// reach).
///
/// # Safety
/// When `is_double` is set and `data` is non-null, it must point to one
/// initialized `f64` — IMAS-Core's own contract for a `dim == 0`
/// `DOUBLE_DATA` read that returned `code == 0`.
pub(crate) unsafe fn classify_scalar(
    status: &al_status_t,
    data: *const c_void,
    is_double: bool,
) -> ReadOutcome {
    match classify(status, data) {
        ReadOutcome::Data if is_double => {
            // SAFETY: `ReadOutcome::Data` establishes `data` non-null, and
            // this function's own contract requires it to point at one
            // initialized `f64` when `is_double` is set.
            if unsafe { *data.cast::<f64>() } == EMPTY_DOUBLE {
                ReadOutcome::NotFound
            } else {
                ReadOutcome::Data
            }
        }
        outcome => outcome,
    }
}

/// IMAS-Core's EMPTY sentinels — the values `Lowlevel::setDefaultValue`
/// writes into a caller-owned scalar buffer where a field is absent, and the
/// values `Lowlevel::data_has_non_zero_shape` reads back to recognise an
/// unset scalar on the way down. They are mirrored here, in one place,
/// because both directions need the same four numbers: a read that decides
/// not-found without calling IMAS-Core has to write them (the caller has no
/// other channel for absence at `dim == 0`), [`classify_scalar`] has to read
/// one back to tell an absent scalar candidate from a stored one, and a write
/// has to leave them alone so a value transformation cannot fabricate a
/// measurement out of a hole (ADR 0018). CONTEXT.md's "read outcome" entry
/// states the rule these satisfy: one definition serves both.
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
    fn a_scalar_holding_the_empty_sentinel_is_not_found() {
        let mut value = EMPTY_DOUBLE;
        assert_eq!(
            unsafe {
                classify_scalar(
                    &al_status_t::default(),
                    std::ptr::from_mut(&mut value).cast::<c_void>(),
                    true,
                )
            },
            ReadOutcome::NotFound
        );
    }

    #[test]
    fn a_scalar_holding_a_real_value_is_data() {
        // Zero is the value the sentinel test must not swallow: it is a real
        // measurement, and telling it from a hole is the whole point.
        for mut value in [0.0f64, 5.2, -9e39, f64::MIN] {
            assert_eq!(
                unsafe {
                    classify_scalar(
                        &al_status_t::default(),
                        std::ptr::from_mut(&mut value).cast::<c_void>(),
                        true,
                    )
                },
                ReadOutcome::Data,
                "{value} is a value, not an absence"
            );
        }
    }

    #[test]
    fn a_non_double_scalar_keeps_the_pointer_classification() {
        // ADR 0011: the shipped artifact has no non-DOUBLE scalar `merged`
        // rule, so no sentinel is invented for one. The bit pattern below is
        // EMPTY_DOUBLE's precisely so this asserts the gate, not the absence
        // of a coincidence.
        let mut value = EMPTY_DOUBLE;
        assert_eq!(
            unsafe {
                classify_scalar(
                    &al_status_t::default(),
                    std::ptr::from_mut(&mut value).cast::<c_void>(),
                    false,
                )
            },
            ReadOutcome::Data
        );
    }

    #[test]
    fn a_scalar_still_hears_the_status_and_pointer_channels() {
        let mut value = 5.2f64;
        let data = std::ptr::from_mut(&mut value).cast::<c_void>();
        assert_eq!(
            unsafe { classify_scalar(&failure_status(), data, true) },
            ReadOutcome::Failure
        );
        assert_eq!(
            unsafe { classify_scalar(&al_status_t::default(), std::ptr::null(), true) },
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
