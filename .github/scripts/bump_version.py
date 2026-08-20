#!/usr/bin/env python3
"""Bump the shared semver version across Cargo.toml, Cargo.lock, and web/package.json.

Usage:
    bump_version.py patch|minor|major   # bump and print the new version
    bump_version.py get                 # print the current version
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CARGO_TOML = ROOT / "Cargo.toml"
CARGO_LOCK = ROOT / "Cargo.lock"
PACKAGE_JSON = ROOT / "web" / "package.json"

WORKSPACE_PACKAGE = "[workspace.package]"


def current_version() -> str:
    lines = CARGO_TOML.read_text().splitlines()
    in_section = False
    for line in lines:
        stripped = line.strip()
        if stripped == WORKSPACE_PACKAGE:
            in_section = True
            continue
        if in_section:
            if stripped.startswith("[") and not stripped.startswith("[["):
                in_section = False
                continue
            match = re.match(r'^version\s*=\s*"([^"]+)"\s*$', stripped)
            if match:
                return match.group(1)
    raise SystemExit("workspace version not found in Cargo.toml")


def bump_version(current: str, kind: str) -> str:
    parts = current.split(".")
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        raise SystemExit(f"unexpected non-semver version: {current!r}")
    major, minor, patch = (int(p) for p in parts)
    if kind == "major":
        major, minor, patch = major + 1, 0, 0
    elif kind == "minor":
        minor, patch = minor + 1, 0
    elif kind == "patch":
        patch += 1
    else:
        raise SystemExit(f"unknown bump kind: {kind!r}")
    return f"{major}.{minor}.{patch}"


def set_workspace_version(text: str, new: str) -> str:
    lines = text.splitlines()
    out = []
    in_section = False
    for line in lines:
        stripped = line.strip()
        if stripped == WORKSPACE_PACKAGE:
            in_section = True
            out.append(line)
            continue
        if in_section and stripped.startswith("[") and not stripped.startswith("[["):
            in_section = False
            out.append(line)
            continue
        if in_section and re.match(r'^version\s*=\s*"', stripped):
            out.append(f'version = "{new}"')
            continue
        out.append(line)
    return "\n".join(out) + "\n"


def set_lock_versions(text: str, new: str) -> str:
    blocks = text.split("[[package]]")
    out = []
    for block in blocks:
        if re.search(r'^name = "light-factory-', block, re.M):
            block = re.sub(r'(?m)^(version = )"[^"]+"$', rf'\1"{new}"', block)
        out.append(block)
    return "[[package]]".join(out)


def set_package_json_version(text: str, new: str) -> str:
    return re.sub(r'("version"\s*:\s*)"[^"]+"', rf'\1"{new}"', text, count=1)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    arg = sys.argv[1]
    current = current_version()
    if arg == "get":
        print(current)
        return
    new = bump_version(current, arg)
    CARGO_TOML.write_text(set_workspace_version(CARGO_TOML.read_text(), new))
    CARGO_LOCK.write_text(set_lock_versions(CARGO_LOCK.read_text(), new))
    PACKAGE_JSON.write_text(set_package_json_version(PACKAGE_JSON.read_text(), new))
    print(new)


if __name__ == "__main__":
    main()
