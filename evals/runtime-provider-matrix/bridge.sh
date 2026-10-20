#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/evals/_lib/lib.sh"
case "$1" in
  cli) shift; dc "$@" ;;
  pid) dev_resolve; dev_pid ;;
  get) shift; dev_get "$@" ;;
  post) shift; dev_post "$@" ;;
  *) exit 2 ;;
esac
