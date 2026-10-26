#!/usr/bin/env bash
# Caller: build.sh notarize_local_macos_package, scripts/release-notary-probe.sh, notarize-dmg.sh
# Staples a notarized app or disk image, retrying because Apple's ticket service drops connections.
set -euo pipefail

target="${1:?usage: staple-with-retry.sh <app-or-dmg>}"
attempts=5
for attempt in $(seq 1 "$attempts"); do
  if xcrun stapler staple "$target"; then
    exit 0
  fi
  [ "$attempt" -lt "$attempts" ] || break
  echo "stapler attempt $attempt/$attempts failed for $target; retrying" >&2
  # Apple's ticket service needs a moment after a dropped connection or a fresh notarization.
  sleep $((attempt * 10))
done
echo "ERROR: stapler failed $attempts times for $target" >&2
exit 1
