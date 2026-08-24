#!/usr/bin/env python3
"""Merge the with-shim and without-shim 3.39.0 equilibrium read results into
one side-by-side, colorized table, so the shim's effect on each path stands
out at a glance.

Each input file already carries, per DD path, the 4.1.1 read (column 1), a
3.39.0 read (column 2) and a VERDICT comparing the two (same/FLIP/DIFF/
SHAPE/only4/only3/--). This script lines the two files' column-2 reads up
side by side and adds a SHIM EFFECT column comparing those two 3.39.0 reads
directly to each other, using the same verdict vocabulary.

Usage:
    python3 docs/colorize_results.py [with_shim_path] [without_shim_path]

Defaults to docs/results/0.1.0/{with,without}-shim-3.39.0-eq-read.
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

FIELD_SPLIT_RE = re.compile(r"\s{2,}")
SECTION_RE = re.compile(r"^--.*--$")

MISSING_VALUES = {"-", "(not provided)", ""}

# VERDICT column, as already computed by the source files (column 2 vs
# column 1, i.e. a 3.39.0 read vs the 4.1.1 read).
VERDICT_COLORS = {
    "same": WHITE,
    "flip": GREEN,
    "diff": RED,
    "shape": YELLOW,
    "only4": CYAN,
    "only3": CYAN,
    "--": GRAY,
}

# SHIM EFFECT column, computed here by comparing the with-shim 3.39.0 read
# to the without-shim 3.39.0 read directly (not either one to 4.1.1).
SHIM_EFFECT_COLORS = {
    "same": WHITE,
    "flip": MAGENTA,
    "diff": RED,
    "shim-only": CYAN,
    "noshim-only": YELLOW,
    "--": GRAY,
}

HEADERS = [
    "PATH",
    "4.1.1",
    "3.39.0 +shim",
    "VERDICT",
    "3.39.0 no-shim",
    "VERDICT",
    "SHIM EFFECT",
]


class Row:
    __slots__ = ("path", "col1", "col2", "verdict")

    def __init__(self, path: str, col1: str, col2: str, verdict: str):
        self.path = path
        self.col1 = col1
        self.col2 = col2
        self.verdict = verdict


def parse_preamble(path: Path) -> list:
    """Non-empty lines before the header's '====' divider."""
    lines = []
    with path.open("r", encoding="utf-8") as f:
        for line in f:
            stripped = line.rstrip("\n")
            if set(stripped.strip()) == {"="}:
                break
            if stripped.strip():
                lines.append(stripped)
    return lines


def parse_results(path: Path) -> list:
    """Parse an eq-read results file into ("section", text) and ("row", Row)
    items, in file order, starting after the header's '====' divider."""
    items = []
    in_table = False
    with path.open("r", encoding="utf-8") as f:
        for line in f:
            text = line.strip()
            if not text:
                continue
            if set(text) == {"="}:
                in_table = True
                continue
            if not in_table or text == "end of table":
                continue
            if SECTION_RE.match(text):
                items.append(("section", text))
                continue
            fields = FIELD_SPLIT_RE.split(text)
            if len(fields) != 4:
                continue
            path_field, col1, col2, verdict = fields
            items.append(("row", Row(path_field, col1, col2, verdict)))
    return items


def parse_float(value: str):
    try:
        return float(value)
    except ValueError:
        return None


def classify_pair(with_val: str, without_val: str) -> str:
    """Classify the relationship between the with-shim and without-shim
    3.39.0 reads for one path, using the same vocabulary the source files'
    own VERDICT column uses."""
    a, b = with_val.strip(), without_val.strip()
    a_missing = a in MISSING_VALUES
    b_missing = b in MISSING_VALUES

    if a_missing and b_missing:
        return "--"
    if a_missing:
        return "noshim-only"
    if b_missing:
        return "shim-only"
    if a == b:
        return "same"

    fa, fb = parse_float(a), parse_float(b)
    if fa is not None and fb is not None:
        if fa == fb:
            return "same"
        if fa != 0 and fa == -fb:
            return "flip"
    return "diff"


def verdict_color(verdict: str) -> str:
    return VERDICT_COLORS.get(verdict.strip().lower(), BLUE)


def shim_effect_color(effect: str) -> str:
    return SHIM_EFFECT_COLORS.get(effect.strip().lower(), BLUE)


