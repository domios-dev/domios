#!/usr/bin/env bash
# Orchestrator-facing entry point for a Windows release worker. Runs the full
# pipeline (checkout → install → bundle → sign → upload) and emits the final
# machine-parseable agent report on stdout:
#
#   WINDOWS_READY=<ver> sig=<base64>
#
#   <prose>
#
# On failure, emits BLOCKED step=<name> err=<short> with prose detail.
#
# Spawn from the orchestrator pane with something like:
#   codeterm agent spawn claude --task "bash .codeterm/skills/github-release/scripts/win-release.sh 1.3.3"
# The agent then dispatches stdout into `codeterm agent report` so the parent
# orchestrator parses the first line as a structured event.
#
# Usage: win-release.sh <ver>

set -uo pipefail

VER="${1:-}"
[ -n "$VER" ] || { echo "usage: win-release.sh <ver>" >&2; exit 2; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
# shellcheck source=lib/report.sh
. "$SCRIPT_DIR/lib/report.sh"

cd "$REPO_ROOT"

fail() {
  local step="$1" msg="$2"
  report_blocked "$step" "$msg" <<EOF
Windows release worker failed on v$VER. See pane stderr above for full logs.
Repo root: $REPO_ROOT
EOF
  exit 1
}

echo "=== fetch + checkout v$VER ===" >&2
git fetch origin --tags >&2 || fail fetch "git fetch origin --tags failed"

# CRLF phantom-dirt recovery — see consolidated mem entry.
git update-index --refresh >/dev/null 2>&1 || true
if ! git -c core.autocrlf=false checkout "v$VER" >&2; then
  fail checkout "git checkout v$VER failed (after autocrlf=false retry)"
fi

ACTUAL_TAG="$(git describe --tags --exact-match HEAD 2>/dev/null || echo)"
[ "$ACTUAL_TAG" = "v$VER" ] || fail checkout "HEAD is $ACTUAL_TAG, expected v$VER"

echo "=== pnpm install ===" >&2
pnpm install --frozen-lockfile >&2 || fail pnpm-install "pnpm install failed"

echo "=== bundle ===" >&2
SIG="$("$SCRIPT_DIR/win-bundle.sh" "$VER")" \
  || fail bundle "win-bundle.sh exited non-zero"

SIG="$(validate_b64_sig "$SIG")" || fail bundle "invalid base64 sig from win-bundle.sh"

echo "=== upload ===" >&2
"$SCRIPT_DIR/win-upload.sh" "$VER" \
  || fail upload "win-upload.sh exited non-zero"

report_ready "WINDOWS_READY=$VER" "sig=$SIG" <<EOF
Windows release v$VER built, signed, and uploaded to both rollacode/codeterm
(private) and rollacode/codeterm-releases (mirror). Signature is the full
.nsis.zip.sig contents base64-encoded — drop directly into latest.json under
platforms.windows-x86_64.signature.
EOF
