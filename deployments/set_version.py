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
        old = re.search(r'"([^"]+)"', line).group(1)
        lines[i] = f'version = "{version}"\n'
        done = True
        break
if not done:
    sys.exit("no version key under [workspace.package]")
path.write_text("".join(lines), encoding="utf-8")

# Keep Cargo.lock in step, or `cargo build --locked` refuses to run. Workspace
# crates are the [[package]] entries without a `source` (path dependencies).
lock = path.with_name("Cargo.lock")
blocks = lock.read_text(encoding="utf-8").split("\n\n")
for i, b in enumerate(blocks):
    if b.startswith("[[package]]") and "\nsource = " not in b:
        blocks[i] = re.sub(rf'^version = "{re.escape(old)}"$', f'version = "{version}"', b, count=1, flags=re.M)
lock.write_text("\n\n".join(blocks), encoding="utf-8", newline="\n")
print(f"version set to {version}")
