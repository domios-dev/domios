#!/usr/bin/env sh
case "${1##*/}" in
  rustc|rustc.exe)
    if [ -n "${CODETERM_LANE_CACHE_WRAPPER:-}" ]; then
      exec "$CODETERM_LANE_CACHE_WRAPPER" "$@"
    fi
    ;;
esac
exec "$@"
