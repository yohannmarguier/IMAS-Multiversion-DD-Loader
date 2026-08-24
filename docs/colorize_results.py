#!/usr/bin/env python3
"""Compare the with-shim and without-shim 3.39.0 equilibrium read results and
print one compact, colorized report grouped by what the shim actually did,
so the shim's effect on each path stands out at a glance.

Each input file already carries, per DD path, the 4.1.1 read (column 1), a
3.39.0 read (column 2) and a Delta comparing the two (same/FLIP/DIFF/SHAPE/
only4/only3/--). This script lines the two files' column-2 reads up side by
side and, from the two Deltas, classifies each path into one of four SHIM
EFFECT groups:

    FIXED   with-shim matches 4.1.1, without-shim didn't
    BROKEN  without-shim matched 4.1.1, with-shim doesn't (a regression)
    CHANGED neither matches 4.1.1, but the shim read something different
    SAME    the shim made no difference to what was read

Rows print grouped by that classification (BROKEN/CHANGED first, since
those are what need attention), and the bulk SAME group is collapsed to a
count by default -- pass --show-same to list it. Each row's own width
adapts to the terminal so a row never wraps.

Usage:
    python3 docs/colorize_results.py [with_shim_path] [without_shim_path] [--show-same]

Defaults to docs/results/0.1.0/{with,without}-shim-3.39.0-eq-read.
"""

import argparse
import re
import shutil
import sys
from collections import Counter
from pathlib import Path

RESET = "\033[0m"
BOLD = "\033[1m"
DIM = "\033[2m"

WHITE = "\033[97m"
GREEN = "\033[32m"
BRIGHT_GREEN = "\033[92m"
CYAN = "\033[36m"
RED = "\033[91m"
YELLOW = "\033[33m"
MAGENTA = "\033[35m"
GRAY = "\033[90m"
BLUE = "\033[34m"

FIELD_SPLIT_RE = re.compile(r"\s{2,}")
SECTION_RE = re.compile(r"^--.*--$")

MISSING_VALUES = {"-", "(not provided)", ""}

# Delta column: the source files' own verdict, a 3.39.0 read vs the 4.1.1
# read, shortened to a 2-character code so a row stays narrow.
DELTA_CODES = {
    "same": "OK",
    "flip": "FL",
    "diff": "DF",
    "shape": "SH",
    "only4": "o4",
    "only3": "o3",
    "--": "--",
}
DELTA_COLORS = {
    "same": GREEN,
    "flip": YELLOW,
    "diff": RED,
    "shape": MAGENTA,
    "only4": CYAN,
    "only3": CYAN,
    "--": GRAY,
}

# SHIM EFFECT: what changed between the with-shim and without-shim reads,
# derived from the two Deltas above -- this is the point of the comparison.
EFFECT_ORDER = ["BROKEN", "CHANGED", "FIXED", "SAME"]
EFFECT_COLORS = {
    "BROKEN": RED,
    "CHANGED": YELLOW,
    "FIXED": BRIGHT_GREEN,
    "SAME": GRAY,
}
EFFECT_DESCRIPTIONS = {
    "BROKEN": "without-shim matched 4.1.1, the shim regressed it",
    "CHANGED": "shim altered the read, still doesn't match 4.1.1",
    "FIXED": "shim corrected the read to match 4.1.1",
    "SAME": "shim made no difference to the read",
}

COL1_WIDTH = 13
VAL_WIDTH = 13
DELTA_WIDTH = 2
SEP = " │ "  # " │ "
MIN_PATH_WIDTH = 20
MAX_PATH_WIDTH = 56


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


def clean_section(text: str) -> str:
    """"-- name  (comment) ----" -> "name  (comment)"."""
    return text.strip("-").strip()


