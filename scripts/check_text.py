#!/usr/bin/env python3
"""Text-resource gate: the Android/iOS-style locale bundles, kept honest.

GATES (exit 1 on any):

  * `resources/strings/*.toml` — same key set in every locale, no empty
    translation, every `{placeholder}` multiset identical across locales (a
    dropped placeholder renders a bare sentence at runtime), keys well formed;
  * `src/text/generated.rs` — exactly what `scripts/gen_text.py` would write
    (the checked-in accessors never drift from the bundles);
  * catalogue-key POSITIONS in `src/bin/gui` — `tr`/`trf` take a `K_…`
    resource constant, never an English literal; `UiText::of/with/with_args`
    and `Arg::Key` likewise. The English literal is no longer the key.

Known migration surface: `autoshade::text::english()` (a key's default text)
and the `From<&'static str>` bridge are allowed only in tests and in
`landing_worker.rs` (the leading-separator convention); anything else is
reported so a new English-text read does not slip in.

Run: py -3 scripts/check_text.py
"""

from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen_text  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
GUI = REPO / "src" / "bin" / "gui"
RESOURCES = REPO / "resources" / "strings"

# Files whose code may read a key's DEFAULT text through the migration bridge:
# the separator convention in `landing_worker.rs`, and the headless logic's
# own tests (which pin the wording of the sentences they produce). Everything
# else must render via `tr`.
ENGLISH_ALLOWED = {"landing_worker.rs", "mod.rs", "lens_blur.rs"}

KEY_RE = re.compile(r"^[a-z][a-z0-9_]*(\.[a-z0-9_]+)+$")


# ── offset-preserving scanning of Rust source (comments + strings masked) ──
def skip_ws(src: str, i: int) -> int:
    n = len(src)
    while i < n:
        c = src[i]
        if c in " \t\r\n":
            i += 1
        elif src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j < 0 else j + 1
        elif src.startswith("/*", i):
            j = src.find("*/", i)
            i = n if j < 0 else j + 2
        else:
            break
    return i


def string_start(src: str, i: int) -> int | None:
    j = i
    if src.startswith("br", j):
        j += 2
    elif src.startswith('b"', j):
        j += 1
    if src.startswith("r", j):
        k = j + 1
        while k < len(src) and src[k] == "#":
            k += 1
        return k if k < len(src) and src[k] == '"' else None
    return j if j < len(src) and src[j] == '"' else None


def string_end(src: str, i: int) -> int:
    j = i
    if src.startswith("br", j):
        j += 2
    elif src.startswith('b"', j):
        j += 1
    if src.startswith("r", j):
        k = j + 1
        hashes = 0
        while k < len(src) and src[k] == "#":
            hashes += 1
            k += 1
        end = src.find('"' + "#" * hashes, k + 1)
        return end + len('"' + "#" * hashes)
    j += 1
    while j < len(src):
        c = src[j]
        if c == "\\":
            j += 2
            continue
        if c == '"':
            return j + 1
        j += 1
    raise AssertionError("unterminated string")


def char_end(src: str, i: int) -> int | None:
    if src[i] != "'":
        return None
    j = i + 1
    if j >= len(src):
        return None
    if src[j] == "\\":
        j += 2
        if j < len(src) and src[j] == "u":
            j = src.find("}", j)
            j = len(src) if j < 0 else j + 1
    else:
        j += 1
    return j + 1 if j < len(src) and src[j] == "'" else None


def mask(src: str) -> str:
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = " "
            i = j
            continue
        if src.startswith("/*", i):
            j = src.find("*/", i)
            j = n if j < 0 else j + 2
            for k in range(i, j):
                out[k] = " "
            i = j
            continue
        if src[i] == '"' or (src[i] in "rb" and string_start(src, i) is not None):
            end = string_end(src, i)
            for k in range(i, end):
                out[k] = " "
            i = end
            continue
        ce = char_end(src, i)
        if ce is not None:
            for k in range(i, ce):
                out[k] = " "
            i = ce
            continue
        i += 1
    return "".join(out)


