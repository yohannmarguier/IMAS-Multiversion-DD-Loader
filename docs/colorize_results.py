#!/usr/bin/env python3
"""Print 0.1.0-results.txt to the terminal with the CONVERSION EFFECT
column colorized, so conversion behaviour stands out at a glance.

Usage:
    python3 docs/colorize_results.py [path/to/results.txt]

Defaults to 0.1.0-results.txt next to this script.
"""

import re
import sys
from pathlib import Path

RESET = "\033[0m"
BOLD = "\033[1m"
DIM = "\033[2m"

WHITE = "\033[97m"
GREEN = "\033[32m"
CYAN = "\033[36m"
RED = "\033[91m"
YELLOW = "\033[33m"
MAGENTA = "\033[35m"
GRAY = "\033[90m"
BLUE = "\033[34m"

# CONVERSION EFFECT phrase -> color, matched by exact value (after
# lowercasing) against the trailing field. Exact matching avoids
# substring traps, e.g. "not recovered" containing "recovered".
EFFECT_COLORS = {
    "unchanged": WHITE,
    "unchanged/unavailable": WHITE,
    "sign corrected": GREEN,
    "recovered": CYAN,
    "recovered aos": CYAN,
    "created, wrong/default": RED,
    "created, invalid/default": RED,
    "dropped/unmappable": YELLOW,
    "not recovered": MAGENTA,
    "unavailable": GRAY,
}

FIELD_SPLIT_RE = re.compile(r"\s{2,}")


def effect_color(effect_text: str) -> str:
    lowered = effect_text.strip().lower()
    return EFFECT_COLORS.get(lowered, BLUE)  # unrecognized category: flag distinctly


def render_line(line: str) -> str:
    stripped = line.rstrip("\n")

    if not stripped.strip():
        return ""

    if set(stripped.strip()) == {"="}:
        return f"{DIM}{stripped}{RESET}"

    if stripped.lstrip().startswith("--") and stripped.rstrip().endswith("--"):
        return f"{BOLD}{CYAN}{stripped}{RESET}"

    if "CONVERSION EFFECT" in stripped and "PATH" in stripped:
        return f"{BOLD}{WHITE}{stripped}{RESET}"

    fields = FIELD_SPLIT_RE.split(stripped.strip())
    effect_text = fields[-1] if fields else ""
    color = effect_color(effect_text)
    return f"{color}{stripped}{RESET}"


def main() -> int:
    if len(sys.argv) > 1:
        results_path = Path(sys.argv[1])
    else:
        results_path = Path(__file__).resolve().parent / "0.1.0-results.txt"

    if not results_path.is_file():
        print(f"error: results file not found: {results_path}", file=sys.stderr)
        return 1

    with results_path.open("r", encoding="utf-8") as f:
        for line in f:
            print(render_line(line))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
