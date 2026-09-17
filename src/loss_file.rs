//! Append-only, process-local delivery of loss-log entries.
//!
//! The registry owns the context-scoped in-memory log. This module owns the
//! separate process-scoped written-key set and deliberately receives copied
//! occurrence facts, so filesystem I/O happens after every registry lock has
//! been released.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::conversion::conversion_map::Fidelity;
use crate::loss::{LossOperation, fidelity_file_word};

static WRITER: LazyLock<LossFileWriter<ProcessFacts>> =
    LazyLock::new(LossFileWriter::process_local);

#[derive(Clone)]
pub(crate) struct LossFileEntry {
    uri: String,
    ids: String,
    stored_version: String,
    hli_version: String,
    operation: LossOperation,
    fidelity: Fidelity,
    path: String,
}

/// The process facts the append-only writer needs at its boundary with the
/// operating system. Tests supply fixed facts; production reads process facts.
trait LossFileFacts {
    fn configured_directory(&self) -> Option<OsString>;
    fn current_directory(&self) -> std::io::Result<PathBuf>;
    fn epoch_seconds(&self) -> Result<u64, std::time::SystemTimeError>;
    fn process_id(&self) -> u32;
}

struct ProcessFacts;

impl LossFileFacts for ProcessFacts {
    fn configured_directory(&self) -> Option<OsString> {
        std::env::var_os("IMAS_MVDD_LOSS_LOG_DIR")
    }

    fn current_directory(&self) -> std::io::Result<PathBuf> {
        std::env::current_dir()
    }

    fn epoch_seconds(&self) -> Result<u64, std::time::SystemTimeError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
    }

    fn process_id(&self) -> u32 {
        std::process::id()
    }
}

/// One append-only writer. Production holds one process-local instance, while
/// tests construct fresh writers with their own state and temporary directory.
struct LossFileWriter<F> {
    facts: F,
    written_keys: Mutex<HashSet<String>>,
    log_path: OnceLock<Option<PathBuf>>,
    file_failed: AtomicBool,
}

impl LossFileWriter<ProcessFacts> {
    fn process_local() -> Self {
        Self::new(ProcessFacts)
    }
}

impl<F: LossFileFacts> LossFileWriter<F> {
    fn new(facts: F) -> Self {
        Self {
            facts,
            written_keys: Mutex::new(HashSet::new()),
            log_path: OnceLock::new(),
            file_failed: AtomicBool::new(false),
        }
    }

    /// The written-key lock is released before path resolution, file creation,
    /// and append I/O.
    fn retain(&self, entry: LossFileEntry) {
        let line = render(&entry);
        let is_new = self.written_keys.lock().unwrap().insert(line.clone());
        if !is_new || self.file_failed.load(Ordering::Relaxed) {
            return;
        }

        let path = self
            .log_path
            .get_or_init(|| self.create_log(&entry))
            .as_ref();
        if let Some(path) = path
            && let Err(error) = append(path, &line)
        {
            self.report_failure(format_args!(
                "could not append loss log {}: {error}",
                path.display()
            ));
        }
    }

