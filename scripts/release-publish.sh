#!/usr/bin/env bash
# Caller: CI release-publish.yml; skill github-release
# Publishes the retained bytes of a green release.yml dry run as vX.Y.Z in the source repo and the public mirror.
# rehearsal runs every read, download, aggregation and signature check of a publication and changes nothing.
set -euo pipefail
RUN_ID="${1:?usage: release-publish.sh <dry-run run id> <vX.Y.Z> rehearsal|publish}"
TAG="${2:?release tag vX.Y.Z required}"
MODE="${3:?rehearsal or publish required}"
SOURCE_REPO="$(node scripts/source-repository.mjs)"
MIRROR_REPO=rollacode/codeterm-releases
: "${GH_TOKEN:?GH_TOKEN must write $SOURCE_REPO}" "${MIRROR_TOKEN:?MIRROR_TOKEN must write $MIRROR_REPO}"
[[ "$TAG" =~ ^v([0-9]+\.[0-9]+\.[0-9]+)$ ]] || { echo "::error::tag must be vX.Y.Z (got '$TAG')"; exit 1; }
VER="${BASH_REMATCH[1]}"
case "$MODE" in rehearsal|publish) ;; *) echo "::error::mode must be rehearsal or publish"; exit 1 ;; esac
step() { echo "=== $* ($(date -u +%T))"; }
json() { node -e 'let d="";process.stdin.on("data",c=>d+=c).on("end",()=>{const v=JSON.parse(d);console.log(process.argv[1].split(".").reduce((o,k)=>o[k],v))})' "$1"; }

step "qualified run $RUN_ID"
run="$(gh api "repos/$SOURCE_REPO/actions/runs/$RUN_ID")"
SHA="$(json head_sha <<<"$run")"; BRANCH="$(json head_branch <<<"$run")"
[ "$BRANCH" = "release/$VER" ] || { echo "::error::run $RUN_ID ran on $BRANCH, not release/$VER"; exit 1; }
node scripts/release-promote.mjs check "$RUN_ID" "$SHA" "$BRANCH"
if gh release view "$TAG" --repo "$SOURCE_REPO" --json isDraft --jq .isDraft 2>/dev/null | grep -qx false; then
  [ "$MODE" = rehearsal ] || { echo "::error::$TAG is already published; a further change needs release/X.Y.(Z+1)"; exit 1; }
  echo "::notice::$TAG is already published; the rehearsal only reads"
fi
work="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/release-publish.XXXXXX")"
trap 'rm -rf "$work"' EXIT

if [ "$MODE" = rehearsal ]; then
  step "rehearsal: token reach and retained evidence of $RUN_ID"
  gh api "repos/$SOURCE_REPO" --jq .permissions.push | grep -qx true || { echo "::error::GH_TOKEN cannot write $SOURCE_REPO"; exit 1; }
  GH_TOKEN="$MIRROR_TOKEN" gh api "repos/$MIRROR_REPO" --jq .permissions.push | grep -qx true \
    || { echo "::error::MIRROR_TOKEN cannot write $MIRROR_REPO"; exit 1; }
  node scripts/release-dry-run-manifest.mjs aggregate "$TAG" "$SHA" "$RUN_ID" "$work/input" "$work/output"
  echo "rehearsal PASS: $TAG from run $RUN_ID ($SHA) would publish; nothing was changed"
  exit 0
fi

step "drafts"
gh release view "$TAG" --repo "$SOURCE_REPO" >/dev/null 2>&1 || gh release create "$TAG" --repo "$SOURCE_REPO" --target "$SHA" \
  --draft --title "Domios $TAG" --notes "See the assets to download this version and install."
GH_TOKEN="$MIRROR_TOKEN" gh release view "$TAG" --repo "$MIRROR_REPO" >/dev/null 2>&1 || GH_TOKEN="$MIRROR_TOKEN" gh release create "$TAG" \
  --repo "$MIRROR_REPO" --draft --title "Domios $TAG" --notes "Auto-mirrored from $SOURCE_REPO. See the source repo for full release notes."

