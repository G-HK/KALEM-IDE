#!/usr/bin/env python3
"""Set [workspace.package] version in the root Cargo.toml (used by the release workflow).

Usage: python3 deployments/set_version.py 0.2.0-rc.1
"""
import pathlib
import re
import sys

version = sys.argv[1].lstrip("v") if len(sys.argv) > 1 else ""
if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?", version):
    sys.exit(f"'{version}' is not a version like 1.2.3 or 1.2.3-rc.1")

path = pathlib.Path(__file__).resolve().parent.parent / "Cargo.toml"
lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
in_pkg = done = False
for i, line in enumerate(lines):
    if line.startswith("["):
        in_pkg = line.strip() == "[workspace.package]"
    elif in_pkg and re.match(r'version\s*=', line):
        lines[i] = f'version = "{version}"\n'
        done = True
        break
if not done:
    sys.exit("no version key under [workspace.package]")
path.write_text("".join(lines), encoding="utf-8")
print(f"version set to {version}")
