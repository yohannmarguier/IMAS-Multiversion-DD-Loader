#!/usr/bin/env python3
"""Check a scoped cargo-mutants run against the line-audit ownership."""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

from rust_audit_scope import Group, ScopeError, SourceRange, load_exclusions, load_groups


OUTCOME_NAMES = {
    "CaughtMutant": "Caught",
    "MissedMutant": "Missed",
    "Timeout": "Timeout",
    "Unviable": "Unviable",
}
EXCLUSION_NAMES = {"equivalent", "integration-only"}


class AuditError(ScopeError):
    """The audit's checked-in scope or cargo-mutants report is incomplete."""


@dataclass(frozen=True)
class Mutant:
    name: str
    path: str
    start_line: int
    end_line: int


@dataclass(frozen=True)
class Disposition:
    classification: str
    observable_behavior: str
    disposition: str


@dataclass
class Totals:
    caught: int = 0
    missed: int = 0
    timed_out: int = 0
    unviable: int = 0
    excluded: int = 0

    def score(self) -> float:
        denominator = self.caught + self.missed + self.timed_out
        if denominator == 0:
            raise AuditError("a group has no scoreable mutants")
        return 100 * self.caught / denominator


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--line-scope", required=True, type=Path)
    parser.add_argument("--audit", type=Path)
    parser.add_argument("--selected", type=Path)
    parser.add_argument("--mutants", type=Path)
    parser.add_argument("--outcomes", type=Path)
    parser.add_argument("--dispositions", type=Path)
    parser.add_argument("--candidate-mutants", type=Path)
    parser.add_argument("--write-selection", type=Path)
    parser.add_argument("--write-cargo-mutants-config", type=Path)
    args = parser.parse_args()
    select_options = (args.candidate_mutants, args.write_selection, args.write_cargo_mutants_config)
    report_options = (args.audit, args.selected, args.mutants, args.outcomes, args.dispositions)
    if all(select_options) and not any(report_options):
        return args
    if all(report_options) and not any(select_options):
        return args
    parser.error("supply either selection inputs or report inputs, but not both")


def load_json(path: Path, what: str) -> object:
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise AuditError(f"cannot read {what} {path}: {error}") from error


def load_line_scope_groups(path: Path) -> tuple[list[Group], list[SourceRange]]:
    """The line audit's own ownership, reused verbatim: the mutation audit
    scores the same ranges and must never re-derive them. Its exclusions come
    along because a candidate belonging to neither is a hole in the scope."""
    value = load_json(path, "line scope")
    return load_groups(value, path), load_exclusions(value)


def load_audit(path: Path) -> tuple[str, Path, float, float]:
    value = load_json(path, "mutation audit configuration")
    if not isinstance(value, dict) or value.get("schema_version") != 1:
        raise AuditError("mutation audit configuration must be a schema_version 1 JSON object")
    tool, minimums, line_scope = value.get("tool"), value.get("minimums"), value.get("line_scope")
    if not isinstance(tool, dict) or not isinstance(tool.get("cargo_mutants_version"), str):
        raise AuditError("mutation audit configuration must name cargo_mutants_version")
    if not isinstance(line_scope, str):
        raise AuditError("mutation audit configuration must name the reused line_scope")
    if not isinstance(minimums, dict):
        raise AuditError("mutation audit configuration must define minimums")
    aggregate, group = minimums.get("aggregate_percent"), minimums.get("group_percent")
    if not all(isinstance(value, (int, float)) and 0 <= value <= 100 for value in (aggregate, group)):
        raise AuditError("mutation audit minimums must be percentages from 0 through 100")
    return tool["cargo_mutants_version"], path.parent / line_scope, float(aggregate), float(group)


def load_mutants(path: Path, what: str) -> dict[str, Mutant]:
    return load_mutants_from_value(load_json(path, what), what)


def write_selection(
    candidates: Path,
    selection: Path,
    config: Path,
    groups: list[Group],
    exclusions: list[SourceRange],
) -> None:
    raw_candidates = load_json(candidates, "cargo-mutants candidates")
    if not isinstance(raw_candidates, list):
        raise AuditError("cargo-mutants candidates must be a JSON array")
    selected: list[object] = []
    for raw in raw_candidates:
        if not isinstance(raw, dict):
            raise AuditError("cargo-mutants candidates has an invalid mutant")
        try:
            candidate = Mutant(
                raw["name"], raw["file"], raw["span"]["start"]["line"], raw["span"]["end"]["line"]
            )
        except (KeyError, TypeError) as error:
            raise AuditError("cargo-mutants candidates has a mutant without identity and span") from error
        if not isinstance(candidate.name, str) or not isinstance(candidate.path, str) or not all(
            isinstance(line, int) and line >= 1 for line in (candidate.start_line, candidate.end_line)
        ) or candidate.end_line < candidate.start_line:
            raise AuditError("cargo-mutants candidates has an invalid mutant identity or span")
        try:
            owner(candidate, groups)
        except AuditError:
            # A candidate no group owns is only acceptable where the line audit
            # says so. Skipping one silently is how a range that stops short
            # shrinks the mutation scope without anyone noticing.
            if not any(
                excluded.path == candidate.path
                and excluded.contains(candidate.start_line, candidate.end_line)
                for excluded in exclusions
            ):
                raise AuditError(
                    f"candidate mutant {candidate.name} belongs to no line-audit group "
                    "and no declared exclusion; the measurement scope has a hole"
                )
            continue
        selected.append(raw)
    selected_mutants = load_mutants_from_value(selected, "selected scoped mutants")
    if any(not any(owner(mutant, groups) == group.name for mutant in selected_mutants.values()) for group in groups):
        raise AuditError("each line-audit group needs selected mutation data")
    selection.write_text(json.dumps(selected, indent=2) + "\n")
    patterns = ["^" + re.sub(r"([\\.^$*+?()[\]{}|])", r"\\\1", mutant.name) + "$" for mutant in selected_mutants.values()]
    config.write_text("examine_re = [\n" + "".join(f"  {json.dumps(pattern)},\n" for pattern in patterns) + "]\n")


