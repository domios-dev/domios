#!/usr/bin/env bash
# qa-prep: one command that makes an isolated worktree dev-runnable.
#
# Stages:
#   1. dist     — vite build directly (skips tsc typecheck)
#   2. sidecars — build from source, or explicitly reuse canonical sidecars
#                 only when provenance manifests match this worktree
#
# Usage:
#   scripts/qa-prep.sh [--timeout SECS] [--warm-canonical]
#
# Env overrides:
#   QA_PREP_TIMEOUT        wall-clock cap in seconds (default 300)
#   CODETERM_WARM_CANONICAL_QA  set to 1 to allow provenance-checked reuse

set -euo pipefail

if [ -n "${CODETERM_BUILD_GATEWAY_ACTIVE:-}" ] || [ -n "${CODETERM_BUILD_GATEWAY_JOB_ID:-}" ]; then
  echo "[qa-prep] ERROR: environment gateway markers are not accepted" >&2
  exit 2
fi

if [ "${1:-}" != "--gateway-context" ] || [ "$#" -lt 5 ]; then
  echo "[qa-prep] ERROR: qa-prep is an internal build.sh child" >&2
  exit 2
fi
GATEWAY_JOB_ID="$2"
GATEWAY_PARENT_PID="$3"
GATEWAY_PARENT_START="$4"
GATEWAY_COMMAND="$5"
shift 5

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
source scripts/cache-policy.sh
source scripts/build-gateway.sh
ct_gateway_assert_internal_child "$GATEWAY_JOB_ID" "$GATEWAY_PARENT_PID" "$GATEWAY_PARENT_START" "$GATEWAY_COMMAND"

if [ "${1:-}" = "--probe-only" ]; then
  echo "[qa-prep] gateway parent/lock/worktree/command/source verified"
  exit 0
fi
[ "${1:-}" != "--" ] || shift

TIMEOUT="${QA_PREP_TIMEOUT:-300}"
WARM_CANONICAL="${CODETERM_WARM_CANONICAL_QA:-0}"

while [[ "${1:-}" == -* ]]; do
  case "$1" in
    --timeout=*) TIMEOUT="${1#*=}"; shift ;;
    --timeout)   TIMEOUT="$2"; shift 2 ;;
    --warm-canonical) WARM_CANONICAL=1; shift ;;
    *) echo "[qa-prep] unknown flag: $1" >&2; shift ;;
  esac
done

# Detect the main worktree (canonical checkout) via git.
CANONICAL=$(git worktree list --porcelain | awk '/^worktree /{print $2; exit}')
if [ "$CANONICAL" = "$REPO" ]; then
  CANONICAL=""  # we ARE the canonical checkout
fi

EXE_EXT=""
case "$OSTYPE" in msys*|cygwin*|win32) EXE_EXT=".exe" ;; esac

# ---- Timeout watchdog: kills the run_stages process group on cap ----
# Job control ensures the background subshell gets its own PGID.
set -m

TIMEOUT_FILE=$(mktemp)
trap 'rm -f "$TIMEOUT_FILE"' EXIT

fail_stage() {
  echo "[qa-prep] FAILED at stage: $1" >&2
  exit 1
}

sidecar_valid() {
  local f="$1"
  [ -f "$f" ] && [ "$(stat -f %z "$f" 2>/dev/null || stat -c %s "$f" 2>/dev/null || echo 0)" -gt 1048576 ]
}

