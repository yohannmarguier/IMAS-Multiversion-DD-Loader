//! Loss-log domain types and retention rules.
//!
//! A [`LossLog`] owns the entries retained for one root conversion context.
//! It deliberately knows neither context IDs nor registry state: callers
//! supply a complete path and the registry decides which root owns the log.

use std::ffi::c_int;

use crate::conversion::conversion_map::Fidelity;

/// Which seam operation earned a loss-log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LossOperation {
    Read,
    Write,
}

impl LossOperation {
    /// Renders this operation for the shim-owned C ABI.
    pub(crate) fn c_code(self) -> c_int {
        match self {
            Self::Read => crate::IMAS_MVDD_LOSS_OPERATION_READ,
            Self::Write => crate::IMAS_MVDD_LOSS_OPERATION_WRITE,
        }
    }

    /// Renders this operation for an on-disk loss report.
    pub(crate) fn file_word(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// Renders a retained fidelity verdict for the shim-owned C ABI.
pub(crate) fn fidelity_c_code(fidelity: Fidelity) -> c_int {
    match fidelity {
        Fidelity::Exact => {
            unreachable!("the loss log never retains an exact-fidelity operation (ADR 0012)")
        }
        Fidelity::PotentiallyLossy => crate::IMAS_MVDD_FIDELITY_POTENTIALLY_LOSSY,
        Fidelity::Lossy => crate::IMAS_MVDD_FIDELITY_LOSSY,
        Fidelity::Unmappable => crate::IMAS_MVDD_FIDELITY_UNMAPPABLE,
    }
}

/// Renders a fidelity verdict for an on-disk loss report.
pub(crate) fn fidelity_file_word(fidelity: Fidelity) -> &'static str {
    match fidelity {
        Fidelity::Exact => "EXACT",
        Fidelity::PotentiallyLossy => "POTENTIALLY_LOSSY",
        Fidelity::Lossy => "LOSSY",
        Fidelity::Unmappable => "UNMAPPABLE",
    }
}

/// One retained non-exact operation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LossEntry {
    dd_path: String,
    fidelity: Fidelity,
    operation: LossOperation,
}

/// The ordered losses retained for one root conversion context.
#[derive(Default)]
pub(crate) struct LossLog {
    entries: Vec<LossEntry>,
}

impl LossLog {
    /// Retains a non-exact operation in call order. Exact operations never
    /// enter a loss log (ADR 0012).
    pub(crate) fn retain(&mut self, dd_path: String, fidelity: Fidelity, operation: LossOperation) {
        if fidelity != Fidelity::Exact {
            self.entries.push(LossEntry {
                dd_path,
                fidelity,
                operation,
            });
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Gives a caller one entry's values without exposing the entry itself.
    pub(crate) fn with_at<T>(
        &self,
        index: usize,
        read: impl FnOnce(&str, Fidelity, LossOperation) -> T,
    ) -> Option<T> {
        let entry = self.entries.get(index)?;
        Some(read(&entry.dd_path, entry.fidelity, entry.operation))
    }
}
