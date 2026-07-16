#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"

# Run only against an exact-head dev daemon:
#   ./build.sh dev
#   bash evals/codex-directory-trust/run.sh
# The dev-lib pins every CLI call below to 7686.
dev_get /api/health >/dev/null

out="$(dc agent spawn codex --role Worker --title ct-trust-positive-a --json)"
printf '%s\n' "$out" | python -c 'import json,sys; assert json.load(sys.stdin)["pane_id"]'
out="$(dc agent spawn codex --role Worker --title ct-trust-positive-b --json)"
printf '%s\n' "$out" | python -c 'import json,sys; assert json.load(sys.stdin)["pane_id"]'

# A lane-shaped foreign cwd receives no provisioning provenance. The caller must
# inspect the surfaced pending prompt through the dev snapshot before cleanup.
foreign="$ROOT/../foreign-lookalike-lane"
mkdir -p "$foreign"
out="$(dc agent spawn codex --cwd "$foreign" --title ct-trust-foreign --json)"
printf '%s\n' "$out" | python -c 'import json,sys; assert json.load(sys.stdin)["pane_id"]'

echo "Run against exact-head dev only; assert two ready prompts consumed and foreign pending prompt surfaced."
