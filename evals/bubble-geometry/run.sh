#!/usr/bin/env bash
set -euo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
dc identify
dev_run node --import tsx "$(dirname "${BASH_SOURCE[0]}")/probe.mjs" "$@"
