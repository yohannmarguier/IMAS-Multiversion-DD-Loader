//! The `al_write_data` seams.
//!
//! `al_write_data` and its `al_plugin_write_data` twin share one body.
//! A write asserts a value, so unlike a read it must resolve to exactly one
//! stored spelling: an ambiguous plan writes only precedence 1 and records
//! what it skipped, while an unservable rule refuses before Core is called.
//! A value transformation runs on a shim-owned copy (ADR 0018), never on the
//! caller's buffer.
//!
//! The decision is not here — [`crate::conversion::seam_policy::run_write`]
//! owns it (ADR 0015); this module supplies the Core call and the buffers.

use std::ffi::{CStr, c_char, c_int, c_void};

use crate::al_status_t;
use crate::conversion::conversion_map::Fidelity;
use crate::conversion::path_conversion;
use crate::conversion::seam_policy;
use crate::core::core_binding::{COMPLEX_DATA_ID, DOUBLE_DATA_ID, INTEGER_DATA_ID};
use crate::loss::LossOperation;
use crate::registry::context_registry::ConversionRecord;

use super::dispatch::{CallFamily, call_write};
use super::loss::retain_loss;
use super::reentry::ReentryGuard;
use super::refusal::{c_str_ref, context_path_refusal, live_conversion_record, read_argument_path};

/// Builds the read-only source view a write-side transformation can copy.
/// This never modifies caller storage; invalid raw shape metadata becomes a
/// policy refusal before an IMAS-Core write is attempted.
///
/// # Safety
/// `data`, when non-null, must point to the caller-owned buffer described by
/// `datatype`, `dim`, and `size`, matching IMAS-Core's write ABI contract.
unsafe fn build_source_view<'a>(
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> seam_policy::SourceView<'a> {
    if unsafe { is_empty_scalar(data, datatype, dim) } {
        return seam_policy::SourceView::UnsetScalar;
    }
    if datatype != DOUBLE_DATA_ID {
        return seam_policy::SourceView::NotDouble;
    }
    let element_count = if dim == 0 {
        Ok(1usize)
    } else if !(0..=crate::MAXDIM as c_int).contains(&dim) {
        Err("value-transform execution received an invalid array shape")
    } else if size.is_null() {
        Err("value-transform execution needs array dimensions")
    } else {
        // SAFETY: the ABI requires one initialized extent per write rank.
        unsafe { std::slice::from_raw_parts(size, dim as usize) }
            .iter()
            .try_fold(1usize, |count, &extent| {
                usize::try_from(extent)
                    .ok()
                    .and_then(|extent| count.checked_mul(extent))
            })
            .ok_or("value-transform execution received an invalid array shape")
    };
    match element_count {
        Ok(_) if data.is_null() => {
            seam_policy::SourceView::InvalidShape("value-transform execution needs a data buffer")
        }
        Ok(count) => {
            // SAFETY: the caller's write ABI contract supplies an initialized
            // DOUBLE_DATA buffer of exactly this shape.
            let values = unsafe { std::slice::from_raw_parts(data.cast::<f64>(), count) };
            seam_policy::SourceView::Double(values)
        }
        Err(reason) => seam_policy::SourceView::InvalidShape(reason),
    }
}

/// Whether a scalar is one of IMAS-Core's own unset sentinels. This mirrors
/// the rank-zero half of `Lowlevel::data_has_non_zero_shape`: forwarding the
/// original bytes preserves Core's silent skip instead of letting a COCOS
/// flip fabricate a measurement (ADR 0018).
///
/// # Safety
/// When non-null, `data` must point to the scalar representation declared by
/// `datatype`. IMAS-Core's C ABI represents `COMPLEX_DATA` as consecutive
/// real and imaginary `double` values, matching its `complex_t` HDF5 bridge.
unsafe fn is_empty_scalar(data: *mut c_void, datatype: c_int, dim: c_int) -> bool {
    const EMPTY_INT: c_int = -999_999_999;
    const EMPTY_DOUBLE: f64 = -9e40;
    if dim != 0 || data.is_null() {
        return false;
    }
    match datatype {
        INTEGER_DATA_ID => unsafe { *data.cast::<c_int>() == EMPTY_INT },
        DOUBLE_DATA_ID => unsafe { *data.cast::<f64>() == EMPTY_DOUBLE },
        COMPLEX_DATA_ID => {
            let values = unsafe { std::slice::from_raw_parts(data.cast::<f64>(), 2) };
            values == [EMPTY_DOUBLE, EMPTY_DOUBLE]
        }
        _ => false,
    }
}

