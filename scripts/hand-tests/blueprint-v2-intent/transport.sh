#!/usr/bin/env bash
set -euo pipefail
proof_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ "${1:-}" == --context && -f "${2:-}" ]] || { echo 'Missing isolated proof context' >&2; exit 7; }
proof_context="$2"
shift 2
[[ "${1:-}" == -- ]] && shift
[[ "${CODETERM_DEV_INSTANCE:-}" == blueprint-v2-proof && "${CODETERM_DEV_API_PORT:-}" == 17693 ]] || {
  echo 'REFUSED: wrong subject DEV identity' >&2; exit 7;
}
export CODETERM_INSTANCE=blueprint-v2-proof CODETERM_API_PORT=17693
unset CODETERM_API_URL CODETERM_API_TOKEN CODETERM_SOCKET_PATH CODETERM_DATA_DIR
source "$proof_dir/../../../evals/_lib/lib.sh"
proof_tab="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["tab"])' "$proof_context")"
proof_mode="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["mode"])' "$proof_context")"
if [[ "$proof_mode" == subject ]]; then
  case "${1:-}:${2:-}" in
    identify:*|docs:BLUEPRINT-AUTHORING|docs:BLUEPRINTS|docs:BLUEPRINT-RECIPES|blueprint:new|blueprint:check|blueprint:show|blueprint:schema) ;;
    *) echo 'REFUSED: subject CLI is limited to identify/docs/new/check/show/schema' >&2; exit 7 ;;
  esac
fi
proof_capture="$(mktemp -d)"
trap 'rm -rf "$proof_capture"' EXIT
set +e
if [[ "$proof_mode" == owner && "${1:-}" == --api-get ]]; then
  dev_get "$2" > "$proof_capture/stdout" 2> "$proof_capture/stderr"
else
  dc --as-tab "$proof_tab" "$@" > "$proof_capture/stdout" 2> "$proof_capture/stderr"
fi
proof_exit=$?
set -e
python3 "$proof_dir/receipts.py" cli "$proof_context" "$proof_capture" "$proof_exit" "$@"
cat "$proof_capture/stdout"
cat "$proof_capture/stderr" >&2
exit "$proof_exit"
