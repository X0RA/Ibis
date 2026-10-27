#!/usr/bin/env python3
"""Set the application version in a release checkout without updating dependencies."""
import re
import sys
from pathlib import Path


def main():
    if len(sys.argv) != 2 or not re.fullmatch(r"\d+\.\d+\.\d+", sys.argv[1]):
        raise SystemExit("Usage: set-release-version.py X.Y.Z")
    root = Path(__file__).resolve().parent.parent
    version = sys.argv[1]
    changes = []
    for name, pattern in [
        ("Cargo.toml", r'(?ms)(^\[package\]\n(?:(?!^\[).)*?^version = ")[^"]+("(?=\n))'),
        ("Cargo.lock", r'(\[\[package\]\]\nname = "ibis"\nversion = ")[^"]+(")'),
    ]:
        path = root / name
        updated, count = re.subn(pattern, lambda m: m[1] + version + m[2], path.read_text())
        if count != 1:
            raise SystemExit(f"Expected one application version in {name}, found {count}")
        changes.append((path, updated))
    for path, updated in changes:
        path.write_text(updated)


if __name__ == "__main__":
    main()
