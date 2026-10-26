#!/usr/bin/env bash
# Caller: CI release-credentials.yml (notary-probe); skill github-release
# Signs, notarizes, staples and assesses a trivial app on this runner, so a notary failure shows in minutes.
set -euo pipefail
: "${RELEASE_KEYCHAIN:?Import the job signing keychain first}"
: "${APPLE_SIGNING_IDENTITY:?Provide APPLE_SIGNING_IDENTITY}"
: "${APPLE_KEYCHAIN_PROFILE:?Import the job notary profile first}"
arch="${1:-$(uname -m)}"
# Outside RELEASE_SIGNING_DIR: the signing cleanup removes that directory and refuses unknown contents.
work="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/notary-probe.XXXXXX")"
trap 'rm -rf "$work"' EXIT
app="$work/NotaryProbe.app"
mkdir -p "$app/Contents/MacOS"
printf '#include <stdio.h>\nint main(void) { puts("notary probe"); return 0; }\n' > "$work/probe.c"
clang -arch "$arch" -mmacosx-version-min=11.0 -o "$app/Contents/MacOS/NotaryProbe" "$work/probe.c"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.domios.notary-probe</string>
  <key>CFBundleExecutable</key><string>NotaryProbe</string>
  <key>CFBundleName</key><string>NotaryProbe</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
</dict></plist>
PLIST
echo "binary: $(file -b "$app/Contents/MacOS/NotaryProbe")"
codesign --force --options runtime --timestamp --keychain "$RELEASE_KEYCHAIN" --sign "$APPLE_SIGNING_IDENTITY" "$app"
codesign --verify --strict --verbose=2 "$app"
ditto -c -k --keepParent "$app" "$work/NotaryProbe.zip"
auth=(--keychain-profile "$APPLE_KEYCHAIN_PROFILE" --keychain "$RELEASE_KEYCHAIN")
started=$(date +%s)
result="$(xcrun notarytool submit "$work/NotaryProbe.zip" "${auth[@]}" --wait --output-format json)" || true
id=$(printf '%s' "$result" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("id",""))' 2>/dev/null || true)
status=$(printf '%s' "$result" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("status",""))' 2>/dev/null || true)
echo "notary submission: id=${id:-none} status=${status:-none} wait=$(( $(date +%s) - started ))s"
if [ "$status" != Accepted ]; then
  printf '%s\n' "$result" >&2
  [ -z "$id" ] || xcrun notarytool log "$id" "${auth[@]}" >&2 || true
  echo "::error::Notarization probe rejected (id=${id:-none}, status=${status:-none})"
  exit 1
fi
bash "$(dirname "$0")/staple-with-retry.sh" "$app"
xcrun stapler validate "$app"
spctl -a -vv "$app" 2>&1 | tee "$work/spctl.txt"
grep -q "accepted" "$work/spctl.txt" || { echo "::error::spctl rejected the stapled probe"; exit 1; }
echo "notary probe PASS: arch=$arch id=$id status=$status $(grep -m1 '^source=' "$work/spctl.txt" || true)"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  echo "Notary probe ($arch): submission $id $status; spctl $(tr '\n' ' ' < "$work/spctl.txt")" >> "$GITHUB_STEP_SUMMARY"
fi
