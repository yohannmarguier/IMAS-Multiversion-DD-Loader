#!/usr/bin/env python3
"""The checked-in Rust audit scope, shared by both audit checkers.

The line-coverage audit and the mutation audit measure the same six logical
groups over the same source ranges, so the ownership model lives here once
rather than once per checker. Only the scoring differs between them.
"""

from __future__ import annotations

import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


GROUP_NAMES = {
    "conversion",
    "dd-version",
    "context-registry",
    "loss",
    "artifact-validation",
    "deterministic-runtime-binding-policy",
}


class ScopeError(ValueError):
    """The checked-in measurement scope is malformed or ambiguous."""


@dataclass(frozen=True)
class SourceRange:
    path: str
    start_line: int | None
    end_line: int | None

    def contains(self, start: int, end: int | None = None) -> bool:
        """Whether this range owns a line, or a whole span of them.

        A mutant spans lines and must be owned entirely; a coverage line is
        the degenerate span that begins and ends on itself.
        """
        end = start if end is None else end
        return (self.start_line is None or self.start_line <= start) and (
            self.end_line is None or end <= self.end_line
        )

    def overlaps(self, other: "SourceRange") -> bool:
        if self.path != other.path:
            return False
        self_start = self.start_line or 1
        self_end = self.end_line or sys.maxsize
        other_start = other.start_line or 1
        other_end = other.end_line or sys.maxsize
        return self_start <= other_end and other_start <= self_end

    def display(self) -> str:
        if self.start_line is None:
            return self.path
        return f"{self.path}:{self.start_line}-{self.end_line}"


@dataclass(frozen=True)
class Group:
    name: str
    sources: tuple[SourceRange, ...]


def load_groups(value: Any, scope_path: Path) -> list[Group]:
    """The six logical groups and their owned source ranges.

    Every range is validated and no two may overlap: one line has one owner,
    or the two audits could score the same code under different groups.
    """
    if not isinstance(value, dict) or value.get("schema_version") != 1:
        raise ScopeError(f"scope {scope_path} must be a schema_version 1 JSON object")
    raw_groups = value.get("groups")
    if not isinstance(raw_groups, list):
        raise ScopeError("scope must define groups")

    groups: list[Group] = []
    assigned: list[tuple[str, SourceRange]] = []
    for raw_group in raw_groups:
        if not isinstance(raw_group, dict) or not isinstance(raw_group.get("name"), str):
            raise ScopeError("each group needs a name")
        raw_sources = raw_group.get("sources")
        if not isinstance(raw_sources, list) or not raw_sources:
            raise ScopeError(f"group {raw_group['name']} must own at least one source")
        sources: list[SourceRange] = []
        for raw_source in raw_sources:
            source = parse_source(raw_source, raw_group["name"])
            for owner, existing in assigned:
                if source.overlaps(existing):
                    raise ScopeError(
                        f"source {source.display()} is assigned to both "
                        f"{owner} and {raw_group['name']}"
                    )
            assigned.append((raw_group["name"], source))
            sources.append(source)
        groups.append(Group(raw_group["name"], tuple(sources)))

    names = {group.name for group in groups}
    if names != GROUP_NAMES or len(names) != len(groups):
        raise ScopeError("scope must declare each of the six required logical groups exactly once")
    return groups


def parse_source(raw_source: Any, group_name: str) -> SourceRange:
    if not isinstance(raw_source, dict) or not isinstance(raw_source.get("path"), str):
        raise ScopeError(f"group {group_name} has a source without a path")
    start = raw_source.get("start_line")
    end = raw_source.get("end_line")
    if (start is None) != (end is None) or (
        start is not None
        and (not isinstance(start, int) or not isinstance(end, int) or start < 1 or end < start)
    ):
        raise ScopeError(
            f"source {raw_source['path']} must give a valid start_line/end_line pair"
        )
    return SourceRange(raw_source["path"], start, end)


def load_exclusions(value: Any) -> list[SourceRange]:
    """The ranges deliberately left to another test layer, each with a reason.

    An exclusion without line bounds excludes the whole file; one with them
    excludes only that part of a file whose rest is measured.
    """
    raw_exclusions = value.get("exclusions", []) if isinstance(value, dict) else None
    if not isinstance(raw_exclusions, list):
        raise ScopeError("scope exclusions must be a list")
    exclusions: list[SourceRange] = []
    for exclusion in raw_exclusions:
        if not isinstance(exclusion, dict) or not isinstance(exclusion.get("reason"), str):
            raise ScopeError("each exclusion needs a path and a reason")
        exclusions.append(parse_source(exclusion, "exclusions"))
    return exclusions


def production_boundary(path: Path) -> int:
    """The last production line of a source file.

    A file's inline test module — `#[cfg(test)]` followed within two lines by
    `mod tests` — ends the production region; a file without one is production
    to its last line.
    """
    lines = path.read_text().splitlines()
    boundary = len(lines)
    for index, line in enumerate(lines):
        if line.strip() != "#[cfg(test)]":
            continue
        if any(following.startswith("mod tests") for following in lines[index + 1 : index + 3]):
            boundary = index
    return boundary


def unpartitioned_production(
    groups: list[Group], exclusions: list[SourceRange], root: Path
) -> list[str]:
    """Production lines of a measured file owned by neither a group nor an exclusion.

    A range that simply stops short takes code out of the denominator without
    saying so — the failure this check exists to make impossible. Files the
    scope names but that are not on disk are left to the measurement check,
    which reports a source with no data.
    """
    measured: dict[str, list[SourceRange]] = {}
    for group in groups:
        for source in group.sources:
            measured.setdefault(source.path, []).append(source)
    for excluded in exclusions:
        if excluded.path in measured:
            measured[excluded.path].append(excluded)

    errors: list[str] = []
    for path, ranges in sorted(measured.items()):
        file_path = root / path
        if not file_path.is_file():
            continue
        last = production_boundary(file_path)
        owned: set[int] = set()
        for source in ranges:
            start = source.start_line or 1
            end = min(source.end_line or last, last)
            owned.update(range(start, end + 1))
        missing = sorted(set(range(1, last + 1)) - owned)
        if not missing:
            continue
        first = missing[0]
        run_end = first
        for line in missing[1:]:
            if line != run_end + 1:
                break
            run_end = line
        errors.append(
            f"{path}: production lines {first}-{run_end} belong to no group and no "
            f"exclusion ({len(missing)} of {last} unassigned); extend a range or "
            "declare a ranged exclusion with its reason"
        )
    return errors