run_stages() {
  set -euo pipefail

  # ================================================================
  # Stage 1: dist (vite build — no tsc)
  # ================================================================
  echo "[qa-prep] 1/2  dist (vite build, skipping tsc)..."

  # Ensure node_modules. In a worktree link from canonical; never pnpm install.
  if [ ! -d "$REPO/node_modules" ]; then
    if [ -n "$CANONICAL" ] && [ -d "$CANONICAL/node_modules" ]; then
      # A prior MSYS `ln` run may have left a partial copy — clear it first.
      rm -rf "$REPO/node_modules"
      case "$OSTYPE" in
        msys*|cygwin*|win32)
          # MSYS ln copies instead of symlinking; a junction needs no privilege.
          cmd //c mklink /J "$(cygpath -w "$REPO/node_modules")" \
            "$(cygpath -w "$CANONICAL/node_modules")" >/dev/null \
            || fail_stage "dist:node-modules-link"
          ;;
        *)
          ln -sf "$CANONICAL/node_modules" "$REPO/node_modules" \
            || fail_stage "dist:node-modules-link"
          ;;
      esac
      echo "[qa-prep] node_modules: linked from canonical checkout"
    else
      echo "[qa-prep] ERROR: no node_modules. Run pnpm install in the canonical checkout first." >&2
      fail_stage "dist:node-modules"
    fi
  fi

  "$REPO/node_modules/.bin/vite" build || fail_stage "dist"
  ct_write_provenance "web-dist" "dev" "$(ct_host_triple)" "" "" \
    "dist/index.html" "dist/.codeterm-provenance.json"
  echo "[qa-prep] dist: ok ($(du -sh "$REPO/dist" 2>/dev/null | cut -f1))"

  # ================================================================
  # Stage 2: sidecars (bundled-cli + bundled-embed)
  # ================================================================
  echo "[qa-prep] 2/2  sidecars (bundled-cli + bundled-embed)..."

  HOST_TRIPLE=$(rustc -vV | awk -F': ' '/^host:/{print $2}')
  CLI_DIR="$REPO/src-tauri/bundled-cli"
  EMBED_DIR="$REPO/src-tauri/bundled-embed"
  CLI_BIN="$CLI_DIR/codeterm${EXE_EXT}"
  EMBED_BIN="$EMBED_DIR/codeterm-embed-${HOST_TRIPLE}${EXE_EXT}"
  mkdir -p "$CLI_DIR" "$EMBED_DIR"

  CAN_CLI="${CANONICAL:+$CANONICAL/src-tauri/bundled-cli/codeterm${EXE_EXT}}"
  CAN_EMBED="${CANONICAL:+$CANONICAL/src-tauri/bundled-embed/codeterm-embed-${HOST_TRIPLE}${EXE_EXT}}"
  CAN_CLI_PROV="${CAN_CLI:+$CAN_CLI.provenance.json}"
  CAN_EMBED_PROV="${CAN_EMBED:+$CAN_EMBED.provenance.json}"

  USE_CANONICAL=0
  if [ "$WARM_CANONICAL" = "1" ] && [ -n "$CANONICAL" ] \
      && sidecar_valid "${CAN_CLI}" && sidecar_valid "${CAN_EMBED}" \
      && [ -f "$CAN_CLI_PROV" ] && [ -f "$CAN_EMBED_PROV" ] \
      && ct_check_provenance "bundled-cli" "release" "$HOST_TRIPLE" "" "" "$CAN_CLI_PROV" \
      && ct_check_provenance "bundled-embed" "release" "$HOST_TRIPLE" "" "" "$CAN_EMBED_PROV"; then
    USE_CANONICAL=1
  fi

  if [ "$USE_CANONICAL" = "1" ]; then
    cp "$CAN_CLI"   "$CLI_BIN"
    cp "$CAN_EMBED" "$EMBED_BIN"
    cp "$CAN_CLI_PROV" "$CLI_BIN.provenance.json"
    cp "$CAN_EMBED_PROV" "$EMBED_BIN.provenance.json"
    # Ad-hoc sign for macOS dev; errors are non-fatal (devs may lack Xcode tools).
    if [[ "$OSTYPE" == darwin* ]]; then
      codesign --force --sign - "$CLI_BIN"   2>/dev/null || true
      codesign --force --sign - "$EMBED_BIN" 2>/dev/null || true
    fi
    CLI_SIZE=$(du -sh "$CLI_BIN"   | cut -f1)
    EMB_SIZE=$(du -sh "$EMBED_BIN" | cut -f1)
    echo "[qa-prep] sidecars: reused from canonical (CLI ${CLI_SIZE}, embed ${EMB_SIZE})"
  else
    echo "[qa-prep] sidecars: building from source (warm-canonical=$WARM_CANONICAL, provenance missing or mismatched)"
    ct_configure_sccache

    node scripts/prepare-cli-stub.js
    node scripts/prepare-embed-stub.js

    touch cli/src/main.rs
    CODETERM_DEV=1 cargo build --release --manifest-path cli/Cargo.toml \
      || fail_stage "sidecars:cli-build"
    CODETERM_DEV=1 node scripts/bundle-cli.js
    ct_write_provenance "bundled-cli" "release" "$HOST_TRIPLE" "" "" \
      "src-tauri/bundled-cli/codeterm${EXE_EXT}" \
      "src-tauri/bundled-cli/codeterm${EXE_EXT}.provenance.json"

    cargo build --release --manifest-path src-tauri/Cargo.toml --bin codeterm-embed \
      || fail_stage "sidecars:embed-build"
    CODETERM_DEV=1 node scripts/bundle-embed.js
    ct_write_provenance "bundled-embed" "release" "$HOST_TRIPLE" "" "" \
      "src-tauri/bundled-embed/codeterm-embed-${HOST_TRIPLE}${EXE_EXT}" \
      "src-tauri/bundled-embed/codeterm-embed-${HOST_TRIPLE}${EXE_EXT}.provenance.json"

    echo "[qa-prep] sidecars: built ok"
  fi

  # ================================================================
  # Ready
  # ================================================================
  echo "[qa-prep] done. Run: ./build.sh dev"
}

run_stages &
STAGES_PID=$!

(
  sleep "$TIMEOUT" || true
  printf '1' > "$TIMEOUT_FILE"
  echo "[qa-prep] TIMEOUT after ${TIMEOUT}s — killing group" >&2
  kill -- "-${STAGES_PID}" 2>/dev/null || true
) &
WATCHDOG=$!

if wait "$STAGES_PID"; then
  RC=0
else
  RC=$?
fi

kill "$WATCHDOG" 2>/dev/null; wait "$WATCHDOG" 2>/dev/null || true

[ -s "$TIMEOUT_FILE" ] && { echo "[qa-prep] TIMED OUT" >&2; exit 124; }
exit "$RC"
