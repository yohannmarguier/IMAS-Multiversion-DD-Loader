//! Deterministic calculations behind the artifact-validation command.
//!
//! The command owns arguments, filesystem reads, terminal output, and exit
//! status. This module accepts those inputs as contents so the coverage and
//! completeness decisions can be tested without a process or files.

use std::collections::HashSet;
use std::fmt;

use crate::conversion::conversion_map::{
    CandidatePath, ConversionMap, Direction, MatchKind, Outcome,
};

/// Contents consumed by the artifact-validation calculation.
#[derive(Clone, Copy)]
pub struct ValidationInputs<'a> {
    pub artifact: &'a str,
    pub left_inventory: &'a str,
    pub right_inventory: &'a str,
    pub rename_baseline: &'a str,
}

/// One direction's classified inventory coverage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoverageCounts {
    supported: usize,
    supported_by_rule: usize,
    deliberate_refusal: usize,
    absent_stored_source: usize,
}

impl CoverageCounts {
    #[must_use]
    pub const fn new(
        supported: usize,
        supported_by_rule: usize,
        deliberate_refusal: usize,
        absent_stored_source: usize,
    ) -> Self {
        Self {
            supported,
            supported_by_rule,
            deliberate_refusal,
            absent_stored_source,
        }
    }

    #[must_use]
    pub const fn supported(self) -> usize {
        self.supported
    }

    #[must_use]
    pub const fn supported_by_rule(self) -> usize {
        self.supported_by_rule
    }

    #[must_use]
    pub const fn identity_defaulted(self) -> usize {
        self.supported - self.supported_by_rule
    }

    #[must_use]
    pub const fn deliberate_refusal(self) -> usize {
        self.deliberate_refusal
    }

    #[must_use]
    pub const fn absent_stored_source(self) -> usize {
        self.absent_stored_source
    }

    #[must_use]
    pub const fn total(self) -> usize {
        self.supported + self.deliberate_refusal + self.absent_stored_source
    }
}

/// The rendered command report, also retained when a later validation fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationReport {
    forward: CoverageCounts,
    reverse: CoverageCounts,
    forward_rename_served: usize,
    reverse_rename_served: usize,
    rename_floor: usize,
    completeness: Option<(usize, usize)>,
}

impl ValidationReport {
    #[must_use]
    pub const fn forward_coverage(&self) -> CoverageCounts {
        self.forward
    }

    #[must_use]
    pub const fn reverse_coverage(&self) -> CoverageCounts {
        self.reverse
    }
}

impl fmt::Display for ValidationReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        report_direction(formatter, "3.39.0 -> 4.1.1", self.forward)?;
        report_direction(formatter, "4.1.1 -> 3.39.0", self.reverse)?;
        writeln!(
            formatter,
            "IMAS-Python rename-only floor 3.39.0 -> 4.1.1: {}/{} mappings served",
            self.forward_rename_served, self.rename_floor
        )?;
        writeln!(
            formatter,
            "IMAS-Python rename-only floor 4.1.1 -> 3.39.0: {}/{} mappings served",
            self.reverse_rename_served, self.rename_floor
        )?;
        if let Some((left_paths, right_paths)) = self.completeness {
            writeln!(
                formatter,
                "completeness 3.39.0 <-> 4.1.1: {left_paths} + {right_paths} inventory paths claimed, \
                 every rule selector backed, every side-only absence confirmed"
            )?;
        }
        Ok(())
    }
}

/// A rejected validation retains the report that the command must still show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationFailure {
    report: Option<ValidationReport>,
    message: String,
    kind: ValidationFailureKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValidationFailureKind {
    ArtifactLoad,
    Validation,
}

impl ValidationFailure {
    #[must_use]
    pub fn report(&self) -> Option<&ValidationReport> {
        self.report.as_ref()
    }

    #[must_use]
    pub const fn is_artifact_load_failure(&self) -> bool {
        matches!(self.kind, ValidationFailureKind::ArtifactLoad)
    }
}

impl fmt::Display for ValidationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

