#!/usr/bin/env bash
# Block until a GitHub Actions workflow run reaches a terminal state, then
# print one summary line. Wraps `gh run watch --exit-status` so the caller
# gets exactly one notification when the run finishes (success or failure)
# instead of streaming N "in_progress" tics.
#
# Usage: wait-ci.sh <run-id>
#        wait-ci.sh latest <workflow-name>      # convenience: find newest run

set -euo pipefail

ARG="${1:-}"
if [ "$ARG" = "latest" ]; then
  WF="${2:-}"
  [ -n "$WF" ] || { echo "usage: wait-ci.sh latest <workflow-name>" >&2; exit 2; }
  RUN_ID=$(gh run list --workflow "$WF" --limit 1 --json databaseId --jq '.[0].databaseId')
else
  RUN_ID="$ARG"
fi
[ -n "${RUN_ID:-}" ] || { echo "usage: wait-ci.sh <run-id>|latest <workflow-name>" >&2; exit 2; }

# --exit-status returns 0 on success, 1 on failure/cancellation.
if gh run watch "$RUN_ID" --exit-status --interval 60 >/dev/null 2>&1; then
  STATE=success
else
  STATE=failure
fi

WORKFLOW=$(gh run view "$RUN_ID" --json workflowName --jq '.workflowName')
TAG=$(gh run view "$RUN_ID" --json headBranch --jq '.headBranch')
URL=$(gh run view "$RUN_ID" --json url --jq '.url')

echo "$STATE  workflow=$WORKFLOW  tag=$TAG  run=$RUN_ID  url=$URL"
[ "$STATE" = "success" ] || exit 1
