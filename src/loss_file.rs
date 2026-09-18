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

static WRITER: LazyLock<LossFileWriter<ProcessFacts, ProcessEffects>> =
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

/// The three delivery operations that can fail after process facts have been
/// resolved. Keeping this boundary specific to a loss-log file lets tests
/// exercise platform-dependent I/O failures without abstracting a filesystem.
trait LossFileEffects {
    fn create_log(&self, path: &Path) -> std::io::Result<File>;
    fn write_preamble(
        &self,
        file: &mut File,
        timestamp: &str,
        process_id: u32,
        entry: &LossFileEntry,
    ) -> std::io::Result<()>;
    fn append(&self, path: &Path, line: &str) -> std::io::Result<()>;
    fn report_failure(&self, message: &str) {
        eprintln!("IMAS-MVDD: {message}");
    }
}

struct ProcessEffects;

impl LossFileEffects for ProcessEffects {
    fn create_log(&self, path: &Path) -> std::io::Result<File> {
        OpenOptions::new().append(true).create_new(true).open(path)
    }

    fn write_preamble(
        &self,
        file: &mut File,
        timestamp: &str,
        process_id: u32,
        entry: &LossFileEntry,
    ) -> std::io::Result<()> {
        write_preamble(file, timestamp, process_id, entry)
    }

    fn append(&self, path: &Path, line: &str) -> std::io::Result<()> {
        append(path, line)
    }
}

/// How far the first-loss filename search walks before giving up. A collision
/// needs another log written in the same UTC second by the same process id, so
/// the real count is zero or one; the bound exists so exhaustion is an outcome
/// this module can report and a test can reach, not an unreachable branch.
const MAX_FILENAME_COLLISIONS: u32 = 1_000;

/// One append-only writer. Production holds one process-local instance, while
/// tests construct fresh writers with their own state and temporary directory.
/// Both parameters are open because both are substituted: every recoverable
/// failure this module reports — a stopped clock, an unresolvable working
/// directory, a refused create, a failed preamble or append — is a test
/// [`LossFileFacts`] or [`LossFileEffects`] implementation away.
struct LossFileWriter<F, E> {
    facts: F,
    effects: E,
    written_keys: Mutex<HashSet<String>>,
    log_path: OnceLock<Option<PathBuf>>,
    file_failed: AtomicBool,
}

impl LossFileWriter<ProcessFacts, ProcessEffects> {
    fn process_local() -> Self {
        Self::with_effects(ProcessFacts, ProcessEffects)
    }
}

