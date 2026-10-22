#!/usr/bin/env bash
set -euo pipefail
proof_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/blueprint-v2-intent" && pwd)"
case "${1:-}" in
  --help|-h) cat "$proof_dir/README.md"; exit 0 ;;
  --self-test) exec python3 "$proof_dir/self_test.py" ;;
esac
if [[ "${CODETERM_INSTANCE:-}" != blueprint-v2-proof || "${CODETERM_API_PORT:-}" != 17693 ]]; then
  echo 'REFUSED: require explicit CODETERM_INSTANCE=blueprint-v2-proof CODETERM_API_PORT=17693' >&2
  exit 7
fi
if [[ "${CODETERM_ROLE:-}" =~ ^([Ww]orker|[Pp]lanner)$ ]]; then
  echo 'REFUSED: DEV lifecycle and live subject launches belong to Chair' >&2
  exit 7
fi
if [[ "${1:-}" == --prepare-launch ]]; then
  shift
  exec python3 "$proof_dir/proof.py" --prepare-launch "$@"
fi
source "$proof_dir/../../../evals/_lib/lib.sh"
dev_resolve
dev_assert_target
dev_assert_cached_identity
exec_args=(python3 "$proof_dir/proof.py" "$@")
dev_run "${exec_args[@]}"
