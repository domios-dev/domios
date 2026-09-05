#!/usr/bin/env bash
# Measure codex ingestion time for a large bracketed paste against CodeTerm's
# submit-key burst window, and record what a CR does while the TUI is Working.
#
# Read-only against the agent: it sends text and reads the screen. It never
# edits the delivery path. Dev-only via the `dc` wrapper.
#
# Usage: run.sh <codex-tab-id> [sizes...]
set -uo pipefail

source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"

TAB="${1:?usage: run.sh <codex-tab-id> [sizes...]}"
shift
SIZES=("${@:-4000 16000 36000 64000}")
OUT="$EVAL_LIB_OUT_BASE/pty-paste-trace"
mkdir -p "$OUT"
LOG="$HOME/.codeterm-dev/logs/codeterm.log"

payload() { # $1 = bytes; multiline so the paste is bracketed
  python3 -c "
import sys
n=int(sys.argv[1]); line='x'*79+'\n'
sys.stdout.write((line*((n//80)+1))[:n])
" "$1"
}

screen() { dc tab get-text --tab "$TAB" 2>/dev/null | tail -25; }

mark() { wc -l < "$LOG"; }

report_since() { # $1 = log line offset
  tail -n +"$1" "$LOG" | grep -E "submit-audit\]|submit-recovery\]|\[status\]|\[idle-poll\]" || true
}

echo "=== pty-paste-trace: tab=$TAB ==="
dc tab get-text --tab "$TAB" >/dev/null || { echo "tab unreachable"; exit 7; }

for size in ${SIZES[@]}; do
  for phase in idle midturn; do
    echo
    echo "########## size=$size phase=$phase ##########"
    if [ "$phase" = midturn ]; then
      # put the TUI into a real turn first, then send into it
      dc send "Count slowly from 1 to 400, one number per line." --tab "$TAB" >/dev/null
    fi
    body="$(payload "$size")"
    start=$(mark)
    t0=$(date +%s.%N)
    dc send "$body" --tab "$TAB" >/dev/null 2>&1
    t1=$(date +%s.%N)
    echo "-- send returned after $(echo "$t1 - $t0" | bc)s"
    echo "-- screen immediately after send:"
    screen | sed 's/^/   /'
    # the burst is bounded at ~2s (MAX_SUBMIT_KEY_WRITES=3, 1s spacing);
    # sample again past it without treating elapsed time as readiness
    echo "-- audit lines for this send:"
    report_since "$start" | sed 's/^/   /'
    echo "-- screen after the burst window closed:"
    screen | sed 's/^/   /'
    echo "-- does the composer show a collapsed [Pasted Content N chars] frame?"
    screen | grep -o "Pasted Content [0-9]* chars" || echo "   (no collapsed frame on screen)"
  done
done

echo
echo "=== correlate the whole run ==="
python3 "$(dirname "${BASH_SOURCE[0]}")/analyze-submit-bursts.py" "$LOG" | tail -30
