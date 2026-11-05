#!/usr/bin/env python3
"""Package self-verification (simulates the shell-side installer.rs check
chain; the verification target is the final zip artifact).

Checks (one-to-one with the shell-side failure forms):
1. The package-root manifest.json exists and parses (MissingManifest /
   ManifestJson)
2. Contract checks aligned with slsdk manifest.rs validate: required fields,
   id/namespace charset, non-empty files, path safety (no absolute paths or
   .. escapes), duplicate paths, manifest.json not in files (ManifestInFiles,
   S5: the manifest is kept outside the install directory)
3. Content set-difference consistency: the file set inside the zip (minus
   manifest.json) matches the files declaration set in both directions
   (shell-side verify_files: missing MissingFile / smuggled ExtraFile)
4. Per-file sha256 fingerprint comparison (shell-side HashMismatch; the
   comparison is case-insensitive, same as the shell)
5. No symlink entries (the shell-side collect_relative_files rejects
   symlinks)
6. The launch binary is declared in files and carries the unix executable
   bit inside the zip (the installer restores unix_mode on extraction; a
   binary without the exec bit cannot start)
7. ui.entry exists under ui/ in the package (shell-side /comps/{id}/{entry}
   static mapping reachable)
8. Prints the content inventory and the manifest summary

Any failure exits non-zero; all checks pass prints the inventory and the
[PASS] verdict.
"""

import hashlib
import json
import posixpath
import re
import sys
import zipfile

ID_NS_RE = re.compile(r"^[A-Za-z0-9-]+$")
MANIFEST_NAME = "manifest.json"


def fail(msg: str):
    print(f"[FAIL] {msg}")
    sys.exit(1)


def ok(msg: str):
    print(f"[ OK ] {msg}")


