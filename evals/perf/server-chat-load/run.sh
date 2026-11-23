#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT"
if [[ ! "${CODETERM_INSTANCE:-}" =~ ^[a-z0-9][a-z0-9-]*$ ||
      ! "${CODETERM_API_PORT:-}" =~ ^[1-9][0-9]{3,4}$ ||
      "$CODETERM_API_PORT" == 7685 || "$CODETERM_API_PORT" == 7686 ||
      "$CODETERM_API_PORT" -gt 65535 ]]; then
  echo "Select an isolated named DEV with CODETERM_INSTANCE and its private CODETERM_API_PORT." >&2
  exit 2
fi

source "$ROOT/evals/_lib/lib.sh"
case "${1:-}" in
  --dc) shift; dc "$@" ;;
  --connection)
    dev_run node -e 'process.stdout.write(JSON.stringify({
      instance: process.env.CODETERM_INSTANCE,
      origin: process.env.CODETERM_API_URL,
      token: process.env.CODETERM_API_TOKEN,
    }))'
    ;;
  *) dev_run node "$ROOT/evals/perf/server-chat-load/run.mjs" "$@" ;;
esac