/// Forwards to IMAS-Core's real `al_write_data`, resolving IMAS-Core
/// lazily on first use. See [`write_data_impl`] for the shared policy this
/// and [`plugin_write_data`] both carry out.
///
/// # Safety
/// `field` and `timebase` must be valid, NUL-terminated C strings, or null
/// where IMAS-Core's own contract allows it. `data` and `size` must be
/// valid pointers, matching IMAS-Core's own contract for this function.
pub(crate) unsafe fn write_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    write_data_impl(
        CallFamily::ORDINARY,
        ctx_id,
        field,
        timebase,
        data,
        datatype,
        dim,
        size,
    )
}

/// Follows the same policy as [`write_data`], forwarded through IMAS-Core's
/// plugin reentry write symbol rather than its ordinary twin.
///
/// # Safety
/// Same contract as [`write_data`].
pub(crate) unsafe fn plugin_write_data(
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    write_data_impl(
        CallFamily::PLUGIN,
        ctx_id,
        field,
        timebase,
        data,
        datatype,
        dim,
        size,
    )
}

/// The policy shared by `write_data` and `plugin_write_data` (issue #125,
/// consolidated onto [`CallFamily`] by issue #109).
///
/// A live conversion record resolves `field` and `timebase` independently;
/// the policy forwards only when both name one safe stored-DD path. Matching,
/// unknown, unstamped, and conversion-disabled contexts carry no record and
/// forward unchanged.
#[allow(clippy::too_many_arguments)]
fn write_data_impl(
    family: CallFamily,
    ctx_id: c_int,
    field: *const c_char,
    timebase: *const c_char,
    data: *mut c_void,
    datatype: c_int,
    dim: c_int,
    size: *mut c_int,
) -> al_status_t {
    let (_reentry_guard, already_entered) = ReentryGuard::enter();
    if already_entered {
        return call_write(family, ctx_id, field, timebase, data, datatype, dim, size);
    }
    let Some(record) = live_conversion_record(ctx_id) else {
        return call_write(family, ctx_id, field, timebase, data, datatype, dim, size);
    };

    let field_argument = seam_policy::WriteArgument {
        resolution: path_conversion::narrow_write_path(
            &record,
            field,
            path_conversion::ArgumentRole::Field,
            path_conversion::resolve(&record, field),
        ),
        // SAFETY: this function's contract requires `field` to be a valid,
        // NUL-terminated C string, or null.
        forward: unsafe { c_str_ref(field) },
        dd_path: read_argument_path(&record, field),
    };
    let timebase_argument = seam_policy::WriteArgument {
        resolution: path_conversion::narrow_write_path(
            &record,
            timebase,
            path_conversion::ArgumentRole::Timebase,
            path_conversion::resolve(&record, timebase),
        ),
        // SAFETY: this function's contract requires `timebase` to be a valid,
        // NUL-terminated C string, or null.
        forward: unsafe { c_str_ref(timebase) },
        dd_path: read_argument_path(&record, timebase),
    };
    let shape = seam_policy::BufferShape {
        datatype: if datatype == DOUBLE_DATA_ID {
            seam_policy::BufferDataType::Double
        } else {
            seam_policy::BufferDataType::Other
        },
        rank: dim,
    };
    // SAFETY: `write_data_impl` has the same pointer contract as
    // `build_source_view`; it borrows the caller buffer only long enough for
    // the policy to build its owned transformed copy.
    let source = unsafe { build_source_view(data, datatype, dim, size) };
    match seam_policy::run_write(&field_argument, &timebase_argument, shape, source) {
        seam_policy::WriteVerdict::Forward {
            field,
            timebase,
            data: transformed_data,
            unwritten_candidates,
        } => {
            let forward_data = transformed_data
                .as_ref()
                .map_or(data, |values| values.as_ptr().cast_mut().cast::<c_void>());
            let status = call_write(
                family,
                ctx_id,
                field.map_or(std::ptr::null(), CStr::as_ptr),
                timebase.map_or(std::ptr::null(), CStr::as_ptr),
                forward_data,
                datatype,
                dim,
                size,
            );
            if status.code == 0 {
                retain_unwritten_candidates(&record, &unwritten_candidates);
            }
            status
        }
        seam_policy::WriteVerdict::Refusal { reason, dd_path } => {
            finish_write_refusal(&record, &reason, &dd_path)
        }
    }
}

