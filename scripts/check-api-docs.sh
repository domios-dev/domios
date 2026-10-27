#!/usr/bin/env bash
# Caller: CI tests.yml and tests-side.yml
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PYTHON_BIN="$(command -v python3 || command -v python)"
"$PYTHON_BIN" "$ROOT/scripts/check_api_docs.py" "$ROOT"
