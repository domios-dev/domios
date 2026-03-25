#!/bin/bash
# restart-clean.sh — Kill CodeTerm, clean ALL state, restart fresh
# Returns the socket path on stdout when ready

# 1. Kill everything
pkill -9 -f "target/debug/codeterm" 2>/dev/null
pkill -9 -f "tauri dev" 2>/dev/null
pkill -9 -f "node.*vite" 2>/dev/null

# 2. Wait until processes are actually dead
for i in $(seq 1 10); do
    if ! pgrep -f "target/debug/codeterm" >/dev/null 2>&1; then
        break
    fi
    sleep 1
done

# 3. Clean ALL state
rm -f /tmp/codeterm-*.sock
rm -f ~/Library/Application\ Support/codeterm/session.json
rm -f ~/Library/Application\ Support/codeterm/sessions/*.json 2>/dev/null

# 4. Start fresh
cd "$(dirname "$0")/.." || exit 1
pnpm tauri dev >/dev/null 2>&1 &

# 5. Wait for socket AND clean state (1 pane only)
CT="cli/target/release/codeterm"
for i in $(seq 1 30); do
    sleep 5
    S=$(ls -t /tmp/codeterm-*.sock 2>/dev/null | head -1)
    if [ -n "$S" ]; then
        PCOUNT=$(CODETERM_SOCKET_PATH=$S $CT pane list --json 2>/dev/null | python3 -c "import json,sys; print(len(json.load(sys.stdin)))" 2>/dev/null)
        if [ "$PCOUNT" = "1" ] 2>/dev/null; then
            echo "$S"
            exit 0
        elif [ "$PCOUNT" -ge 2 ] 2>/dev/null; then
            # Old session restored — kill and retry
            pkill -9 -f "target/debug/codeterm" 2>/dev/null
            sleep 2
            rm -f /tmp/codeterm-*.sock
            rm -f ~/Library/Application\ Support/codeterm/session.json
            pnpm tauri dev >/dev/null 2>&1 &
        fi
    fi
done
echo "TIMEOUT" >&2
exit 1
