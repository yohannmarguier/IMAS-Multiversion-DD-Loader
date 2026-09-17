#!/usr/bin/env python3
"""Create compact cargo-mutants report fixtures for the mutation-audit checker."""

from __future__ import annotations

import json
import sys
from pathlib import Path


SOURCES = {
    "conversion": "src/conversion.rs",
    "dd-version": "src/version.rs",
    "context-registry": "src/registry.rs",
    "loss": "src/loss.rs",
    "artifact-validation": "src/bin/validate.rs",
    "deterministic-runtime-binding-policy": "src/core/policy.rs",
}


def mutant(group: str, line: int, suffix: str = "") -> dict[str, object]:
    source = SOURCES[group]
    action = suffix or f"replace {group} outcome {line}"
    return {
        "name": f"{source}:{line}:1: {action}",
        "file": source,
        "span": {"start": {"line": line}, "end": {"line": line}},
    }


def write(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n")


def outcome_report(outcomes: list[tuple[dict[str, object], str]]) -> dict[str, object]:
    names = [summary for _, summary in outcomes]
    cargo_mutants_summaries = {
        "Caught": "CaughtMutant",
        "Missed": "MissedMutant",
        "Timeout": "Timeout",
        "Unviable": "Unviable",
    }
    return {
        "cargo_mutants_version": "27.1.0",
        "outcomes": [{"scenario": "Baseline", "summary": "Success"}]
        + [
            {"scenario": {"Mutant": item}, "summary": cargo_mutants_summaries[summary]}
            for item, summary in outcomes
        ],
        "total_mutants": len(outcomes),
        "caught": names.count("Caught"),
        "missed": names.count("Missed"),
        "timeout": names.count("Timeout"),
        "unviable": names.count("Unviable"),
    }


def standard(count: int) -> list[dict[str, object]]:
    return [mutant(group, line) for group in SOURCES for line in range(1, count + 1)]


def fixture(name: str) -> tuple[list[dict[str, object]], list[tuple[dict[str, object], str]], list[dict[str, str]]]:
    if name == "output_mismatch":
        return fixture("all_pass")
    if name == "integration_only":
        selected, outcomes, dispositions = fixture("all_pass")
        dispositions[0]["classification"] = "integration-only"
        dispositions[0]["observable_behavior"] = "the observable backend behavior is covered by a real-Core integration test"
        dispositions[0]["disposition"] = "the observable backend behavior is covered by a real-Core integration test"
        return selected, outcomes, dispositions
    if name == "all_pass":
        conversion = [
            mutant("conversion", 1),
            mutant("conversion", 2),
            mutant("conversion", 3),
            mutant("conversion", 4, "replace branch with false"),
        ]
        rest = [mutant(group, 1) for group in SOURCES if group != "conversion"]
        selected = conversion + rest
        statuses = ["Caught", "Caught", "Unviable", "Missed"] + ["Caught"] * len(rest)
        dispositions = [
            {
                "mutant": conversion[3]["name"],
                "classification": "equivalent",
                "observable_behavior": "the documented equivalent branch is unchanged",
                "disposition": "zero and positive values take the same branch",
            }
        ]
    elif name in {"group_boundary", "group_failure", "aggregate_failure"}:
        selected = standard(4)
        statuses = ["Caught"] * len(selected)
        if name == "aggregate_failure":
            for group in SOURCES:
                if group != "conversion":
                    item = next(item for item in selected if item["file"] == SOURCES[group] and item["span"]["start"]["line"] == 4)
                    statuses[selected.index(item)] = "Missed"
        else:
            loss = [item for item in selected if item["file"] == SOURCES["loss"]]
            statuses[selected.index(loss[-1])] = "Missed"
            if name == "group_failure":
                statuses[selected.index(loss[-2])] = "Missed"
        dispositions = []
    elif name == "timeout":
        selected = [mutant(group, 1) for group in SOURCES] + [mutant("loss", 2)]
        statuses = ["Caught"] * len(selected)
        statuses[-1] = "Timeout"
        dispositions = []
    elif name == "unviable":
        selected = [mutant(group, 1) for group in SOURCES] + [mutant("conversion", 2)]
        statuses = ["Caught"] * len(selected)
        statuses[-1] = "Unviable"
        dispositions = []
    elif name == "unscoreable":
        selected = [mutant(group, 1) for group in SOURCES]
        statuses = ["Unviable"] * len(selected)
        dispositions = []
    elif name == "incomplete":
        selected = [mutant(group, 1, "replace loss with none" if group == "loss" else "") for group in SOURCES]
        statuses = [(item, "Caught") for item in selected if item["file"] != SOURCES["loss"]]
        return selected, statuses, []
    else:
        raise ValueError(f"unknown fixture {name}")
    return selected, list(zip(selected, statuses, strict=True)), dispositions


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: create-rust-mutation-fixture.py <fixture> <output-dir>")
    output = Path(sys.argv[2])
    output.mkdir(parents=True)
    selected, outcomes, dispositions = fixture(sys.argv[1])
    actual = json.loads(json.dumps(selected))
    if sys.argv[1] == "output_mismatch":
        actual[0]["file"] = "src/output-mismatch.rs"
    write(output / "selected.json", selected)
    write(output / "mutants.json", actual)
    write(output / "outcomes.json", outcome_report(outcomes))
    write(output / "dispositions.json", {"schema_version": 1, "dispositions": dispositions})


if __name__ == "__main__":
    main()
