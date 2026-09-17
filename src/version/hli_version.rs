//! Process-wide HLI DD version latch (issue #45, ADR 0005).
//!
//! The HLI DD version arrives through the shim-owned setter
//! (`imas_mvdd_set_hli_dd_version`) or the `IMAS_MVDD_HLI_DD_VERSION`
//! environment variable, and *latches* on first use for the life of the
//! process: an identical later report is accepted, a conflicting later
//! report is refused naming both versions, and the setter always takes
//! precedence over the environment. Resolution is safe from any thread —
//! the latch is backed by `OnceLock`, so first-writer-wins is decided
//! atomically and the conflict check can never observe a torn value.
//!
//! Unset latches too: if the setter is never called and the environment
//! variable is unset, the first open settles the process to "unset" for
//! good, and a setter call arriving after that is refused rather than
//! silently applied to later opens.
//!
//! The process-wide store is deliberately small: [`HliVersionLatch`] owns the
//! decision model, while `LATCH` applies its first settled state atomically in
//! production. Rust tests therefore exercise fresh decision-model instances;
//! process-isolated C tests remain the authority for the public entry point and
//! concurrent identical-setter safety.

use std::env::{self, VarError};
use std::ffi::{CStr, c_char};
use std::sync::OnceLock;

use crate::version::dd_version::DdVersion;

/// Environment-variable fallback, read only if the setter was never called
/// (ADR 0005): the setter always takes precedence and the environment can
/// never itself produce a conflict.
const ENV_VAR: &str = "IMAS_MVDD_HLI_DD_VERSION";

#[derive(Debug, Clone)]
enum Latch {
    /// A valid HLI DD version, from the setter or the environment.
    Set(DdVersion),
    /// No setter call and no environment variable: conversion stays off.
    Unset,
    /// The environment variable held a value `DdVersion` rejects. This
    /// latches like any other outcome, so the refusal is consistent for the
    /// rest of the process rather than silently retried on every open.
    Invalid(String),
}

/// The environment observation a first open can make. Keeping it a value
/// lets the latch model stay process-local and makes a non-Unicode value
/// testable without modifying this process's environment.
#[derive(Debug)]
enum EnvironmentValue {
    Absent,
    Value(String),
    NotUnicode,
}

impl EnvironmentValue {
    fn current() -> Self {
        match env::var(ENV_VAR) {
            Ok(raw) => Self::Value(raw),
            Err(VarError::NotPresent) => Self::Absent,
            Err(VarError::NotUnicode(_)) => Self::NotUnicode,
        }
    }
}

impl Latch {
    fn from_environment(environment: EnvironmentValue) -> Self {
        match environment {
            EnvironmentValue::Absent => Self::Unset,
            EnvironmentValue::Value(raw) => match raw.parse::<DdVersion>() {
                Ok(version) => Self::Set(version),
                Err(reason) => Self::Invalid(reason),
            },
            EnvironmentValue::NotUnicode => Self::Invalid(format!("{ENV_VAR} is not valid UTF-8")),
        }
    }
}

/// The isolated ADR 0005 decision model. Production copies its settled state
/// into the one process-wide [`OnceLock`]; tests create one model per case.
#[derive(Debug, Default)]
struct HliVersionLatch {
    latch: Option<Latch>,
}

impl HliVersionLatch {
    fn from_latch(latch: Latch) -> Self {
        Self { latch: Some(latch) }
    }

    fn into_latch(self) -> Latch {
        self.latch
            .expect("a production latch candidate must settle before storage")
    }

    fn set(&mut self, version: &str) -> Result<(), String> {
        self.set_parsed(version.parse()?)
    }

    fn set_parsed(&mut self, parsed: DdVersion) -> Result<(), String> {
        match self.latch.get_or_insert_with(|| Latch::Set(parsed.clone())) {
            Latch::Set(existing) if *existing == parsed => Ok(()),
            Latch::Set(existing) => Err(format!(
                "conflicting HLI DD version: this process already latched to '{existing}' \
                 and cannot also serve '{parsed}' — one process cannot host two HLIs built \
                 against different DD versions"
            )),
            Latch::Unset => Err(format!(
                "cannot set HLI DD version to '{parsed}': this process already latched to \
                 unset, after an earlier open found no setter call and no valid {ENV_VAR}"
            )),
            Latch::Invalid(reason) => Err(format!(
                "cannot set HLI DD version to '{parsed}': this process already latched to \
                 an invalid {ENV_VAR} value at an earlier open ({reason})"
            )),
        }
    }

    fn resolve_for_open<F>(&mut self, environment: F) -> Result<(), String>
    where
        F: FnOnce() -> EnvironmentValue,
    {
        if self.latch.is_none() {
            self.latch = Some(Latch::from_environment(environment()));
        }
        self.open_result()
    }

    fn open_result(&self) -> Result<(), String> {
        match self.latch.as_ref() {
            Some(Latch::Invalid(reason)) => Err(reason.clone()),
            Some(Latch::Set(_) | Latch::Unset) => Ok(()),
            None => unreachable!("an open resolution always settles the model"),
        }
    }

