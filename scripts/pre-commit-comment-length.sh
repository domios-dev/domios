#!/usr/bin/env bash
# Pre-commit hook — runs check-comment-length.sh over the staged change only.
# A tree sweep refuses every commit while the backlog stands; the staged set is
# the change under test.
# Install: ln -sf "$PWD/scripts/pre-commit-comment-length.sh" .git/hooks/pre-commit
# Or call from an existing pre-commit chain.

set -euo pipefail

staged="$(mktemp "${TMPDIR:-/tmp}/comment-length-staged.XXXXXX")"
trap 'rm -f "$staged"' EXIT
git diff --cached --name-only --diff-filter=ACMR > "$staged"
"$(dirname "$0")/check-comment-length.sh" --fail --changed "$staged"
