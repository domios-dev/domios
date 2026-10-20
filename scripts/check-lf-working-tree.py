#!/usr/bin/env python3
"""Verify that tracked files pinned to LF have LF-only working copies."""

import subprocess
import sys
from pathlib import Path


result = subprocess.run(
    ["git", "ls-files", "--eol", "-z"], capture_output=True, check=True
)
failures = []
for entry in result.stdout.split(b"\0"):
    if not entry:
        continue
    metadata, raw_path = entry.split(b"\t", 1)
    if b"eol=lf" not in metadata:
        continue
    path = Path(raw_path.decode("utf-8", errors="surrogateescape"))
    try:
        data = path.read_bytes()
    except OSError as error:
        failures.append(f"{path}: cannot read working copy: {error}")
        continue
    if b"\r" in data:
        failures.append(f"{path}: contains CR bytes but is attributed eol=lf")

if failures:
    print("LF checkout guard failed:", file=sys.stderr)
    print("\n".join(f"  {failure}" for failure in failures), file=sys.stderr)
    sys.exit(1)

print("LF checkout guard passed")