    #[cfg(test)]
    fn latched(&self) -> Option<DdVersion> {
        Self::latched_from(self.latch.as_ref())
    }

    fn latched_from(latch: Option<&Latch>) -> Option<DdVersion> {
        match latch? {
            Latch::Set(version) => Some(version.clone()),
            Latch::Unset | Latch::Invalid(_) => None,
        }
    }

    #[cfg(test)]
    fn conversion_is_possible(&self) -> bool {
        Self::conversion_is_possible_from(self.latch.as_ref())
    }

    fn conversion_is_possible_from(latch: Option<&Latch>) -> bool {
        matches!(latch, Some(Latch::Set(_)))
    }
}

static LATCH: OnceLock<Latch> = OnceLock::new();

/// Reports the calling HLI's DD version (the setter half of ADR 0005).
///
/// An invalid version string fails immediately and never touches the
/// latch. A first, valid report latches it. An identical repeat is
/// accepted. A conflicting repeat — a different version already latched,
/// whether by an earlier setter call or by the environment resolving at an
/// earlier open — is refused, naming both versions and the one-process/
/// two-HLI conflict this guards against. A report arriving after the
/// process already latched to unset (an earlier open with no setter and no
/// valid environment variable) is refused too.
pub(crate) fn set(version: &str) -> Result<(), String> {
    let mut candidate = HliVersionLatch::default();
    candidate.set(version)?;
    let settled = LATCH.get_or_init(|| candidate.into_latch());
    HliVersionLatch::from_latch(settled.clone()).set(version)
}

/// Resolves the latch for the first open (ADR 0005): the setter's value if
/// already latched, else `IMAS_MVDD_HLI_DD_VERSION`, else unset. Whichever
/// outcome is found settles atomically for the rest of the process. Returns
/// an error only for an invalid environment value — the shim refusing to
/// silently fall back to passthrough.
pub(crate) fn resolve_for_open() -> Result<(), String> {
    let settled = match LATCH.get() {
        Some(latch) => latch,
        None => {
            let mut candidate = HliVersionLatch::default();
            let _ = candidate.resolve_for_open(EnvironmentValue::current);
            LATCH.get_or_init(|| candidate.into_latch())
        }
    };
    HliVersionLatch::from_latch(settled.clone())
        .resolve_for_open(|| unreachable!("a settled latch must not read the environment"))
}

/// The HLI DD version already latched for this process, if any. `None`
/// covers every case a seam must treat as "no conversion basis": unset (no
/// setter call and no valid environment variable), an invalid environment
/// value, or a latch that has not resolved yet because no open has happened.
/// Callers reach this only after `al_begin_dataentry_action` has already run
/// at least once for the calling process, since that is the earliest point
/// the latch can resolve (ADR 0005) — a seam calling this beforehand simply
/// sees `None` and forwards unchanged, same as the unset case.
pub(crate) fn latched() -> Option<DdVersion> {
    HliVersionLatch::latched_from(LATCH.get())
}

/// Whether this process has any conversion basis at all — the same question
/// [`latched`] answers, without cloning the version out of the latch.
///
/// The data-path seams (`al_read_data`, `al_write_data`, `al_delete_data` and
/// their plugin reentry twins) ask it once per call, ahead of any registry
/// lookup: with the latch unset, invalid, or not yet resolved, no context can
/// carry a conversion record, so taking the registry's lock to discover that
/// would be pure cost on the hot path every non-converting HLI takes for every
/// field it reads (issue #56's "conversion-disabled contexts bypass registry
/// lookup and rule resolution", ADR 0003's one-lookup budget). The `begin_*`
/// seams short-circuit on [`latched`] instead, since they go on to use the
/// version itself.
pub(crate) fn conversion_is_possible() -> bool {
    HliVersionLatch::conversion_is_possible_from(LATCH.get())
}

