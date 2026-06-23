#!/usr/bin/env bash
# Shared release-pipeline reporting helpers. Sourced by every per-platform
# release script (mac-bundle.sh, notarize-dmg.sh, pack-mac-updater.sh,
# win-bundle.sh, win-upload.sh, win-release.sh) so the parent orchestrator
# can parse the FIRST LINE of any worker report as a machine token.
#
# Contract — every worker report's first line is one of:
#   KEY=VAL [KEY2=VAL2 ...]          (success, parseable)
#   BLOCKED step=NAME err=MESSAGE    (failure, parseable)
#
# Followed by a blank line and free-form prose for humans. Orchestrator
# greps `^([A-Z_][A-Z0-9_]*=|BLOCKED step=)` on line 1 — never regex over
# the prose body.
#
# Why this exists: v1.3.2 release wasted a round-trip because the windows
# worker reported "sig: <base64>" inside prose; the orchestrator couldn't
# extract the signature without human relay. Now: `WINDOWS_READY ver=1.3.2
# sig=<base64>` on line 1, prose explanation on line 3+.

set -u

# Build a "KEY=VAL [KEY=VAL ...]" line from the args. Each arg must already
# be in `KEY=VAL` form (the leading event token like WINDOWS_READY is just
# `KEY=` with the rest blank — pass `WINDOWS_READY=$VER` if you want the
# token in the first slot).
_report_kv_line() {
  local out=""
  local pair
  for pair in "$@"; do
    if [[ -z "$out" ]]; then out="$pair"; else out="$out $pair"; fi
  done
  printf '%s\n' "$out"
}

# report_ready KEY=VAL ...
# Followed by stdin = prose body (optional). Emits machine line + \n\n + prose.
#
# Usage:
#   report_ready WINDOWS_READY=$VER sig="$SIG" <<EOF
#   Built CodeTerm $VER.nsis.zip; signed with $KEY_NAME.
#   EOF
report_ready() {
  _report_kv_line "$@"
  printf '\n'
  if ! [ -t 0 ]; then cat; fi
}

# report_blocked STEP MSG
# Followed by stdin = optional prose body.
report_blocked() {
  local step="${1:?report_blocked needs STEP}"
  local msg="${2:?report_blocked needs MSG}"
  printf 'BLOCKED step=%s err=%s\n' "$step" "$msg"
  printf '\n'
  if ! [ -t 0 ]; then cat; fi
}

# Validate that a string LOOKS like a base64 minisign sig (single line,
# only base64 chars, length > 200). Echoes the trimmed sig on success,
# exits non-zero on failure. Use to reject "BLOCKED" in place of a sig
# before reporting it as ready.
#
# Usage:
#   SIG=$(validate_b64_sig "$RAW_SIG") || { report_blocked sign "invalid sig"; exit 1; }
validate_b64_sig() {
  local raw="${1:-}"
  local trimmed
  trimmed=$(printf '%s' "$raw" | tr -d '\n\r ')
  if [ ${#trimmed} -lt 200 ]; then return 1; fi
  if ! [[ "$trimmed" =~ ^[A-Za-z0-9+/=]+$ ]]; then return 1; fi
  printf '%s' "$trimmed"
}
