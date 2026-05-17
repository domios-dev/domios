#!/usr/bin/env bash
# Orchestrate: seed → cleanup → assert.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== SEED ==="
bash "$HERE/seed.sh"
echo
echo "=== CLEANUP (Dreamer-equivalent CLI calls) ==="
bash "$HERE/cleanup.sh"
echo
echo "=== ASSERT ==="
bash "$HERE/assert.sh"
