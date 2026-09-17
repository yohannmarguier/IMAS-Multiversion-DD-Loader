#!/usr/bin/env python3
"""Check the checked-in Rust decision-coverage scope against an LCOV report."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from rust_audit_scope import Group, ScopeError, SourceRange, load_groups, parse_source


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", type=Path, required=True)
    parser.add_argument("--lcov", type=Path, required=True)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path.cwd(),
        help="repository root used to normalize absolute LCOV paths",
    )
    return parser.parse_args()


def load_scope(path: Path) -> tuple[float, float, list[Group], list[SourceRange]]:
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
    if not all(isinstance(value, (int, float)) and 0 <= value <= 100 for value in (aggregate, group)):
        raise ScopeError("coverage minimums must be percentages from 0 through 100")

    groups = load_groups(value, path)
    raw_exclusions = value.get("exclusions", [])
    if not isinstance(raw_exclusions, list):
        raise ScopeError("scope exclusions must be a list")
    exclusions: list[SourceRange] = []
    for exclusion in raw_exclusions:
        if not isinstance(exclusion, dict) or not isinstance(exclusion.get("reason"), str):
            raise ScopeError("each exclusion needs a path and a reason")
        exclusions.append(parse_source(exclusion, "exclusions"))
    for source in (source for group in groups for source in group.sources):
        for excluded in exclusions:
            if source.overlaps(excluded):
                raise ScopeError(
                    f"source {source.display()} is both included and excluded "
                    f"by {excluded.display()}"
                )
    return float(aggregate), float(group), groups, exclusions


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
        elif raw_line.startswith(("TN:", "FN:", "FNDA:", "FNF:", "FNH:", "BRDA:", "BRF:", "BRH:", "LF:", "LH:")):
            continue
        else:
            raise ScopeError(f"unrecognised LCOV record: {raw_line}")
    if current is not None:
        raise ScopeError(f"LCOV source {current_path} has no end_of_record")
    if not files:
        raise ScopeError("LCOV report contains no source files")
    return files


def measure(groups: list[Group], lcov: dict[str, dict[int, int]]) -> tuple[list[tuple[str, int, int]], list[str]]:
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


def main() -> int:
    args = parse_args()
    try:
        aggregate_minimum, group_minimum, groups, exclusions = load_scope(args.scope)
        lcov = load_lcov(args.lcov, args.root.resolve())
        measurements, errors = measure(groups, lcov)
    except ScopeError as error:
        print(f"coverage audit input error: {error}", file=sys.stderr)
        return 2

    mapped_paths = {source.path for group in groups for source in group.sources}
    mapped_paths |= {excluded.path for excluded in exclusions}
    for path in lcov:
        if path.startswith("src/") and path not in mapped_paths:
            errors.append(f"source {path} is unmapped; assign it to a group or an exclusion")

    if errors:
        for error in errors:
            print(f"coverage audit input error: {error}", file=sys.stderr)
        return 2

    failures: list[str] = []
    for name, covered, total in measurements:
        rate = percent(covered, total)
        suffix = "PASS" if rate >= group_minimum else f"below {group_minimum:.1f}%"
        print(f"{name}: {covered}/{total} lines ({rate:.1f}%) {suffix}")
        if rate < group_minimum:
            failures.append(f"{name}: {covered}/{total} lines ({rate:.1f}%) below {group_minimum:.1f}%")
    covered = sum(covered for _, covered, _ in measurements)
    total = sum(total for _, _, total in measurements)
    rate = percent(covered, total)
    suffix = "PASS" if rate >= aggregate_minimum else f"below {aggregate_minimum:.1f}%"
    print(f"aggregate: {covered}/{total} lines ({rate:.1f}%) {suffix}")
    if rate < aggregate_minimum:
        failures.append(f"aggregate: {covered}/{total} lines ({rate:.1f}%) below {aggregate_minimum:.1f}%")
    if failures:
        print("coverage audit failed: " + "; ".join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
