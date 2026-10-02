#!/usr/bin/env bash
# WHAT: Exercise real provider prompts on an explicitly named instance.
# WHY: A source-only Worker must hand canonical QA an isolated, bounded gate.
# ORACLE: The Python runner requires prompt, decision, and far-end evidence.
set -euo pipefail
matrix_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "$matrix_dir/matrix.py" "$@"
