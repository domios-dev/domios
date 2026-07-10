#!/usr/bin/env bash
# Aggregate metric: count inline // comment blocks of 3+ consecutive lines.
# Uses the same rules as check-comment-length.sh. Skips references/, vendor/, etc.
#
# Usage:
#   scripts/count-comment-blocks.sh           # human summary
#   scripts/count-comment-blocks.sh --json    # {"blocks":N,"files":N}

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
JSON=false
[ "${1:-}" = --json ] && JSON=true

EXCLUDE_PATHS='(/target/|/node_modules/|/dist/|/build/|/gen/|/references/|/vendor/|/pty-fork/|/\.git/)'

count_violations() {
    awk '
    BEGIN { run=0; total=0 }
    {
        s = $0
        gsub(/^[[:space:]]+/, "", s)
        if (s ~ /^\/\/!/ || s ~ /^\/\/\// || s ~ /^#\[/ ||
            s ~ /^\/\/ eslint-disable/ || s ~ /^\/\/ @ts-/ ||
            s ~ /^\/\/ NOSONAR/ || s ~ /^\/\/ prettier-ignore/) {
            if (run >= 3) total++
            run = 0
            next
        }
        if (s ~ /^\/\//) {
            run++
        } else {
            if (run >= 3) total++
            run = 0
        }
    }
    END { if (run >= 3) total++; print total }
    ' "$1"
}

total_blocks=0
file_count=0

while IFS= read -r file; do
    rel="${file#$ROOT/}"
    count="$(count_violations "$file")"
    [ "$count" -eq 0 ] && continue
    total_blocks=$((total_blocks + count))
    file_count=$((file_count + 1))
done < <(
    find "$ROOT" -type f \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' \) \
        | grep -Ev "$EXCLUDE_PATHS"
)

if $JSON; then
    printf '{"blocks":%d,"files":%d}\n' "$total_blocks" "$file_count"
else
    echo "comment blocks (3+ consecutive // lines): $total_blocks across $file_count files"
fi
