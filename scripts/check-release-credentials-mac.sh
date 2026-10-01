#!/usr/bin/env bash
# Release signing and notarization preflight for the macOS builder. Collects every
# result before exiting, so one bad credential reports the whole picture.
set -u
rc=0
echo "host: $(uname -msr)"
security find-identity -v -p codesigning | grep -q ANDREY && echo "✅ Developer ID present" || { echo "::error::no Developer ID"; rc=1; }
test -f "$HOME/.tauri/codeterm.key" && echo "✅ minisign key" || { echo "::error::missing minisign key"; rc=1; }
gh auth status 2>&1 | grep -q "Logged in" && echo "✅ gh auth" || { echo "::error::gh not authed"; rc=1; }
docker image inspect codeterm-build-linux-arm64 >/dev/null 2>&1 && echo "✅ docker arm64 image" || echo "::warning::bake codeterm-build-linux-arm64"
docker image inspect codeterm-build-linux-x64 >/dev/null 2>&1 && echo "✅ docker x64 image" || echo "::warning::bake codeterm-build-linux-x64"

# The keychain profile is deliberately not consulted: it cannot be provisioned
# non-interactively, so it rots silently (cost v1.7.5 a run).
if [ -z "${APPLE_ID:-}" ] && [ -f "$HOME/.claude/.secrets/apple-notary.env" ]; then
  set -a; . "$HOME/.claude/.secrets/apple-notary.env"; set +a
fi
NOTARY_PASSWORD="${APPLE_APP_SPECIFIC_PASSWORD:-${APPLE_PASSWORD:-}}"
log="${RUNNER_TEMP:-/tmp}/codeterm-notary-preflight.log"
if [ -z "${APPLE_ID:-}" ] || [ -z "${APPLE_TEAM_ID:-}" ] || [ -z "$NOTARY_PASSWORD" ]; then
  echo "::error::notary credentials missing — set the APPLE_ID / APPLE_TEAM_ID / APPLE_PASSWORD repo secrets"; rc=1
elif xcrun notarytool history --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
       --password "$NOTARY_PASSWORD" >"$log" 2>&1; then
  echo "✅ Apple notary credentials accepted"
else
  tail -20 "$log"
  if grep -qi "agreement" "$log"; then
    echo "::error::Apple requires a new Developer agreement — sign it at https://developer.apple.com/account (team $APPLE_TEAM_ID); the password is fine"
  elif grep -qE "401|Unauthorized|invalid credentials" "$log"; then
    echo "::error::Apple notary rejected the credentials — rotate the APPLE_PASSWORD app-specific password"
  else
    echo "::error::Apple notary check failed; see the output above"
  fi
  rc=1
fi
exit "$rc"