impl<F: LossFileFacts, E: LossFileEffects> LossFileWriter<F, E> {
    fn with_effects(facts: F, effects: E) -> Self {
        Self {
            facts,
            effects,
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
            && let Err(error) = self.effects.append(path, &line)
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

        for suffix in 0..=MAX_FILENAME_COLLISIONS {
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
            match self.effects.create_log(&path) {
                Ok(mut file) => {
                    if let Err(error) = self.effects.write_preamble(
                        &mut file,
                        &timestamp,
                        self.facts.process_id(),
                        entry,
                    ) {
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
        self.report_failure(format_args!(
            "could not write loss log in {}: {} names taken for this second and process",
            directory.display(),
            MAX_FILENAME_COLLISIONS + 1
        ));
        None
    }

    /// Reports one filesystem failure without changing an HLI call's outcome.
    fn report_failure(&self, message: std::fmt::Arguments<'_>) {
        if !self.file_failed.swap(true, Ordering::Relaxed) {
            self.effects.report_failure(&message.to_string());
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
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds_of_day / 3_600,
        (seconds_of_day / 60) % 60,
        seconds_of_day % 60,
    )
}

// Howard Hinnant's public-domain civil-date conversion, with day zero set to
// 1970-01-01. Keeping it integer-only makes the filename clock testable. His
// pre-epoch era adjustment is dropped and the day count is unsigned: the only
// caller divides a `u64` clock reading, so `z` can never be negative and a
// branch for that case would be one no test could reach (ADR 0011).
fn civil_from_days(days_since_epoch: u64) -> (i64, u32, u32) {
    let z = days_since_epoch as i64 + 719_468;
    let era = z / 146_097;
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
    use std::io;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        LossFileEffects, LossFileEntry, LossFileFacts, LossFileWriter, MAX_FILENAME_COLLISIONS,
        ProcessEffects, ProcessFacts, select_directory, utc_timestamp,
    };
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

    fn second_entry() -> LossFileEntry {
        LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Read,
            Fidelity::Lossy,
            "time_slice/boundary_separatrix/gap/r",
        )
    }

    fn only_log(directory: &TestDirectory) -> PathBuf {
        only_log_path(directory.path())
    }

    const PROCESS_CONTRACT_CHILD_MODE: &str = "IMAS_MVDD_LOSS_FILE_TEST_CHILD_MODE";

    fn process_seconds() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn assert_production_facts_match_the_child_process() {
        let facts = ProcessFacts;
        assert_eq!(
            facts.configured_directory(),
            std::env::var_os("IMAS_MVDD_LOSS_LOG_DIR")
        );
        assert_eq!(
            facts.current_directory().unwrap(),
            std::env::current_dir().unwrap()
        );
        let started = process_seconds();
        let seconds = facts.epoch_seconds().unwrap();
        let finished = process_seconds();
        assert!(finished >= started);
        assert!(finished - started <= 60);
        assert!((started..=finished).contains(&seconds));
        assert_eq!(facts.process_id(), std::process::id());
    }

    fn assert_production_log(directory: &std::path::Path) {
        let log = only_log_path(directory);
        let name = log.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("imas-mvdd-loss-"));
        assert!(name.ends_with(&format!("-{}.txt", std::process::id())));
        let timestamp = &name["imas-mvdd-loss-".len()..20 + "imas-mvdd-loss-".len()];
        assert!(timestamp.contains('T'));

        let contents = fs::read_to_string(&log).unwrap();
        assert!(contents.starts_with("# imas-mvdd loss log format 1\n# written "));
        assert!(contents.contains(&format!("# written {timestamp}\n")));
        assert!(contents.contains(&format!("# process {}\n", std::process::id())));
        assert!(contents.contains("uri\tids\tstored-dd\thli-dd\toperation\tfidelity\tpath\n"));
        assert!(contents.contains(
            "\tequilibrium/3\t4.1.1\t3.39.0\tread\tPOTENTIALLY_LOSSY\ttime_slice/ggd/b_field_phi\n"
        ));
    }

    fn write_with_production_facts_and_effects(directory: &std::path::Path) {
        let started = process_seconds();
        let writer = LossFileWriter::with_effects(ProcessFacts, ProcessEffects);
        writer.retain(entry());
        let finished = process_seconds();
        assert!(finished >= started);
        assert!(finished - started <= 60);
        assert_production_log(directory);

        let name = only_log_path(directory)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let timestamp = &name["imas-mvdd-loss-".len()..20 + "imas-mvdd-loss-".len()];
        assert!((started..=finished).any(|seconds| utc_timestamp(seconds) == timestamp));
    }

    fn only_log_path(directory: &std::path::Path) -> PathBuf {
        let paths = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 1);
        paths.into_iter().next().unwrap()
    }

    fn run_process_contract_child(
        mode: &str,
        working_directory: &TestDirectory,
        configured_directory: Option<&std::path::Path>,
    ) -> std::process::Output {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "loss_file::tests::production_process_contract",
                "--nocapture",
            ])
            .current_dir(working_directory.path())
            .env(PROCESS_CONTRACT_CHILD_MODE, mode);
        if let Some(directory) = configured_directory {
            command.env("IMAS_MVDD_LOSS_LOG_DIR", directory);
        } else {
            command.env_remove("IMAS_MVDD_LOSS_LOG_DIR");
        }
        command.output().unwrap()
    }

