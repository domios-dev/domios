#!/usr/bin/env bash
set -euo pipefail
unset CODETERM_INSTANCE CODETERM_API_PORT
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
dc "$@"
