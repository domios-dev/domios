#!/usr/bin/env bash
set -euo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
if [[ $# != 2 ]]; then
  echo 'Usage: bash capture.sh OUTPUT_DIRECTORY CLIENT_ID' >&2
  exit 2
fi
mkdir -p "$1"
dev_get /api/health > "$1/health.json"
dev_get /api/frontend/status > "$1/frontend.json"
dev_get "/api/frontend/clients/$2/checkpoint" > "$1/checkpoint.json"
dc tab list --all-workspaces --json > "$1/tabs.json"
dc workspace list --json > "$1/workspaces.json"
dev_run python3 evals/frontend-swap/process_identity.py > "$1/process.json"
python3 - "$1/tabs.json" > "$1/tab-ids.txt" <<'PY'
import json
import sys
from pathlib import Path
for tab in json.loads(Path(sys.argv[1]).read_text())["tabs"]:
    print(tab["id"])
PY
mkdir -p "$1/inspect"
while IFS= read -r tab_id; do
  dc tab inspect --tab "$tab_id" --json > "$1/inspect/$tab_id.json"
done < "$1/tab-ids.txt"
