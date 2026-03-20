#!/bin/bash
set -e

echo "=== CodeTerm Test Suite ==="
echo "Platform: $(uname -s)"
echo ""

# ── Rust Unit Tests ──────────────────────────────────────────────

echo "--- Rust Unit Tests ---"
cd "$(dirname "$0")/../src-tauri"

cargo test --lib 2>&1
echo ""
echo "[OK] Rust unit tests passed"

echo ""
echo "--- Rust Integration Tests ---"
cargo test --test pty_test 2>&1
echo ""
echo "[OK] Rust integration tests passed"

# ── Frontend Build ───────────────────────────────────────────────

echo ""
echo "--- Frontend Build Check ---"
cd "$(dirname "$0")/.."

# Use pnpm if available, fall back to npx
if command -v pnpm &> /dev/null; then
    pnpm run build 2>&1
else
    npx vite build 2>&1
fi
echo ""
echo "[OK] Frontend build passed"

echo ""
echo "=== All tests passed ==="
