#!/usr/bin/env bash
set -euo pipefail
if [[ ${B3_GO_LIVE:-} != yes ]]; then
  echo 'Stage 2 only: requires Orchestrator GO live' >&2
  exit 7
fi
repo=/Users/kandrey/Developer/codeterm-worktrees/rc-mac
export CODETERM_INSTANCE=aiderqa CODETERM_API_PORT=7697
source "$repo/evals/_lib/lib.sh"
mode=${1:?preflight or snapshot}
peer=${2:?exact Windows DEV peer id}
case "$mode" in
  preflight)
    dc mesh status
    dc tab list
    dc tab list --mesh "$peer"
    ;;
  snapshot)
    tab=${3:?tab id}
    home=${4:?mac or windows}
    out=${5:?new snapshot directory}
    mkdir "$out"
    route=()
    case "$home" in
      mac) ;;
      windows) route=(--mesh "$peer");;
      *) exit 2;;
    esac
    dc tab inspect "$tab" --json "${route[@]}" > "$out/tab.json"
    dc tab get-text --tab "$tab" --formatted "${route[@]}" > "$out/daemon-formatted.ansi"
    dc tab get-text --tab "$tab" "${route[@]}" > "$out/daemon-plain.txt"
    ;;
  *) exit 2;;
esac
