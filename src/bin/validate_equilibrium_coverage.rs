//! Artifact-validation command for issue #50's completeness proof and issue
//! #51's autoconvert-equivalence gate.
//!
//! This is deliberately a command instead of a unit-test-only helper: CTest
//! runs the exact executable that maintainers can run while reviewing a
//! changed artifact.  It measures the complete leaf-path surface, checks the
//! independent IMAS-Python rename-only floor in both directions, and then
//! runs `ConversionMap::check_completeness` over the same two inventories.
//!
//! Both issues declare the same test seam — "artifact-validation command
//! invoked through the project test runner" — but the completeness half used
//! to run only as a `#[cfg(test)]` unit test, so this command could report a
//! healthy coverage summary for an artifact whose claims did not hold. It now
//! fails on either.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use imas_mvdd_loader::conversion_map::{ConversionMap, Direction, MatchKind, Outcome};

const APPROVED_ARTIFACT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/3.39.0--4.1.1.xml");
const LEFT_INVENTORY: &str = include_str!("../../docs/inventory/equilibrium-3.39.0.txt");
const RIGHT_INVENTORY: &str = include_str!("../../docs/inventory/equilibrium-4.1.1.txt");
const IMAS_PYTHON_RENAMES: &str =
    include_str!("../../docs/inventory/equilibrium-3.39.0--4.1.1-imas-python-renames.tsv");

#[derive(Clone, Copy, Default)]
struct Coverage {
    supported: usize,
    /// How much of `supported` an explicit rule claims, as against the
    /// document-level identity default. Reported because the two are not the
    /// same evidence: a rule states the conversion, while the default only
    /// assumes the spelling did not change — an assumption the completeness
    /// proof verifies against the other side's inventory rather than trusts
    /// (`ConversionMap::check_completeness`).
    supported_by_rule: usize,
    deliberate_refusal: usize,
    absent_stored_source: usize,
}

impl Coverage {
    fn total(self) -> usize {
        self.supported + self.deliberate_refusal + self.absent_stored_source
    }
}

fn main() -> ExitCode {
    match validate() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("artifact coverage validation failed: {message}");
            ExitCode::FAILURE
        }
    }
}

fn validate() -> Result<(), String> {
    let artifact = artifact_argument()?;
    let xml = fs::read_to_string(&artifact)
        .map_err(|error| format!("cannot read {}: {error}", artifact.display()))?;
    let map = ConversionMap::load(&xml)
        .map_err(|error| format!("cannot load {}: {error}", artifact.display()))?;

    let left = inventory(LEFT_INVENTORY);
    let right = inventory(RIGHT_INVENTORY);
    let forward = measure(&map, &left, &right, Direction::Forward);
    let reverse = measure(&map, &right, &left, Direction::Reverse);
    report("3.39.0 -> 4.1.1", forward);
    report("4.1.1 -> 3.39.0", reverse);

    let baseline = rename_baseline()?;
    let forward_served = baseline_served(&map, &right, &baseline, Direction::Forward);
    let reverse_served = baseline_served(&map, &left, &baseline, Direction::Reverse);
    println!(
        "IMAS-Python rename-only floor 3.39.0 -> 4.1.1: {forward_served}/{} mappings served",
        baseline.len()
    );
    println!(
        "IMAS-Python rename-only floor 4.1.1 -> 3.39.0: {reverse_served}/{} mappings served",
        baseline.len()
    );

    if forward_served < baseline.len() || reverse_served < baseline.len() {
        return Err(format!(
            "shim coverage falls below the IMAS-Python rename-only floor: \
             3.39.0 -> 4.1.1 {forward_served}/{}, 4.1.1 -> 3.39.0 {reverse_served}/{}",
            baseline.len(),
            baseline.len()
        ));
    }

    // Deliberately last. The floor error above is the contract two of this
    // gate's own fixtures assert on -- the all-rules-removed artifact and the
    // one-rename-short artifact are both built to fall below the floor, and
    // both would instead trip a completeness violation if this ran first,
    // making the gate prove something other than what it says it proves.
    // Coverage magnitude and claim integrity are separate assertions; this is
    // the second one, not a replacement for the first.
    let left_paths = inventory_paths(LEFT_INVENTORY);
    let right_paths = inventory_paths(RIGHT_INVENTORY);
    match map.check_completeness(&left_paths, &right_paths) {
        Ok(()) => println!(
            "completeness 3.39.0 <-> 4.1.1: {} + {} inventory paths claimed, \
             every rule selector backed, every side-only absence confirmed",
            left_paths.len(),
            right_paths.len()
        ),
        Err(violations) => {
            return Err(format!(
                "the artifact is not complete against its own inventories: \
                 {} violation(s): {violations:?}",
                violations.len()
            ));
        }
    }
    Ok(())
}