def unix_mode(info: zipfile.ZipInfo) -> int:
    """The upper 16 bits of the zip external attributes are the unix perms
    (valid when create_system == 3)."""
    return (info.external_attr >> 16) & 0xFFFF


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(f"usage: {sys.argv[0]} <component.zip>")
    zip_path = sys.argv[1]

    try:
        zf = zipfile.ZipFile(zip_path)
    except (OSError, zipfile.BadZipFile) as e:
        fail(f"cannot read the package {zip_path}: {e}")
    names = zf.namelist()

    # ---- 1. manifest exists and parses ----
    if MANIFEST_NAME not in names:
        fail(f"package root is missing {MANIFEST_NAME} (shell-side MissingManifest)")
    try:
        manifest = json.loads(zf.read(MANIFEST_NAME).decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as e:
        fail(f"{MANIFEST_NAME} is not valid contract JSON (shell-side ManifestJson): {e}")
    ok(f"{MANIFEST_NAME} parsed")

    # ---- 2. Contract checks (shell-side validate rule set) ----
    for field in (
        "id",
        "version",
        "min_shell_version",
        "namespace",
        "display_name",
    ):
        if not str(manifest.get(field) or ""):
            fail(f"required field is empty: {field}")
    if not ID_NS_RE.match(manifest["id"]):
        fail(f"invalid id (only letters/digits/hyphens allowed): {manifest['id']}")
    if not ID_NS_RE.match(manifest["namespace"]):
        fail(f"invalid namespace (only letters/digits/hyphens allowed): {manifest['namespace']}")
    ok(
        f"contract fields: id={manifest['id']} ns={manifest['namespace']} "
        f"v={manifest['version']} min_shell={manifest['min_shell_version']}"
    )

    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        fail("files list is empty or missing (shell-side EmptyFiles)")
    declared: list[tuple[str, str]] = []
    for entry in files:
        if not isinstance(entry, dict) or "path" not in entry or "sha256" not in entry:
            fail(f"invalid files entry shape (path+sha256 required): {entry}")
        p, digest = entry["path"], entry["sha256"]
        if not p:
            fail("files contains an empty path (shell-side InvalidFilePath: empty path)")
        if p.startswith("/") or posixpath.isabs(p):
            fail(f"files contains an absolute path (shell-side InvalidFilePath): {p}")
        if ".." in p.split("/"):
            fail(f"files contains a .. escape path (shell-side InvalidFilePath): {p}")
        declared.append((p, str(digest)))
    declared_paths = [p for p, _ in declared]
    if len(set(declared_paths)) != len(declared_paths):
        fail("files contains duplicate paths")
    if MANIFEST_NAME in declared_paths:
        fail("files list includes manifest.json (shell refuses install with ManifestInFiles, S5 verdict)")
    ok(f"contract checks passed ({len(declared)} declared entries, paths safe, manifest not self-listed)")

    # ---- 3+5. zip file set vs declaration set (two-way set difference),
    # symlink rejection ----
    actual: list[str] = []
    for info in zf.infolist():
        if info.is_dir():
            continue
        mode = unix_mode(info)
        if (mode & 0o170000) == 0o120000:
            fail(f"the package contains a symlink entry (rejected by the shell): {info.filename}")
        actual.append(info.filename)
    actual_set = set(actual) - {MANIFEST_NAME}
    declared_set = set(declared_paths)
    missing = sorted(declared_set - actual_set)
    extra = sorted(actual_set - declared_set)
    if missing:
        fail(f"declared files missing from the package (shell-side MissingFile): {missing}")
    if extra:
        fail(f"the package contains undeclared files (shell refuses install with ExtraFile): {extra}")
    ok(f"set difference consistent: {len(actual_set)} files in the package match the declaration set both ways")

    # ---- 4. Per-file sha256 (case-insensitive, same as the shell) ----
    for p, digest in declared:
        actual_digest = hashlib.sha256(zf.read(p)).hexdigest()
        if actual_digest.lower() != digest.lower():
            fail(f"fingerprint mismatch (shell-side HashMismatch): {p} declared {digest} actual {actual_digest}")
    ok(f"fingerprint comparison passed (all {len(declared)} sha256 entries match)")

    # ---- 6. launch binary declaration + executable bit ----
    launch = manifest.get("launch") or {}
    command = str(launch.get("command") or "")
    if not command:
        fail("launch.command missing")
    bin_rel = command
    while bin_rel.startswith("./"):
        bin_rel = bin_rel[2:]
    if bin_rel not in declared_set:
        fail(f"launch binary not declared in files: {bin_rel}")
    if bin_rel not in actual_set:
        fail(f"launch binary not in the package: {bin_rel}")
    bin_mode = unix_mode(zf.getinfo(bin_rel)) & 0o777
    if not bin_mode & 0o111:
        fail(f"launch binary lacks the executable bit (cannot start after extraction): {bin_rel} mode={oct(bin_mode)}")
    ok(f"launch: {command} (unix mode {oct(bin_mode)}, args={launch.get('args', [])})")

    # ---- 7. ui.entry reachable ----
    ui = manifest.get("ui") or {}
    ui_type = str(ui.get("ui_type") or "")
    entry = str(ui.get("entry") or "")
    if ui_type not in ("module", "iframe"):
        fail(f"invalid ui.ui_type (module|iframe): {ui_type}")
    if not entry:
        fail("ui.entry missing")
    entry_path = posixpath.join("ui", entry)
    if entry_path not in declared_set:
        fail(f"ui.entry not declared in files: {entry_path}")
    ok(f"ui: {ui_type} tier, entry ui/{entry}")

    # ---- 7.5 module-tier artifact semantics (guards against tree-shaken
    # empty-shell regression) ----
    # The shell's ComponentHost consumes mod.default to render the component;
    # the component entry's only static consumer is a runtime dynamic import,
    # invisible to rollup at build time — without a preserved entry signature
    # the component tree is shaken off wholesale and the artifact becomes an
    # empty shell with no default export (this actually happened). The
    # fingerprint/set-difference checks only verify file consistency, not
    # artifact semantics; this adds the empty-shell criterion.
    if ui_type == "module":
        code = zf.read(entry_path).decode("utf-8", errors="replace")
        if not re.search(r"as\s+default\b|export\s+default\b", code):
            fail(
                f"module-tier entry {entry_path} lacks a default export"
                " (the shell mounts the component via mod.default; this failure usually means the"
                " build config did not preserve the entry signature and the component tree was"
                " shaken empty — check vite rollupOptions.preserveEntrySignatures)"
            )
        ok(f"module-tier entry contains a default export (not an empty shell)")

    # ---- 8. Content inventory ----
    print("\n--- package content inventory ---")
    print(f"{'path':<44} {'size':>10}  mode")
    for info in sorted(zf.infolist(), key=lambda i: i.filename):
        size = info.file_size
        mode = unix_mode(info) & 0o777
        kind = "dir " if info.is_dir() else "file"
        print(f"{kind} {info.filename:<39} {size:>10}  {oct(mode)}")

    print(
        f"\n[PASS] self-verification passed: {zip_path}\n"
        f"       component {manifest['id']} v{manifest['version']} "
        f"(ns={manifest['namespace']}, {len(declared)} files)"
    )


if __name__ == "__main__":
    main()
