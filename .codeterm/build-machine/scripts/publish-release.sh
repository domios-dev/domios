#!/usr/bin/env bash
# Publish a built+signed release: private archive (rollacode/codeterm) + public mirror
# (rollacode/codeterm-releases) + signed latest.json. Verifies anon updater URLs.
#
# Expects $ART to contain the built+signed artifacts:
#   CodeTerm_<ver>_aarch64.dmg / _x64.dmg
#   CodeTerm_<ver>_aarch64.app.tar.gz(.sig) / _x64.app.tar.gz(.sig)
#   CodeTerm_<ver>_x64-setup.exe(.sig) / _arm64-setup.exe(.sig)
#   CodeTerm_<ver>_arm64.deb / _amd64.deb
#
# Usage: publish-release.sh <ver> [artifacts-dir]
set -euo pipefail
VER="${1:?usage: publish-release.sh <ver> [art-dir]}"
ART="${2:-/tmp/codeterm-${VER}-artifacts}"
SRC=rollacode/codeterm
MIRROR=rollacode/codeterm-releases
cd "$ART"

# Versionless mac updater tarballs -> stable /latest/download/ URLs across versions.
cp -f "CodeTerm_${VER}_aarch64.app.tar.gz"     CodeTerm_aarch64.app.tar.gz
cp -f "CodeTerm_${VER}_aarch64.app.tar.gz.sig" CodeTerm_aarch64.app.tar.gz.sig
cp -f "CodeTerm_${VER}_x64.app.tar.gz"         CodeTerm_x64.app.tar.gz
cp -f "CodeTerm_${VER}_x64.app.tar.gz.sig"     CodeTerm_x64.app.tar.gz.sig

ARM_SIG=$(tr -d '\n' < CodeTerm_aarch64.app.tar.gz.sig)
X64_SIG=$(tr -d '\n' < CodeTerm_x64.app.tar.gz.sig)
WIN_SIG=$(tr -d '\n' < "CodeTerm_${VER}_x64-setup.exe.sig")
PUB=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
B="https://github.com/${MIRROR}/releases/latest/download"
cat > latest.json <<JSON
{
  "version": "$VER",
  "notes": "See the assets to download this version and install.",
  "pub_date": "$PUB",
  "platforms": {
    "darwin-aarch64": { "url": "$B/CodeTerm_aarch64.app.tar.gz", "signature": "$ARM_SIG" },
    "darwin-x86_64":  { "url": "$B/CodeTerm_x64.app.tar.gz", "signature": "$X64_SIG" },
    "windows-x86_64": { "url": "$B/CodeTerm_${VER}_x64-setup.exe", "signature": "$WIN_SIG" }
  }
}
JSON
python3 -m json.tool latest.json >/dev/null

PRIV_ASSETS=( "CodeTerm_${VER}_aarch64.dmg" "CodeTerm_${VER}_x64.dmg"
  CodeTerm_aarch64.app.tar.gz CodeTerm_aarch64.app.tar.gz.sig
  CodeTerm_x64.app.tar.gz CodeTerm_x64.app.tar.gz.sig
  "CodeTerm_${VER}_x64-setup.exe" "CodeTerm_${VER}_x64-setup.exe.sig"
  "CodeTerm_${VER}_arm64-setup.exe" "CodeTerm_${VER}_arm64-setup.exe.sig"
  "CodeTerm_${VER}_arm64.deb" "CodeTerm_${VER}_amd64.deb" )

echo "### private release v$VER"
gh release view "v$VER" --repo "$SRC" >/dev/null 2>&1 || \
  gh release create "v$VER" --repo "$SRC" --draft --title "v$VER" --notes "CodeTerm v$VER"
gh release upload "v$VER" --repo "$SRC" --clobber "${PRIV_ASSETS[@]}"
gh release edit "v$VER" --repo "$SRC" --draft=false

echo "### public mirror v$VER (+ latest.json)"
gh release view "v$VER" --repo "$MIRROR" >/dev/null 2>&1 || \
  gh release create "v$VER" --repo "$MIRROR" --draft --title "CodeTerm v$VER" --notes "Auto-mirrored release artifacts for CodeTerm v$VER."
gh release upload "v$VER" --repo "$MIRROR" --clobber "${PRIV_ASSETS[@]}" latest.json
gh release edit "v$VER" --repo "$MIRROR" --draft=false --latest

echo "### verify anon updater URLs"
sleep 4
for u in latest.json CodeTerm_aarch64.app.tar.gz CodeTerm_x64.app.tar.gz "CodeTerm_${VER}_x64-setup.exe"; do
  code=$(curl -sIL -o /dev/null -w '%{http_code}' "$B/$u")
  echo "  $code  $u"; [ "$code" = 200 ] || { echo "::error::updater URL $u -> $code"; exit 1; }
done
LIVE=$(curl -sL "$B/latest.json" | python3 -c "import json,sys;print(json.load(sys.stdin)['version'])")
[ "$LIVE" = "$VER" ] && echo "PUBLISH_OK v$VER is live" || { echo "::error::manifest=$LIVE != $VER"; exit 1; }
