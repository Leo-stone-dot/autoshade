#!/usr/bin/env python3
"""Reject tests that lock source shape or bypass deterministic isolation."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SUPPORT_FILES = {
    (ROOT / "tests" / "support" / "mod.rs").resolve(),
    (ROOT / "src/bin/gui/tests/support.rs").resolve(),
}
DEFAULT_TEST_FILES = [
    ROOT / "src/bin/gui/tests.rs",
    ROOT / "src/bin/gui/tests/support.rs",
    *sorted((ROOT / "tests").rglob("*.rs")),
]

FORBIDDEN = [
    ("source text helper", re.compile(r"\b(?:source_text|source_before_tests|gui_source)\b")),
    ("source directory scan", re.compile(r"read_dir\s*\([^\n]*\bsrc\b")),
    ("source file read", re.compile(r"read_to_string\s*\([^\n]*\.rs\b")),
    ("source-shape assertion", re.compile(r"contains\s*\(\s*[\"']fn\s")),
    ("vacuous assertion", re.compile(r"assert!\s*\(\s*true\s*\)")),
    ("self-written skip line", re.compile(r"\b(?:println|eprintln)!\s*\(\s*[\"']\s*SKIP\b")),
]

EMBEDDED_SOURCE = re.compile(
    r"include_(?:str|bytes)!\s*\(\s*[\"'](?P<path>[^\"']+)[\"']"
)
SOURCE_FILE_LITERAL = re.compile(r"[\"'](?P<path>[^\"']*\.rs)[\"']")
PROCESS_ENV = re.compile(r"\b(?:std::env::)?(?:set_var|remove_var)\s*\(")
SLEEP = re.compile(r"\b(?:std::thread::)?sleep\s*\(")
FIXED_TEMP = re.compile(r"temp_dir\(\)\s*\.join\(\s*[\"'](?P<name>[^\"']*)[\"']\s*\)")


def strip_comments(text: str) -> str:
    """Remove Rust comments while preserving string literals and line layout."""
    out: list[str] = []
    i = 0
    block_depth = 0
    quote: str | None = None
    escaped = False
    while i < len(text):
        ch = text[i]
        nxt = text[i + 1] if i + 1 < len(text) else ""
        if quote is not None:
            out.append(ch)
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == quote:
                quote = None
            i += 1
            continue
        if block_depth:
            if ch == "/" and nxt == "*":
                block_depth += 1
                out.extend("  ")
                i += 2
            elif ch == "*" and nxt == "/":
                block_depth -= 1
                out.extend("  ")
                i += 2
            else:
                out.append("\n" if ch == "\n" else " ")
                i += 1
            continue
        if ch in "\"'":
            quote = ch
            out.append(ch)
            i += 1
            continue
        if ch == "/" and nxt == "/":
            while i < len(text) and text[i] != "\n":
                out.append(" ")
                i += 1
            continue
        if ch == "/" and nxt == "*":
            block_depth = 1
            out.extend("  ")
            i += 2
            continue
        out.append(ch)
        i += 1
    return "".join(out)


def scan_text(path: Path, text: str) -> list[str]:
    findings: list[str] = []
    text = strip_comments(text)
    for label, pattern in FORBIDDEN:
        for match in pattern.finditer(text):
            line = text.count("\n", 0, match.start()) + 1
            findings.append(f"{path}:{line}: {label}: {match.group(0)}")
    for match in EMBEDDED_SOURCE.finditer(text):
        embedded = match.group("path")
        if embedded.endswith(".rs") or embedded.startswith("src/") or "/src/" in embedded:
            line = text.count("\n", 0, match.start()) + 1
            findings.append(f"{path}:{line}: embedded Rust source: {embedded}")
    for match in SOURCE_FILE_LITERAL.finditer(text):
        line = text.count("\n", 0, match.start()) + 1
        findings.append(f"{path}:{line}: Rust source path literal: {match.group('path')}")
    for label, pattern in (
        ("unbounded sleep", SLEEP),
        ("fixed shared temp directory", FIXED_TEMP),
    ):
        for match in pattern.finditer(text):
            line = text.count("\n", 0, match.start()) + 1
            findings.append(f"{path}:{line}: {label}: {match.group(0)}")
    if path.resolve() not in SUPPORT_FILES:
        for match in PROCESS_ENV.finditer(text):
            line = text.count("\n", 0, match.start()) + 1
            findings.append(
                f"{path}:{line}: process environment write outside a test support module: "
                f"{match.group(0)}"
            )
    return findings


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", help="test files to scan; defaults to the repository test suite")
    parser.add_argument("--self-test", action="store_true", help="verify the scanner catches a forbidden example")
    args = parser.parse_args()

    if args.self_test:
        samples = {
            "embedded source": 'fn synthetic() { let _ = include_str!("src/lib.rs"); }',
            "indirect source path": 'let path = "src/lib.rs"; let _ = std::fs::read_to_string(path);',
            "vacuous assertion": "fn synthetic() { assert!(true); }",
            "self-written skip line": 'fn synthetic() { println!("SKIP: synthetic"); }',
            "unbounded sleep": 'fn synthetic() { std::thread::sleep(std::time::Duration::from_secs(1)); }',
            "environment write": 'fn synthetic() { unsafe { std::env::set_var("X", "1"); } }',
            "fixed temp directory": 'let _ = std::env::temp_dir().join("shared-test-dir");',
        }
        if any(not scan_text(Path("<self-test>"), text) for text in samples.values()):
            print("test policy self-test: scanner missed a forbidden example", file=sys.stderr)
            return 1
        allowed = [
            'let _ = include_str!("fixtures/sample.xmp");',
            'let _ = include_bytes!("fixtures/sample.png");',
            '// source_text in a comment is documentation, not a test assertion',
            'let _ = std::env::temp_dir().join(format!("autoshade-{}-{}", process::id(), 1));',
        ]
        if any(scan_text(Path("<self-test-allowed>"), text) for text in allowed):
            print("test policy self-test: scanner rejected a non-source fixture or comment", file=sys.stderr)
            return 1
        print("test policy self-test: forbidden source/env/timing patterns rejected; allowed fixtures pass")
        return 0

    paths = [Path(path) for path in args.paths] if args.paths else DEFAULT_TEST_FILES
    if not paths:
        print("test policy: no test files found", file=sys.stderr)
        return 1

    findings: list[str] = []
    for path in paths:
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as error:
            print(f"test policy: cannot read {path}: {error}", file=sys.stderr)
            return 1
        findings.extend(scan_text(path, text))

    if findings:
        print("test policy: forbidden test patterns:", file=sys.stderr)
        print("\n".join(findings), file=sys.stderr)
        return 1
    print(f"test policy OK: {len(paths)} test files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