/// Records the candidates a successful write deliberately left alone.
///
/// This is the write path's only fidelity verdict, and it is deliberately not
/// the one the artifact declares. Every `merged` rule in the shipped artifact
/// declares `lossy` — ADR 0008's *certain* bucket — but that declaration is a
/// statement about a **read**, where two stored spellings may disagree and the
/// reader cannot tell which it got. A write puts one value into one slot, so
/// what it risks is only that some other reader later finds a stale value
/// under a spelling this write did not touch: unverified, hence
/// `PotentiallyLossy` (ADR 0016 decision 12).
///
/// Together with `finish_write_refusal`'s `Fidelity::Unmappable`, these are
/// the only two fidelities the write seam can produce, which is what makes
/// `Fidelity::Lossy` unreachable from a write. That claim is pinned by
/// `a_declared_lossy_candidate_plan_still_retains_a_potential_loss` rather
/// than left to a reader to derive from these two literals (ADR 0011).
fn retain_unwritten_candidates(record: &ConversionRecord, unwritten: &[&str]) {
    for dd_path in unwritten {
        retain_loss(
            record,
            (*dd_path).to_string(),
            Fidelity::PotentiallyLossy,
            LossOperation::Write,
        );
    }
}

/// Turns a write-policy refusal into the two caller-visible consequences the
/// write seam owes: a root-owned `WRITE` loss and the formatted conversion
/// refusal. The path was already resolved against the live record, so both
/// effects use that same complete HLI-DD spelling.
fn finish_write_refusal(record: &ConversionRecord, reason: &str, dd_path: &str) -> al_status_t {
    retain_loss(
        record,
        dd_path.to_string(),
        Fidelity::Unmappable,
        LossOperation::Write,
    );
    context_path_refusal(record, reason, dd_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    use crate::conversion::conversion_map::{ConversionMap, Direction};
    use crate::conversion::path_conversion::WritePath;
    use crate::registry::context_registry::{MapCacheKey, REGISTRY, RootRegistration};

    #[test]
    fn a_declared_unmappable_write_refusal_carries_its_message_and_write_loss() {
        const CTX_ID: c_int = 0x5D03;
        const FIXTURE_IDS: &str = "equilibrium-unmappable-write-seam-fixture";
        const ARTIFACT: &str = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="declared-impossible" rel="renamed" left="impossible" right="stored">
                  <fidelity forward="unmappable" reverse="exact"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        let stored = "4.1.1".parse().expect("known release");
        let hli = "3.39.0".parse().expect("known release");
        assert!(REGISTRY.record_root(
            RootRegistration {
                ctx_id: CTX_ID,
                resolved_path: String::new(),
                pulse_ctx_id: CTX_ID,
                dataobjectname: "equilibrium".to_string(),
                key: MapCacheKey::new(FIXTURE_IDS.to_string(), stored, hli),
                direction_to_stored: Direction::Forward,
            },
            || ConversionMap::load(ARTIFACT).expect("fixture artifact must load"),
        ));
        let record = REGISTRY
            .lookup(CTX_ID)
            .expect("the root record was just registered");
        let path = CString::new("impossible").expect("fixture path contains no NUL");
        let (reason, dd_path) = match path_conversion::narrow_write_path(
            &record,
            path.as_ptr(),
            path_conversion::ArgumentRole::Field,
            path_conversion::resolve(&record, path.as_ptr()),
        ) {
            WritePath::Refusal {
                reason, dd_path, ..
            } => (reason, dd_path),
            WritePath::Forward | WritePath::Translated { .. } | WritePath::Candidates(_) => {
                panic!("a declared-unmappable write must refuse")
            }
        };

        let status = finish_write_refusal(&record, &reason, &dd_path);
        assert_eq!(status.code, crate::IMAS_MVDD_CONVERSION_ERROR);
        let message = unsafe { CStr::from_ptr(status.message.as_ptr()) }
            .to_str()
            .expect("refusal message is UTF-8");
        assert_eq!(
            message,
            "IMAS-MVDD: this path has no safe conversion between DD versions; DD path: impossible; \
             HLI DD version: 3.39.0; stored DD version: 4.1.1"
        );
        assert_eq!(REGISTRY.loss_count(CTX_ID), 1);
        REGISTRY
            .with_loss_at(CTX_ID, 0, |path, fidelity, operation| {
                assert_eq!(path, "impossible");
                assert_eq!(fidelity, Fidelity::Unmappable);
                assert_eq!(operation, crate::loss::LossOperation::Write);
            })
            .expect("the refused write must retain its loss");

        REGISTRY.remove(CTX_ID);
    }

    /// Issue #128 / ADR 0016 decision 12: the write path produces no
    /// `Fidelity::Lossy` verdict at all.
    ///
    /// The fixture declares its `merged` rule `lossy` in the direction under
    /// test, which is the one input that could make the certain bucket
    /// reachable — every `merged` rule in the shipped artifact declares
    /// exactly that. The write seam must still record `PotentiallyLossy`,
    /// because the declared fidelity describes a read: it is certain that two
    /// stored spellings may disagree when *read*, and merely possible that
    /// some later reader finds the stale one after a write put its value in
    /// the primary slot.
    ///
    /// If this ever records `Lossy`, the write path has grown a producer for a
    /// verdict that has never had coverage — add real coverage for it rather
    /// than relaxing this assertion (ADR 0011).
    #[test]
    fn a_declared_lossy_candidate_plan_still_retains_a_potential_loss() {
        const CTX_ID: c_int = 0x5D05;
        const FIXTURE_IDS: &str = "equilibrium-write-lossy-candidate-fixture";
        const ARTIFACT: &str = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
              <rules>
                <rule id="fold-two" rel="merged" right="folded">
                  <from left="primary" precedence="1"/>
                  <from left="secondary" precedence="2"/>
                  <fidelity forward="exact" reverse="lossy"/>
                </rule>
              </rules>
            </ids-map>
        "#;
        // A `merged` rule offers its candidate plan on the side that folds —
        // the HLI asks for the one canonical name and several stored names can
        // serve it — so this record travels reverse: a 4.1.1 HLI over a
        // 3.39.0 occurrence, which is also the direction the fixture declares
        // `lossy`.
        let stored = "3.39.0".parse().expect("known release");
        let hli = "4.1.1".parse().expect("known release");
        assert!(REGISTRY.record_root(
            RootRegistration {
                ctx_id: CTX_ID,
                resolved_path: String::new(),
                pulse_ctx_id: CTX_ID,
                dataobjectname: "equilibrium".to_string(),
                key: MapCacheKey::new(FIXTURE_IDS.to_string(), stored, hli),
                direction_to_stored: Direction::Reverse,
            },
            || ConversionMap::load(ARTIFACT).expect("fixture artifact must load"),
        ));
        let record = REGISTRY
            .lookup(CTX_ID)
            .expect("the root record was just registered");

        let field = CString::new("folded").expect("fixture path contains no NUL");
        let resolution = path_conversion::narrow_write_path(
            &record,
            field.as_ptr(),
            path_conversion::ArgumentRole::Field,
            path_conversion::resolve(&record, field.as_ptr()),
        );
        assert!(
            matches!(resolution, WritePath::Candidates(_)),
            "the fixture must resolve to a candidate plan, or this proves nothing"
        );
        let field_argument = seam_policy::WriteArgument {
            resolution,
            forward: None,
            dd_path: "folded".to_string(),
        };
        let timebase_argument = seam_policy::WriteArgument {
            resolution: WritePath::Forward,
            forward: None,
            dd_path: String::new(),
        };
        let values = [1.0f64];
        let verdict = seam_policy::run_write(
            &field_argument,
            &timebase_argument,
            seam_policy::BufferShape {
                datatype: seam_policy::BufferDataType::Double,
                rank: 1,
            },
            seam_policy::SourceView::Double(&values),
        );
        let seam_policy::WriteVerdict::Forward {
            unwritten_candidates,
            ..
        } = verdict
        else {
            panic!("a precedence-1 write over a candidate plan must forward")
        };
        assert_eq!(unwritten_candidates, vec!["secondary"]);

        retain_unwritten_candidates(&record, &unwritten_candidates);
        assert_eq!(REGISTRY.loss_count(CTX_ID), 1);
        REGISTRY
            .with_loss_at(CTX_ID, 0, |path, fidelity, operation| {
                assert_eq!(path, "secondary");
                assert_eq!(fidelity, Fidelity::PotentiallyLossy);
                assert_eq!(operation, crate::loss::LossOperation::Write);
            })
            .expect(
                "the write seam recorded something other than one PotentiallyLossy entry \
                 for a rule the artifact declares certainly lossy",
            );

        REGISTRY.remove(CTX_ID);
    }
}