def render_cell(text: str, width: int, color: str = "", align: str = "right") -> str:
    padded = text.ljust(width) if align == "left" else text.rjust(width)
    return f"{color}{padded}{RESET}" if color else padded


def print_header(with_shim_path: Path, without_shim_path: Path) -> None:
    with_preamble = parse_preamble(with_shim_path)
    without_preamble = parse_preamble(without_shim_path)

    def find(lines, prefix):
        return next((l for l in lines if l.strip().startswith(prefix)), None)

    with_status = find(with_preamble, "column 2 read")
    without_status = find(without_preamble, "column 2 read")

    print(f"{BOLD}{WHITE}with-shim file    : {with_shim_path}{RESET}")
    print(f"{BOLD}{WHITE}without-shim file : {without_shim_path}{RESET}")
    for line in with_preamble:
        text = line.strip()
        if text.startswith("column 2 read") or text.startswith("legend") or (
            text.startswith("PATH") and "VERDICT" in text
        ):
            continue
        print(f"{DIM}{line}{RESET}")
    if with_status:
        print(f"{DIM}{with_status} (with shim){RESET}")
    if without_status:
        print(f"{DIM}{without_status} (without shim){RESET}")
    print()
    print(
        f"{DIM}legend: same  FLIP  DIFF  SHAPE  only4  only3  --   |  "
        f"SHIM EFFECT: same  flip  diff  shim-only  noshim-only  --{RESET}"
    )
    print()


def build_rows(with_items: list, without_rows: dict) -> list:
    rows_out = []
    for kind, payload in with_items:
        if kind == "section":
            rows_out.append(("section", payload))
            continue

        row = payload
        without_row = without_rows.get(row.path)
        without_col2 = without_row.col2 if without_row else "?"
        without_verdict = without_row.verdict if without_row else "?"
        shim_effect = classify_pair(row.col2, without_col2)
        rows_out.append(
            (
                "row",
                [
                    row.path,
                    row.col1,
                    row.col2,
                    row.verdict,
                    without_col2,
                    without_verdict,
                    shim_effect,
                ],
            )
        )
    return rows_out


def print_table(rows_out: list) -> None:
    widths = [len(h) for h in HEADERS]
    for kind, cells in rows_out:
        if kind != "row":
            continue
        for i, cell in enumerate(cells):
            widths[i] = max(widths[i], len(cell))

    header_cells = [HEADERS[0].ljust(widths[0])] + [
        h.rjust(w) for h, w in zip(HEADERS[1:], widths[1:])
    ]
    print(f"{BOLD}{WHITE}{'  '.join(header_cells)}{RESET}")
    total_width = sum(widths) + 2 * (len(widths) - 1)
    print(f"{DIM}{'=' * total_width}{RESET}")
    print()

    for kind, cells in rows_out:
        if kind == "section":
            print(f"{BOLD}{CYAN}{cells}{RESET}")
            continue

        path, col1, with_col2, with_verdict, without_col2, without_verdict, shim_effect = cells
        line = "  ".join(
            [
                render_cell(path, widths[0], align="left"),
                render_cell(col1, widths[1]),
                render_cell(with_col2, widths[2]),
                render_cell(with_verdict, widths[3], color=verdict_color(with_verdict)),
                render_cell(without_col2, widths[4]),
                render_cell(without_verdict, widths[5], color=verdict_color(without_verdict)),
                render_cell(shim_effect, widths[6], color=shim_effect_color(shim_effect)),
            ]
        )
        print(line)


def main() -> int:
    default_dir = Path(__file__).resolve().parent / "results" / "0.1.0"
    with_shim_path = (
        Path(sys.argv[1]) if len(sys.argv) > 1 else default_dir / "with-shim-3.39.0-eq-read"
    )
    without_shim_path = (
        Path(sys.argv[2]) if len(sys.argv) > 2 else default_dir / "without-shim-3.39.0-eq-read"
    )

    for p in (with_shim_path, without_shim_path):
        if not p.is_file():
            print(f"error: results file not found: {p}", file=sys.stderr)
            return 1

    with_items = parse_results(with_shim_path)
    without_rows = {
        row.path: row for kind, row in parse_results(without_shim_path) if kind == "row"
    }

    print_header(with_shim_path, without_shim_path)
    print_table(build_rows(with_items, without_rows))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
