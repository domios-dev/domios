#!/usr/bin/env bash
# Caller: build.sh notarize_local_macos_package, scripts/release-notary-probe.sh, notarize-dmg.sh, mac-bundle.sh, release-bundle-smoke.mjs
# Staples or validates a notarized app or disk image, retrying because Apple's ticket service drops connections.
set -euo pipefail

target="${1:?usage: staple-with-retry.sh <app-or-dmg> [staple|validate]}"
action="${2:-staple}"
case "$action" in staple|validate) ;; *) echo "unknown stapler action: $action" >&2; exit 2 ;; esac
attempts=5
for attempt in $(seq 1 "$attempts"); do
  if output="$(xcrun stapler "$action" "$target" 2>&1)"; then
    printf '%s\n' "$output"
    exit 0
  fi
  printf '%s\n' "$output" >&2
  [ "$attempt" -lt "$attempts" ] || break
  echo "stapler $action attempt $attempt/$attempts failed for $target; retrying" >&2
  # Apple's ticket service needs a moment after a dropped connection or a fresh notarization.
  sleep $((attempt * 10))
done
echo "ERROR: stapler $action failed $attempts times for $target" >&2
exit 1
