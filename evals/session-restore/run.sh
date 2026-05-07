#!/usr/bin/env bash
# Session-restore bench — full kill/restart drill against live dev daemon.
# Cross-platform via evals/_lib/lib.sh (mac/linux/win-bash all supported).
# See ./README.md for the test plan and the bug it guards against.

set -u
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
dev_env || exit 1
dev_alive || { echo "ERROR: dev not running. Start it: ./build.sh dev" >&2; exit 1; }

echo "== reset dev HARD (kill, blank last.json, restart) for guaranteed clean baseline"
reset_dev_state_hard

echo "== create 6 scenarios"
WS_E=$(new_workspace WS-empty C:/temp/restore-WS-empty)
WS_SS=$(new_workspace WS-shell-silent C:/temp/restore-WS-shell-silent)
WS_ST=$(new_workspace WS-shell-with-text C:/temp/restore-WS-shell-with-text)
WS_CS=$(new_workspace WS-claude-silent C:/temp/restore-WS-claude-silent)
WS_CT=$(new_workspace WS-claude-with-text C:/temp/restore-WS-claude-with-text)
WS_CR=$(new_workspace WS-claude-renamed C:/temp/restore-WS-claude-renamed)

P_SS=$(new_shell_pane $WS_SS C:/temp/restore-WS-shell-silent silent-shell)

P_ST=$(new_shell_pane $WS_ST C:/temp/restore-WS-shell-with-text shell-with-text)
# pwsh PTY linked != shell-ready-for-input. await_pty_link returns
# as soon as the PTY is registered, but the shell process inside
# (powershell.exe) takes another 5-10s to render its prompt and
# attach stdin. exec before that point gets bytes-eaten silently.
# 12s is the empirical safe window on Win11/PS5.
sleep 12 && dc pane exec --pane $P_ST 'echo MARKER_SHELL_BEFORE_KILL' > /dev/null 2>&1

P_CS=$(new_claude_pane $WS_CS C:/temp/restore-WS-claude-silent silent-claude)

P_CT=$(new_claude_pane $WS_CT C:/temp/restore-WS-claude-with-text claude-with-text 'reply with single word PINEAPPLEMARKER')

P_CR=$(new_claude_pane $WS_CR C:/temp/restore-WS-claude-renamed will-rename)
dc pane rename 'MY-CUSTOM-TITLE' --pane $P_CR > /dev/null 2>&1

echo "  ws_ids: empty=$WS_E shell-s=$WS_SS shell-t=$WS_ST cl-s=$WS_CS cl-t=$WS_CT cl-r=$WS_CR"
echo "  pane_ids: $P_SS $P_ST $P_CS $P_CT $P_CR"

echo "== wait 25s for claude bootstrap + initial task response"
sleep 25

echo "== assert text actually arrived in panes BEFORE kill"
shell_out=$(dc get-output --pane $P_ST --lines 30 2>&1)
if echo "$shell_out" | grep -q "MARKER_SHELL_BEFORE_KILL"; then
  echo "  OK shell-with-text contains MARKER_SHELL_BEFORE_KILL"
else
  echo "  FAIL shell-with-text missing MARKER_SHELL_BEFORE_KILL"
  echo "$shell_out" | tail -5
fi

claude_out=$(dc get-output --pane $P_CT --lines 30 2>&1)
if echo "$claude_out" | grep -q "PINEAPPLEMARKER"; then
  echo "  OK claude-with-text response contains PINEAPPLEMARKER"
else
  echo "  FAIL claude-with-text response missing PINEAPPLEMARKER (claude may not have responded yet)"
  echo "$claude_out" | tail -8
fi

echo "== wait extra 10s for autosave"
sleep 10
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
