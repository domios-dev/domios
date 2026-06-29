#!/usr/bin/env bash
# Publish a built+signed release: private archive (rollacode/codeterm) + public mirror
# (rollacode/codeterm-releases) + signed latest.json. Verifies anon updater URLs.
#
# Expects $ART to contain the built+signed mac+linux artifacts:
#   CodeTerm_<ver>_aarch64.dmg / _x64.dmg
#   CodeTerm_<ver>_aarch64.app.tar.gz(.sig) / _x64.app.tar.gz(.sig)
#   CodeTerm_<ver>_arm64.deb / _amd64.deb
#   CodeTerm_<ver>_aarch64.AppImage(.sig)
# Windows installers are built+signed on the ROG self-hosted runner
# (release-windows.yml) and published from there — they are normally absent here
# and are folded in only if a CodeTerm_<ver>_*-setup.exe happens to be in $ART.
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
LINUX_ARM_SIG=$(tr -d '\n' < "CodeTerm_${VER}_aarch64.AppImage.sig")
PUB=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
B="https://github.com/${MIRROR}/releases/latest/download"

# Windows is published by release-windows.yml (ROG runner); fold it into the
# manifest here only if a signed setup.exe happens to be present in $ART.
WIN_SIG=""
[ -f "CodeTerm_${VER}_x64-setup.exe.sig" ] && WIN_SIG=$(tr -d '\n' < "CodeTerm_${VER}_x64-setup.exe.sig")

VER="$VER" PUB="$PUB" B="$B" ARM_SIG="$ARM_SIG" X64_SIG="$X64_SIG" \
LINUX_ARM_SIG="$LINUX_ARM_SIG" WIN_SIG="$WIN_SIG" python3 - > latest.json <<'PY'
import json, os
ver, B = os.environ["VER"], os.environ["B"]
plats = {
    "darwin-aarch64": {"url": f"{B}/CodeTerm_aarch64.app.tar.gz", "signature": os.environ["ARM_SIG"]},
    "darwin-x86_64":  {"url": f"{B}/CodeTerm_x64.app.tar.gz",     "signature": os.environ["X64_SIG"]},
    "linux-aarch64":  {"url": f"{B}/CodeTerm_{ver}_aarch64.AppImage", "signature": os.environ["LINUX_ARM_SIG"]},
}
if os.environ.get("WIN_SIG"):
    plats["windows-x86_64"] = {"url": f"{B}/CodeTerm_{ver}_x64-setup.exe", "signature": os.environ["WIN_SIG"]}
print(json.dumps({
    "version": ver,
    "notes": "See the assets to download this version and install.",
    "pub_date": os.environ["PUB"],
    "platforms": plats,
}, indent=2))
PY
python3 -m json.tool latest.json >/dev/null

PRIV_ASSETS=( "CodeTerm_${VER}_aarch64.dmg" "CodeTerm_${VER}_x64.dmg"
  CodeTerm_aarch64.app.tar.gz CodeTerm_aarch64.app.tar.gz.sig
  CodeTerm_x64.app.tar.gz CodeTerm_x64.app.tar.gz.sig
  "CodeTerm_${VER}_aarch64.AppImage" "CodeTerm_${VER}_aarch64.AppImage.sig"
  "CodeTerm_${VER}_arm64.deb" )
# Windows (release-windows.yml) and x86_64 Linux (release-linux.yml) are built +
# published on their own ROG runners — fold them in here only if present in $ART.
for w in "CodeTerm_${VER}_amd64.deb" \
         "CodeTerm_${VER}_x64-setup.exe" "CodeTerm_${VER}_x64-setup.exe.sig" \
         "CodeTerm_${VER}_arm64-setup.exe" "CodeTerm_${VER}_arm64-setup.exe.sig"; do
  [ -f "$w" ] && PRIV_ASSETS+=("$w")
done

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
VERIFY_URLS=( latest.json CodeTerm_aarch64.app.tar.gz CodeTerm_x64.app.tar.gz "CodeTerm_${VER}_aarch64.AppImage" )
[ -n "$WIN_SIG" ] && VERIFY_URLS+=("CodeTerm_${VER}_x64-setup.exe")
for u in "${VERIFY_URLS[@]}"; do
  code=$(curl -sIL -o /dev/null -w '%{http_code}' "$B/$u")
  echo "  $code  $u"; [ "$code" = 200 ] || { echo "::error::updater URL $u -> $code"; exit 1; }
done
LIVE=$(curl -sL "$B/latest.json" | python3 -c "import json,sys;print(json.load(sys.stdin)['version'])")
[ "$LIVE" = "$VER" ] && echo "PUBLISH_OK v$VER is live" || { echo "::error::manifest=$LIVE != $VER"; exit 1; }