    fn assert_successful_child(output: std::process::Output) {
        assert!(
            output.status.success(),
            "child failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn production_process_contract() {
        if let Ok(mode) = std::env::var(PROCESS_CONTRACT_CHILD_MODE) {
            assert_production_facts_match_the_child_process();
            match mode.as_str() {
                "default" => write_with_production_facts_and_effects(std::path::Path::new(".")),
                "configured" => write_with_production_facts_and_effects(std::path::Path::new(
                    &std::env::var("IMAS_MVDD_LOSS_LOG_DIR").unwrap(),
                )),
                "diagnostic" => {
                    let writer = LossFileWriter::with_effects(ProcessFacts, ProcessEffects);
                    writer.retain(entry());
                    writer.retain(second_entry());
                }
                unexpected => panic!("unexpected process-contract mode {unexpected}"),
            }
            return;
        }

        let default_directory = TestDirectory::new();
        assert_successful_child(run_process_contract_child(
            "default",
            &default_directory,
            None,
        ));

        let configured_directory = TestDirectory::new();
        let different_working_directory = TestDirectory::new();
        assert_successful_child(run_process_contract_child(
            "configured",
            &different_working_directory,
            Some(configured_directory.path()),
        ));

        let invalid_destination_parent = TestDirectory::new();
        let invalid_destination = invalid_destination_parent.path().join("not-a-directory");
        fs::write(&invalid_destination, "not a directory").unwrap();
        let output = run_process_contract_child(
            "diagnostic",
            &different_working_directory,
            Some(&invalid_destination),
        );
        assert!(
            output.status.success(),
            "child failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.starts_with("IMAS-MVDD: could not write loss log in "));
        assert!(stderr.contains("directory does not exist"));
        assert_eq!(stderr.matches("IMAS-MVDD: ").count(), 1);
        assert_eq!(stderr.matches('\n').count(), 1);
    }

    struct PreambleFails;

    impl LossFileEffects for PreambleFails {
        fn create_log(&self, path: &std::path::Path) -> io::Result<std::fs::File> {
            std::fs::OpenOptions::new()
                .append(true)
                .create_new(true)
                .open(path)
        }

        fn write_preamble(
            &self,
            _file: &mut std::fs::File,
            _timestamp: &str,
            _process_id: u32,
            _entry: &LossFileEntry,
        ) -> io::Result<()> {
            Err(io::Error::other("preamble write failed"))
        }

        fn append(&self, _path: &std::path::Path, _line: &str) -> io::Result<()> {
            panic!("delivery must stop after the preamble failure")
        }
    }

    struct CreationFails {
        attempts: AtomicUsize,
        diagnostics: AtomicUsize,
    }

    impl LossFileEffects for CreationFails {
        fn create_log(&self, _path: &std::path::Path) -> io::Result<std::fs::File> {
            self.attempts.fetch_add(1, Ordering::Relaxed);
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "creation failed",
            ))
        }

        fn write_preamble(
            &self,
            _file: &mut std::fs::File,
            _timestamp: &str,
            _process_id: u32,
            _entry: &LossFileEntry,
        ) -> io::Result<()> {
            panic!("a failed creation must not initialize a file")
        }

        fn append(&self, _path: &std::path::Path, _line: &str) -> io::Result<()> {
            panic!("a failed creation must disable delivery")
        }

        fn report_failure(&self, _message: &str) {
            self.diagnostics.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Every name is taken, so the search can only exhaust its bound.
    struct AlwaysCollides {
        attempts: AtomicUsize,
        diagnostics: AtomicUsize,
    }

    impl LossFileEffects for AlwaysCollides {
        fn create_log(&self, _path: &std::path::Path) -> io::Result<std::fs::File> {
            self.attempts.fetch_add(1, Ordering::Relaxed);
            Err(io::Error::new(io::ErrorKind::AlreadyExists, "name taken"))
        }

        fn write_preamble(
            &self,
            _file: &mut std::fs::File,
            _timestamp: &str,
            _process_id: u32,
            _entry: &LossFileEntry,
        ) -> io::Result<()> {
            panic!("no file was ever created")
        }

        fn append(&self, _path: &std::path::Path, _line: &str) -> io::Result<()> {
            panic!("an exhausted search must disable delivery")
        }

        fn report_failure(&self, _message: &str) {
            self.diagnostics.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct AppendFails {
        attempts: AtomicUsize,
    }

    struct NoDelivery {
        diagnostics: AtomicUsize,
    }

    impl LossFileEffects for NoDelivery {
        fn create_log(&self, _path: &std::path::Path) -> io::Result<std::fs::File> {
            panic!("the failure must happen before file creation")
        }

        fn write_preamble(
            &self,
            _file: &mut std::fs::File,
            _timestamp: &str,
            _process_id: u32,
            _entry: &LossFileEntry,
        ) -> io::Result<()> {
            panic!("the failure must happen before preamble writing")
        }

        fn append(&self, _path: &std::path::Path, _line: &str) -> io::Result<()> {
            panic!("the failure must happen before appending")
        }

        fn report_failure(&self, _message: &str) {
            self.diagnostics.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct ClockFails {
        directory: PathBuf,
    }

    impl LossFileFacts for ClockFails {
        fn configured_directory(&self) -> Option<OsString> {
            Some(self.directory.as_os_str().to_os_string())
        }

        fn current_directory(&self) -> std::io::Result<PathBuf> {
            Ok(self.directory.clone())
        }

        fn epoch_seconds(&self) -> Result<u64, std::time::SystemTimeError> {
            std::time::UNIX_EPOCH
                .duration_since(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
                .map(|duration| duration.as_secs())
        }

        fn process_id(&self) -> u32 {
            196
        }
    }

    struct DirectoryResolutionFails;

    impl LossFileFacts for DirectoryResolutionFails {
        fn configured_directory(&self) -> Option<OsString> {
            None
        }

        fn current_directory(&self) -> std::io::Result<PathBuf> {
            Err(io::Error::other("working directory unavailable"))
        }

        fn epoch_seconds(&self) -> Result<u64, std::time::SystemTimeError> {
            Ok(951_782_400)
        }

        fn process_id(&self) -> u32 {
            196
        }
    }

    impl LossFileEffects for AppendFails {
        fn create_log(&self, path: &std::path::Path) -> io::Result<std::fs::File> {
            std::fs::OpenOptions::new()
                .append(true)
                .create_new(true)
                .open(path)
        }

        fn write_preamble(
            &self,
            file: &mut std::fs::File,
            timestamp: &str,
            process_id: u32,
            entry: &LossFileEntry,
        ) -> io::Result<()> {
            super::write_preamble(file, timestamp, process_id, entry)
        }

        fn append(&self, _path: &std::path::Path, _line: &str) -> io::Result<()> {
            self.attempts.fetch_add(1, Ordering::Relaxed);
            Err(io::Error::other("append failed"))
        }
    }

    #[test]
    fn a_preamble_failure_latches_delivery_without_discarding_distinct_keys() {
        let directory = TestDirectory::new();
        let writer =
            LossFileWriter::with_effects(FixedFacts::in_directory(&directory), PreambleFails);

        writer.retain(entry());
        writer.retain(entry());
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Read,
            Fidelity::Lossy,
            "time_slice/boundary_separatrix/gap/r",
        ));

        assert!(writer.file_failed.load(Ordering::Relaxed));
        assert!(writer.log_path.get().is_some_and(Option::is_none));
        assert_eq!(writer.written_keys.lock().unwrap().len(), 2);
        assert_eq!(fs::read_to_string(only_log(&directory)).unwrap(), "");
    }

    #[test]
    fn a_non_collision_creation_error_is_not_retried_and_latches_delivery() {
        let directory = TestDirectory::new();
        let writer = LossFileWriter::with_effects(
            FixedFacts::in_directory(&directory),
            CreationFails {
                attempts: AtomicUsize::new(0),
                diagnostics: AtomicUsize::new(0),
            },
        );

        writer.retain(entry());
        writer.retain(entry());
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Read,
            Fidelity::Lossy,
            "time_slice/boundary_separatrix/gap/r",
        ));

        assert_eq!(writer.effects.attempts.load(Ordering::Relaxed), 1);
        assert_eq!(writer.effects.diagnostics.load(Ordering::Relaxed), 1);
        assert!(writer.file_failed.load(Ordering::Relaxed));
        assert!(writer.log_path.get().is_some_and(Option::is_none));
        assert_eq!(writer.written_keys.lock().unwrap().len(), 2);
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }

    /// The suffix search is bounded, so a directory where every candidate name
    /// collides ends in one reported failure rather than an endless retry. It
    /// tries the bare name and every suffix once, and nothing else.
    #[test]
    fn an_exhausted_filename_search_reports_once_and_delivers_nothing() {
        let directory = TestDirectory::new();
        let writer = LossFileWriter::with_effects(
            FixedFacts::in_directory(&directory),
            AlwaysCollides {
                attempts: AtomicUsize::new(0),
                diagnostics: AtomicUsize::new(0),
            },
        );

        writer.retain(entry());
        writer.retain(entry());

        assert_eq!(
            writer.effects.attempts.load(Ordering::Relaxed),
            MAX_FILENAME_COLLISIONS as usize + 1
        );
        assert_eq!(writer.effects.diagnostics.load(Ordering::Relaxed), 1);
        assert!(writer.file_failed.load(Ordering::Relaxed));
        assert!(writer.log_path.get().is_some_and(Option::is_none));
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }

    #[test]
    fn an_append_failure_latches_delivery_without_discarding_distinct_keys() {
        let directory = TestDirectory::new();
        let writer = LossFileWriter::with_effects(
            FixedFacts::in_directory(&directory),
            AppendFails {
                attempts: AtomicUsize::new(0),
            },
        );

        writer.retain(entry());
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Read,
            Fidelity::Lossy,
            "time_slice/boundary_separatrix/gap/r",
        ));

        assert_eq!(writer.effects.attempts.load(Ordering::Relaxed), 1);
        assert!(writer.file_failed.load(Ordering::Relaxed));
        assert_eq!(writer.written_keys.lock().unwrap().len(), 2);
        let contents = fs::read_to_string(only_log(&directory)).unwrap();
        assert!(contents.ends_with("uri\tids\tstored-dd\thli-dd\toperation\tfidelity\tpath\n"));
        assert!(!contents.contains("time_slice/ggd/b_field_phi"));
        assert!(!contents.contains("time_slice/boundary_separatrix/gap/r"));
    }

    #[test]
    fn a_clock_failure_latches_delivery_before_touching_the_directory() {
        let directory = TestDirectory::new();
        let writer = LossFileWriter::with_effects(
            ClockFails {
                directory: directory.path().clone(),
            },
            NoDelivery {
                diagnostics: AtomicUsize::new(0),
            },
        );

        writer.retain(entry());
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Read,
            Fidelity::Lossy,
            "time_slice/boundary_separatrix/gap/r",
        ));

