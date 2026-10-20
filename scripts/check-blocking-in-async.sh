#!/usr/bin/env bash
# Caller: qa-gates.lib.sh, pre-push
set -euo pipefail
REPO="$(git rev-parse --show-toplevel)"
if command -v python3 >/dev/null 2>&1; then
    PYTHON=python3
else
    PYTHON=python
fi
exec "$PYTHON" "$REPO/scripts/check-blocking-in-async.py"