def classify_effect(with_verdict: str, without_verdict: str, with_val: str, without_val: str) -> str:
    with_ok = with_verdict.strip().lower() == "same"
    without_ok = without_verdict.strip().lower() == "same"
    if with_ok and not without_ok:
        return "FIXED"
    if without_ok and not with_ok:
        return "BROKEN"
    if with_val.strip() == without_val.strip():
        return "SAME"
    return "CHANGED"


def build_rows(with_items: list, without_rows: dict) -> list:
    rows = []
    current_section = None
    for kind, payload in with_items:
        if kind == "section":
            current_section = clean_section(payload)
            continue

        row = payload
        without_row = without_rows.get(row.path)
        without_val = without_row.col2 if without_row else "?"
        without_delta = without_row.verdict if without_row else "?"
        rows.append(
            {
                "section": current_section,
                "path": row.path,
                "col1": row.col1,
                "with_val": row.col2,
                "with_delta": row.verdict,
                "without_val": without_val,
                "without_delta": without_delta,
                "effect": classify_effect(row.verdict, without_delta, row.col2, without_val),
            }
        )
    return rows


def truncate(text: str, width: int) -> str:
    if len(text) <= width:
        return text
    if width <= 1:
        return text[:width]
    return text[: width - 1] + "…"


def render_cell(text: str, width: int, color: str = "", align: str = "right") -> str:
    padded = truncate(text, width)
    padded = padded.ljust(width) if align == "left" else padded.rjust(width)
    return f"{color}{padded}{RESET}" if color else padded


def delta_cell(delta: str, width: int) -> str:
    key = delta.strip().lower()
    code = DELTA_CODES.get(key, delta[:width])
    color = DELTA_COLORS.get(key, BLUE)
    return render_cell(code, width, color=color)


def path_width_for_terminal() -> int:
    term_cols = shutil.get_terminal_size(fallback=(100, 24)).columns
    fixed = COL1_WIDTH + len(SEP) + (VAL_WIDTH + 1 + DELTA_WIDTH) + len(SEP) + (VAL_WIDTH + 1 + DELTA_WIDTH)
    available = term_cols - fixed - len(SEP)
    return max(MIN_PATH_WIDTH, min(MAX_PATH_WIDTH, available))


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(Path.cwd()))
    except ValueError:
        return str(path)


def print_source_header(with_shim_path: Path, without_shim_path: Path) -> None:
    with_preamble = parse_preamble(with_shim_path)
    without_preamble = parse_preamble(without_shim_path)

    def find(lines, prefix):
        return next((l for l in lines if l.strip().startswith(prefix)), None)

    with_status = find(with_preamble, "column 2 read")
    without_status = find(without_preamble, "column 2 read")

    print(f"{BOLD}{WHITE}with-shim file    : {display_path(with_shim_path)}{RESET}")
    print(f"{BOLD}{WHITE}without-shim file : {display_path(without_shim_path)}{RESET}")
    for line in with_preamble:
        text = line.strip()
        if (
            text.startswith("column 2 read")
            or text.startswith("legend")
            or (text.startswith("PATH") and "VERDICT" in text)
        ):
            continue
        print(f"{DIM}{line}{RESET}")
    if with_status:
        print(f"{DIM}{with_status} (with shim){RESET}")
    if without_status:
        print(f"{DIM}{without_status} (without shim){RESET}")
    print()


def print_legend() -> None:
    print(f"{DIM}Delta vs 4.1.1: {RESET}", end="")
    print(
        "  ".join(
            f"{DELTA_COLORS[k]}{v}{RESET}={DIM}{k}{RESET}" for k, v in DELTA_CODES.items()
        )
    )
    print()


def print_summary(rows: list) -> None:
    counts = Counter(r["effect"] for r in rows)
    parts = [
        f"{EFFECT_COLORS[name]}{BOLD}{name.lower()}{RESET} {counts.get(name, 0)}"
        for name in EFFECT_ORDER
    ]
    print("   ".join(parts))
    print()


