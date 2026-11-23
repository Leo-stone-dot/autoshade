#!/usr/bin/env python3
"""Verify that every test in the audited main snapshot has one disposition."""

from __future__ import annotations

import argparse
import csv
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
AUDIT = ROOT / "docs" / "main_test_case_audit.tsv"
MAIN_SNAPSHOT = "dce7e8a3594850bdaf6d05b2f50eb60dc6804414"
DISPOSITIONS = {"equivalent", "rewritten", "lint", "drop", "external-gate", "open"}
GATES = {"default", "explicit-or-feature"}
FN_AFTER_TEST = re.compile(
    r"(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("
)


def git(*args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=120,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or f"git {' '.join(args)} failed")
    return result.stdout


def snapshot_tests() -> dict[tuple[str, str], int]:
    prefix = f"{MAIN_SNAPSHOT}:"
    files = [
        line.removeprefix(prefix)
        for line in git("grep", "-l", r"#\[test\]", MAIN_SNAPSHOT, "--", "*.rs").splitlines()
        if line
    ]
    tests: dict[tuple[str, str], int] = {}
    for path in files:
        text = git("show", f"{MAIN_SNAPSHOT}:{path}")
        at = 0
        while True:
            marker = text.find("#[test]", at)
            if marker < 0:
                break
            match = FN_AFTER_TEST.search(text, marker, min(len(text), marker + 1200))
            if match is None:
                raise RuntimeError(f"{path}: no function found after #[test] at byte {marker}")
            name = match.group(1)
            line = text.count("\n", 0, match.start()) + 1
            key = (path, name)
            if key in tests:
                raise RuntimeError(f"{path}: duplicate test function {name}")
            tests[key] = line
            at = match.end()
    return tests


def audit_rows() -> tuple[dict[tuple[str, str], dict[str, str]], list[str]]:
    required = [
        "main_path",
        "line",
        "case",
        "assertions",
        "gate",
        "disposition",
        "current_evidence",
        "reason",
    ]
    with AUDIT.open(encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        if reader.fieldnames != required:
            raise RuntimeError(
                f"{AUDIT}: columns must be {required}, got {reader.fieldnames}"
            )
        rows: dict[tuple[str, str], dict[str, str]] = {}
        findings: list[str] = []
        for number, row in enumerate(reader, start=2):
            key = (row["main_path"], row["case"])
            if key in rows:
                findings.append(f"{AUDIT}:{number}: duplicate case {key[0]}::{key[1]}")
                continue
            rows[key] = row
            try:
                if int(row["line"]) < 1 or int(row["assertions"]) < 0:
                    raise ValueError
            except ValueError:
                findings.append(f"{AUDIT}:{number}: line/assertions must be non-negative integers")
            if row["gate"] not in GATES:
                findings.append(f"{AUDIT}:{number}: unknown gate {row['gate']!r}")
            if row["disposition"] not in DISPOSITIONS:
                findings.append(
                    f"{AUDIT}:{number}: unknown disposition {row['disposition']!r}"
                )
            if not row["reason"].strip():
                findings.append(f"{AUDIT}:{number}: missing reason")
            if row["disposition"] != "open" and not row["current_evidence"].strip():
                findings.append(
                    f"{AUDIT}:{number}: {row['disposition']} needs current_evidence"
                )
        return rows, findings


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--allow-open",
        action="store_true",
        help="permit unfinished rows while the audit goal is still active",
    )
    args = parser.parse_args()

    try:
        expected = snapshot_tests()
        rows, findings = audit_rows()
    except (OSError, RuntimeError) as error:
        print(f"main test audit: {error}", file=sys.stderr)
        return 1

    missing = sorted(set(expected) - set(rows))
    extra = sorted(set(rows) - set(expected))
    for path, name in missing:
        findings.append(f"missing: {path}::{name}")
    for path, name in extra:
        findings.append(f"not in {MAIN_SNAPSHOT}: {path}::{name}")
    for key in sorted(set(expected) & set(rows)):
        observed = int(rows[key]["line"])
        if observed != expected[key]:
            findings.append(
                f"{key[0]}::{key[1]}: line drift, audit={observed} snapshot={expected[key]}"
            )

    open_rows = [key for key, row in rows.items() if row["disposition"] == "open"]
    if open_rows and not args.allow_open:
        findings.append(
            f"{len(open_rows)} case(s) remain open; use --allow-open only during active audit work"
        )

    counts = Counter(row["disposition"] for row in rows.values())
    summary = ", ".join(f"{name}={counts[name]}" for name in sorted(counts))
    if findings:
        print("main test audit findings:", file=sys.stderr)
        print("\n".join(findings[:100]), file=sys.stderr)
        if len(findings) > 100:
            print(f"... {len(findings) - 100} more", file=sys.stderr)
        print(f"inventory: snapshot={len(expected)}, rows={len(rows)}, {summary}", file=sys.stderr)
        return 1

    print(f"main test audit OK: snapshot={len(expected)}, rows={len(rows)}, {summary}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
