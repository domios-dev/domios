#!/usr/bin/env bash
# Pre-commit hook — runs check-comment-length.sh in --fail mode.
# Install: ln -sf "$PWD/scripts/pre-commit-comment-length.sh" .git/hooks/pre-commit
# Or call from an existing pre-commit chain.

set -euo pipefail
exec "$(dirname "$0")/check-comment-length.sh" --fail
