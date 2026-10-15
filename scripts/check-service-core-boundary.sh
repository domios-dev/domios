#!/usr/bin/env bash
# Caller: build.sh verify
# Keeps ServiceContext's inherent impls inside codeterm-service-core and ports/ free of lock and PTY internals.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
core="src-tauri/crates/codeterm-service-core"
status=0

inherent_impls="$(grep -rnE --include='*.rs' --exclude-dir=target --exclude-dir=node_modules \
  '^\s*impl(<[^>]*>)?\s+([A-Za-z_:]*::)?ServiceContext\s*(<[^>]*>)?\s*\{' src-tauri \
  | grep -v "^$core/" || true)"
if [ -n "$inherent_impls" ]; then
  echo "service-core boundary: inherent impl ServiceContext outside $core:" >&2
  echo "$inherent_impls" >&2
  status=1
fi

# Named exceptions to port rule 1: the socket entry-point bridge and the one-time sentinel runtime composition.
port_leaks="$(grep -rnE --include='*.rs' \
  'codeterm_pty|crate::multiplexer|MuxState|lock_hold|Guard<|&ServiceContext|&MultiplexerState|&mut ServiceContext' \
  "$core/src/ports" | grep -vE "^$core/src/ports/(socket|sentinel)\.rs:" || true)"
if [ -n "$port_leaks" ]; then
  echo "service-core boundary: ports/ names a lock, PTY or ServiceContext reference:" >&2
  echo "$port_leaks" >&2
  status=1
fi

node scripts/check-crate-layering.mjs || status=1
for leaf in src-tauri/crates/*; do
  case "${leaf##*/}" in
    codeterm-service-core|codeterm-api|codeterm-commands|codeterm-host-runtime|codeterm-orchestration|codeterm-sentinel-host|codeterm-socket) continue ;;
  esac
  context_refs="$(grep -rnE --include='*.rs' '\bServiceContext\b' "$leaf/src" "$leaf/tests" 2>/dev/null | grep -vE ':[0-9]+:[[:space:]]*//' || true)"
  if [ -n "$context_refs" ]; then
    echo "service-core boundary: leaf names ServiceContext:" >&2
    echo "$context_refs" >&2
    status=1
  fi
done

if [ "$status" -eq 0 ]; then
  echo "service-core boundary: ok"
fi
exit "$status"