def scan_to_arg2(src: str, i: int) -> int:
    depth = 0
    while i < len(src):
        if src[i] == '"' or (src[i] in "rb" and string_start(src, i) is not None):
            i = string_end(src, i)
            continue
        c = src[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == "," and depth == 0:
            return skip_ws(src, i + 1)
        i += 1
    return -1


def literal_positions() -> list[str]:
    """Catalogue-key positions that still hold a string literal."""
    bad: list[str] = []
    for path in sorted(GUI.glob("**/*.rs")):
        if path.name == "i18n.rs":
            continue
        rel = path.relative_to(GUI).as_posix()
        src = path.read_text(encoding="utf-8")
        masked = mask(src)
        for m in re.finditer(r"\b(trf?)\s*\(", masked):
            st = scan_to_arg2(src, m.end())
            if st >= 0 and string_start(src, st) is not None:
                line = src[:st].count("\n") + 1
                bad.append(f"{rel}:{line}: `{m.group(1)}(<…>, \"…\")` — a resource constant belongs here")
        for m in re.finditer(r"UiText::(of|with|with_args)\s*\(", masked):
            st = skip_ws(src, m.end())
            if string_start(src, st) is not None:
                line = src[:st].count("\n") + 1
                bad.append(f"{rel}:{line}: `UiText::{m.group(1)}(\"…\")` — a resource constant belongs here")
        for m in re.finditer(r"Arg::Key\s*\(", masked):
            st = skip_ws(src, m.end())
            if string_start(src, st) is not None:
                line = src[:st].count("\n") + 1
                bad.append(f"{rel}:{line}: `Arg::Key(\"…\")` — a resource constant belongs here")
    return bad


def english_reads() -> list[str]:
    """Production reads of a key's default text (the migration bridge)."""
    out: list[str] = []
    for path in sorted(GUI.glob("**/*.rs")):
        rel = path.relative_to(GUI).as_posix()
        if path.name == "i18n.rs" or rel.startswith("tests/") or rel == "tests.rs":
            continue
        if path.name in ENGLISH_ALLOWED:
            continue
        src = path.read_text(encoding="utf-8")
        for needle, why in (
            ("autoshade::text::english(", "a key's default text"),
            ("key_of_text(", "English-text identity"),
        ):
            if needle in src:
                line = src[: src.index(needle)].count("\n") + 1
                out.append(f"{rel}:{line}: {why} outside the allowed bridge")
    return out


def main() -> int:
    failures: list[str] = []

    keys, texts = gen_text.load()
    key_list = [k for k, _ in keys]
    dup = [k for k, n in Counter(key_list).items() if n > 1]
    if dup:
        failures.append(f"duplicate resource keys: {dup[:5]}")
    bad_keys = [k for k in key_list if not KEY_RE.match(k)]
    if bad_keys:
        failures.append(f"malformed resource keys: {bad_keys[:5]}")

    en = texts["en"]
    zh = texts["zh-Hans"]
    empty = [key_list[i] for i, text in enumerate(zh) if not text.strip()]
    if empty:
        failures.append(f"{len(empty)} untranslated resource(s), first: {empty[:3]}")
    mismatched = [
        key_list[i]
        for i in range(len(key_list))
        if Counter(gen_text.placeholders(en[i])) != Counter(gen_text.placeholders(zh[i]))
    ]
    if mismatched:
        failures.append(f"{len(mismatched)} placeholder mismatch(es), first: {mismatched[:3]}")

    # A legacy-codepage accident in a translation (U+FFFD, C1 controls) is a
    # mojibake value, not Chinese: the old audit's "real Chinese" pin.
    mojibake = [
        key_list[i]
        for i, text in enumerate(zh)
        if "\ufffd" in text or any(0x80 <= ord(c) <= 0x9F for c in text)
    ]
    if mojibake:
        failures.append(f"{len(mojibake)} mojibake translation(s), first: {mojibake[:3]}")

    expected = gen_text.render(keys, texts)
    current = gen_text.GENERATED.read_text(encoding="utf-8")
    if current != expected:
        failures.append("src/text/generated.rs is stale — run `py -3 scripts/gen_text.py`")

    bad_positions = literal_positions()
    failures.extend(bad_positions[:20])
    if len(bad_positions) > 20:
        failures.append(f"…and {len(bad_positions) - 20} more literal key position(s)")
    failures.extend(english_reads())

    if failures:
        print(f"text gate: {len(failures)} failure(s)")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    rationale = sum(1 for key, _ in keys if key.startswith("rationale."))
    values = sum(1 for key, _ in keys if key.startswith("value."))
    print(
        f"text gate OK: {len(keys)} resources ({rationale} rationale, {values} values), "
        f"{len(keys)} keys in 2 locales, placeholders aligned, generated in step, "
        "no literal key positions"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
