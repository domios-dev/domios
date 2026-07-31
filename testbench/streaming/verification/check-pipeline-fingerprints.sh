#!/usr/bin/env bash
# Post-merge proof: compare the worktree copies of the four streaming-pipeline
# files (what hot-reload actually serves) against a given merged commit's copies.
# Usage: ./check-pipeline-fingerprints.sh <merged-sha>
set -u
SHA="${1:?usage: check-pipeline-fingerprints.sh <merged-sha>}"
REPO=/Users/kandrey/Developer/codeterm-worktrees/lane-streaming-consilium
FILES=(
  src/components/Terminal/streamPreviewMatch.ts
  src/components/Terminal/useStreamPreview.ts
  testbench/streaming/simulate.ts
  testbench/streaming/score-all.mjs
)
drift=0
for f in "${FILES[@]}"; do
  work="$(shasum -a 256 "$REPO/$f" | awk '{print $1}')"
  merged="$(git -C "$REPO" show "$SHA:$f" 2>/dev/null | shasum -a 256 | awk '{print $1}')"
  pre="$(grep -F "$f" "$(dirname "$0")/live/pipeline-fingerprints-premerge.txt" | awk '{print $1}')"
  if [ "$work" = "$merged" ] && [ "$work" = "$pre" ]; then
    echo "OK    $f"
  else
    drift=1
    echo "DRIFT $f"
    echo "        worktree=$work"
    echo "        merged  =$merged"
    echo "        premerge=$pre"
  fi
done
if [ "$drift" = 0 ]; then
  echo "RESULT: no drift — signed table stands, no re-runs needed"
else
  echo "RESULT: drift detected — re-run the full 8-capture sweep before signing"
  exit 1
fi
