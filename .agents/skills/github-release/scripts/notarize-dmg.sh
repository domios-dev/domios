#!/usr/bin/env bash
# Submit a DMG to Apple notary, wait, staple, verify with spctl.
# Quiet mode: Apple's "Current status: In Progress." dots are dropped — only
# the final outcome line is printed, so an observing agent burns one event
# instead of 7-10. On failure, prints the full notary log for triage.
#
# Usage: notarize-dmg.sh <path-to-dmg>

set -euo pipefail

DMG="${1:-}"
[ -n "$DMG" ] || { echo "usage: notarize-dmg.sh <path-to-dmg>" >&2; exit 2; }
[ -f "$DMG" ] || { echo "ERROR: $DMG not found" >&2; exit 1; }

TMPLOG=$(mktemp -t notarize-XXXXXX.log)
trap 'rm -f "$TMPLOG"' EXIT

: "${RELEASE_KEYCHAIN:?Import the job signing keychain first}"
NOTARY_AUTH=(--keychain-profile "${APPLE_KEYCHAIN_PROFILE:?}" --keychain "$RELEASE_KEYCHAIN")

if ! xcrun notarytool submit "$DMG" "${NOTARY_AUTH[@]}" --wait \
       > "$TMPLOG" 2>&1; then
  echo "FAIL: notarytool submit returned non-zero. Tail:" >&2
  tail -30 "$TMPLOG" >&2
  exit 1
fi

# Extract submission id + final status from the (verbose) log.
SUBID=$(awk '/Submission ID received/{getline; sub(/^  id: /, ""); print; exit}' "$TMPLOG")
FINAL=$(grep -E '^  status:' "$TMPLOG" | tail -1 | awk '{print $2}')

if [ "$FINAL" != "Accepted" ]; then
  echo "FAIL: Apple rejected (status=$FINAL, id=$SUBID)" >&2
  echo "--- detailed issues ---" >&2
  xcrun notarytool log "$SUBID" "${NOTARY_AUTH[@]}" 2>&1 | jq '.issues // .' >&2 || cat "$TMPLOG" >&2
  exit 1
fi

xcrun stapler staple "$DMG" >/dev/null 2>&1 \
  || { echo "FAIL: stapler refused (notary accepted but ticket not yet propagated — retry in 30s)" >&2; exit 1; }

if spctl -a -t open --context context:primary-signature "$DMG" >/dev/null 2>&1; then
  echo "OK $DMG (id=$SUBID, stapled, spctl accepted)"
else
  echo "FAIL: spctl rejected stapled DMG" >&2
  spctl -a -t open --context context:primary-signature -vv "$DMG" >&2
  exit 1
fi