def table_width(path_width: int) -> int:
    val_group_width = VAL_WIDTH + 1 + DELTA_WIDTH
    return path_width + len(SEP) + COL1_WIDTH + len(SEP) + val_group_width + len(SEP) + val_group_width


def print_column_header(path_width: int) -> None:
    val_group_width = VAL_WIDTH + 1 + DELTA_WIDTH

    def center(text, width):
        return text.center(width)

    top = SEP.join(
        [
            " " * path_width,
            center(truncate("4.1.1", COL1_WIDTH), COL1_WIDTH),
            center(truncate("3.39 +shim", val_group_width), val_group_width),
            center(truncate("3.39 no-shim", val_group_width), val_group_width),
        ]
    )
    sub = SEP.join(
        [
            "PATH".ljust(path_width),
            "value".rjust(COL1_WIDTH),
            "value".rjust(VAL_WIDTH) + " " + "Δ".rjust(DELTA_WIDTH),
            "value".rjust(VAL_WIDTH) + " " + "Δ".rjust(DELTA_WIDTH),
        ]
    )
    print(f"{BOLD}{WHITE}{top}{RESET}")
    print(f"{BOLD}{WHITE}{sub}{RESET}")
    print(f"{DIM}{'=' * table_width(path_width)}{RESET}")


def render_row(r: dict, path_width: int) -> str:
    return SEP.join(
        [
            render_cell(r["path"], path_width, align="left"),
            render_cell(r["col1"], COL1_WIDTH),
            render_cell(r["with_val"], VAL_WIDTH) + " " + delta_cell(r["with_delta"], DELTA_WIDTH),
            render_cell(r["without_val"], VAL_WIDTH) + " " + delta_cell(r["without_delta"], DELTA_WIDTH),
        ]
    )


def print_group(effect_name: str, rows_in_group: list, path_width: int, show_same: bool) -> None:
    if not rows_in_group:
        return

    color = EFFECT_COLORS[effect_name]
    banner = f"{effect_name} -- {EFFECT_DESCRIPTIONS[effect_name]} ({len(rows_in_group)})"
    print()
    print(f"{BOLD}{color}{banner}{RESET}")

    if effect_name == "SAME" and not show_same:
        print(f"{DIM}  (hidden by default -- pass --show-same to list){RESET}")
        return

    print(f"{color}{'-' * table_width(path_width)}{RESET}")
    current_section = object()
    for r in rows_in_group:
        if r["section"] != current_section:
            current_section = r["section"]
            if current_section:
                print(f"{DIM}▸ {current_section}{RESET}")
        print(render_row(r, path_width))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("with_shim", nargs="?", type=Path, help="path to the with-shim results file")
    parser.add_argument("without_shim", nargs="?", type=Path, help="path to the without-shim results file")
    parser.add_argument(
        "--show-same",
        action="store_true",
        help="also list paths where the shim made no difference (hidden by default)",
    )
    args = parser.parse_args()

    default_dir = Path(__file__).resolve().parent / "results" / "0.1.0"
    with_shim_path = args.with_shim or default_dir / "with-shim-3.39.0-eq-read"
    without_shim_path = args.without_shim or default_dir / "without-shim-3.39.0-eq-read"

    for p in (with_shim_path, without_shim_path):
        if not p.is_file():
            print(f"error: results file not found: {p}", file=sys.stderr)
            return 1

    with_items = parse_results(with_shim_path)
    without_rows = {
        row.path: row for kind, row in parse_results(without_shim_path) if kind == "row"
    }
    rows = build_rows(with_items, without_rows)

    print_source_header(with_shim_path, without_shim_path)
    print_legend()
    print_summary(rows)

    path_width = path_width_for_terminal()
    print_column_header(path_width)

    grouped = {name: [r for r in rows if r["effect"] == name] for name in EFFECT_ORDER}
    for name in EFFECT_ORDER:
        print_group(name, grouped[name], path_width, args.show_same)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
