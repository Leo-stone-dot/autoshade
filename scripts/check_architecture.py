#!/usr/bin/env python3
"""Mechanical architecture gate for invariants clippy cannot express well.

These are code-shape rules, not product-behavior tests. Each rule scans every
production Rust file in its scope and names the exact violation it rejects.
"""

from __future__ import annotations

import argparse
import re
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def strip_comments(text: str) -> str:
    """Remove Rust comments while preserving strings and line positions."""
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


def line_of(text: str, pos: int) -> int:
    return text.count("\n", 0, pos) + 1


def nearest_fn(text: str, pos: int) -> tuple[str, int] | None:
    found = None
    for match in re.finditer(r"\bfn\s+([A-Za-z_]\w*)\s*\(", text[:pos]):
        found = (match.group(1), match.start())
    return found


def following_lines(text: str, pos: int, count: int) -> str:
    end = pos
    for _ in range(count):
        newline = text.find("\n", end)
        if newline < 0:
            return text[pos:]
        end = newline + 1
    return text[pos:end]


def preceding_lines(text: str, pos: int, count: int) -> str:
    return "\n".join(text[:pos].splitlines()[-count:])


def production_rust_files(root: Path) -> list[Path]:
    src = root / "src"
    return sorted(
        path
        for path in src.rglob("*.rs")
        if "tests" not in path.relative_to(src).parts
        and not path.name.endswith("tests.rs")
    )


def under(path: Path, root: Path, rel: str) -> bool:
    return path.is_relative_to(root / rel)


def finding(path: Path, text: str, pos: int, rule: str, detail: str) -> str:
    try:
        shown = path.relative_to(ROOT)
    except ValueError:
        shown = path
    return f"{shown}:{line_of(text, pos)}: {rule}: {detail}"


def raw_image_plane_guards(root: Path) -> list[str]:
    findings: list[str] = []
    for path in production_rust_files(root):
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        for match in re.finditer(r"\.raw_image\s*\(", text):
            scope = nearest_fn(text, match.start())
            if scope is None:
                findings.append(finding(path, raw, match.start(), "raw-image-plane-guard", "raw_image call outside a named function"))
                continue
            name, start = scope
            if "guard_raw_plane_extent(" not in text[start:match.start()]:
                findings.append(finding(path, raw, match.start(), "raw-image-plane-guard", f"{name} reaches raw_image before measuring the plane"))
    return findings


def raw_develop_ceiling(root: Path) -> list[str]:
    findings: list[str] = []
    for path in production_rust_files(root):
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        for match in re.finditer(r"\.raw_image\s*\([^;]*?\bfalse\b", text, re.DOTALL):
            scope = nearest_fn(text, match.start())
            if scope is None:
                findings.append(finding(path, raw, match.start(), "raw-develop-ceiling", "real raw_image decompression outside a named function"))
                continue
            name, start = scope
            if "refuse_raw_develop_over_ceiling_for(" not in text[start:match.start()]:
                findings.append(finding(path, raw, match.start(), "raw-develop-ceiling", f"{name} decompresses before charging the develop ceiling"))
    return findings


def bounded_store_reads(root: Path) -> list[str]:
    findings: list[str] = []
    pattern = re.compile(r"std::fs::read_to_string\s*\(|std::fs::read\s*\(")
    for path in production_rust_files(root):
        if not (under(path, root, "src/store") or path == root / "src/segment.rs"):
            continue
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        for match in pattern.finditer(text):
            findings.append(finding(path, raw, match.start(), "store-capped-read", "store data must go through the bounded reader"))
    return findings


def engine_analyzer_boundary(root: Path) -> list[str]:
    findings: list[str] = []
    for path in production_rust_files(root):
        render_side = (
            path == root / "src/recipe.rs"
            or path == root / "src/render.rs"
            or under(path, root, "src/render")
            or under(path, root, "src/xmp")
        )
        if not render_side:
            continue
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        for match in re.finditer(r"\b(?:fit_field|freemask)\b", text):
            findings.append(finding(path, raw, match.start(), "engine-analyzer-boundary", "render/recipe/XMP must not depend on the analyzer"))
    return findings


