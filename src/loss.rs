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
    Delete,
}

impl LossOperation {
    /// Renders this operation for the shim-owned C ABI.
    pub(crate) fn c_code(self) -> c_int {
        match self {
            Self::Read => crate::IMAS_MVDD_LOSS_OPERATION_READ,
            Self::Write => crate::IMAS_MVDD_LOSS_OPERATION_WRITE,
            Self::Delete => crate::IMAS_MVDD_LOSS_OPERATION_DELETE,
        }
    }

    /// Renders this operation for an on-disk loss report.
    pub(crate) fn file_word(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
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

#[cfg(test)]
mod tests {
    use super::{LossLog, LossOperation, fidelity_c_code, fidelity_file_word};
    use crate::conversion::conversion_map::Fidelity;

    #[test]
    fn loss_operation_codes_and_file_words_match_the_literal_delivery_contract() {
        for (operation, expected_code, expected_word) in [
            (LossOperation::Read, 0, "read"),
            (LossOperation::Write, 1, "write"),
            (LossOperation::Delete, 2, "delete"),
        ] {
            assert_eq!(operation.c_code(), expected_code);
            assert_eq!(operation.file_word(), expected_word);
        }
    }

    #[test]
    fn fidelity_codes_and_file_words_match_the_literal_delivery_contract() {
        for (fidelity, expected_word) in [
            (Fidelity::Exact, "EXACT"),
            (Fidelity::PotentiallyLossy, "POTENTIALLY_LOSSY"),
            (Fidelity::Lossy, "LOSSY"),
            (Fidelity::Unmappable, "UNMAPPABLE"),
        ] {
            assert_eq!(fidelity_file_word(fidelity), expected_word);
        }

        for (fidelity, expected_code) in [
            (Fidelity::PotentiallyLossy, 0),
            (Fidelity::Lossy, 1),
            (Fidelity::Unmappable, 2),
        ] {
            assert_eq!(fidelity_c_code(fidelity), expected_code);
        }
    }

    #[test]
    fn exact_operations_never_enter_an_empty_loss_log() {
        let mut losses = LossLog::default();

        for operation in [
            LossOperation::Read,
            LossOperation::Write,
            LossOperation::Delete,
        ] {
            losses.retain(format!("exact/{operation:?}"), Fidelity::Exact, operation);
        }

        assert_eq!(losses.len(), 0);
        assert_eq!(losses.with_at(0, |_, _, _| ()), None);
    }

    #[test]
    fn non_exact_operations_retain_the_supplied_path_verdict_and_operation_in_order() {
        let mut losses = LossLog::default();
        let expected = [
            (
                "read/potential",
                Fidelity::PotentiallyLossy,
                LossOperation::Read,
            ),
            ("read/lossy", Fidelity::Lossy, LossOperation::Read),
            ("read/unmappable", Fidelity::Unmappable, LossOperation::Read),
            (
                "write/potential",
                Fidelity::PotentiallyLossy,
                LossOperation::Write,
            ),
            ("write/lossy", Fidelity::Lossy, LossOperation::Write),
            (
                "write/unmappable",
                Fidelity::Unmappable,
                LossOperation::Write,
            ),
            (
                "delete/potential",
                Fidelity::PotentiallyLossy,
                LossOperation::Delete,
            ),
            ("delete/lossy", Fidelity::Lossy, LossOperation::Delete),
            (
                "delete/unmappable",
                Fidelity::Unmappable,
                LossOperation::Delete,
            ),
        ];

        for (path, fidelity, operation) in expected {
            losses.retain(path.to_string(), fidelity, operation);
        }

        assert_eq!(losses.len(), expected.len());
        for (index, expected_entry) in expected.iter().enumerate() {
            assert_eq!(
                losses.with_at(index, |path, fidelity, operation| {
                    (path.to_string(), fidelity, operation)
                }),
                Some((
                    expected_entry.0.to_string(),
                    expected_entry.1,
                    expected_entry.2,
                ))
            );
        }
        assert_eq!(losses.with_at(expected.len(), |_, _, _| ()), None);
    }

    #[test]
    fn repeated_non_exact_entries_remain_individually_available_in_memory() {
        let mut losses = LossLog::default();
        for _ in 0..2 {
            losses.retain(
                "repeated/path".to_string(),
                Fidelity::PotentiallyLossy,
                LossOperation::Read,
            );
        }

        assert_eq!(losses.len(), 2);
        for index in 0..2 {
            assert_eq!(
                losses.with_at(index, |path, fidelity, operation| {
                    (path.to_string(), fidelity, operation)
                }),
                Some((
                    "repeated/path".to_string(),
                    Fidelity::PotentiallyLossy,
                    LossOperation::Read,
                ))
            );
        }
    }
}
