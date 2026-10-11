#!/usr/bin/env bash
set -euo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
arrival_tab="$1"
arrival_label="$2"
arrival_kind="$3"
arrival_sender="${CHAT_QA_SENDER:-26de263e}"
arrival_pad="$(dc --as-tab "$arrival_sender" pad path)"
printf 'UI QA arrival fixture. Do not use tools, edit files, or contact agents. Reply with exactly ARRIVAL-DONE-%s.\n' "$arrival_label" > "$arrival_pad/chat-arrival.md"
if [ "$arrival_kind" = delayed ]; then
  dc --as-tab "$arrival_sender" send --tab "$arrival_tab" --from-pad chat-arrival.md --delay 1s
else
  dc --as-tab "$arrival_sender" send --tab "$arrival_tab" --from-pad chat-arrival.md
fi