def load_mutants_from_value(value: object, what: str) -> dict[str, Mutant]:
    if not isinstance(value, list):
        raise AuditError(f"{what} must be a JSON array")
    mutants: dict[str, Mutant] = {}
    for raw in value:
        try:
            name = raw["name"]
            source = raw["file"]
            start = raw["span"]["start"]["line"]
            end = raw["span"]["end"]["line"]
        except (KeyError, TypeError) as error:
            raise AuditError(f"{what} has a mutant without cargo-mutants identity and span") from error
        if not isinstance(name, str) or not isinstance(source, str) or not all(
            isinstance(line, int) and line >= 1 for line in (start, end)
        ) or end < start:
            raise AuditError(f"{what} has an invalid mutant identity or span")
        if name in mutants:
            raise AuditError(f"{what} repeats mutant {name}")
        mutants[name] = Mutant(name, source, start, end)
    if not mutants:
        raise AuditError(f"{what} contains no mutants")
    return mutants


def load_dispositions(path: Path) -> dict[str, Disposition]:
    value = load_json(path, "mutation dispositions")
    if not isinstance(value, dict) or value.get("schema_version") != 1:
        raise AuditError("mutation dispositions must be a schema_version 1 JSON object")
    raw_dispositions = value.get("dispositions")
    if not isinstance(raw_dispositions, list):
        raise AuditError("mutation dispositions must define dispositions")
    dispositions: dict[str, Disposition] = {}
    for raw in raw_dispositions:
        if not isinstance(raw, dict) or not isinstance(raw.get("mutant"), str):
            raise AuditError("each mutation disposition needs a mutant identity")
        values = (raw.get("classification"), raw.get("observable_behavior"), raw.get("disposition"))
        if values[0] not in EXCLUSION_NAMES or not all(isinstance(value, str) and value for value in values[1:]):
            raise AuditError(
                f"excluded mutant {raw['mutant']} needs an equivalent/integration-only classification, observable behavior and disposition"
            )
        if raw["mutant"] in dispositions:
            raise AuditError(f"mutation dispositions repeat mutant {raw['mutant']}")
        dispositions[raw["mutant"]] = Disposition(*values)
    return dispositions


def owner(mutant: Mutant, groups: list[Group]) -> str:
    owners = [group.name for group in groups if any(source.path == mutant.path and source.contains(mutant.start_line, mutant.end_line) for source in group.sources)]
    if len(owners) != 1:
        raise AuditError(f"selected mutant {mutant.name} is not owned by exactly one line-audit group")
    return owners[0]


def load_outcomes(path: Path, selected: dict[str, Mutant], expected_version: str) -> dict[str, str]:
    value = load_json(path, "cargo-mutants outcomes")
    if not isinstance(value, dict) or value.get("cargo_mutants_version") != expected_version:
        raise AuditError(f"cargo-mutants outcomes must report pinned version {expected_version}")
    raw_outcomes = value.get("outcomes")
    if not isinstance(raw_outcomes, list):
        raise AuditError("cargo-mutants outcomes must define outcomes")
    outcome_by_mutant: dict[str, str] = {}
    baseline = False
    for raw in raw_outcomes:
        if not isinstance(raw, dict) or not isinstance(raw.get("summary"), str):
            raise AuditError("cargo-mutants outcomes has an invalid outcome")
        scenario, summary = raw.get("scenario"), raw["summary"]
        if scenario == "Baseline":
            baseline = summary == "Success"
            continue
        if not isinstance(scenario, dict) or not isinstance(scenario.get("Mutant"), dict):
            raise AuditError("cargo-mutants outcomes has an invalid mutant scenario")
        name = scenario["Mutant"].get("name")
        if not isinstance(name, str) or name not in selected:
            raise AuditError(f"cargo-mutants outcome names unselected mutant {name}")
        if summary not in OUTCOME_NAMES:
            raise AuditError(f"cargo-mutants outcome for {name} has unrecognised summary {summary}")
        if name in outcome_by_mutant:
            raise AuditError(f"cargo-mutants outcomes repeat mutant {name}")
        outcome_by_mutant[name] = OUTCOME_NAMES[summary]
    if not baseline:
        raise AuditError("cargo-mutants baseline did not succeed")
    for name in selected:
        if name not in outcome_by_mutant:
            raise AuditError(f"selected mutant {name} has no outcome")
    if set(outcome_by_mutant) != set(selected):
        raise AuditError("cargo-mutants outcomes do not cover the selected mutants")
    for field, actual in (("total_mutants", len(selected)),):
        if value.get(field) != actual:
            raise AuditError(f"cargo-mutants outcomes {field} does not match the selected mutants")
    expected_counts = {
        "caught": sum(summary == "Caught" for summary in outcome_by_mutant.values()),
        "missed": sum(summary == "Missed" for summary in outcome_by_mutant.values()),
        "timeout": sum(summary == "Timeout" for summary in outcome_by_mutant.values()),
        "unviable": sum(summary == "Unviable" for summary in outcome_by_mutant.values()),
    }
    for field, actual in expected_counts.items():
        if value.get(field) != actual:
            raise AuditError(f"cargo-mutants outcomes {field} does not match the selected mutants")
    return outcome_by_mutant