step "retained bytes to both drafts, read back against their receipts"
PUBLISH_MIRROR=true node scripts/release-promote.mjs publish "$TAG" "$SHA" "$RUN_ID" "$work/promote"

step "channel manifests"
mkdir -p "$work/frag/raw" "$work/frag/main" "$work/frag/deb" "$work/man" "$work/draft"
gh release download "$TAG" --repo "$SOURCE_REPO" --pattern 'updater-*.json' --dir "$work/frag/raw"
for fragment in "$work"/frag/raw/updater-*.json; do
  name="$(basename "$fragment" .json)"; destination="$work/frag/main/$name"
  [[ "$name" == *-deb ]] && destination="$work/frag/deb/$name"
  mkdir -p "$destination"; cp "$fragment" "$destination/domios-latest.json"
done
node scripts/merge-latest-json.js --in "$work/frag/main" --out "$work/man/domios-latest.json"
node scripts/merge-latest-json.js --in "$work/frag/deb" --out "$work/man/domios-latest-deb.json"
node scripts/legacy-updater-refusal.mjs "$work/man"
gh release upload "$TAG" "$work/man/"*.json --repo "$SOURCE_REPO" --clobber
GH_TOKEN="$MIRROR_TOKEN" gh release upload "$TAG" "$work/man/"*.json --repo "$MIRROR_REPO" --clobber

step "draft signatures against the mirror bytes"
for manifest in domios-latest.json domios-latest-deb.json; do
  node -e 'for (const p of Object.values(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).platforms)) console.log(p.url.split("/").pop())' \
    "$work/man/$manifest" | tr -d '\r' | while IFS= read -r asset; do
      GH_TOKEN="$MIRROR_TOKEN" gh release download "$TAG" --repo "$MIRROR_REPO" --pattern "$asset" --dir "$work/draft" --clobber
    done
  node scripts/verify-updater-manifest.mjs --manifest "$work/man/$manifest" --local "$work/draft" --require-version "$VER"
done

step "tag $TAG -> $SHA"
if git ls-remote --exit-code origin "refs/tags/$TAG" >/dev/null 2>&1; then
  [ "$(git ls-remote origin "refs/tags/$TAG^{}" | cut -f1)" = "$SHA" ] || { echo "::error::$TAG already points elsewhere"; exit 1; }
else
  git fetch --no-tags origin "$SHA"
  git -c user.name="github-actions[bot]" -c user.email="41898282+github-actions[bot]@users.noreply.github.com" tag -a "$TAG" "$SHA" -m "$TAG"
  git push origin "refs/tags/$TAG"
fi

step "publish"
GH_TOKEN="$MIRROR_TOKEN" node scripts/sync-release-notes.mjs --tag "$TAG"
gh release edit "$TAG" --repo "$SOURCE_REPO" --draft=false
GH_TOKEN="$MIRROR_TOKEN" gh release edit "$TAG" --repo "$MIRROR_REPO" --draft=false --latest

step "published manifests"
node scripts/legacy-updater-refusal.mjs verify "https://github.com/$MIRROR_REPO/releases/download/$TAG/latest.json?cb=$(date +%s)" "$VER"
node scripts/verify-updater-manifest.mjs --manifest "https://github.com/$MIRROR_REPO/releases/download/$TAG/domios-latest.json?cb=$(date +%s)" \
  --require-version "$VER" --require-platform darwin-aarch64 --require-platform darwin-x86_64 --require-platform windows-x86_64 \
  --require-platform windows-aarch64 --require-platform linux-x86_64 --require-platform linux-aarch64
node scripts/verify-updater-manifest.mjs --manifest "https://github.com/$MIRROR_REPO/releases/download/$TAG/domios-latest-deb.json?cb=$(date +%s)" \
  --require-version "$VER" --require-platform linux-x86_64
echo "published $TAG from run $RUN_ID ($SHA)"
