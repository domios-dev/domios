#!/usr/bin/env bash
set -euo pipefail

FIXTURE=${1:?usage: check-release-verification-order.sh <fixture.json>}

python3 - "$FIXTURE" <<'PY'
import json
import sys
from pathlib import Path

try:
    fixture = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
except (OSError, json.JSONDecodeError) as error:
    print(f"MATRIX_FAILED: completion fixture is unreadable: {error}")
    raise SystemExit(1)
signals = fixture.get("signals", {})
required_lanes = ("release", "release-linux", "release-windows")
required_platforms = (
    "darwin-aarch64",
    "darwin-x86_64",
    "linux-x86_64",
    "windows-x86_64",
)

failures = []
for lane, signal in signals.items():
    if lane not in required_lanes:
        continue
    status = signal.get("status")
    if status != "success":
        reason = signal.get("reason") or f"status={status or 'missing'}"
        failures.append(f"{lane}: {reason}")
if failures:
    print("MATRIX_FAILED: release matrix did not complete: " + "; ".join(failures))
    raise SystemExit(1)

missing_signals = [lane for lane in required_lanes if lane not in signals]
if missing_signals:
    print(
        "MATRIX_PENDING: awaiting completion signal(s): "
        + ", ".join(missing_signals)
    )
    raise SystemExit(0)

published = set(fixture.get("published_platforms", []))
missing_platforms = [platform for platform in required_platforms if platform not in published]
if missing_platforms:
    print(
        "MATRIX_FAILED: completion signals claim success, but published manifest "
        "is missing: "
        + ", ".join(missing_platforms)
    )
    raise SystemExit(1)

print("MATRIX_COMPLETE: all completion signals and required published platforms are observable")
PY
