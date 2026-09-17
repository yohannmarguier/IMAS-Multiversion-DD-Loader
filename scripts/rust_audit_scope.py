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
