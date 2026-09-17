//! Artifact-validation command for issue #50's completeness proof and issue
//! #51's autoconvert-equivalence gate.
//!
//! This binary deliberately owns only command-line arguments, filesystem
//! reads, terminal output, and exit status. The testable coverage calculation
//! and its ADR 0013 completeness check live in `artifact_validation`.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use imas_mvdd_loader::artifact_validation::{ValidationInputs, validate};

const APPROVED_ARTIFACT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/3.39.0--4.1.1.xml");
const LEFT_INVENTORY: &str = include_str!("../../docs/inventory/equilibrium-3.39.0.txt");
const RIGHT_INVENTORY: &str = include_str!("../../docs/inventory/equilibrium-4.1.1.txt");
const IMAS_PYTHON_RENAMES: &str =
    include_str!("../../docs/inventory/equilibrium-3.39.0--4.1.1-imas-python-renames.tsv");

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("artifact coverage validation failed: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let artifact = artifact_argument()?;
    let xml = fs::read_to_string(&artifact)
        .map_err(|error| format!("cannot read {}: {error}", artifact.display()))?;

    match validate(ValidationInputs {
        artifact: &xml,
        left_inventory: LEFT_INVENTORY,
        right_inventory: RIGHT_INVENTORY,
        rename_baseline: IMAS_PYTHON_RENAMES,
    }) {
        Ok(report) => {
            print!("{report}");
            Ok(())
        }
        Err(failure) => {
            if let Some(report) = failure.report() {
                print!("{report}");
            }
            if failure.is_artifact_load_failure() {
                Err(format!("cannot load {}: {failure}", artifact.display()))
            } else {
                Err(failure.to_string())
            }
        }
    }
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
