#!/usr/bin/env python3
"""Check the checked-in Rust decision-coverage scope against an LCOV report."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path

from rust_audit_scope import (
    Group,
    ScopeError,
    SourceRange,
    load_exclusions,
    load_groups,
    production_boundary,
    unpartitioned_production,
)


@dataclass(frozen=True)
class CoverageScope:
    aggregate_minimum: float
    group_minimum: float
    cargo_llvm_cov_version: str
    groups: tuple[Group, ...]
    exclusions: tuple[SourceRange, ...]
    exclusion_reasons: dict[SourceRange, str]


@dataclass(frozen=True)
class ReportOptions:
    configuration: str
    revision: str
    rustc_version: str
    cargo_llvm_cov_version: str
    scope_path: Path
    lcov_path: Path
    output_path: Path
    append: bool


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", type=Path)
    parser.add_argument("--lcov", type=Path)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path.cwd(),
        help="repository root used to normalize absolute LCOV paths",
    )
    parser.add_argument("--configuration")
    parser.add_argument("--revision")
    parser.add_argument("--rustc-version")
    parser.add_argument("--cargo-llvm-cov-version")
    parser.add_argument("--report-output", type=Path)
    parser.add_argument("--append-report", action="store_true")
    parser.add_argument(
        "--initialize-report",
        action="store_true",
        help="replace the report output with an incomplete audit placeholder",
    )
    return parser.parse_args()


def load_scope(path: Path) -> CoverageScope:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise ScopeError(f"cannot read scope {path}: {error}") from error
    if not isinstance(value, dict) or value.get("schema_version") != 1:
        raise ScopeError("scope must be a schema_version 1 JSON object")
    tool = value.get("tool")
    if not isinstance(tool, dict) or not all(
        isinstance(tool.get(key), str)
        for key in ("cargo_llvm_cov_version", "minimum_rust_version")
    ):
        raise ScopeError("scope must name cargo_llvm_cov_version and minimum_rust_version")
    minimums = value.get("minimums")
    if not isinstance(minimums, dict):
        raise ScopeError("scope must define minimums")
    aggregate = minimums.get("aggregate_percent")
    group = minimums.get("group_percent")
    if not all(
        isinstance(value, (int, float)) and 0 <= value <= 100
        for value in (aggregate, group)
    ):
        raise ScopeError("coverage minimums must be percentages from 0 through 100")

    groups = load_groups(value, path)
    exclusions = load_exclusions(value)
    for source in (source for group in groups for source in group.sources):
        for excluded in exclusions:
            if source.overlaps(excluded):
                raise ScopeError(
                    f"source {source.display()} is both included and excluded "
                    f"by {excluded.display()}"
                )
    exclusion_reasons = {
        SourceRange(
            exclusion["path"],
            exclusion.get("start_line"),
            exclusion.get("end_line"),
        ): exclusion["reason"]
        for exclusion in value.get("exclusions", [])
    }
    return CoverageScope(
        float(aggregate),
        float(group),
        tool["cargo_llvm_cov_version"],
        tuple(groups),
        tuple(exclusions),
        exclusion_reasons,
    )


def normalize_path(raw: str, root: Path) -> str:
    path = Path(raw)
    if path.is_absolute():
        try:
            path = path.relative_to(root)
        except ValueError:
            return path.as_posix()
    return path.as_posix()


def load_lcov(path: Path, root: Path) -> dict[str, dict[int, int]]:
    files: dict[str, dict[int, int]] = {}
    current: dict[int, int] | None = None
    current_path: str | None = None
    try:
        lines = path.read_text().splitlines()
    except OSError as error:
        raise ScopeError(f"cannot read LCOV report {path}: {error}") from error
    for raw_line in lines:
        if not raw_line:
            continue
        if raw_line.startswith("SF:"):
            if current is not None:
                raise ScopeError(f"LCOV source {current_path} has no end_of_record")
            normalized = normalize_path(raw_line[3:], root)
            if not normalized:
                raise ScopeError("LCOV source file is empty")
            current = files.setdefault(normalized, {})
            current_path = normalized
        elif raw_line.startswith("DA:"):
            if current is None:
                raise ScopeError("LCOV data line appears before a source file")
            try:
                line_text, count_text, *_ = raw_line[3:].split(",")
                line = int(line_text)
                count = int(count_text)
            except ValueError as error:
                raise ScopeError(f"invalid LCOV data line: {raw_line}") from error
            if line < 1 or count < 0:
                raise ScopeError(f"invalid LCOV data line: {raw_line}")
            current[line] = current.get(line, 0) + count
        elif raw_line == "end_of_record":
            if current is None:
                raise ScopeError("LCOV end_of_record appears before a source file")
            current = None
            current_path = None
        elif raw_line.startswith(
            ("TN:", "FN:", "FNDA:", "FNF:", "FNH:", "BRDA:", "BRF:", "BRH:", "LF:", "LH:")
        ):
            continue
        else:
            raise ScopeError(f"unrecognised LCOV record: {raw_line}")
    if current is not None:
        raise ScopeError(f"LCOV source {current_path} has no end_of_record")
    if not files:
        raise ScopeError("LCOV report contains no source files")
    return files


def measure(
    groups: Sequence[Group], lcov: dict[str, dict[int, int]]
) -> tuple[list[tuple[str, int, int]], list[str]]:
    measured: list[tuple[str, int, int]] = []
    errors: list[str] = []
    for group in groups:
        covered = 0
        total = 0
        for source in group.sources:
            lines = lcov.get(source.path)
            if lines is None:
                errors.append(f"required source {source.display()} has no measurement data")
                continue
            selected = [count for line, count in lines.items() if source.contains(line)]
            if not selected:
                errors.append(f"required source {source.display()} has empty measurement data")
                continue
            total += len(selected)
            covered += sum(count > 0 for count in selected)
        measured.append((group.name, covered, total))
    return measured, errors


def percent(covered: int, total: int) -> float:
    return 100 * covered / total


def report_options(args: argparse.Namespace) -> ReportOptions | None:
    report_values = (
        args.configuration,
        args.revision,
        args.rustc_version,
        args.cargo_llvm_cov_version,
        args.report_output,
    )
    if any(value is not None for value in report_values) and not all(
        value is not None for value in report_values
    ):
        raise ScopeError(
            "configuration, revision, rustc version, cargo-llvm-cov version and "
            "report output must be supplied together"
        )
    if args.append_report and args.report_output is None:
        raise ScopeError("--append-report requires --report-output")
    if args.report_output is None:
        return None
    return ReportOptions(
        args.configuration,
        args.revision,
        args.rustc_version,
        args.cargo_llvm_cov_version,
        args.scope,
        args.lcov,
        args.report_output,
        args.append_report,
    )


def validate_report_metadata(
    options: ReportOptions, scope: CoverageScope, root: Path
) -> None:
    try:
        head = subprocess.run(
            ["git", "-C", str(root), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as error:
        raise ScopeError(f"cannot resolve HEAD beneath {root}: {error}") from error
    if options.revision != head:
        raise ScopeError(f"recorded revision {options.revision} does not match HEAD {head}")

    expected_tool_output = f"cargo-llvm-cov {scope.cargo_llvm_cov_version}"
    if options.cargo_llvm_cov_version != expected_tool_output:
        raise ScopeError(
            f"recorded coverage tool {options.cargo_llvm_cov_version} does not match "
            f"scope requirement {expected_tool_output}"
        )


def excluded_production_lines(
    exclusions: Sequence[SourceRange], lcov: dict[str, dict[int, int]], root: Path
) -> dict[tuple[str, int], int]:
    selected: dict[tuple[str, int], int] = {}
    for exclusion in exclusions:
        file_path = root / exclusion.path
        boundary = None
        if file_path.is_file():
            boundary = production_boundary(file_path)
        for line, count in lcov.get(exclusion.path, {}).items():
            if exclusion.contains(line) and (boundary is None or line <= boundary):
                selected[(exclusion.path, line)] = count
    return selected


def inline_test_lines(
    lcov: dict[str, dict[int, int]], root: Path
) -> dict[tuple[str, int], int]:
    selected: dict[tuple[str, int], int] = {}
    for path, lines in lcov.items():
        file_path = root / path
        if not file_path.is_file():
            continue
        boundary = production_boundary(file_path)
        source_line_count = len(file_path.read_text().splitlines())
        if boundary < source_line_count:
            selected.update(
                ((path, line), count) for line, count in lines.items() if line > boundary
            )
    return selected


def scoped_inline_test_ranges(groups: Sequence[Group], root: Path) -> list[str]:
    errors: list[str] = []
    for group in groups:
        for source in group.sources:
            file_path = root / source.path
            if not file_path.is_file():
                continue
            boundary = production_boundary(file_path)
            source_line_count = len(file_path.read_text().splitlines())
            if boundary == source_line_count:
                continue
            overlap_start = max(source.start_line or 1, boundary + 1)
            overlap_end = min(source.end_line or source_line_count, source_line_count)
            if overlap_start <= overlap_end:
                errors.append(
                    f"{source.path}:{overlap_start}-{overlap_end} belongs to an inline "
                    "test module, not the production scope"
                )
    return errors


def relative_display(path: Path, root: Path) -> str:
    try:
        return path.resolve().relative_to(root).as_posix()
    except ValueError:
        return path.resolve().as_posix()


def markdown_percent(covered: int, total: int) -> str:
    return f"{percent(covered, total):.1f}%" if total else "n/a"


def report_title() -> list[str]:
    return ["# Rust line-coverage audit", ""]


def report_header(options: ReportOptions) -> list[str]:
    return report_title() + [
        f"Revision: `{options.revision}`",
        f"Rust toolchain: `{options.rustc_version}`",
        f"Coverage tool: `{options.cargo_llvm_cov_version}`",
        "",
    ]


def write_lines(path: Path, append: bool, lines: list[str]) -> None:
    mode = "a" if append else "w"
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open(mode) as report:
            report.write("\n".join(lines))
    except OSError as error:
        raise ScopeError(f"cannot write coverage report {path}: {error}") from error


def write_report_lines(options: ReportOptions, lines: list[str]) -> None:
    write_lines(options.output_path, options.append, lines)


def write_initial_report(path: Path) -> None:
    write_lines(
        path,
        False,
        report_title()
        + [
            "Overall audit result: **INCOMPLETE**",
            "",
            "Coverage measurement stopped before both configuration reports were written.",
            "",
        ],
    )


def report_configuration_header(options: ReportOptions, root: Path) -> list[str]:
    return [
        f"## {options.configuration}",
        "",
        f"Scope definition: `{relative_display(options.scope_path, root)}`  ",
        f"LCOV input: `{relative_display(options.lcov_path, root)}`",
        "",
    ]


def write_input_error_report(
    options: ReportOptions, errors: list[str], root: Path
) -> None:
    lines = [] if options.append else report_header(options)
    lines.extend(
        report_configuration_header(options, root)
        + [
            "Overall audit result: **INPUT ERROR**",
            "",
            *[f"- {error}" for error in errors],
            "",
            "",
        ]
    )
    write_report_lines(options, lines)


def write_report(
    options: ReportOptions,
    scope: CoverageScope,
    measurements: list[tuple[str, int, int]],
    lcov: dict[str, dict[int, int]],
    failures: list[str],
    root: Path,
) -> None:
    full_lines = [count for lines in lcov.values() for count in lines.values()]
    full_covered = sum(count > 0 for count in full_lines)
    excluded_lines = excluded_production_lines(scope.exclusions, lcov, root)
    excluded_covered = sum(count > 0 for count in excluded_lines.values())
    test_lines = inline_test_lines(lcov, root)
    test_covered = sum(count > 0 for count in test_lines.values())
    scoped_covered = sum(covered for _, covered, _ in measurements)
    scoped_total = sum(total for _, _, total in measurements)
    aggregate_passes = percent(scoped_covered, scoped_total) >= scope.aggregate_minimum
    scoped_keys = {
        (source.path, line)
        for group in scope.groups
        for source in group.sources
        for line in lcov.get(source.path, {})
        if source.contains(line)
    }
    test_scope_overlap = len(scoped_keys & test_lines.keys())
    test_meaning = (
        "outside the scoped gate"
        if test_scope_overlap == 0
        else f"{test_scope_overlap} line(s) also named by the scope"
    )

    lines = [] if options.append else report_header(options)
    lines.extend(
        report_configuration_header(options, root)
        + [
            "The pass condition is the scoped aggregate and every scoped group; "
            "the full unfiltered percentage is diagnostic.",
            "",
            "| Accounting | Covered/total lines | Result | Meaning |",
            "| --- | ---: | ---: | --- |",
            f"| Scoped aggregate | {scoped_covered}/{scoped_total} | "
            f"{percent(scoped_covered, scoped_total):.1f}% | "
            f"{'PASS' if aggregate_passes else 'FAIL'} "
            f"(minimum {scope.aggregate_minimum:.1f}%) |",
            f"| Full unfiltered report | {full_covered}/{len(full_lines)} | "
            f"{markdown_percent(full_covered, len(full_lines))} | diagnostic only |",
            f"| Excluded production | {excluded_covered}/{len(excluded_lines)} | "
            f"{markdown_percent(excluded_covered, len(excluded_lines))} | "
            "outside the scoped gate |",
            f"| Inline test bodies in full metric | {test_covered}/{len(test_lines)} | "
            f"{'present' if test_lines else 'absent'} | {test_meaning} |",
            "",
            "### Scoped groups",
            "",
            "| Group | Covered/total lines | Coverage | Verdict | Measured ranges |",
            "| --- | ---: | ---: | --- | --- |",
        ]
    )
    groups_by_name = {group.name: group for group in scope.groups}
    for name, covered, total in measurements:
        rate = percent(covered, total)
        ranges = "<br>".join(
            source.display() for source in groups_by_name[name].sources
        )
        verdict = (
            "PASS"
            if rate >= scope.group_minimum
            else f"FAIL (minimum {scope.group_minimum:.1f}%)"
        )
        lines.append(
            f"| {name} | {covered}/{total} | {rate:.1f}% | {verdict} | {ranges} |"
        )
    lines.extend(["", "### Excluded production ranges", ""])
    for exclusion in scope.exclusions:
        reason = scope.exclusion_reasons[exclusion].replace("|", "\\|")
        lines.append(f"- {exclusion.display()} — {reason}")
    lines.extend(
        [
            "",
            f"Overall scoped verdict: **{'FAIL' if failures else 'PASS'}**",
            "",
            "",
        ]
    )

    write_report_lines(options, lines)


def main() -> int:
    args = parse_args()
    if args.initialize_report:
        if args.report_output is None:
            print(
                "coverage audit input error: --initialize-report requires --report-output",
                file=sys.stderr,
            )
            return 2
        try:
            write_initial_report(args.report_output)
        except ScopeError as error:
            print(f"coverage audit input error: {error}", file=sys.stderr)
            return 2
        return 0
    if args.scope is None or args.lcov is None:
        print(
            "coverage audit input error: --scope and --lcov are required unless "
            "--initialize-report is used",
            file=sys.stderr,
        )
        return 2
    root = args.root.resolve()
    try:
        options = report_options(args)
    except ScopeError as error:
        print(f"coverage audit input error: {error}", file=sys.stderr)
        return 2

    try:
        scope = load_scope(args.scope)
        if options is not None:
            validate_report_metadata(options, scope, root)
        lcov = load_lcov(args.lcov, root)
        measurements, errors = measure(scope.groups, lcov)
    except ScopeError as error:
        print(f"coverage audit input error: {error}", file=sys.stderr)
        if options is not None:
            try:
                write_input_error_report(options, [str(error)], root)
            except ScopeError as report_error:
                print(f"coverage audit input error: {report_error}", file=sys.stderr)
        return 2

    errors.extend(unpartitioned_production(scope.groups, scope.exclusions, root))
    errors.extend(scoped_inline_test_ranges(scope.groups, root))

    mapped_paths = {source.path for group in scope.groups for source in group.sources}
    mapped_paths |= {excluded.path for excluded in scope.exclusions}
    for path in lcov:
        if path.startswith("src/") and path not in mapped_paths:
            errors.append(f"source {path} is unmapped; assign it to a group or an exclusion")

    if errors:
        for error in errors:
            print(f"coverage audit input error: {error}", file=sys.stderr)
        if options is not None:
            try:
                write_input_error_report(options, errors, root)
            except ScopeError as report_error:
                print(f"coverage audit input error: {report_error}", file=sys.stderr)
        return 2

    failures: list[str] = []
    for name, covered, total in measurements:
        rate = percent(covered, total)
        suffix = (
            "PASS" if rate >= scope.group_minimum else f"below {scope.group_minimum:.1f}%"
        )
        print(f"{name}: {covered}/{total} lines ({rate:.1f}%) {suffix}")
        if rate < scope.group_minimum:
            failures.append(
                f"{name}: {covered}/{total} lines ({rate:.1f}%) "
                f"below {scope.group_minimum:.1f}%"
            )
    covered = sum(covered for _, covered, _ in measurements)
    total = sum(total for _, _, total in measurements)
    rate = percent(covered, total)
    suffix = (
        "PASS"
        if rate >= scope.aggregate_minimum
        else f"below {scope.aggregate_minimum:.1f}%"
    )
    print(f"aggregate: {covered}/{total} lines ({rate:.1f}%) {suffix}")
    if rate < scope.aggregate_minimum:
        failures.append(
            f"aggregate: {covered}/{total} lines ({rate:.1f}%) "
            f"below {scope.aggregate_minimum:.1f}%"
        )
    if options is not None:
        try:
            write_report(options, scope, measurements, lcov, failures, root)
        except ScopeError as error:
            print(f"coverage audit input error: {error}", file=sys.stderr)
            return 2
    if failures:
        print("coverage audit failed: " + "; ".join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
