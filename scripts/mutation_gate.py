#!/usr/bin/env python3
"""Thin cargo-mutants wrapper for the agent coding gate.

The gate is deliberately about mutation outcomes, not function inventories:

* `--worktree` tests the mutation lines in the current worktree diff.
* `--in-diff FILE` tests the mutants that overlap a unified diff.
* `--files FILE...` tests whole files explicitly.
* `--full` is the explicit, slow repository-wide run.

Examples:
    python3 scripts/mutation_gate.py --worktree --jobs 2
    git diff --unified=0 origin/main...HEAD > /tmp/change.diff
    python3 scripts/mutation_gate.py --in-diff /tmp/change.diff --jobs 2 --in-place
    python3 scripts/mutation_gate.py --full --shard 1/8 --jobs 2
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path


REPO = Path(__file__).resolve().parent.parent
BASE_ARGS = (
    "cargo",
    "mutants",
    "--no-config",
    "--cargo-arg=--locked",
    "--features",
    "gui",
    "--cargo-test-arg=--all-targets",
    "--no-times",
    "--colors",
    "never",
)


def git_lines(*args: str) -> list[str]:
    result = subprocess.run(
        ["git", *args],
        cwd=REPO,
        capture_output=True,
        text=True,
        timeout=120,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or f"git {' '.join(args)} failed")
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def rust_files(paths: list[str]) -> list[str]:
    return sorted({path for path in paths if path.endswith(".rs")})


def test_rust_files(paths: list[str]) -> list[str]:
    return sorted(
        path
        for path in paths
        if path.startswith("tests/")
        or path.endswith("/tests.rs")
        or "/tests/" in path
    )


def production_rust_files(paths: list[str]) -> list[str]:
    return sorted(
        path
        for path in paths
        if path.startswith("src/") and path not in test_rust_files(paths)
    )


def worktree_rust_files() -> list[str]:
    paths = [
        *git_lines("diff", "--name-only", "--diff-filter=ACMRTUXB"),
        *git_lines("diff", "--cached", "--name-only", "--diff-filter=ACMRTUXB"),
        *git_lines("ls-files", "--others", "--exclude-standard"),
    ]
    return rust_files(paths)


def write_worktree_diff(path: Path) -> tuple[list[str], list[str]]:
    changed = worktree_rust_files()
    files = production_rust_files(changed)
    if not files:
        return files, changed

    untracked = set(git_lines("ls-files", "--others", "--exclude-standard")) & set(files)
    tracked = [file for file in files if file not in untracked]
    chunks: list[str] = []
    if tracked:
        result = subprocess.run(
            ["git", "diff", "--unified=0", "HEAD", "--", *tracked],
            cwd=REPO,
            capture_output=True,
            text=True,
            timeout=120,
        )
        if result.returncode != 0:
            raise RuntimeError(result.stderr.strip() or "git diff failed")
        chunks.append(result.stdout)
    for file in sorted(untracked):
        result = subprocess.run(
            ["git", "diff", "--no-index", "--unified=0", "/dev/null", file],
            cwd=REPO,
            capture_output=True,
            text=True,
            timeout=120,
        )
        if result.returncode not in (0, 1):
            raise RuntimeError(result.stderr.strip() or f"git diff failed for {file}")
        chunks.append(result.stdout)
    path.write_text("".join(chunks), encoding="utf-8")
    return files, changed


def diff_rust_files(path: Path) -> list[str]:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise RuntimeError(f"could not read diff {path}: {error}") from error
    paths: list[str] = []
    for line in lines:
        if not line.startswith("+++ "):
            continue
        path_text = line[4:].strip().split("\t", 1)[0]
        if path_text == "/dev/null":
            continue
        if path_text.startswith("b/"):
            path_text = path_text[2:]
        paths.append(path_text)
    return rust_files(paths)


def configured_mutation_files() -> list[str]:
    value = os.environ.get("AUTOSHADE_MUTATION_FILES", "").strip()
    if not value:
        return []
    files = production_rust_files(value.split())
    if len(files) != len(set(value.split())):
        raise RuntimeError(
            "AUTOSHADE_MUTATION_FILES must contain only production Rust files under src/ "
            "(not tests)"
        )
    return files


def cargo_mutants_command(args: argparse.Namespace, *, list_only: bool) -> list[str]:
    command = [*BASE_ARGS, "--jobs", str(args.jobs)]
    if args.test_tool != "cargo":
        command.extend(["--test-tool", args.test_tool])
    if args.baseline:
        command.extend(["--baseline", args.baseline])
    if args.in_place:
        command.append("--in-place")
    if args.in_diff:
        command.extend(["--in-diff", args.in_diff])
    if args.regex:
        command.extend(["--re", args.regex])
    for file in args.files:
        command.extend(["--file", file])
    if args.shard:
        command.extend(["--shard", args.shard])
    for test_arg in args.test_args:
        command.extend(["--cargo-test-arg", test_arg])
    if list_only:
        command.append("--list")
    return command


def run_command(command: list[str], *, capture: bool = False) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            command,
            cwd=REPO,
            capture_output=capture,
            text=True,
            timeout=24 * 60 * 60,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise RuntimeError(f"could not run {command[0]} {command[1]}: {error}") from error


def fail(message: str) -> int:
    print(f"mutation gate: {message}", file=sys.stderr)
    return 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--worktree", action="store_true", help="test mutation lines in the current worktree diff")
    mode.add_argument("--in-diff", metavar="FILE", help="test mutants overlapping a unified diff")
    mode.add_argument("--files", nargs="+", metavar="FILE", help="test whole production Rust files")
    mode.add_argument("--full", action="store_true", help="run the whole repository mutation suite")
    parser.add_argument("--jobs", type=int, default=max(1, min(4, os.cpu_count() or 1)))
    parser.add_argument("--test-tool", choices=("cargo", "nextest"), default="cargo")
    parser.add_argument("--baseline", choices=("run", "skip"), default="run")
    parser.add_argument("--shard", help="full-run shard, for example 1/8")
    parser.add_argument("--re", dest="regex", help="only test mutants whose listed name matches this regex")
    parser.add_argument(
        "--test-arg",
        dest="test_args",
        action="append",
        default=[],
        help="extra cargo test argument, repeatable (for example --ignored)",
    )
    parser.add_argument("--in-place", action="store_true", help="let cargo-mutants edit the checkout; intended for CI")
    parser.add_argument("--list", action="store_true", help="list selected mutants without running them")
    args = parser.parse_args()

    if args.shard and not args.full:
        return fail("--shard is only valid with --full")
    if args.in_place and args.worktree:
        return fail("--in-place is not allowed with --worktree")

    changed_files: list[str] | None = None
    changed_tests: list[str] = []
    worktree_temp = None
    try:
        if args.worktree:
            worktree_temp = tempfile.TemporaryDirectory(prefix="autoshade-worktree-")
            diff_path = Path(worktree_temp.name) / "change.diff"
            files, all_files = write_worktree_diff(diff_path)
            changed_tests = test_rust_files(all_files)
            if not files:
                if not changed_tests:
                    print("mutation gate: no changed Rust files; nothing to mutate")
                    return 0
                files = configured_mutation_files()
                if not files:
                    return fail(
                        "test-only Rust changes require AUTOSHADE_MUTATION_FILES to name "
                        "the production behavior they protect"
                    )
                args.files = files
                changed_files = files
                args.in_diff = None
            else:
                changed_files = files
                args.files = []
                args.in_diff = str(diff_path)
        elif args.in_diff:
            all_files = diff_rust_files(Path(args.in_diff))
            files = production_rust_files(all_files)
            changed_tests = test_rust_files(all_files)
            if not files:
                if not changed_tests:
                    print("mutation gate: diff contains no Rust files; nothing to mutate")
                    return 0
                files = configured_mutation_files()
                if not files:
                    return fail(
                        "test-only Rust changes require AUTOSHADE_MUTATION_FILES to name "
                        "the production behavior they protect"
                    )
                args.files = files
                changed_files = files
                args.in_diff = None
            else:
                changed_files = files
                args.files = []
        elif args.files:
            args.files = production_rust_files(args.files)
            if not args.files:
                return fail("--files contained no production Rust files")
            changed_files = args.files
        else:
            args.files = []
    except RuntimeError as error:
        return fail(str(error))

    list_command = cargo_mutants_command(args, list_only=True)
    try:
        listed = run_command(list_command, capture=True)
    except RuntimeError as error:
        return fail(str(error))
    if listed.returncode != 0:
        print(listed.stdout, end="")
        print(listed.stderr, end="", file=sys.stderr)
        return fail("cargo-mutants could not list mutants")

    selected = [line for line in listed.stdout.splitlines() if line.strip()]
    if not selected:
        if args.full:
            return fail("no mutants were generated for the repository")
        if changed_tests:
            try:
                files = configured_mutation_files()
            except RuntimeError as error:
                return fail(str(error))
            if files:
                args.in_diff = None
                args.files = files
                try:
                    configured = run_command(
                        cargo_mutants_command(args, list_only=True),
                        capture=True,
                    )
                except RuntimeError as error:
                    return fail(str(error))
                if configured.returncode != 0:
                    print(configured.stdout, end="")
                    print(configured.stderr, end="", file=sys.stderr)
                    return fail("cargo-mutants could not list the configured behavior scope")
                selected = [line for line in configured.stdout.splitlines() if line.strip()]
                if not selected:
                    return fail("AUTOSHADE_MUTATION_FILES generated no mutation targets")
                print(
                    "mutation gate: changed lines generated no mutants; "
                    "testing the explicitly configured behavior files"
                )
            else:
                return fail(
                    "changed lines generated no mutants; because tests also changed, set "
                    "AUTOSHADE_MUTATION_FILES to the production behavior they protect"
                )
        else:
            return fail(
                "changed production lines generated no mutants; cargo-mutants cannot verify "
                "this change, so provide a separate targeted fault example"
            )
    if args.list:
        print("\n".join(selected))
        return 0

    print(f"mutation gate: testing {len(selected)} mutant(s)")
    try:
        result = run_command(cargo_mutants_command(args, list_only=False))
    except RuntimeError as error:
        return fail(str(error))
    if result.returncode == 0:
        print("mutation gate: cargo-mutants accepted this run; inspect outcomes for unviable mutants")
        return 0
    if result.returncode == 2:
        return fail("surviving mutant(s): review behavioral relevance and missing assertions")
    if result.returncode == 3:
        return fail("mutation test timed out")
    if result.returncode == 4:
        return fail("baseline tests failed; mutation results are not meaningful")
    return fail(f"cargo-mutants failed with exit code {result.returncode}")


if __name__ == "__main__":
    raise SystemExit(main())
