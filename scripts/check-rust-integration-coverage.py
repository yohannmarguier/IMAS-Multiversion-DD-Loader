#!/usr/bin/env python3
"""Validate and record one Rust coverage run driven by C executables."""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path


class EvidenceError(ValueError):
    """The collected integration-coverage evidence is incomplete or inconsistent."""


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", required=True)
    parser.add_argument("--artifact", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--configuration", required=True)
    parser.add_argument("--profile-dir", type=Path, required=True)
    parser.add_argument("--lcov", type=Path, required=True)
    artifact = parser.add_mutually_exclusive_group(required=True)
    artifact.add_argument("--library", type=Path)
    artifact.add_argument("--executable", type=Path)
    parser.add_argument("--loaded-library-file", type=Path)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--baseline-lcov", type=Path)
    parser.add_argument("--probe-lcov", type=Path)
    parser.add_argument("--probe-source")
    parser.add_argument("--component-lcov", type=Path, action="append")
    parser.add_argument("--component-profile-dir", type=Path)
    parser.add_argument("--component-scope", type=Path)
    parser.add_argument("--detail-source", action="append", default=[],
                        help="publish uncovered lines and require nonzero execution for this source")
    parser.add_argument(
        "--source",
        action="append",
        default=[],
        help="source path whose covered and measured line counts must be published",
    )
    return parser.parse_args()


def load_lcov(path: Path) -> dict[str, dict[int, int]]:
    try:
        raw_lines = path.read_text().splitlines()
    except OSError as error:
        raise EvidenceError(f"cannot read LCOV report {path}: {error}") from error

    files: dict[str, dict[int, int]] = {}
    current: dict[int, int] | None = None
    current_name: str | None = None
    for raw in raw_lines:
        if not raw:
            continue
        if raw.startswith("SF:"):
            if current is not None:
                raise EvidenceError(f"LCOV source {current_name} has no end_of_record")
            current_name = raw[3:]
            if not current_name:
                raise EvidenceError("LCOV source file is empty")
            current = files.setdefault(current_name, {})
        elif raw.startswith("DA:"):
            if current is None:
                raise EvidenceError("LCOV data line appears before a source file")
            try:
                line_text, count_text, *_ = raw[3:].split(",")
                line = int(line_text)
                count = int(count_text)
            except ValueError as error:
                raise EvidenceError(f"invalid LCOV data line: {raw}") from error
            if line < 1 or count < 0:
                raise EvidenceError(f"invalid LCOV data line: {raw}")
            current[line] = current.get(line, 0) + count
        elif raw == "end_of_record":
            if current is None:
                raise EvidenceError("LCOV end_of_record appears before a source file")
            current = None
            current_name = None
        elif raw.startswith(
            ("TN:", "FN:", "FNDA:", "FNF:", "FNH:", "BRDA:", "BRF:", "BRH:", "LF:", "LH:")
        ):
            continue
        else:
            raise EvidenceError(f"unrecognised LCOV record: {raw}")
    if current is not None:
        raise EvidenceError(f"LCOV source {current_name} has no end_of_record")
    if not files:
        raise EvidenceError(f"LCOV report {path} contains no source files")
    if not any(lines for lines in files.values()):
        raise EvidenceError(f"LCOV report {path} contains no measured lines")
    return files


def matching_source_counts(
    files: dict[str, dict[int, int]],
    requested: str,
    start_line: int | None = None,
    end_line: int | None = None,
) -> dict[int, int]:
    requested = Path(requested).as_posix()
    matches = [
        lines
        for name, lines in files.items()
        if Path(name).as_posix() == requested or Path(name).as_posix().endswith("/" + requested)
    ]
    if not matches:
        raise EvidenceError(f"LCOV report has no measurement data for {requested}")
    counts: dict[int, int] = {}
    for lines in matches:
        for line, count in lines.items():
            if (start_line is None or line >= start_line) and (
                end_line is None or line <= end_line
            ):
                counts[line] = counts.get(line, 0) + count
    if not counts:
        raise EvidenceError(
            f"LCOV report has no measurement data for {requested} in requested range"
        )
    return counts


def source_counts(files: dict[str, dict[int, int]], requested: str) -> tuple[int, int]:
    counts = matching_source_counts(files, requested)
    return sum(count > 0 for count in counts.values()), len(counts)


def validate_probe_args(args: argparse.Namespace) -> bool:
    supplied = (args.baseline_lcov, args.probe_lcov, args.probe_source)
    if any(value is not None for value in supplied) and not all(
        value is not None for value in supplied
    ):
        raise EvidenceError(
            "--baseline-lcov, --probe-lcov and --probe-source must be supplied together"
        )
    return all(value is not None for value in supplied)


def validate_component_args(args: argparse.Namespace) -> bool:
    supplied = (
        args.component_lcov,
        args.component_profile_dir,
        args.component_scope,
    )
    if any(value is not None for value in supplied) and not all(value is not None for value in supplied):
        raise EvidenceError(
            "--component-lcov, --component-profile-dir and --component-scope "
            "must be supplied together"
        )
    if args.component_lcov is not None and len(args.component_lcov) != 2:
        raise EvidenceError("component coverage requires exactly two LCOV reports")
    return all(value is not None for value in supplied)


def source_report(
    files: dict[str, dict[int, int]],
    requested: str,
    start_line: int | None = None,
    end_line: int | None = None,
) -> dict[str, object]:
    matching_lines = matching_source_counts(files, requested, start_line, end_line)
    result: dict[str, object] = {
        "source": requested,
        "covered_lines": sum(count > 0 for count in matching_lines.values()),
        "measured_lines": len(matching_lines),
        "uncovered_lines": sorted(line for line, count in matching_lines.items() if count == 0),
    }
    if start_line is not None and end_line is not None:
        result["start_line"] = start_line
        result["end_line"] = end_line
    return result


def function_range(source: Path, names: list[str]) -> tuple[int, int]:
    try:
        lines = source.read_text().splitlines()
    except OSError as error:
        raise EvidenceError(f"cannot read scoped source {source}: {error}") from error

    ranges: list[tuple[int, int]] = []
    for name in names:
        signature = f"fn {name}("
        starts = [index for index, line in enumerate(lines) if signature in line]
        if len(starts) != 1:
            raise EvidenceError(f"expected exactly one {signature} in {source}")
        start = starts[0]
        depth = 0
        opened = False
        for index in range(start, len(lines)):
            depth += lines[index].count("{") - lines[index].count("}")
            opened = opened or "{" in lines[index]
            if opened and depth == 0:
                ranges.append((start + 1, index + 1))
                break
        else:
            raise EvidenceError(f"cannot find the end of {name} in {source}")
    return min(start for start, _ in ranges), max(end for _, end in ranges)


def load_component_scope(path: Path) -> tuple[str, str, int, int]:
    try:
        value = json.loads(path.read_text())
        adapter = value["adapter"]["path"]
        policy = value["policy"]
        policy_path = policy["path"]
        functions = policy["functions"]
    except (OSError, json.JSONDecodeError, KeyError, TypeError) as error:
        raise EvidenceError(f"invalid component scope {path}: {error}") from error
    if not isinstance(adapter, str) or not isinstance(policy_path, str) or not (
        isinstance(functions, list) and functions and all(isinstance(name, str) for name in functions)
    ):
        raise EvidenceError(f"invalid component scope {path}: invalid source selection")
    source_root = path.parent.parent
    start_line, end_line = function_range(source_root / policy_path, functions)
    return adapter, policy_path, start_line, end_line


def merge_reports(reports: list[dict[str, dict[int, int]]]) -> dict[str, dict[int, int]]:
    merged: dict[str, dict[int, int]] = {}
    for report in reports:
        for name, lines in report.items():
            merged_lines = merged.setdefault(name, {})
            for line, count in lines.items():
                merged_lines[line] = merged_lines.get(line, 0) + count
    return merged


def run(args: argparse.Namespace) -> dict[str, object]:
    if not args.profile_dir.is_dir():
        raise EvidenceError(f"raw-profile directory does not exist: {args.profile_dir}")
    profiles = sorted(args.profile_dir.glob("*.profraw"))
    if not profiles:
        raise EvidenceError(f"raw-profile directory {args.profile_dir} contains no .profraw files")
    binary = args.library if args.library is not None else args.executable
    if not binary.is_file():
        if args.executable is not None:
            raise EvidenceError(f"instrumented executable does not exist: {binary}")
        raise EvidenceError(f"instrumented library does not exist: {args.library}")

    if args.library is not None:
        if args.loaded_library_file is None:
            raise EvidenceError("--library requires --loaded-library-file")
        try:
            observed_lines = [line for line in args.loaded_library_file.read_text().splitlines() if line]
        except OSError as error:
            raise EvidenceError(
                f"cannot read loaded-library evidence {args.loaded_library_file}: {error}"
            ) from error
        if len(observed_lines) != 1:
            raise EvidenceError("loaded-library evidence must contain exactly one nonempty path")
        observed_library = Path(observed_lines[0]).resolve()
        if observed_library != binary.resolve():
            raise EvidenceError(
                "loaded shim does not match the instrumented library: "
                f"expected {binary.resolve()}, observed {observed_library}"
            )

    report = load_lcov(args.lcov)
    covered_lines = sum(count > 0 for lines in report.values() for count in lines.values())
    measured_lines = sum(len(lines) for lines in report.values())
    if covered_lines == 0:
        raise EvidenceError(f"LCOV report {args.lcov} contains no covered Rust lines")

    sources: dict[str, dict[str, int]] = {}
    for source in args.source:
        source_covered, source_measured = source_counts(report, source)
        sources[source] = {
            "covered_lines": source_covered,
            "measured_lines": source_measured,
        }

    probe: dict[str, object] | None = None
    if validate_probe_args(args):
        baseline_covered, baseline_total = source_counts(
            load_lcov(args.baseline_lcov), args.probe_source
        )
        scenario_covered, scenario_total = source_counts(
            load_lcov(args.probe_lcov), args.probe_source
        )
        if baseline_total != scenario_total:
            raise EvidenceError(
                f"probe source {args.probe_source} changed measured-line count "
                f"from {baseline_total} to {scenario_total}"
            )
        if baseline_covered != 0:
            raise EvidenceError(
                f"baseline already covers {baseline_covered} lines in {args.probe_source}"
            )
        if scenario_covered == 0:
            raise EvidenceError(
                f"scenario leaves {args.probe_source} at zero covered lines"
            )
        probe = {
            "source": args.probe_source,
            "baseline_covered_lines": baseline_covered,
            "scenario_covered_lines": scenario_covered,
            "measured_lines": scenario_total,
        }

    metadata: dict[str, object] = {
        "schema_version": 1,
        "profile": args.profile,
        "artifact": args.artifact,
        "revision": args.revision,
        "configuration": args.configuration,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "library" if args.library is not None else "executable": str(binary.resolve()),
        "profile_count": len(profiles),
        "lcov": str(args.lcov.resolve()),
        "covered_lines": covered_lines,
        "measured_lines": measured_lines,
        "sources": sources,
    }
    if args.detail_source:
        metadata["details"] = [source_report(report, source) for source in args.detail_source]
        for detail in metadata["details"]:
            if detail["covered_lines"] == 0:
                raise EvidenceError(f"required source {detail['source']} has no executed lines")
    if probe is not None:
        metadata["probe"] = probe
    if validate_component_args(args):
        component_report = merge_reports([load_lcov(path) for path in args.component_lcov])
        adapter_source, policy_source, policy_start, policy_end = load_component_scope(
            args.component_scope
        )
        component_profiles = sorted(args.component_profile_dir.rglob("*.profraw"))
        if not component_profiles:
            raise EvidenceError(
                f"component raw-profile directory {args.component_profile_dir} "
                "contains no .profraw files"
            )
        metadata["component"] = {
            "lcov": [str(path.resolve()) for path in args.component_lcov],
            "profile_count": len(component_profiles),
            "scope": str(args.component_scope.resolve()),
            "adapter": source_report(component_report, adapter_source),
            "policy": source_report(
                component_report,
                policy_source,
                policy_start,
                policy_end,
            ),
        }
    args.metadata.parent.mkdir(parents=True, exist_ok=True)
    args.metadata.write_text(json.dumps(metadata, indent=2) + "\n")
    return metadata


def main() -> int:
    try:
        metadata = run(parse_args())
    except EvidenceError as error:
        print(f"integration coverage evidence error: {error}", file=sys.stderr)
        return 2
    print(
        f"{metadata['profile']}: {metadata['covered_lines']}/{metadata['measured_lines']} "
        f"Rust lines covered from {metadata['profile_count']} raw profiles"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
