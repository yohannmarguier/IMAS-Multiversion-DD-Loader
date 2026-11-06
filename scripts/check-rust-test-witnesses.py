#!/usr/bin/env python3
"""Require named production test witnesses to be discovered and runnable."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def listed_tests(path: Path) -> set[str]:
    return {
        line.removesuffix(": test")
        for line in path.read_text().splitlines()
        if line.endswith(": test")
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--discovered", type=Path, required=True)
    parser.add_argument("--ignored", type=Path, required=True)
    args = parser.parse_args()

    witnesses = json.loads(args.manifest.read_text())["witnesses"]
    discovered = listed_tests(args.discovered)
    ignored = listed_tests(args.ignored)
    missing = [f"{scenario}: {test}" for scenario, test in witnesses.items() if test not in discovered]
    skipped = [f"{scenario}: {test}" for scenario, test in witnesses.items() if test in ignored]
    failures = []
    if missing:
        failures.append("production runtime-map test witnesses were not discovered:\n  " + "\n  ".join(missing))
    if skipped:
        failures.append("production runtime-map test witnesses are ignored:\n  " + "\n  ".join(skipped))
    if failures:
        raise SystemExit("\n".join(failures))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
