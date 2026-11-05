#!/usr/bin/env python3
"""Generates the superlive v2 component manifest.json (component packaged by
package-component.sh).

Collects sha256 fingerprints of all regular files from the staging directory,
validates the ui entry (fixed name entry.js in the component build form), and
writes the package-root manifest.json.

Contract aligned with slsdk-rs/src/manifest.rs:
- id/namespace are alphanumeric plus hyphen only (validate charset rule)
- files are relative posix paths + sha256 hex (lowercase), excluding
  manifest.json itself
- ui.entry is relative to ui/ (shell-side mapping /comps/{id}/{entry} →
  components/{id}/ui/)
- route entries require path/title (RouteDeclaration has no default)

manifest.json must be written last: the files collection happens before the
write, guaranteeing "every file in the package is declared" aligns with the
shell-side set-difference check (verify_files' ExtraFile).
"""

import argparse
import hashlib
import json
import os
import re
import sys
from pathlib import Path

ID_NS_RE = re.compile(r"^[A-Za-z0-9-]+$")

# Project root (derived from the script's own location): used to hint about
# web/public when the surface entry is missing from the package
PROJECT_ROOT = Path(__file__).resolve().parent.parent


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def discover_entry(ui_dir: Path, ui_type: str, entry: str) -> str:
    """Validates that the UI entry is in place under the package's ui/ and
    returns its relative path.

    module type: the component build form's entry is the fixed name entry.js
    (contract source: web/vite.config.js — rollupOptions.input is the sole
    src/entry.tsx, entryFileNames is fixed as 'entry.js' with no hash, and
    index.html is not part of the build artifact) — manifest.ui.entry
    statically declares this name.

    surface type: the entry is an A2UI descriptor (default surface.json). The
    descriptor normally lives in web/public/ (copied into the build artifact
    by vite build); the dev form registers web/dist as the static space,
    while the packaged variant must place the descriptor into the staging
    ui/.
    """
    if (ui_dir / entry).is_file():
        return entry
    hint = ""
    if ui_type == "surface":
        public_copy = PROJECT_ROOT / "web" / "public" / entry
        if public_copy.is_file():
            hint = (
                f" (web/public/{entry} exists; in the dev form it registers as the build "
                "artifact after vite build — for the packaged variant, put this file "
                "into the staging ui/)"
            )
    expect = (
        "for web components the build-artifact entry is the fixed name entry.js; "
        "rebuild with the component-form vite config and repackage"
        if ui_type == "module"
        else "for surface-type UI the entry is a descriptor file"
    )
    sys.exit(f"error: missing ui/{entry} ({expect}){hint}")


def collect_files(staging: Path) -> list[dict]:
    """Recursively collects all regular files under staging (relative posix
    path + lowercase sha256 hex).

    symlinks are rejected outright (the shell-side collect_relative_files
    rejects symlinks per lstat semantics; the packaging source should never
    produce them).
    """
    entries = []
    for dirpath, dirnames, filenames in os.walk(staging):
        dirnames.sort()
        for name in sorted(filenames):
            p = Path(dirpath) / name
            if p.is_symlink():
                sys.exit(f"error: the package contains a symlink (rejected by the shell): {p}")
            # manifest.json is this script's own artifact and never enters
            # files (the shell refuses to install with ManifestInFiles; this
            # also keeps a stale manifest from a previous generation in the
            # same staging out of the list)
            if p.relative_to(staging).as_posix() == "manifest.json":
                continue
            entries.append(
                {
                    "path": p.relative_to(staging).as_posix(),
                    "sha256": sha256_file(p),
                }
            )
    return entries


def main() -> None:
    parser = argparse.ArgumentParser(description="Generates the component package manifest.json")
    parser.add_argument("--staging", required=True, help="package assembly directory (package root)")
    parser.add_argument("--id", required=True)
    parser.add_argument("--namespace", required=True)
    parser.add_argument("--display-name", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--min-shell-version", required=True)
    parser.add_argument("--launch-command", required=True)
    parser.add_argument(
        "--ui-type",
        choices=["module", "surface"],
        default="module",
        help="UI form: module=React bundle (default); surface=A2UI descriptor (rendered by the shell)",
    )
    parser.add_argument(
        "--entry",
        default=None,
        help="ui entry relative path; defaults: module=entry.js, surface=surface.json",
    )
    args = parser.parse_args()

    # Pre-flight checks (aligned with slsdk manifest.rs validate; errors fire
    # earlier and closer to the packaging context than the shell side)
    for field, value in (("id", args.id), ("namespace", args.namespace)):
        if not ID_NS_RE.match(value):
            sys.exit(f"error: invalid {field} (only letters/digits/hyphens allowed): {value}")
    for field, value in (
        ("version", args.version),
        ("min_shell_version", args.min_shell_version),
    ):
        if not value:
            sys.exit(f"error: {field} must not be empty")

    staging = Path(args.staging).resolve()
    if not staging.is_dir():
        sys.exit(f"error: staging directory does not exist: {staging}")

    # Staging structure checks: the launch binary and ui/ must be in place
    bin_rel = args.launch_command
    while bin_rel.startswith("./"):
        bin_rel = bin_rel[2:]
    if not (staging / bin_rel).is_file():
        sys.exit(f"error: the launch binary is not in staging: {bin_rel}")
    ui_dir = staging / "ui"
    if not ui_dir.is_dir():
        sys.exit(f"error: missing ui/ subdirectory (component UI assets must live in the package's ui/): {ui_dir}")

    entry = args.entry or ("entry.js" if args.ui_type == "module" else "surface.json")
    entry = discover_entry(ui_dir, args.ui_type, entry)
    # manifest.json has not been written to staging yet, so what is collected
    # here is exactly the final package content
    files = collect_files(staging)
    if not files:
        sys.exit("error: the files list is empty (staging contains no files)")

    manifest = {
        "id": args.id,
        "version": args.version,
        "min_shell_version": args.min_shell_version,
        "namespace": args.namespace,
        "display_name": args.display_name,
        "ui": {"ui_type": args.ui_type, "entry": entry},
        "menu": {"title": args.display_name, "icon_path": None},
        "routes": [{"path": "/", "title": args.display_name}],
        "launch": {"command": args.launch_command, "args": []},
        "files": files,
    }

    manifest_path = staging / "manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(f"    ui.entry = {entry}")
    print(f"    files    = {len(files)} entries")
    print(f"    written  = {manifest_path}")


if __name__ == "__main__":
    main()
