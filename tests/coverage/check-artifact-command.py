#!/usr/bin/env python3
"""Assert the public validator contract by executing a supplied binary."""

import argparse
import json
import re
import subprocess
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    binary = args.executable.resolve()
    root = args.root.resolve()
    results = []

    def run(name, arguments, status, stdout=(), stderr=()):
        process = subprocess.run([str(binary), *map(str, arguments)],
                                 capture_output=True, text=True, timeout=30)
        failures = []
        if process.returncode != status:
            failures.append(f"expected exit {status}, observed {process.returncode}")
        for expected in stdout:
            if expected not in process.stdout:
                failures.append(f"stdout missing {expected!r}")
        for expected in stderr:
            if expected not in process.stderr:
                failures.append(f"stderr missing {expected!r}")
        if status == 0 and process.stderr:
            failures.append("successful validation wrote to stderr")
        results.append({"scenario": name, "exit_status": process.returncode,
                        "failures": failures, "stdout": process.stdout,
                        "stderr": process.stderr})
        return process.stdout

    def entries(path):
        return sum(bool(line.strip()) and not line.lstrip().startswith("#")
                   for line in path.read_text().splitlines())

    approved = root / "docs/3.39.0--4.1.1.xml"
    inventories = root / "docs/inventory"
    left = entries(inventories / "equilibrium-3.39.0.txt")
    right = entries(inventories / "equilibrium-4.1.1.txt")
    floor = entries(inventories / "equilibrium-3.39.0--4.1.1-imas-python-renames.tsv")
    report_fields = [
        f"IMAS-Python rename-only floor {direction}: {floor}/{floor} mappings served"
        for direction in ("3.39.0 -> 4.1.1", "4.1.1 -> 3.39.0")
    ] + [f"completeness 3.39.0 <-> 4.1.1: {left} + {right} inventory paths claimed"]
    for name, arguments in (("default-artifact", []), ("explicit-artifact", ["--artifact", approved])):
        output = run(name, arguments, 0, stdout=report_fields)
        for direction, total, minimum in (("3.39.0 -> 4.1.1", left, 346),
                                           ("4.1.1 -> 3.39.0", right, 339)):
            match = re.search(r"shim " + re.escape(direction) +
                              r": supported=(\d+), by rule=(\d+), by identity default=(\d+), "
                              r"deliberate refusal=(\d+), absent stored source=(\d+), total=(\d+)", output)
            if match is None:
                results[-1]["failures"].append(f"missing numeric report for {direction}")
                continue
            supported, rules, identity, refused, absent, observed_total = map(int, match.groups())
            if not (supported >= minimum and supported == rules + identity and
                    supported + refused + absent == observed_total == total):
                results[-1]["failures"].append(f"incorrect coverage counts for {direction}")

    prefix = "artifact coverage validation failed: "
    run("unknown-argument", ["--unknown"], 1, stderr=[prefix, "unrecognised argument `--unknown`"])
    run("missing-argument", ["--artifact"], 1, stderr=[prefix, "--artifact requires a path"])
    with tempfile.TemporaryDirectory(prefix="imas-validator-") as temporary:
        directory = Path(temporary)
        missing = directory / "missing.xml"
        run("missing-file", ["--artifact", missing], 1, stderr=[prefix, f"cannot read {missing}"])
        # A directory is unreadable as artifact text even when the test runs as root.
        run("unreadable-file", ["--artifact", directory], 1,
            stderr=[prefix, f"cannot read {directory}"])
        malformed = directory / "malformed.xml"
        malformed.write_text("<not-closed")
        run("malformed-artifact", ["--artifact", malformed], 1,
            stderr=[prefix, f"cannot load {malformed}"])
        run("below-floor", ["--artifact", root / "tests/fixtures/equilibrium-3.39.0--4.1.1-reduced.xml"],
            1, stdout=["shim 3.39.0 -> 4.1.1:", "IMAS-Python rename-only floor"],
            stderr=[prefix, "falls below the IMAS-Python rename-only floor"])
        incomplete = directory / "incomplete.xml"
        xml, replaced = re.subn(r'<rule id="drop-b-flux-pol-norm"[^<]*<fidelity[^<]*</rule>',
                                "", approved.read_text())
        if replaced != 1:
            raise RuntimeError("incomplete fixture must remove exactly one documented rule")
        incomplete.write_text(xml)
        run("incomplete-artifact", ["--artifact", incomplete], 1,
            stdout=["shim 3.39.0 -> 4.1.1:", "IMAS-Python rename-only floor"],
            stderr=[prefix, "is not complete against its own inventories", "DefaultAssumesMissingCounterpart"])

    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps({"executable": str(binary), "scenarios": results}, indent=2) + "\n")
    failures = [f"{result['scenario']}: {failure}" for result in results for failure in result["failures"]]
    print("\n".join(failures) if failures else f"{len(results)} artifact command scenarios passed")
    return bool(failures)


if __name__ == "__main__":
    raise SystemExit(main())