def report_line(name: str, totals: Totals, minimum: float) -> tuple[str, bool]:
    score = totals.score()
    failures: list[str] = []
    if score < minimum:
        failures.append(f"below {minimum:.1f}%")
    if totals.timed_out:
        failures.append("timed-out")
    suffix = "PASS" if not failures else " ".join(failures)
    return (
        f"{name}: caught={totals.caught} missed={totals.missed} timed-out={totals.timed_out} "
        f"unviable={totals.unviable} excluded={totals.excluded} score={score:.1f}% {suffix}",
        not failures,
    )


def main() -> int:
    args = parse_args()
    try:
        groups, exclusions = load_line_scope_groups(args.line_scope)
        if args.candidate_mutants:
            write_selection(
                args.candidate_mutants,
                args.write_selection,
                args.write_cargo_mutants_config,
                groups,
                exclusions,
            )
            return 0
        version, expected_line_scope, aggregate_minimum, group_minimum = load_audit(args.audit)
        if args.line_scope.resolve() != expected_line_scope.resolve():
            raise AuditError("mutation audit must use the line scope named by its configuration")
        selected = load_mutants(args.selected, "selected mutants")
        actual = load_mutants(args.mutants, "cargo-mutants mutants")
        if actual != selected:
            raise AuditError("cargo-mutants mutant list does not match the selected scoped mutants")
        dispositions = load_dispositions(args.dispositions)
        unknown = set(dispositions) - set(selected)
        if unknown:
            raise AuditError(f"mutation disposition names unselected mutant {sorted(unknown)[0]}")
        owners = {name: owner(mutant, groups) for name, mutant in selected.items()}
        outcomes = load_outcomes(args.outcomes, selected, version)
        group_totals = {group.name: Totals() for group in groups}
        survivors: list[tuple[Mutant, str, str, str]] = []
        for name, summary in outcomes.items():
            totals = group_totals[owners[name]]
            if summary == "Caught":
                totals.caught += 1
            elif summary == "Unviable":
                totals.unviable += 1
            elif summary == "Timeout":
                totals.timed_out += 1
                survivors.append((selected[name], "timed-out", "the test run exceeded its timeout", "investigate or reduce the test scope"))
            else:
                disposition = dispositions.get(name)
                if disposition is None:
                    totals.missed += 1
                    survivors.append((selected[name], "missed", f"unclassified mutation: {name}", "add a discriminating test or a narrow exclusion"))
                else:
                    totals.excluded += 1
                    survivors.append((selected[name], disposition.classification, disposition.observable_behavior, f"excluded: {disposition.disposition}"))
        if any(not any(owners[name] == group.name for name in selected) for group in groups):
            raise AuditError("each line-audit group needs selected mutation data")
        for group in groups:
            totals = group_totals[group.name]
            if totals.caught + totals.missed + totals.timed_out == 0:
                raise AuditError(f"group {group.name} has no scoreable mutants")
    except ScopeError as error:
        print(f"mutation audit input error: {error}", file=sys.stderr)
        return 2

    passed = True
    aggregate = Totals()
    for group in groups:
        totals = group_totals[group.name]
        line, group_passed = report_line(group.name, totals, group_minimum)
        print(line)
        passed = passed and group_passed
        for field in ("caught", "missed", "timed_out", "unviable", "excluded"):
            setattr(aggregate, field, getattr(aggregate, field) + getattr(totals, field))
    line, aggregate_passed = report_line("aggregate", aggregate, aggregate_minimum)
    print(line)
    passed = passed and aggregate_passed
    print("survivor inventory:")
    if not survivors:
        print("none")
    for mutant, classification, behavior, disposition in survivors:
        print(
            f"{mutant.name} | observable behavior: {behavior} | classification: {classification} | disposition: {disposition}"
        )
    if not passed:
        print("mutation audit failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