def base_look_single_entry(root: Path) -> list[str]:
    findings: list[str] = []
    for path in production_rust_files(root):
        if path == root / "src/render/base_look.rs":
            continue
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        for match in re.finditer(r"camera_base_knots\s*\(", text):
            findings.append(finding(path, raw, match.start(), "base-look-single-entry", "callers use camera_base_look, never camera_base_knots"))
    for rel in ("src/pipeline.rs", "src/serve.rs", "src/bin/gui/open_actions.rs"):
        path = root / rel
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        if not re.search(r"camera_base_look\s*\(", text):
            findings.append(f"{rel}:1: base-look-single-entry: open path no longer enters through camera_base_look")
    return findings


def mask_paint_notification(root: Path) -> list[str]:
    findings: list[str] = []
    for path in production_rust_files(root):
        if not under(path, root, "src/bin/gui"):
            continue
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        for match in re.finditer(r"\bmask_paint\s*=", text):
            if "paint_mask_changed(" not in following_lines(text, match.end(), 8):
                findings.append(finding(path, raw, match.start(), "mask-paint-notification", "paint buffer mutation must notify the overlay"))
        for match in re.finditer(r"\bmask_dirty\s*=\s*true\b", text):
            scope = nearest_fn(text, match.start())
            if scope is None or scope[0] != "paint_mask_changed":
                findings.append(finding(path, raw, match.start(), "mask-paint-notification", "mask_dirty=true belongs to paint_mask_changed"))
    return findings


def function_body(text: str, name: str) -> tuple[int, int] | None:
    match = re.search(rf"\bfn\s+{re.escape(name)}\s*\(", text)
    if match is None:
        return None
    next_fn = re.search(r"\n\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+", text[match.end():])
    end = len(text) if next_fn is None else match.end() + next_fn.start()
    return match.start(), end


def store_advisory_metadata_lifecycle(root: Path) -> list[str]:
    findings: list[str] = []
    versions = root / "src/store/versions.rs"
    versions_raw = versions.read_text(encoding="utf-8")
    versions_text = strip_comments(versions_raw)
    path_body = function_body(versions_text, "version_meta_path")
    if path_body is None:
        findings.append(
            "src/store/versions.rs:1: store-advisory-metadata-lifecycle: "
            "version_meta_path is missing"
        )
    else:
        start, end = path_body
        if '".version-meta.json"' not in versions_text[start:end]:
            findings.append(
                finding(
                    versions,
                    versions_raw,
                    start,
                    "store-advisory-metadata-lifecycle",
                    "version metadata must keep its one advisory file name",
                )
            )

    write_body = function_body(versions_text, "write_version_meta_unlocked")
    if write_body is None:
        findings.append(
            "src/store/versions.rs:1: store-advisory-metadata-lifecycle: "
            "write_version_meta_unlocked is missing"
        )
    else:
        start, end = write_body
        scope = versions_text[start:end]
        if "durable_write(" not in scope or "publish_json_sidecar(" in scope:
            findings.append(
                finding(
                    versions,
                    versions_raw,
                    start,
                    "store-advisory-metadata-lifecycle",
                    "advisory metadata must publish directly, never as a generational sidecar",
                )
            )

    forbidden_scopes = (
        ("src/store/key.rs", "adoption_skips"),
        ("src/store/snapshot.rs", "recover_orphan_baks_unlocked"),
        ("src/store/clear.rs", "clear_sweep_in"),
        ("src/store/commit.rs", "apply_commit_members"),
    )
    for rel, name in forbidden_scopes:
        path = root / rel
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        body = function_body(text, name)
        if body is None:
            findings.append(
                f"{rel}:1: store-advisory-metadata-lifecycle: {name} is missing"
            )
            continue
        start, end = body
        scope = text[start:end]
        if "version_meta_path" in scope or '".version-meta.json"' in scope:
            findings.append(
                finding(
                    path,
                    raw,
                    start,
                    "store-advisory-metadata-lifecycle",
                    f"{name} must not make advisory metadata generational or non-adoptable",
                )
            )
    return findings