        assert_eq!(writer.effects.diagnostics.load(Ordering::Relaxed), 1);
        assert!(writer.file_failed.load(Ordering::Relaxed));
        assert_eq!(writer.written_keys.lock().unwrap().len(), 2);
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }

    #[test]
    fn a_directory_resolution_failure_latches_delivery_before_file_creation() {
        let writer = LossFileWriter::with_effects(
            DirectoryResolutionFails,
            NoDelivery {
                diagnostics: AtomicUsize::new(0),
            },
        );

        writer.retain(entry());
        writer.retain(LossFileEntry::new(
            "imas:hdf5?path=/tmp/pulse",
            "equilibrium/3",
            "4.1.1",
            "3.39.0",
            LossOperation::Read,
            Fidelity::Lossy,
            "time_slice/boundary_separatrix/gap/r",
        ));

        assert_eq!(writer.effects.diagnostics.load(Ordering::Relaxed), 1);
        assert!(writer.file_failed.load(Ordering::Relaxed));
        assert_eq!(writer.written_keys.lock().unwrap().len(), 2);
    }

    #[test]
    fn writes_lazily_to_the_explicit_directory_with_complete_appended_records() {
        let directory = TestDirectory::new();
        let writer =
            LossFileWriter::with_effects(FixedFacts::in_directory(&directory), ProcessEffects);
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
        let writer = LossFileWriter::with_effects(facts, ProcessEffects);

        writer.retain(entry());

        assert!(only_log(&directory).is_file());
    }

    #[test]
    fn empty_configured_directory_disables_file_delivery() {
        let directory = TestDirectory::new();
        let mut facts = FixedFacts::in_directory(&directory);
        facts.configured_directory = Some(OsString::new());
        let writer = LossFileWriter::with_effects(facts, ProcessEffects);

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
    fn filename_collisions_skip_existing_suffixes_without_touching_existing_bytes() {
        let directory = TestDirectory::new();
        let base = directory
            .path()
            .join("imas-mvdd-loss-2000-02-29T00:00:00Z-196.txt");
        fs::write(&base, "reserved by another writer\n").unwrap();
        let first_suffix = directory
            .path()
            .join("imas-mvdd-loss-2000-02-29T00:00:00Z-196-1.txt");
        fs::write(&first_suffix, "also reserved\n").unwrap();
        let writer =
            LossFileWriter::with_effects(FixedFacts::in_directory(&directory), ProcessEffects);

        writer.retain(entry());

        assert_eq!(
            fs::read_to_string(&base).unwrap(),
            "reserved by another writer\n"
        );
        assert_eq!(fs::read_to_string(first_suffix).unwrap(), "also reserved\n");
        let suffix = directory
            .path()
            .join("imas-mvdd-loss-2000-02-29T00:00:00Z-196-2.txt");
        assert_eq!(
            fs::read_to_string(suffix).unwrap().lines().last(),
            Some(
                "imas:hdf5?path=/tmp/pulse\tequilibrium/3\t4.1.1\t3.39.0\tread\tPOTENTIALLY_LOSSY\ttime_slice/ggd/b_field_phi"
            ),
        );
    }

    #[test]
    fn deduplicates_only_an_identical_complete_rendered_line() {
        let directory = TestDirectory::new();
        let writer =
            LossFileWriter::with_effects(FixedFacts::in_directory(&directory), ProcessEffects);
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