/// C entry point for `imas_mvdd_set_hli_dd_version`: validates the pointer
/// itself (null, non-UTF-8) as an immediate refusal before parsing the
/// version string.
///
/// # Safety
/// `version` must be a valid, NUL-terminated C string, or null.
pub(crate) unsafe fn set_from_c(version: *const c_char) -> crate::al_status_t {
    if version.is_null() {
        return crate::conversion_refusal("HLI DD version must not be null");
    }
    let version = match unsafe { CStr::from_ptr(version) }.to_str() {
        Ok(version) => version,
        Err(_) => return crate::conversion_refusal("HLI DD version must be valid UTF-8"),
    };
    match set(version) {
        Ok(()) => crate::al_status_t::default(),
        Err(reason) => crate::conversion_refusal(&reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(input: &str) -> DdVersion {
        input.parse().expect("test DD version must be valid")
    }

    #[test]
    fn a_first_valid_setter_report_latches_conversion_and_identical_repeats() {
        let mut latch = HliVersionLatch::default();

        assert_eq!(latch.latched(), None);
        assert!(!latch.conversion_is_possible());
        assert_eq!(latch.set("4.1.1"), Ok(()));
        assert_eq!(latch.latched(), Some(version("4.1.1")));
        assert!(latch.conversion_is_possible());
        assert_eq!(latch.set("4.1.1"), Ok(()));
    }

    #[test]
    fn a_conflicting_setter_report_keeps_the_first_version() {
        let mut latch = HliVersionLatch::default();
        assert_eq!(latch.set("3.39.0"), Ok(()));

        assert_eq!(
            latch.set("4.1.1"),
            Err(
                "conflicting HLI DD version: this process already latched to '3.39.0' \
                 and cannot also serve '4.1.1' — one process cannot host two HLIs built \
                 against different DD versions"
                    .to_string()
            )
        );
        assert_eq!(latch.latched(), Some(version("3.39.0")));
        assert!(latch.conversion_is_possible());
    }

    #[test]
    fn an_invalid_setter_report_leaves_the_latch_unresolved() {
        let mut latch = HliVersionLatch::default();

        assert_eq!(
            latch.set("not-a-version"),
            Err("'not' is not MAJOR.MINOR.PATCH".to_string())
        );
        assert_eq!(latch.latched(), None);
        assert!(!latch.conversion_is_possible());
        assert_eq!(latch.set("4.1.1"), Ok(()));
    }

    #[test]
    fn reconstructing_each_settled_latch_state_preserves_its_conversion_basis() {
        let cases = [
            (Latch::Set(version("4.1.1")), Some(version("4.1.1")), true),
            (Latch::Unset, None, false),
            (
                Latch::Invalid("IMAS_MVDD_HLI_DD_VERSION is not valid UTF-8".to_string()),
                None,
                false,
            ),
        ];

        for (settled, expected_version, conversion_is_possible) in cases {
            let reconstructed = HliVersionLatch::from_latch(settled);

            assert_eq!(reconstructed.latched(), expected_version);
            assert_eq!(
                reconstructed.conversion_is_possible(),
                conversion_is_possible
            );
        }
    }

    #[test]
    fn absent_environment_permanently_latches_unset_and_refuses_late_setters() {
        let mut latch = HliVersionLatch::default();

        assert_eq!(latch.resolve_for_open(|| EnvironmentValue::Absent), Ok(()));
        assert_eq!(latch.latched(), None);
        assert!(!latch.conversion_is_possible());
        assert_eq!(
            latch.resolve_for_open(|| panic!("a settled latch must not reread the environment")),
            Ok(())
        );
        assert_eq!(
            latch.set("4.1.1"),
            Err(
                "cannot set HLI DD version to '4.1.1': this process already latched to \
                 unset, after an earlier open found no setter call and no valid \
                 IMAS_MVDD_HLI_DD_VERSION"
                    .to_string()
            )
        );
    }

    #[test]
    fn valid_environment_latches_conversion_and_ignores_later_environment_changes() {
        let mut latch = HliVersionLatch::default();

        assert_eq!(
            latch.resolve_for_open(|| EnvironmentValue::Value("3.39.0".to_string())),
            Ok(())
        );
        assert_eq!(latch.latched(), Some(version("3.39.0")));
        assert!(latch.conversion_is_possible());
        assert_eq!(
            latch.resolve_for_open(|| panic!("a settled latch must not reread the environment")),
            Ok(())
        );
        assert_eq!(latch.set("3.39.0"), Ok(()));
        assert_eq!(
            latch.set("4.1.1"),
            Err(
                "conflicting HLI DD version: this process already latched to '3.39.0' \
                 and cannot also serve '4.1.1' — one process cannot host two HLIs built \
                 against different DD versions"
                    .to_string()
            )
        );
    }

    #[test]
    fn invalid_and_non_unicode_environment_values_permanently_disable_conversion() {
        let cases = [
            (
                EnvironmentValue::Value("not-a-version".to_string()),
                "'not' is not MAJOR.MINOR.PATCH",
            ),
            (
                EnvironmentValue::NotUnicode,
                "IMAS_MVDD_HLI_DD_VERSION is not valid UTF-8",
            ),
        ];

        for (environment, expected) in cases {
            let mut latch = HliVersionLatch::default();
            assert_eq!(
                latch.resolve_for_open(|| environment),
                Err(expected.to_string())
            );
            assert_eq!(latch.latched(), None);
            assert!(!latch.conversion_is_possible());
            assert_eq!(
                latch
                    .resolve_for_open(|| panic!("a settled latch must not reread the environment")),
                Err(expected.to_string())
            );
            assert_eq!(
                latch.set("4.1.1"),
                Err(format!(
                    "cannot set HLI DD version to '4.1.1': this process already latched to \
                     an invalid {ENV_VAR} value at an earlier open ({expected})"
                ))
            );
        }
    }

    #[test]
    fn an_accepted_setter_wins_without_consulting_the_environment() {
        let mut latch = HliVersionLatch::default();
        assert_eq!(latch.set("4.1.1"), Ok(()));

        assert_eq!(
            latch.resolve_for_open(|| panic!("a setter-set latch must not read the environment")),
            Ok(())
        );
        assert_eq!(latch.latched(), Some(version("4.1.1")));
        assert!(latch.conversion_is_possible());
    }
}
