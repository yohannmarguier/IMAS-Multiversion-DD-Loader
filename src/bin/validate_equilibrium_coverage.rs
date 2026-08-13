//! Artifact-validation command for issue #51's autoconvert-equivalence gate.
//!
//! This is deliberately a command instead of a unit-test-only helper: CTest
//! runs the exact executable that maintainers can run while reviewing a
//! changed artifact.  It measures the complete leaf-path surface and then
//! checks the independent IMAS-Python rename-only floor in both directions.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use imas_mvdd_loader::conversion_map::{ConversionMap, Direction, Outcome};

const APPROVED_ARTIFACT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/3.39.0--4.1.1.xml");
const LEFT_INVENTORY: &str = include_str!("../../docs/inventory/equilibrium-3.39.0.txt");
const RIGHT_INVENTORY: &str = include_str!("../../docs/inventory/equilibrium-4.1.1.txt");
const IMAS_PYTHON_RENAMES: &str =
    include_str!("../../docs/inventory/equilibrium-3.39.0--4.1.1-imas-python-renames.tsv");

#[derive(Clone, Copy, Default)]
struct Coverage {
    supported: usize,
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
                        coverage.supported += 1
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
        "shim {direction}: supported={}, deliberate refusal={}, absent stored source={}, total={}",
        coverage.supported,
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