def store_root_platform_contract(root: Path) -> list[str]:
    path = root / "src/store/root.rs"
    raw = path.read_text(encoding="utf-8")
    text = strip_comments(raw)
    required = (
        (
            r'#\s*\[\s*cfg\s*\(\s*target_os\s*=\s*"macos"\s*\)\s*\]\s*'
            r'pub\s*\(\s*crate\s*\)\s+const\s+STORE_DIR_NAME\s*:\s*&str\s*=\s*"AutoShade"\s*;',
            "macOS store directory must use the application display name",
        ),
        (
            r'#\s*\[\s*cfg\s*\(\s*not\s*\(\s*target_os\s*=\s*"macos"\s*\)\s*\)\s*\]\s*'
            r'pub\s*\(\s*crate\s*\)\s+const\s+STORE_DIR_NAME\s*:\s*&str\s*=\s*"autoshade"\s*;',
            "non-macOS store directory must keep the lowercase name",
        ),
        (
            r'pub\s*\(\s*super\s*\)\s+const\s+ADOPT_PRE_RENAME\s*:\s*bool\s*=\s*'
            r'!\s*cfg\s*!\s*\(\s*target_os\s*=\s*"macos"\s*\)\s*;',
            "pre-rename adoption must stay disabled only on macOS",
        ),
        (
            r'\.join\s*\(\s*"Library"\s*\)\s*\.join\s*\(\s*"Application Support"\s*\)',
            "macOS store root must remain under Library/Application Support",
        ),
    )
    findings: list[str] = []
    for pattern, detail in required:
        if re.search(pattern, text, re.DOTALL) is None:
            findings.append(
                finding(
                    path,
                    raw,
                    0,
                    "store-root-platform-contract",
                    detail,
                )
            )
    return findings


def server_heavy_slots(root: Path) -> list[str]:
    findings: list[str] = []
    path = root / "src/serve.rs"
    raw = path.read_text(encoding="utf-8")
    text = strip_comments(raw)
    for name in ("api_export", "api_download"):
        body = function_body(text, name)
        if body is None:
            findings.append(f"{path.relative_to(root)}:1: server-heavy-slot: {name} is missing")
            continue
        start, end = body
        if "heavy()" not in text[start:end]:
            findings.append(finding(path, raw, start, "server-heavy-slot", f"{name} does not take the shared full-resolution slot"))
    return findings


def generative_slot_scope(root: Path) -> list[str]:
    findings: list[str] = []
    path = root / "src/generative.rs"
    raw = path.read_text(encoding="utf-8")
    text = strip_comments(raw)
    body = function_body(text, "edit_onto")
    if body is None:
        return [f"{path.relative_to(root)}:1: generative-slot-scope: edit_onto is missing"]
    start, end = body
    scope = text[start:end]
    first = scope.find("let heavy = crate::full_res_slot();")
    released = scope.find("drop(heavy);")
    model = scope.find("call_images_edit(")
    composite = scope.find("let _heavy = crate::full_res_slot();")
    if min(first, released, model, composite) < 0 or not (first < released < model < composite):
        findings.append(finding(path, raw, start, "generative-slot-scope", "edit_onto must hold the slot only around local work and reacquire it for the composite"))
    return findings


def heal_slot_scope(root: Path) -> list[str]:
    findings: list[str] = []
    path = root / "src/retouch.rs"
    raw = path.read_text(encoding="utf-8")
    text = strip_comments(raw)
    body = function_body(text, "heal_and_save")
    if body is None:
        return [f"{path.relative_to(root)}:1: heal-slot-scope: heal_and_save is missing"]
    start, end = body
    scope = text[start:end]
    slot = scope.find("crate::full_res_slot()")
    first_alloc = min(
        [pos for pos in (scope.find("split_alpha("), scope.find("heal_image("), scope.find("save_master(")) if pos >= 0]
        or [-1]
    )
    if slot < 0 or first_alloc < 0 or slot > first_alloc:
        findings.append(finding(path, raw, start, "heal-slot-scope", "heal_and_save must take the slot before its allocation phase"))
    return findings


def linear_ramp_single_definition(root: Path) -> list[str]:
    findings: list[str] = []
    definitions: list[tuple[Path, int]] = []
    for path in production_rust_files(root):
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        for match in re.finditer(r"\bfn\s+linear_ramp_coverage\s*\(", text):
            definitions.append((path, match.start()))
    if len(definitions) != 1:
        findings.append(f"src:1: linear-ramp-single-definition: expected one definition, found {len(definitions)}")
    elif definitions[0][0] != root / "src/render.rs":
        findings.append(finding(definitions[0][0], definitions[0][0].read_text(encoding="utf-8"), definitions[0][1], "linear-ramp-single-definition", "the one ramp definition belongs in render.rs"))
    subzones = root / "src/fit_zoned/subzones.rs"
    raw = subzones.read_text(encoding="utf-8")
    text = strip_comments(raw)
    if "render::linear_ramp_coverage(" not in text:
        findings.append(f"{subzones.relative_to(root)}:1: linear-ramp-single-definition: subzone coverage no longer uses the shared ramp")
    return findings