fn artifact_argument() -> Result<PathBuf, String> {
    let mut arguments = env::args_os().skip(1);
    match arguments.next() {
        None => Ok(PathBuf::from(APPROVED_ARTIFACT)),
        Some(flag) if flag == "--artifact" => arguments
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| "--artifact requires a path".to_string()),
        Some(flag) => Err(format!(
            "unrecognised argument `{}`",
            flag.to_string_lossy()
        )),
    }
}

fn inventory(text: &str) -> HashSet<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

/// The same inventory lines as owned `String`s, which is what
/// `ConversionMap::check_completeness` takes. The measurement wants set
/// membership and the proof wants a slice it can report a path out of, so the
/// one file is read into both shapes rather than one being derived from the
/// other by a collect that would drop the line order the proof reports in.
fn inventory_paths(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn measure(
    map: &ConversionMap,
    requested: &HashSet<&str>,
    stored: &HashSet<&str>,
    direction: Direction,
) -> Coverage {
    requested
        .iter()
        .fold(Coverage::default(), |mut coverage, path| {
            match map.resolve(path, direction) {
                Some(explanation) => match explanation.outcome {
                    Outcome::Path {
                        resolved_path,
                        candidates,
                        ..
                    } if candidate_paths(&resolved_path, &candidates)
                        .iter()
                        .any(|candidate| stored.contains(*candidate)) =>
                    {
                        coverage.supported += 1;
                        if explanation.match_kind == MatchKind::Explicit {
                            coverage.supported_by_rule += 1;
                        }
                    }
                    Outcome::Path { .. } | Outcome::NoSource => coverage.absent_stored_source += 1,
                    Outcome::Refusal(_) => coverage.deliberate_refusal += 1,
                },
                None => coverage.absent_stored_source += 1,
            }
            coverage
        })
}

fn candidate_paths<'a>(
    resolved_path: &'a str,
    candidates: &'a [imas_mvdd_loader::conversion_map::CandidatePath],
) -> Vec<&'a str> {
    if candidates.is_empty() {
        vec![resolved_path]
    } else {
        candidates
            .iter()
            .map(|candidate| candidate.path.as_str())
            .collect()
    }
}

fn report(direction: &str, coverage: Coverage) {
    println!(
        "shim {direction}: supported={}, by rule={}, by identity default={}, \
         deliberate refusal={}, absent stored source={}, total={}",
        coverage.supported,
        coverage.supported_by_rule,
        coverage.supported - coverage.supported_by_rule,
        coverage.deliberate_refusal,
        coverage.absent_stored_source,
        coverage.total()
    );
}

fn rename_baseline() -> Result<Vec<(&'static str, &'static str)>, String> {
    IMAS_PYTHON_RENAMES
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut columns = line.split('\t');
            let old = columns
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("baseline has no old path: `{line}`"))?;
            let new = columns
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("baseline has no new path: `{line}`"))?;
            if columns.next().is_some() {
                return Err(format!("baseline has extra columns: `{line}`"));
            }
            Ok((old, new))
        })
        .collect()
}

fn baseline_served(
    map: &ConversionMap,
    stored: &HashSet<&str>,
    baseline: &[(&str, &str)],
    direction: Direction,
) -> usize {
    baseline
        .iter()
        .filter(|(old, new)| {
            let (requested, expected) = match direction {
                Direction::Forward => (*old, *new),
                Direction::Reverse => (*new, *old),
            };
            map.resolve(requested, direction)
                .is_some_and(|explanation| match explanation.outcome {
                    Outcome::Path {
                        resolved_path,
                        candidates,
                        ..
                    } => candidate_paths(&resolved_path, &candidates)
                        .iter()
                        .any(|candidate| *candidate == expected && stored.contains(expected)),
                    Outcome::NoSource | Outcome::Refusal(_) => false,
                })
        })
        .count()
}
