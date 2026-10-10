#!/usr/bin/env bash
set -euo pipefail
export CODETERM_INSTANCE=aiderqa CODETERM_API_PORT=7697
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "/Users/dev/Developer/codeterm-worktrees/pl-fni7jw-task-1yh44r-f2ec7f4f/evals/_lib/lib.sh"
dev_assert_target
dev_run node "$script_dir/stream-live.mjs" "$@"