def matching_brace(text: str, open_pos: int) -> int:
    depth = 0
    for pos in range(open_pos, len(text)):
        if text[pos] == "{":
            depth += 1
        elif text[pos] == "}":
            depth -= 1
            if depth == 0:
                return pos
    return len(text) - 1


def edit_recipe_fields(root: Path) -> set[str]:
    text = strip_comments((root / "src/recipe.rs").read_text(encoding="utf-8"))
    match = re.search(r"\bpub\s+struct\s+EditRecipe\s*\{", text)
    if match is None:
        return set()
    open_pos = text.find("{", match.start())
    body = text[open_pos + 1 : matching_brace(text, open_pos)]
    return set(re.findall(r"\bpub\s+([a-z_]\w*)\s*:", body))


def recipe_control_names(root: Path) -> set[str]:
    text = strip_comments((root / "src/advisor/catalogue.rs").read_text(encoding="utf-8"))
    start = text.find("pub const RECIPE_CONTROLS")
    end = text.find("pub const LOCAL_CONTROLS", start)
    if start < 0 or end < 0:
        return set()
    return set(re.findall(r'name:\s*"([^"]+)"', text[start:end]))


def literal_fields(body: str) -> set[str]:
    fields: set[str] = set()
    depth = 0
    pos = 0
    identifier = re.compile(r"[a-z_]\w*")
    while pos < len(body):
        ch = body[pos]
        if ch == "{":
            depth += 1
            pos += 1
            continue
        if ch == "}":
            depth -= 1
            pos += 1
            continue
        match = identifier.match(body, pos)
        if match is None:
            pos += 1
            continue
        after = match.end()
        while after < len(body) and body[after].isspace():
            after += 1
        if depth == 0 and after < len(body) and body[after] == ":" and body[after : after + 2] != "::":
            fields.add(match.group(0))
        pos = match.end()
    return fields


def gui_control_registry(root: Path) -> list[str]:
    findings: list[str] = []
    registered = recipe_control_names(root)
    recipe_fields = edit_recipe_fields(root)
    for path in production_rust_files(root):
        if not under(path, root, "src/bin/gui"):
            continue
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        for match in re.finditer(r"\bEditRecipe\s*\{", text):
            open_pos = text.find("{", match.start())
            body = text[open_pos + 1 : matching_brace(text, open_pos)]
            for field in sorted(literal_fields(body) - registered):
                findings.append(finding(path, raw, open_pos, "gui-control-registry", f"EditRecipe field {field!r} is not in RECIPE_CONTROLS"))
        for match in re.finditer(r"\.\s*([a-z_]\w*)\s*=", text):
            field = match.group(1)
            if field in recipe_fields and field not in registered:
                findings.append(finding(path, raw, match.start(), "gui-control-registry", f"GUI assigns unregistered EditRecipe field {field!r}"))
    return findings


def gui_logic_is_headless(root: Path) -> list[str]:
    findings: list[str] = []
    calls = ("tr(", "trf(", "spawn_worker(")
    names = ("egui::", "AutoShadeApp", "rfd::", "Msg::", "ToastKind", "std::thread")
    logic = root / "src/bin/gui/logic"
    if not logic.exists():
        return findings
    for path in sorted(logic.rglob("*.rs")):
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        for needle in (*calls, *names):
            for match in re.finditer(re.escape(needle), text):
                findings.append(
                    finding(
                        path,
                        raw,
                        match.start(),
                        "gui-logic-headless",
                        f"logic reaches the view/runtime through {needle!r}",
                    )
                )
    return findings


RULES = (
    raw_image_plane_guards,
    raw_develop_ceiling,
    bounded_store_reads,
    engine_analyzer_boundary,
    base_look_single_entry,
    mask_paint_notification,
    store_advisory_metadata_lifecycle,
    store_root_platform_contract,
    server_heavy_slots,
    generative_slot_scope,
    heal_slot_scope,
    linear_ramp_single_definition,
    gui_control_registry,
    gui_logic_is_headless,
)


