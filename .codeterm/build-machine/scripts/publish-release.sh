#!/usr/bin/env bash
# Publish a built+signed release: private archive (rollacode/codeterm) + public mirror
# (rollacode/codeterm-releases) + signed latest.json. Verifies anon updater URLs.
#
# Expects $ART to contain the built+signed mac+linux artifacts:
#   CodeTerm_<ver>_aarch64.dmg / _x64.dmg
#   CodeTerm_<ver>_aarch64.app.tar.gz(.sig) / _x64.app.tar.gz(.sig)
#   CodeTerm_<ver>_arm64.deb / _amd64.deb
#   CodeTerm_<ver>_aarch64.AppImage(.sig)
#
# Manifest model: this script publishes ONLY its own per-platform updater
# fragments (darwin-aarch64, darwin-x86_64, linux-aarch64) to the source
# release, then rebuilds latest.json by merging ALL fragments present plus the
# existing manifest (monotonic, see merge-latest-json.js --base). The Windows
# (release-windows.yml) and x86_64-Linux (release-linux.yml) workflows publish
# their own fragments + run the same merge — so whichever finishes last yields a
# complete manifest, and no workflow can ever drop another's platform. The .sig
# blobs feed the fragment `signature` field but are NOT published as assets (the
# Tauri updater reads the signature string from latest.json, never the .sig).
#
# Usage: publish-release.sh <ver> [artifacts-dir]
set -euo pipefail
VER="${1:?usage: publish-release.sh <ver> [art-dir]}"
ART="${2:-/tmp/codeterm-${VER}-artifacts}"
SRC=rollacode/codeterm
MIRROR=rollacode/codeterm-releases
TAG="v$VER"
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../../.." && pwd)
MERGE="$REPO_ROOT/scripts/merge-latest-json.js"
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

# Per-platform updater fragments owned by this (mac+linux-arm) runner.
mkdir -p _frags
VER="$VER" PUB="$PUB" B="$B" ARM_SIG="$ARM_SIG" X64_SIG="$X64_SIG" \
LINUX_ARM_SIG="$LINUX_ARM_SIG" python3 - <<'PY'
import json, os
ver, B, pub = os.environ["VER"], os.environ["B"], os.environ["PUB"]
notes = "See the assets to download this version and install."
frags = {
    "darwin-aarch64": {"url": f"{B}/CodeTerm_aarch64.app.tar.gz", "signature": os.environ["ARM_SIG"]},
    "darwin-x86_64":  {"url": f"{B}/CodeTerm_x64.app.tar.gz",     "signature": os.environ["X64_SIG"]},
    "linux-aarch64":  {"url": f"{B}/CodeTerm_{ver}_aarch64.AppImage", "signature": os.environ["LINUX_ARM_SIG"]},
}
for key, entry in frags.items():
    with open(f"_frags/updater-{key}.json", "w") as f:
        json.dump({"version": ver, "notes": notes, "pub_date": pub,
                   "platforms": {key: entry}}, f, indent=2)
PY

PRIV_ASSETS=( "CodeTerm_${VER}_aarch64.dmg" "CodeTerm_${VER}_x64.dmg"
  CodeTerm_aarch64.app.tar.gz CodeTerm_x64.app.tar.gz
  "CodeTerm_${VER}_aarch64.AppImage"
  "CodeTerm_${VER}_arm64.deb" )
# x86_64 Linux (.deb / .AppImage) is built + published on the ubunta runner;
# fold its .deb in here only if it happens to be present in $ART.
[ -f "CodeTerm_${VER}_amd64.deb" ] && PRIV_ASSETS+=("CodeTerm_${VER}_amd64.deb")

echo "### private release $TAG"
gh release view "$TAG" --repo "$SRC" >/dev/null 2>&1 || \
  gh release create "$TAG" --repo "$SRC" --draft --title "$TAG" --notes "CodeTerm $TAG"
gh release upload "$TAG" --repo "$SRC" --clobber "${PRIV_ASSETS[@]}"
gh release upload "$TAG" --repo "$SRC" --clobber _frags/updater-*.json
gh release edit "$TAG" --repo "$SRC" --draft=false

echo "### rebuild merged latest.json (fragments + existing manifest, monotonic)"
rm -rf _merge && mkdir -p _merge
gh release download "$TAG" --repo "$SRC" --pattern 'updater-*.json' --dir _merge 2>/dev/null || true
# subdir-per-fragment so the recursive finder treats each as a fragment. The -deb
# fragment claims the same linux-x86_64 key as the AppImage one — it owns
# latest-deb.json only, and merged here it would win and break AppImage updates.
i=0; for f in _merge/updater-*.json; do
  case "$(basename "$f")" in *-deb.json) rm -f "$f"; continue ;; esac
  d="_merge/frag-$i"; mkdir -p "$d"; mv "$f" "$d/latest.json"; i=$((i+1))
done
gh release download "$TAG" --repo "$SRC" --pattern 'latest.json' --dir _merge 2>/dev/null || true
BASE_ARG=()
[ -f _merge/latest.json ] && BASE_ARG=(--base _merge/latest.json)
node "$MERGE" --in _merge --out latest.json "${BASE_ARG[@]}"
python3 -m json.tool latest.json >/dev/null

echo "### publish merged manifest (source + public mirror)"
gh release upload "$TAG" --repo "$SRC" --clobber latest.json
gh release view "$TAG" --repo "$MIRROR" >/dev/null 2>&1 || \
  gh release create "$TAG" --repo "$MIRROR" --draft --title "CodeTerm $TAG" --notes "Auto-mirrored release artifacts for CodeTerm $TAG."
gh release upload "$TAG" --repo "$MIRROR" --clobber "${PRIV_ASSETS[@]}" latest.json
gh release edit "$TAG" --repo "$MIRROR" --draft=false --latest

echo "### verify anon updater URLs"
sleep 4
for u in latest.json CodeTerm_aarch64.app.tar.gz CodeTerm_x64.app.tar.gz "CodeTerm_${VER}_aarch64.AppImage"; do
  code=$(curl -sIL -o /dev/null -w '%{http_code}' "$B/$u")
  echo "  $code  $u"; [ "$code" = 200 ] || { echo "::error::updater URL $u -> $code"; exit 1; }
done
LIVE=$(curl -sL "$B/latest.json" | python3 -c "import json,sys;print(json.load(sys.stdin)['version'])")
[ "$LIVE" = "$VER" ] && echo "PUBLISH_OK v$VER is live" || { echo "::error::manifest=$LIVE != $VER"; exit 1; }
