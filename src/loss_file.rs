//! Append-only, process-local delivery of loss-log entries.
//!
//! The registry owns the context-scoped in-memory log. This module owns the
//! separate process-scoped written-key set and deliberately receives copied
//! occurrence facts, so filesystem I/O happens after every registry lock has
//! been released.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::conversion::conversion_map::Fidelity;
use crate::loss::{LossOperation, fidelity_file_word};

static WRITTEN_KEYS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static LOG_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
static FILE_FAILED: AtomicBool = AtomicBool::new(false);

pub(crate) struct LossFileEntry {
    uri: String,
    ids: String,
    stored_version: String,
    hli_version: String,
    operation: LossOperation,
    fidelity: Fidelity,
    path: String,
}

impl LossFileEntry {
    /// Copies the occurrence facts a caller needs to render one line, so
    /// nothing this module holds is a reference back into the registry.
    pub(crate) fn new(
        uri: &str,
        ids: &str,
        stored_version: &str,
        hli_version: &str,
        operation: LossOperation,
        fidelity: Fidelity,
        path: &str,
    ) -> Self {
        Self {
            uri: uri.to_string(),
            ids: ids.to_string(),
            stored_version: stored_version.to_string(),
            hli_version: hli_version.to_string(),
            operation,
            fidelity,
            path: path.to_string(),
        }
    }
}

/// Writes `entry` once for this process. Its caller is the one place that
/// filters an exact-fidelity operation out of both loss sinks (ADR 0012), so
/// this never sees one. The key lock is released before any path resolution,
/// file creation, or write, so a slow filesystem cannot hold a seam lock.
pub(crate) fn retain(entry: LossFileEntry) {
    let line = format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        entry.uri,
        entry.ids,
        entry.stored_version,
        entry.hli_version,
        entry.operation.file_word(),
        fidelity_file_word(entry.fidelity),
        entry.path,
    );
    let is_new = WRITTEN_KEYS.lock().unwrap().insert(line.clone());
    if !is_new {
        return;
    }
    if FILE_FAILED.load(Ordering::Relaxed) {
        return;
    }

    let path = LOG_PATH.get_or_init(|| create_log(&entry)).as_ref();
    if let Some(path) = path
        && let Err(error) = append(path, &line)
    {
        report_failure(format_args!(
            "could not append loss log {}: {error}",
            path.display()
        ));
    }
}

fn create_log(entry: &LossFileEntry) -> Option<PathBuf> {
    let seconds = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(error) => {
            report_failure(format_args!("could not read the UTC clock: {error}"));
            return None;
        }
    };
    let timestamp = utc_timestamp(seconds);
    let directory = match std::env::var_os("IMAS_MVDD_LOSS_LOG_DIR") {
        Some(directory) if directory.is_empty() => return None,
        Some(directory) => PathBuf::from(directory),
        None => match std::env::current_dir() {
            Ok(directory) => directory,
            Err(error) => {
                report_failure(format_args!(
                    "could not resolve the loss-log working directory: {error}"
                ));
                return None;
            }
        },
    };
    if !directory.is_dir() {
        report_failure(format_args!(
            "could not write loss log in {}: directory does not exist",
            directory.display()
        ));
        return None;
    }

    for suffix in 0_u32.. {
        let suffix = if suffix == 0 {
            String::new()
        } else {
            format!("-{suffix}")
        };
        let path = directory.join(format!(
            "imas-mvdd-loss-{timestamp}-{}{}.txt",
            std::process::id(),
            suffix
        ));
        match OpenOptions::new().append(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = write_preamble(&mut file, &timestamp, entry) {
                    report_failure(format_args!(
                        "could not initialize loss log {}: {error}",
                        path.display()
                    ));
                    return None;
                }
                return Some(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                report_failure(format_args!(
                    "could not write loss log in {}: {error}",
                    directory.display()
                ));
                return None;
            }
        }
    }
    unreachable!("the u32 suffix space cannot be exhausted")
}

/// Reports one filesystem failure without changing an HLI call's outcome.
fn report_failure(message: std::fmt::Arguments<'_>) {
    if !FILE_FAILED.swap(true, Ordering::Relaxed) {
        eprintln!("IMAS-MVDD: {message}");
    }
}

fn write_preamble(file: &mut File, timestamp: &str, entry: &LossFileEntry) -> std::io::Result<()> {
    writeln!(file, "# imas-mvdd loss log format 1")?;
    writeln!(file, "# written {timestamp}")?;
    writeln!(file, "# process {}", std::process::id())?;
    writeln!(file, "# hli-dd-version {}", entry.hli_version)?;
    writeln!(
        file,
        "uri\tids\tstored-dd\thli-dd\toperation\tfidelity\tpath"
    )
}

fn append(path: &Path, line: &str) -> std::io::Result<()> {
    OpenOptions::new()
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

/// Formats a non-negative Unix timestamp in UTC without a calendar crate.
pub(crate) fn utc_timestamp(epoch_seconds: u64) -> String {
    let days = epoch_seconds / 86_400;
    let seconds_of_day = epoch_seconds % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds_of_day / 3_600,
        (seconds_of_day / 60) % 60,
        seconds_of_day % 60,
    )
}

// Howard Hinnant's public-domain civil-date conversion, with day zero set to
// 1970-01-01. Keeping it integer-only makes the filename clock testable.
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::utc_timestamp;

    #[test]
    fn utc_timestamp_covers_epoch_leap_day_year_boundary_and_far_future() {
        assert_eq!(utc_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_timestamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc_timestamp(1_609_459_199), "2020-12-31T23:59:59Z");
        assert_eq!(utc_timestamp(4_102_444_800), "2100-01-01T00:00:00Z");
    }
}
