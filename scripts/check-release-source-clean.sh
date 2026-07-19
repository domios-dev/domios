#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="${TMPDIR:-/tmp}/codeterm-release-source-$$"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

mkdir -p "$TEST_ROOT"
cd "$TEST_ROOT"
git init -q
git config user.email test@codeterm.local
git config user.name CodeTerm
printf '* text=auto\n' > .gitattributes
printf '.codeterm-rem-tmp/\n' > .gitignore
printf 'alpha\nbeta\n' > tracked.txt
git add .
git commit -qm fixture

printf 'alpha\r\nbeta\r\n' > tracked.txt
git status --porcelain=v1 | grep -q 'tracked.txt'
git diff --name-only HEAD -- | grep -q . && {
  echo "line-ending-only worktree noise was treated as source dirt" >&2
  exit 1
}

mkdir -p .codeterm-rem-tmp
printf 'runtime scratch\n' > .codeterm-rem-tmp/state.json
[ -z "$(git ls-files --others --exclude-standard)" ]

printf 'real change\n' >> tracked.txt
git diff --name-only HEAD -- | grep -q '^tracked.txt$'

git checkout -q -- tracked.txt
printf 'untracked source\n' > source.txt
git ls-files --others --exclude-standard | grep -q '^source.txt$'

cd "$ROOT"
echo "PASS release source ignores CRLF stat noise and runtime scratch, rejects real changes"
