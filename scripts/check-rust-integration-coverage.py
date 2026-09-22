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
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--loaded-library-file", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--baseline-lcov", type=Path)
    parser.add_argument("--probe-lcov", type=Path)
    parser.add_argument("--probe-source")
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


def source_counts(files: dict[str, dict[int, int]], requested: str) -> tuple[int, int]:
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
            counts[line] = counts.get(line, 0) + count
    return sum(count > 0 for count in counts.values()), len(counts)


def validate_probe_args(args: argparse.Namespace) -> bool:
    supplied = (args.baseline_lcov, args.probe_lcov, args.probe_source)
    if any(value is not None for value in supplied) and not all(value is not None for value in supplied):
        raise EvidenceError(
            "--baseline-lcov, --probe-lcov and --probe-source must be supplied together"
        )
    return all(value is not None for value in supplied)


def run(args: argparse.Namespace) -> dict[str, object]:
    if not args.profile_dir.is_dir():
        raise EvidenceError(f"raw-profile directory does not exist: {args.profile_dir}")
    profiles = sorted(args.profile_dir.glob("*.profraw"))
    if not profiles:
        raise EvidenceError(f"raw-profile directory {args.profile_dir} contains no .profraw files")
    if not args.library.is_file():
        raise EvidenceError(f"instrumented library does not exist: {args.library}")

    try:
        observed_lines = [line for line in args.loaded_library_file.read_text().splitlines() if line]
    except OSError as error:
        raise EvidenceError(
            f"cannot read loaded-library evidence {args.loaded_library_file}: {error}"
        ) from error
    if len(observed_lines) != 1:
        raise EvidenceError("loaded-library evidence must contain exactly one nonempty path")
    expected_library = args.library.resolve()
    observed_library = Path(observed_lines[0]).resolve()
    if observed_library != expected_library:
        raise EvidenceError(
            "loaded shim does not match the instrumented library: "
            f"expected {expected_library}, observed {observed_library}"
        )

    report = load_lcov(args.lcov)
    covered_lines = sum(count > 0 for lines in report.values() for count in lines.values())
    measured_lines = sum(len(lines) for lines in report.values())
    if covered_lines == 0:
        raise EvidenceError(f"LCOV report {args.lcov} contains no covered Rust lines")

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
        "library": str(expected_library),
        "profile_count": len(profiles),
        "lcov": str(args.lcov.resolve()),
        "covered_lines": covered_lines,
        "measured_lines": measured_lines,
    }
    if probe is not None:
        metadata["probe"] = probe
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