def run_rules(root: Path) -> list[str]:
    findings: list[str] = []
    for rule in RULES:
        findings.extend(rule(root))
    return findings


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="autoshade-architecture-") as temp:
        root = Path(temp)
        write(root / "src/decode.rs", "fn door(d: D) { d.raw_image(); }")
        write(root / "src/render/source.rs", "fn develop(d: D) { d.raw_image(src, params, false); }")
        write(root / "src/store/sidecar.rs", "fn read(p: &Path) { let _ = std::fs::read_to_string(p); }")
        write(root / "src/render.rs", "fn leak() { let _ = fit_field::answer(); }")
        write(root / "src/pipeline.rs", "fn open() { camera_base_knots(); camera_base_look(); }")
        write(root / "src/serve.rs", "fn open() { camera_base_look(); }")
        write(root / "src/bin/gui/open_actions.rs", "fn open() { camera_base_look(); }")
        write(root / "src/render/source.rs", "fn develop(d: D) { d.raw_image(src, params, false); }")
        write(root / "src/bin/gui/masks.rs", "fn edit() { mask_paint = Some(x); mask_dirty = true; }")
        write(
            root / "src/store/versions.rs",
            'fn version_meta_path(dev: &Path) -> PathBuf { dev.join(".version-meta.json") }\n'
            "fn write_version_meta_unlocked() { publish_json_sidecar(); }",
        )
        write(
            root / "src/store/key.rs",
            'fn adoption_skips(name: &str) -> bool { name == ".version-meta.json" }',
        )
        write(
            root / "src/store/snapshot.rs",
            "fn recover_orphan_baks_unlocked() { version_meta_path(); }",
        )
        write(
            root / "src/store/clear.rs",
            "fn clear_sweep_in() { version_meta_path(); }",
        )
        write(
            root / "src/store/commit.rs",
            "fn apply_commit_members() { version_meta_path(); }",
        )
        write(
            root / "src/store/root.rs",
            '#[cfg(target_os = "macos")] pub(crate) const STORE_DIR_NAME: &str = "autoshade";\n'
            '#[cfg(not(target_os = "macos"))] pub(crate) const STORE_DIR_NAME: &str = "AutoShade";\n'
            'pub(super) const ADOPT_PRE_RENAME: bool = true;\n'
            'fn mac_root(home: &Path) { home.join("Library"); }',
        )
        write(root / "src/serve.rs", "fn api_export() { render(); }\nfn api_download() { render(); }")
        write(root / "src/generative.rs", "fn edit_onto() { let heavy = crate::full_res_slot(); call_images_edit(); }")
        write(root / "src/retouch.rs", "fn heal_and_save() { split_alpha(); }")
        write(root / "src/render.rs", "fn leak() { let _ = fit_field::answer(); }\nfn linear_ramp_coverage(t: f32) -> f32 { t }\nfn linear_ramp_coverage(t: f32) -> f32 { t }")
        write(root / "src/fit_zoned/subzones.rs", "fn coverage(t: f32) -> f32 { t }")
        write(root / "src/advisor/catalogue.rs", 'pub const RECIPE_CONTROLS: [Control; 1] = [Control { name: "exposure_ev" }];\npub const LOCAL_CONTROLS: [Control; 0] = [];')
        write(root / "src/recipe.rs", "pub struct EditRecipe { pub exposure_ev: f32, pub mystery: f32 }")
        write(root / "src/bin/gui/panel.rs", "fn edit(r: &mut EditRecipe) { r.mystery = 1.0; }")
        write(root / "src/bin/gui/logic/gallery.rs", "fn leak() { egui::Button::new(\"open\"); }")

        bad = "\n".join(run_rules(root))
        expected = (
            "raw-image-plane-guard",
            "raw-develop-ceiling",
            "store-capped-read",
            "engine-analyzer-boundary",
            "base-look-single-entry",
            "mask-paint-notification",
            "store-advisory-metadata-lifecycle",
            "store-root-platform-contract",
            "server-heavy-slot",
            "generative-slot-scope",
            "heal-slot-scope",
            "linear-ramp-single-definition",
            "gui-control-registry",
            "gui-logic-headless",
        )
        if any(label not in bad for label in expected):
            print("architecture self-test: one or more violation rules did not fire", file=sys.stderr)
            print(bad, file=sys.stderr)
            return 1

        with tempfile.TemporaryDirectory(prefix="autoshade-architecture-valid-") as valid_temp:
            valid = Path(valid_temp)
            write(valid / "src/decode.rs", "fn door(d: D) { guard_raw_plane_extent(path)?; d.raw_image(); }")
            write(valid / "src/render/source.rs", "fn develop(d: D) { refuse_raw_develop_over_ceiling_for(path, &probe)?; d.raw_image(src, params, false); }")
            write(valid / "src/store/sidecar.rs", "fn read(p: &Path) { let _ = read_text_capped(p); }")
            write(valid / "src/render.rs", "fn render() { recipe(); }\nfn linear_ramp_coverage(t: f32) -> f32 { t }")
            write(valid / "src/pipeline.rs", "fn open() { camera_base_look(); }")
            write(valid / "src/serve.rs", "fn open() { camera_base_look(); }")
            write(valid / "src/bin/gui/open_actions.rs", "fn open() { camera_base_look(); }")
            write(valid / "src/render/source.rs", "fn develop(d: D) { guard_raw_plane_extent(path)?; refuse_raw_develop_over_ceiling_for(path, &probe)?; d.raw_image(src, params, false); }")
            write(
                valid / "src/bin/gui/masks.rs",
                "fn edit() { mask_paint = Some(x); paint_mask_changed(None); }\nfn paint_mask_changed() { mask_dirty = true; }",
            )
            write(
                valid / "src/store/versions.rs",
                'fn version_meta_path(dev: &Path) -> PathBuf { dev.join(".version-meta.json") }\n'
                "fn write_version_meta_unlocked() { durable_write(); }",
            )
            write(
                valid / "src/store/key.rs",
                'fn adoption_skips(name: &str) -> bool { name == ".develop.lock" }',
            )
            write(
                valid / "src/store/snapshot.rs",
                "fn recover_orphan_baks_unlocked() { recover_recipe(); }",
            )
            write(
                valid / "src/store/clear.rs",
                "fn clear_sweep_in() { clear_recipe(); }",
            )
            write(
                valid / "src/store/commit.rs",
                "fn apply_commit_members() { publish_recipe(); }",
            )
            write(
                valid / "src/store/root.rs",
                '#[cfg(target_os = "macos")] pub(crate) const STORE_DIR_NAME: &str = "AutoShade";\n'
                '#[cfg(not(target_os = "macos"))] pub(crate) const STORE_DIR_NAME: &str = "autoshade";\n'
                'pub(super) const ADOPT_PRE_RENAME: bool = !cfg!(target_os = "macos");\n'
                'fn mac_root(home: &Path) { home.join("Library").join("Application Support"); }',
            )
            write(valid / "src/serve.rs", "fn open() { camera_base_look(); }\nfn api_export() { let _heavy = heavy(); render(); }\nfn api_download() { let _heavy = heavy(); render(); }")
            write(valid / "src/generative.rs", "fn edit_onto() { let heavy = crate::full_res_slot(); drop(heavy); call_images_edit(); let _heavy = crate::full_res_slot(); }")
            write(valid / "src/retouch.rs", "fn heal_and_save() { let _heavy = crate::full_res_slot(); split_alpha(); heal_image(); save_master(); }")
            write(valid / "src/fit_zoned/subzones.rs", "fn coverage(t: f32) -> f32 { render::linear_ramp_coverage(t) }")
            write(valid / "src/advisor/catalogue.rs", 'pub const RECIPE_CONTROLS: [Control; 1] = [Control { name: "exposure_ev" }];\npub const LOCAL_CONTROLS: [Control; 0] = [];')
            write(valid / "src/recipe.rs", "pub struct EditRecipe { pub exposure_ev: f32, pub mystery: f32 }")
            write(valid / "src/bin/gui/panel.rs", "fn edit(r: &mut EditRecipe) { r.exposure_ev = 1.0; }")
            write(valid / "src/bin/gui/logic/gallery.rs", "fn action() { Effect::Open; }")
            valid_findings = run_rules(valid)
            if valid_findings:
                print("architecture self-test: valid fixture was rejected", file=sys.stderr)
                print("\n".join(valid_findings), file=sys.stderr)
                return 1
    print(
        f"architecture self-test: all {len(RULES)} code-shape classes rejected; "
        "valid fixture passes"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    findings = run_rules(ROOT)
    if findings:
        print("architecture gate: forbidden code shape:", file=sys.stderr)
        print("\n".join(findings), file=sys.stderr)
        return 1
    files = production_rust_files(ROOT)
    print(f"architecture gate OK: {len(RULES)} rules over {len(files)} production Rust files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
