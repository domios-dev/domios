#!/usr/bin/env bash
# Pre-commit hook wrapper — same source of truth as scripts/check-file-sizes.sh,
# just runs in --fail mode against the working tree.
#
# Install: ln -sf "$PWD/scripts/pre-commit-file-sizes.sh" .git/hooks/pre-commit
# Or call from an existing pre-commit chain.

set -euo pipefail
exec "$(dirname "$0")/check-file-sizes.sh" --fail