/// Validates supplied artifact and audit inputs without reading files or
/// printing. The command adapter renders any retained report.
pub fn validate(inputs: ValidationInputs<'_>) -> Result<ValidationReport, Box<ValidationFailure>> {
    let map = ConversionMap::load(inputs.artifact).map_err(|error| {
        Box::new(ValidationFailure {
            report: None,
            message: error.to_string(),
            kind: ValidationFailureKind::ArtifactLoad,
        })
    })?;
    let baseline = rename_baseline(inputs.rename_baseline)?;
    let left_paths = inventory_paths(inputs.left_inventory);
    let right_paths = inventory_paths(inputs.right_inventory);
    let left = inventory(&left_paths);
    let right = inventory(&right_paths);
    let forward = measure(&map, &left, &right, Direction::Forward);
    let reverse = measure(&map, &right, &left, Direction::Reverse);
    let mut report = ValidationReport {
        forward,
        reverse,
        forward_rename_served: baseline_served(&map, &right, &baseline, Direction::Forward),
        reverse_rename_served: baseline_served(&map, &left, &baseline, Direction::Reverse),
        rename_floor: baseline.len(),
        completeness: None,
    };

    if report.forward_rename_served < report.rename_floor
        || report.reverse_rename_served < report.rename_floor
    {
        return Err(Box::new(ValidationFailure {
            message: format!(
                "shim coverage falls below the IMAS-Python rename-only floor: \
                 3.39.0 -> 4.1.1 {}/{}, 4.1.1 -> 3.39.0 {}/{}",
                report.forward_rename_served,
                report.rename_floor,
                report.reverse_rename_served,
                report.rename_floor
            ),
            report: Some(report),
            kind: ValidationFailureKind::Validation,
        }));
    }

    // The floor must remain before this existing ADR 0013 proof: its two
    // near-boundary CTest fixtures assert floor failures specifically.
    if let Err(violations) = map.check_completeness(&left_paths, &right_paths) {
        return Err(Box::new(ValidationFailure {
            report: Some(report),
            kind: ValidationFailureKind::Validation,
            message: format!(
                "the artifact is not complete against its own inventories: {} violation(s): {violations:?}",
                violations.len()
            ),
        }));
    }
    report.completeness = Some((left_paths.len(), right_paths.len()));
    Ok(report)
}

