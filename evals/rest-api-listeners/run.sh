#!/usr/bin/env bash
# Curated suite: REST API listener lifecycle on real sockets (bind, rebind, restart, outage recovery).
# It runs alone because forked children elsewhere would inherit the sockets and hold ports open (#620).
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
exec bash build.sh nextest codeterm-service-core 'test(/startup::rest_api::tests::/)'
