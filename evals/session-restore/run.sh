#!/usr/bin/env bash
# Session-restore bench — full kill/restart drill against live dev daemon.
# Cross-platform via evals/_lib/lib.sh (mac/linux/win-bash all supported).
# See ./README.md for the test plan and the bug it guards against.

set -u
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
dev_env || exit 1
dev_alive || { echo "ERROR: dev not running. Start it: ./build.sh dev" >&2; exit 1; }

echo "== cleanup leftover WS-* from prior runs"
cleanup_test_workspaces "WS-"

echo "== create 6 scenarios"
WS_E=$(new_workspace WS-empty C:/temp/restore-WS-empty)
WS_SS=$(new_workspace WS-shell-silent C:/temp/restore-WS-shell-silent)
WS_ST=$(new_workspace WS-shell-with-text C:/temp/restore-WS-shell-with-text)
WS_CS=$(new_workspace WS-claude-silent C:/temp/restore-WS-claude-silent)
WS_CT=$(new_workspace WS-claude-with-text C:/temp/restore-WS-claude-with-text)
WS_CR=$(new_workspace WS-claude-renamed C:/temp/restore-WS-claude-renamed)

P_SS=$(new_shell_pane $WS_SS C:/temp/restore-WS-shell-silent silent-shell)
P_ST=$(new_shell_pane $WS_ST C:/temp/restore-WS-shell-with-text shell-with-text)
sleep 2 && codeterm pane exec --pane $P_ST 'Get-Date; Get-Location' > /dev/null 2>&1

P_CS=$(new_claude_pane $WS_CS C:/temp/restore-WS-claude-silent silent-claude)
P_CT=$(new_claude_pane $WS_CT C:/temp/restore-WS-claude-with-text claude-with-text 'reply with single word: PINEAPPLE')
P_CR=$(new_claude_pane $WS_CR C:/temp/restore-WS-claude-renamed will-rename)
codeterm pane rename 'MY-CUSTOM-TITLE' --pane $P_CR > /dev/null 2>&1

echo "  ws_ids: empty=$WS_E shell-s=$WS_SS shell-t=$WS_ST cl-s=$WS_CS cl-t=$WS_CT cl-r=$WS_CR"
echo "  pane_ids: $P_SS $P_ST $P_CS $P_CT $P_CR"

echo "== wait 35s for autosave"
sleep 35
PRE=$(snapshot session-restore-pre)
echo "  pre snapshot: $PRE"

echo "== HARD KILL (taskkill /F or kill -9 — Windows shutdown simulator)"
dev_kill_hard
assert_atomic_save_clean || exit 1

echo "== restart dev"
dev_start_bg
dev_wait_ready 180 || exit 1
sleep 5
POST=$(snapshot session-restore-post)
echo "  post snapshot: $POST"

echo
echo "== METADATA DIFF"
diff_snapshots "$PRE" "$POST"
DIFF_EXIT=$?

echo
echo "== UI-mount step is MANUAL: open dev window, click each WS-* tab,"
echo "   then verify silent-claude / claude-with-text / claude-renamed all"
echo "   show claude UI (not bare powershell), and silent-shell /"
echo "   shell-with-text show plain powershell (no accidental claude)."

exit $DIFF_EXIT