fn inventory(paths: &[String]) -> HashSet<&str> {
    paths.iter().map(String::as_str).collect()
}

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
) -> CoverageCounts {
    requested
        .iter()
        .fold(CoverageCounts::default(), |mut coverage, path| {
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

fn candidate_paths<'a>(resolved_path: &'a str, candidates: &'a [CandidatePath]) -> Vec<&'a str> {
    if candidates.is_empty() {
        vec![resolved_path]
    } else {
        candidates
            .iter()
            .map(|candidate| candidate.path.as_str())
            .collect()
    }
}

fn rename_baseline(text: &str) -> Result<Vec<(&str, &str)>, Box<ValidationFailure>> {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .map(|line| {
            let mut columns = line.split('\t');
            let old = columns
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| baseline_failure(format!("baseline has no old path: `{line}`")))?;
            let new = columns
                .next()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| baseline_failure(format!("baseline has no new path: `{line}`")))?;
            if columns.next().is_some() {
                return Err(baseline_failure(format!(
                    "baseline has extra columns: `{line}`"
                )));
            }
            Ok((old, new))
        })
        .collect()
}

fn baseline_failure(message: String) -> Box<ValidationFailure> {
    Box::new(ValidationFailure {
        report: None,
        message,
        kind: ValidationFailureKind::Validation,
    })
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

fn report_direction(
    formatter: &mut fmt::Formatter<'_>,
    direction: &str,
    coverage: CoverageCounts,
) -> fmt::Result {
    writeln!(
        formatter,
        "shim {direction}: supported={}, by rule={}, by identity default={}, deliberate refusal={}, absent stored source={}, total={}",
        coverage.supported(),
        coverage.supported_by_rule(),
        coverage.identity_defaulted(),
        coverage.deliberate_refusal(),
        coverage.absent_stored_source(),
        coverage.total()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPACT_ARTIFACT: &str = r#"
        <ids-map ids="equilibrium" format-version="1">
          <side id="left" dd="3.39.0" cocos="11"/>
          <side id="right" dd="4.1.1" cocos="17"/>
          <default rel="identical"/>
          <rules>
            <rule id="rename" rel="renamed" left="renamed-left" right="renamed-right"><fidelity forward="exact" reverse="exact"/></rule>
            <rule id="merge" rel="merged" right="merged-right"><from left="merged-left-present" precedence="1"/><from left="merged-left-absent" precedence="2"/><fidelity forward="lossy" reverse="exact"/></rule>
            <rule id="retype" rel="retyped" left="retyped-left" right="retyped-right"><fidelity forward="exact" reverse="exact"/></rule>
            <rule id="retype-second" rel="retyped" left="retyped-left-second" right="retyped-right-second"><fidelity forward="exact" reverse="exact"/></rule>
            <rule id="left-only" rel="left_only" left="absent-left"><fidelity forward="exact" reverse="unmappable"/></rule>
            <rule id="right-only" rel="right_only" right="absent-right"><fidelity forward="unmappable" reverse="exact"/></rule>
          </rules>
        </ids-map>
    "#;

    const RENAMED_ARTIFACT: &str = r#"
        <ids-map ids="equilibrium" format-version="1">
          <side id="left" dd="3.39.0" cocos="11"/>
          <side id="right" dd="4.1.1" cocos="17"/>
          <rules>
            <rule id="rename" rel="renamed" left="renamed-left" right="renamed-right"><fidelity forward="exact" reverse="exact"/></rule>
          </rules>
        </ids-map>
    "#;

    fn compact_inputs<'a>(
        left_inventory: &'a str,
        right_inventory: &'a str,
        rename_baseline: &'a str,
    ) -> ValidationInputs<'a> {
        ValidationInputs {
            artifact: COMPACT_ARTIFACT,
            left_inventory,
            right_inventory,
            rename_baseline,
        }
    }

    #[test]
    fn measures_each_coverage_bucket_in_both_directions() {
        let result = validate(compact_inputs(
            " renamed-left\n identity\n identity-second\n identity-third\n merged-left-present\n retyped-left\n retyped-left-second\n absent-left\n",
            "renamed-right\nidentity\nidentity-second\nidentity-third\nmerged-right\nretyped-right\nretyped-right-second\nabsent-right\n",
            "\n  # independent rename floor\nrenamed-left\trenamed-right\n",
        ))
        .expect("the compact complete artifact must validate");

        assert_eq!(result.forward_coverage(), CoverageCounts::new(5, 2, 2, 1));
        assert_eq!(result.reverse_coverage(), CoverageCounts::new(5, 2, 2, 1));
        assert_eq!(
            result.to_string(),
            concat!(
                "shim 3.39.0 -> 4.1.1: supported=5, by rule=2, by identity default=3, deliberate refusal=2, absent stored source=1, total=8\n",
                "shim 4.1.1 -> 3.39.0: supported=5, by rule=2, by identity default=3, deliberate refusal=2, absent stored source=1, total=8\n",
                "IMAS-Python rename-only floor 3.39.0 -> 4.1.1: 1/1 mappings served\n",
                "IMAS-Python rename-only floor 4.1.1 -> 3.39.0: 1/1 mappings served\n",
                "completeness 3.39.0 <-> 4.1.1: 8 + 8 inventory paths claimed, every rule selector backed, every side-only absence confirmed\n"
            )
        );
    }

    #[test]
    fn classifies_a_candidate_plan_without_a_present_stored_candidate_as_absent() {
        let result = validate(compact_inputs(
            "renamed-left\nidentity\nidentity-second\nretyped-left\nretyped-left-second\nabsent-left\n",
            "renamed-right\nidentity\nidentity-second\nmerged-right\nretyped-right\nretyped-right-second\nabsent-right\n",
            "renamed-left\trenamed-right\n",
        ))
        .expect("candidate paths are exempt from the completeness backing check");

        assert_eq!(result.forward_coverage(), CoverageCounts::new(3, 1, 2, 1));
        assert_eq!(result.reverse_coverage(), CoverageCounts::new(3, 1, 2, 2));
        assert!(
            result
                .to_string()
                .contains("absent stored source=2, total=7")
        );
    }

    #[test]
    fn rename_floors_are_checked_independently_and_accept_equality() {
        let forward_failure = validate(compact_inputs(
            "renamed-left\n",
            "",
            "renamed-left\trenamed-right\n",
        ))
        .expect_err("the forward floor needs the expected stored path");
        assert_eq!(
            forward_failure.to_string(),
            "shim coverage falls below the IMAS-Python rename-only floor: 3.39.0 -> 4.1.1 0/1, 4.1.1 -> 3.39.0 1/1"
        );
        assert_eq!(
            forward_failure
                .report()
                .expect("a floor rejection retains the coverage report")
                .forward_coverage(),
            CoverageCounts::new(0, 0, 0, 1)
        );
        assert!(!forward_failure.is_artifact_load_failure());

        let reverse_failure = validate(compact_inputs(
            "",
            "renamed-right\n",
            "renamed-left\trenamed-right\n",
        ))
        .expect_err("the reverse floor needs the expected stored path");
        assert_eq!(
            reverse_failure.to_string(),
            "shim coverage falls below the IMAS-Python rename-only floor: 3.39.0 -> 4.1.1 1/1, 4.1.1 -> 3.39.0 0/1"
        );

        validate(ValidationInputs {
            artifact: RENAMED_ARTIFACT,
            left_inventory: "renamed-left\n",
            right_inventory: "renamed-right\n",
            rename_baseline: "renamed-left\trenamed-right\n",
        })
        .expect("one served mapping is exactly at the one-mapping floor");
    }

    #[test]
    fn malformed_rename_baselines_name_the_column_that_is_wrong() {
        let valid_artifact = r#"
            <ids-map ids="equilibrium" format-version="1">
              <side id="left" dd="3.39.0" cocos="11"/>
              <side id="right" dd="4.1.1" cocos="17"/>
            </ids-map>
        "#;
        for (baseline, expected) in [
            ("\tnew", "baseline has no old path: `\\tnew`"),
            ("old", "baseline has no new path: `old`"),
            ("old\t", "baseline has no new path: `old\\t`"),
            (
                "old\tnew\textra",
                "baseline has extra columns: `old\\tnew\\textra`",
            ),
        ] {
            let failure = validate(ValidationInputs {
                artifact: valid_artifact,
                left_inventory: "\n  \n",
                right_inventory: "\t\n",
                rename_baseline: baseline,
            })
            .expect_err("a malformed baseline must be rejected before validation");
            assert_eq!(failure.to_string(), expected.replace("\\t", "\t"));
        }
    }

    #[test]
    fn rename_floor_failure_precedes_the_existing_completeness_proof() {
        let failure = validate(ValidationInputs {
            artifact: r#"
                <ids-map ids="equilibrium" format-version="1">
                  <side id="left" dd="3.39.0" cocos="11"/>
                  <side id="right" dd="4.1.1" cocos="17"/>
                  <default rel="identical"/>
                  <rules><rule id="rename" rel="renamed" left="old" right="new"><fidelity forward="exact" reverse="exact"/></rule></rules>
                </ids-map>
            "#,
            left_inventory: "unclaimed-left\n",
            right_inventory: "",
            rename_baseline: "old\tnew\n",
        })
        .expect_err("the floor failure is the gate contract for this input");
        assert_eq!(
            failure.to_string(),
            "shim coverage falls below the IMAS-Python rename-only floor: 3.39.0 -> 4.1.1 0/1, 4.1.1 -> 3.39.0 0/1"
        );
    }

    #[test]
    fn incomplete_artifacts_still_fail_the_adr_0013_completeness_proof() {
        let failure = validate(ValidationInputs {
            artifact: r#"
                <ids-map ids="equilibrium" format-version="1">
                  <side id="left" dd="3.39.0" cocos="11"/>
                  <side id="right" dd="4.1.1" cocos="17"/>
                  <default rel="identical"/>
                </ids-map>
            "#,
            left_inventory: "only-left\n",
            right_inventory: "",
            rename_baseline: "",
        })
        .expect_err("the identity default cannot claim a missing counterpart");
        assert!(failure
            .to_string()
            .contains("the artifact is not complete against its own inventories: 1 violation(s): [DefaultAssumesMissingCounterpart"));
    }

    #[test]
    fn unclaimed_paths_are_absent_before_the_completeness_proof_rejects_them() {
        let failure = validate(ValidationInputs {
            artifact: r#"
                <ids-map ids="equilibrium" format-version="1">
                  <side id="left" dd="3.39.0" cocos="11"/>
                  <side id="right" dd="4.1.1" cocos="17"/>
                </ids-map>
            "#,
            left_inventory: "unclaimed-left\n",
            right_inventory: "",
            rename_baseline: "",
        })
        .expect_err("an unclaimed inventory path makes the artifact incomplete");
        assert_eq!(
            failure
                .report()
                .expect("completeness runs after the report is measured")
                .forward_coverage(),
            CoverageCounts::new(0, 0, 0, 1)
        );
    }

    #[test]
    fn malformed_artifacts_need_the_command_adapter_to_name_their_path() {
        let failure = validate(ValidationInputs {
            artifact: "<not-xml",
            left_inventory: "",
            right_inventory: "",
            rename_baseline: "",
        })
        .expect_err("the artifact must parse before it can be measured");
        assert!(failure.report().is_none());
        assert!(failure.is_artifact_load_failure());
    }
}