    fn create_log(&self, entry: &LossFileEntry) -> Option<PathBuf> {
        let seconds = match self.facts.epoch_seconds() {
            Ok(seconds) => seconds,
            Err(error) => {
                self.report_failure(format_args!("could not read the UTC clock: {error}"));
                return None;
            }
        };
        let timestamp = utc_timestamp(seconds);
        let directory = match select_directory(
            self.facts.configured_directory(),
            self.facts.current_directory(),
        ) {
            Ok(Some(directory)) => directory,
            Ok(None) => return None,
            Err(error) => {
                self.report_failure(format_args!(
                    "could not resolve the loss-log working directory: {error}"
                ));
                return None;
            }
        };
        if !directory.is_dir() {
            self.report_failure(format_args!(
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
                self.facts.process_id(),
                suffix
            ));
            match OpenOptions::new().append(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    if let Err(error) =
                        write_preamble(&mut file, &timestamp, self.facts.process_id(), entry)
                    {
                        self.report_failure(format_args!(
                            "could not initialize loss log {}: {error}",
                            path.display()
                        ));
                        return None;
                    }
                    return Some(path);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    self.report_failure(format_args!(
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
    fn report_failure(&self, message: std::fmt::Arguments<'_>) {
        if !self.file_failed.swap(true, Ordering::Relaxed) {
            eprintln!("IMAS-MVDD: {message}");
        }
    }
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

/// Resolves the configured destination without touching process-wide state.
/// An empty configuration deliberately opts out, while no configuration uses
/// the supplied working-directory fact.
fn select_directory(
    configured_directory: Option<OsString>,
    current_directory: std::io::Result<PathBuf>,
) -> std::io::Result<Option<PathBuf>> {
    match configured_directory {
        Some(directory) if directory.is_empty() => Ok(None),
        Some(directory) => Ok(Some(PathBuf::from(directory))),
        None => current_directory.map(Some),
    }
}

/// Writes `entry` once for this process. Its caller is the one place that
/// filters an exact-fidelity operation out of both loss sinks (ADR 0012), so
/// this never sees one. The key lock is released before any path resolution,
/// file creation, or write, so a slow filesystem cannot hold a seam lock.
pub(crate) fn retain(entry: LossFileEntry) {
    WRITER.retain(entry);
}

fn render(entry: &LossFileEntry) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        entry.uri,
        entry.ids,
        entry.stored_version,
        entry.hli_version,
        entry.operation.file_word(),
        fidelity_file_word(entry.fidelity),
        entry.path,
    )
}

fn write_preamble(
    file: &mut File,
    timestamp: &str,
    process_id: u32,
    entry: &LossFileEntry,
) -> std::io::Result<()> {
    writeln!(file, "# imas-mvdd loss log format 1")?;
    writeln!(file, "# written {timestamp}")?;
    writeln!(file, "# process {process_id}")?;
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
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{LossFileEntry, LossFileFacts, LossFileWriter, select_directory, utc_timestamp};
    use crate::conversion::conversion_map::Fidelity;
    use crate::loss::LossOperation;

    static NEXT_TEST_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let number = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "imas-mvdd-loss-file-test-{}-{number}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    struct FixedFacts {
        configured_directory: Option<OsString>,
        current_directory: PathBuf,
        epoch_seconds: u64,
        process_id: u32,
    }

    impl FixedFacts {
        fn in_directory(directory: &TestDirectory) -> Self {
            Self {
                configured_directory: Some(directory.path().as_os_str().to_os_string()),
                current_directory: directory.path().clone(),
                epoch_seconds: 951_782_400,
                process_id: 196,
            }
        }
    }

    impl LossFileFacts for FixedFacts {
        fn configured_directory(&self) -> Option<OsString> {
            self.configured_directory.clone()
        }

        fn current_directory(&self) -> std::io::Result<PathBuf> {
            Ok(self.current_directory.clone())
        }

        fn epoch_seconds(&self) -> Result<u64, std::time::SystemTimeError> {
            Ok(self.epoch_seconds)
        }

        fn process_id(&self) -> u32 {
            self.process_id
        }
    }

    fn entry() -> LossFileEntry {
        LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Read,
            Fidelity::PotentiallyLossy,
            "time_slice/ggd/b_field_phi",
        )
    }

    fn only_log(directory: &TestDirectory) -> PathBuf {
        let paths = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 1);
        paths.into_iter().next().unwrap()
    }

    #[test]
    fn writes_lazily_to_the_explicit_directory_with_complete_appended_records() {
        let directory = TestDirectory::new();
        let writer = LossFileWriter::new(FixedFacts::in_directory(&directory));
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());

        writer.retain(entry());
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Delete,
            Fidelity::Unmappable,
            "time_slice/boundary_separatrix/gap/r",
        ));

        let log = only_log(&directory);
        assert_eq!(
            log.file_name().unwrap().to_str().unwrap(),
            "imas-mvdd-loss-2000-02-29T00:00:00Z-196.txt"
        );
        assert_eq!(
            fs::read_to_string(log).unwrap(),
            concat!(
                "# imas-mvdd loss log format 1\n",
                "# written 2000-02-29T00:00:00Z\n",
                "# process 196\n",
                "# hli-dd-version 3.39.0\n",
                "uri\tids\tstored-dd\thli-dd\toperation\tfidelity\tpath\n",
                "imas:hdf5?path=/tmp/pulse\tequilibrium/3\t4.1.1\t3.39.0\tread\tPOTENTIALLY_LOSSY\ttime_slice/ggd/b_field_phi\n",
                "imas:hdf5?path=/tmp/pulse\tequilibrium/3\t4.1.1\t3.39.0\tdelete\tUNMAPPABLE\ttime_slice/boundary_separatrix/gap/r\n",
            ),
        );
    }

    #[test]
    fn uses_the_injected_default_directory_when_no_directory_is_configured() {
        let directory = TestDirectory::new();
        let mut facts = FixedFacts::in_directory(&directory);
        facts.configured_directory = None;
        let writer = LossFileWriter::new(facts);

        writer.retain(entry());

        assert!(only_log(&directory).is_file());
    }

    #[test]
    fn empty_configured_directory_disables_file_delivery() {
        let directory = TestDirectory::new();
        let mut facts = FixedFacts::in_directory(&directory);
        facts.configured_directory = Some(OsString::new());
        let writer = LossFileWriter::new(facts);

        writer.retain(entry());

        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }

    #[test]
    fn directory_selection_keeps_explicit_default_and_empty_settings_distinct() {
        let directory = TestDirectory::new();
        let explicit = directory.path().join("explicit");

        assert_eq!(
            select_directory(
                Some(explicit.as_os_str().to_os_string()),
                Ok(directory.path().join("ignored")),
            )
            .unwrap(),
            Some(explicit),
        );
        assert_eq!(
            select_directory(None, Ok(directory.path().clone())).unwrap(),
            Some(directory.path().clone()),
        );
        assert_eq!(
            select_directory(Some(OsString::new()), Ok(directory.path().clone())).unwrap(),
            None,
        );
    }

    #[test]
    fn a_filename_collision_uses_the_next_numeric_suffix() {
        let directory = TestDirectory::new();
        let base = directory
            .path()
            .join("imas-mvdd-loss-2000-02-29T00:00:00Z-196.txt");
        fs::write(&base, "reserved by another writer\n").unwrap();
        let writer = LossFileWriter::new(FixedFacts::in_directory(&directory));

        writer.retain(entry());

        assert_eq!(
            fs::read_to_string(&base).unwrap(),
            "reserved by another writer\n"
        );
        let suffix = directory
            .path()
            .join("imas-mvdd-loss-2000-02-29T00:00:00Z-196-1.txt");
        assert_eq!(
            fs::read_to_string(suffix).unwrap().lines().last(),
            Some(
                "imas:hdf5?path=/tmp/pulse\tequilibrium/3\t4.1.1\t3.39.0\tread\tPOTENTIALLY_LOSSY\ttime_slice/ggd/b_field_phi"
            ),
        );
    }

    #[test]
    fn a_non_collision_file_error_does_not_try_more_suffixes() {
        let directory = TestDirectory::new();
        let original_permissions = fs::metadata(directory.path()).unwrap().permissions();
        let mut read_only = original_permissions.clone();
        read_only.set_mode(0o500);
        fs::set_permissions(directory.path(), read_only).unwrap();
        let writer = LossFileWriter::new(FixedFacts::in_directory(&directory));

        writer.retain(entry());

        fs::set_permissions(directory.path(), original_permissions).unwrap();
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }

    #[test]
    fn deduplicates_only_an_identical_complete_rendered_line() {
        let directory = TestDirectory::new();
        let writer = LossFileWriter::new(FixedFacts::in_directory(&directory));
        let original = entry();
        writer.retain(original.clone());
        writer.retain(original);
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/other-pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Read,
            Fidelity::PotentiallyLossy,
            "time_slice/ggd/b_field_phi",
        ));
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/4",
            "4.1.1",
            "3.39.0",
            LossOperation::Read,
            Fidelity::PotentiallyLossy,
            "time_slice/ggd/b_field_phi",
        ));
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "3.39.0",
            "4.1.1",
            LossOperation::Read,
            Fidelity::PotentiallyLossy,
            "time_slice/ggd/b_field_phi",
        ));
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Write,
            Fidelity::PotentiallyLossy,
            "time_slice/ggd/b_field_phi",
        ));
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Read,
            Fidelity::Lossy,
            "time_slice/ggd/b_field_phi",
        ));
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Read,
            Fidelity::PotentiallyLossy,
            "time_slice/boundary/gap/r",
        ));

        let contents = fs::read_to_string(only_log(&directory)).unwrap();
        assert_eq!(contents.lines().count(), 12);
        assert_eq!(contents.matches("time_slice/ggd/b_field_phi\n").count(), 6);
    }

    #[test]
    fn utc_timestamp_covers_day_month_year_and_leap_century_boundaries() {
        assert_eq!(utc_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_timestamp(86_399), "1970-01-01T23:59:59Z");
        assert_eq!(utc_timestamp(86_400), "1970-01-02T00:00:00Z");
        assert_eq!(utc_timestamp(951_782_399), "2000-02-28T23:59:59Z");
        assert_eq!(utc_timestamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc_timestamp(951_868_800), "2000-03-01T00:00:00Z");
        assert_eq!(utc_timestamp(1_609_459_199), "2020-12-31T23:59:59Z");
        assert_eq!(utc_timestamp(1_609_459_200), "2021-01-01T00:00:00Z");
        assert_eq!(utc_timestamp(4_107_542_399), "2100-02-28T23:59:59Z");
        assert_eq!(utc_timestamp(4_107_542_400), "2100-03-01T00:00:00Z");
    }
}
