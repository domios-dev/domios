#!/usr/bin/env bash
# A fresh agent gets a plain web task with no hint about Domios; it passes when it
# drives its own built-in browser tab instead of a provider browser extension.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
source "$ROOT/evals/_lib/lib.sh"

TIMEOUT="${CODETERM_BROWSER_DISCOVERY_TIMEOUT:-240}"
TASK="${CODETERM_BROWSER_DISCOVERY_TASK:-Open https://example.com and tell me the exact text of its main heading.}"
EXPECT="${CODETERM_BROWSER_DISCOVERY_EXPECT:-example.com}"
CASES=("$@")
[ "${#CASES[@]}" -eq 0 ] && CASES=("claude:haiku" "codex")

SCRATCH="$(mktemp -d)"
WS_NAME="browser-discovery-$$"
dc workspace create "$WS_NAME" --cwd "$SCRATCH" >/dev/null
WS="$(dc workspace list 2>&1 | awk -v n="$WS_NAME" '$1 == n {print $2; exit}')"
SPAWNED=()

cleanup() {
  for tab in "${SPAWNED[@]}"; do dc tab close --tab "$tab" >/dev/null 2>&1 || true; done
  [ -n "$WS" ] && dc workspace close "$WS" >/dev/null 2>&1 || true
  rm -rf "$SCRATCH"
}
trap cleanup EXIT

owned_surface() {
  dc surface list 2>&1 | awk -v a="$1" '$0 ~ "owner " a {print $1; exit}'
}

failures=0
for case in "${CASES[@]}"; do
  provider="${case%%:*}"
  model=""
  [ "$provider" != "$case" ] && model="${case#*:}"
  args=(agent spawn "$provider" --workspace "$WS" --title "Disc${provider^}" --description "browser discovery eval" --task "$TASK")
  [ -n "$model" ] && args+=(--model "$model")
  agent="$(dc "${args[@]}" 2>&1 | grep -o 'Tab [0-9a-f]*' | awk '{print $2}')"
  if [ -z "$agent" ]; then
    echo "FAIL $case: spawn failed"
    failures=$((failures + 1))
    continue
  fi
  SPAWNED+=("$agent")

  verdict="timeout"
  deadline=$((SECONDS + TIMEOUT))
  while [ "$SECONDS" -lt "$deadline" ]; do
    transcript="$(dc tab get-text --tab "$agent" 2>&1)"
    if echo "$transcript" | grep -qiE 'claude-in-chrome|chrome-browser|computer-use'; then
      verdict="extension"
      break
    fi
    surface="$(owned_surface "$agent")"
    if [ -n "$surface" ] && dc --as-tab "$agent" plugin browser status 2>/dev/null | grep -qi "$EXPECT"; then
      verdict="plugin"
      break
    fi
    sleep 5
  done

  verbs="$(echo "$transcript" | grep -oE 'codeterm (plugin browser [a-z]+|docs BROWSER)' | sort -u | paste -sd, -)"
  case "$verdict" in
    plugin) echo "PASS $case: built-in browser (${verbs:-no verbs captured})" ;;
    extension) echo "FAIL $case: reached for a provider browser extension"; failures=$((failures + 1)) ;;
    *) echo "FAIL $case: no built-in browser reached $EXPECT within ${TIMEOUT}s (${verbs:-no verbs})"; failures=$((failures + 1)) ;;
  esac
done

exit "$failures"
