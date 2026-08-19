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
//! This module is deliberately untested by ordinary `#[cfg(test)]` unit
//! tests: `LATCH` is a single process-wide `OnceLock`, and `cargo test`
//! runs every test in one process, so two tests exercising this module
//! would race for who latches it first. The test seam is the public C ABI,
//! exercised in isolated ctest processes (see `tests/hli_dd_version_test.c`).

use std::env::{self, VarError};
use std::ffi::{CStr, c_char};
use std::sync::OnceLock;

use crate::dd_version::DdVersion;

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
    let parsed: DdVersion = version.parse()?;
    match LATCH.get_or_init(|| Latch::Set(parsed.clone())) {
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

/// Resolves the latch for the first open (ADR 0005): the setter's value if
/// already latched, else `IMAS_MVDD_HLI_DD_VERSION`, else unset. Whichever
/// outcome is found settles atomically for the rest of the process. Returns
/// an error only for an invalid environment value — the shim refusing to
/// silently fall back to passthrough.
pub(crate) fn resolve_for_open() -> Result<(), String> {
    match LATCH.get_or_init(|| match env::var(ENV_VAR) {
        Ok(raw) => match raw.parse::<DdVersion>() {
            Ok(version) => Latch::Set(version),
            Err(reason) => Latch::Invalid(reason),
        },
        Err(VarError::NotPresent) => Latch::Unset,
        Err(VarError::NotUnicode(_)) => Latch::Invalid(format!("{ENV_VAR} is not valid UTF-8")),
    }) {
        Latch::Invalid(reason) => Err(reason.clone()),
        Latch::Set(_) | Latch::Unset => Ok(()),
    }
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
    match LATCH.get()? {
        Latch::Set(version) => Some(version.clone()),
        Latch::Unset | Latch::Invalid(_) => None,
    }
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
    matches!(LATCH.get(), Some(Latch::Set(_)))
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
