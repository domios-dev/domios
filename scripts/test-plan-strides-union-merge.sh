#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
TMPDIR="${TMPDIR:-/tmp}"
WORKDIR="$(mktemp -d "$TMPDIR/codeterm-plan-union-merge.XXXXXX")"
PYTHON_BIN="$(command -v python3 || command -v python)"
trap 'rm -rf "$WORKDIR"' EXIT

cd "$WORKDIR"
git init --quiet
git config user.name "CodeTerm Test"
git config user.email "codeterm-test@example.invalid"

cp "$ROOT/.gitattributes" .gitattributes
mkdir -p .codeterm/plans/pl_1/tasks/task_1
LOG=.codeterm/plans/pl_1/tasks/task_1/strides.jsonl
BASE='{"id":"evt_000","ts":"2026-06-26T00:00:00Z","kind":"base"}'
LEFT='{"id":"evt_left","ts":"2026-06-26T00:00:01Z","kind":"stride","branch":"left"}'
RIGHT='{"id":"evt_right","ts":"2026-06-26T00:00:02Z","kind":"stride","branch":"right"}'

printf '%s\n' "$BASE" > "$LOG"
git add .gitattributes "$LOG"
git commit --quiet -m "base"
git branch -M main

attr="$(git check-attr merge -- "$LOG")"
case "$attr" in
  *"merge: union") ;;
  *)
    echo "expected $LOG to resolve merge=union, got: $attr" >&2
    exit 1
    ;;
esac

git checkout --quiet -b left
printf '%s\n' "$LEFT" >> "$LOG"
git add "$LOG"
git commit --quiet -m "left append"

git checkout --quiet -b right main
printf '%s\n' "$RIGHT" >> "$LOG"
git add "$LOG"
git commit --quiet -m "right append"

git checkout --quiet left
git merge --quiet right

for expected in "$BASE" "$LEFT" "$RIGHT"; do
  count="$(grep -Fxc "$expected" "$LOG" || true)"
  if [ "$count" -ne 1 ]; then
    echo "expected exactly one merged JSONL line, count=$count: $expected" >&2
    echo "merged file:" >&2
    sed 's/^/  /' "$LOG" >&2
    exit 1
  fi
done

line_count="$(wc -l < "$LOG" | tr -d ' ')"
if [ "$line_count" -ne 3 ]; then
  echo "expected 3 merged JSONL lines, got $line_count" >&2
  sed 's/^/  /' "$LOG" >&2
  exit 1
fi

fold_log() {
  "$PYTHON_BIN" - "$1" <<'PY'
import json
import sys

events = {}
with open(sys.argv[1], encoding="utf-8") as handle:
    for line in handle:
        event = json.loads(line)
        events[event["id"]] = event

for event in sorted(events.values(), key=lambda item: (item["ts"], item["id"])):
    print(event["id"])
PY
}

raw_folded="$(fold_log "$LOG")"
folded="${raw_folded//$'\r'/}"
expected_fold=$'evt_000\nevt_left\nevt_right'
if [ "$folded" != "$expected_fold" ]; then
  echo "expected deterministic fold order:" >&2
  printf '%s\n' "$expected_fold" | sed 's/^/  /' >&2
  echo "got:" >&2
  printf '%s\n' "$folded" | sed 's/^/  /' >&2
  exit 1
fi

MUTATED_LOG="$WORKDIR/mutated-strides.jsonl"
"$PYTHON_BIN" - "$LOG" "$MUTATED_LOG" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source, open(
    sys.argv[2], "w", encoding="utf-8", newline="\n"
) as target:
    for line in source:
        event = json.loads(line)
        if event["id"] == "evt_right":
            event["ts"] = "2026-06-25T23:59:59Z"
        target.write(json.dumps(event, separators=(",", ":")) + "\n")
PY

mutated_raw_fold="$(fold_log "$MUTATED_LOG")"
mutated_fold="${mutated_raw_fold//$'\r'/}"
expected_mutated_fold=$'evt_right\nevt_000\nevt_left'
if [ "$mutated_fold" = "$expected_fold" ] || [ "$mutated_fold" != "$expected_mutated_fold" ]; then
  echo "expected timestamp mutation to be detected by the real JSONL fold" >&2
  echo "got:" >&2
  printf '%s\n' "$mutated_fold" | sed 's/^/  /' >&2
  exit 1
fi

echo "plan strides merge=union preserved concurrent EOF appends"
