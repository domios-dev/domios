#!/usr/bin/env bash
# One-shot compact dashboard for a release in progress. Answers all the
# "where are we" questions in a single call so a coordinating agent stops
# polling 4 different `gh` commands.
#
# Usage: release-status.sh <ver>

set -euo pipefail

VER="${1:-}"
[ -n "$VER" ] || { echo "usage: release-status.sh <ver>" >&2; exit 2; }
TAG="v$VER"
MIRROR=rollacode/codeterm-releases

# Private release assets
PRIV=$(gh release view "$TAG" --json isDraft,assets 2>/dev/null \
  | jq -r '"draft=\(.isDraft) assets=[\(.assets | map(.name) | join(","))]"' \
  || echo "missing")
echo "private:  $PRIV"

# Public mirror assets
PUB=$(gh release view "$TAG" --repo "$MIRROR" --json assets 2>/dev/null \
  | jq -r '"assets=[\(.assets | map(.name) | join(","))]"' \
  || echo "missing")
echo "public :  $PUB"

# latest.json platform completeness (anon GET)
MANIFEST_JSON=$(curl -sSL "https://github.com/$MIRROR/releases/latest/download/latest.json" 2>/dev/null)
if [ -n "$MANIFEST_JSON" ]; then
  PLATS=$(echo "$MANIFEST_JSON" | jq -r '
    .platforms
    | to_entries
    | map("\(.key)=\(if .value.signature == "" then "EMPTY" else "ok" end)")
    | join(" ")')
  MVER=$(echo "$MANIFEST_JSON" | jq -r '.version')
  echo "manifest: v=$MVER  $PLATS"
else
  echo "manifest: missing"
fi

# CI workflow runs for this tag
echo "ci:"
for wf in release-macos.yml release-linux.yml release-windows.yml; do
  ROW=$(gh run list --workflow "$wf" --branch "$TAG" --limit 1 --json status,conclusion,databaseId 2>/dev/null \
    | jq -r '.[0] | if . == null then "no-runs" else "\(.status) \(.conclusion // "—") id=\(.databaseId)" end')
  printf "  %-22s %s\n" "${wf%.yml}" "$ROW"
done
