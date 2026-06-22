#!/usr/bin/env bash
# Inline comment length discipline check.
#
# Walks .rs / .ts / .tsx and flags files containing inline // comment blocks
# of 3+ consecutive lines. Rule: inline comments are hard-limited to 2 lines
# (AGENTS.md Code style). Exempt: //! (Rust module docs), /// (Rust item docs),
# directive comments (eslint-disable, @ts-*, NOSONAR, prettier-ignore, #[).
# Out of scope: license/file headers, TSDoc /** */ on exported items.
#
# Modes:
#   --warn  (default)  Print violations, exit 0.
#   --fail             Exit 1 on:
#                        * any file NOT in allowlist that has a violation
#                        * any file IN allowlist whose violation count grew
#
# Usage:
#   scripts/check-comment-length.sh
#   scripts/check-comment-length.sh --fail

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ALLOWLIST="$ROOT/scripts/comment-length-allowlist.txt"
MODE="${1:-warn}"
case "$MODE" in
    --warn|warn) MODE=warn ;;
    --fail|fail) MODE=fail ;;
    *) echo "usage: $0 [--warn|--fail]" >&2; exit 2 ;;
esac

EXCLUDE_PATHS='(/target/|/node_modules/|/dist/|/build/|/gen/|/references/|/vendor/|/pty-fork/|/\.git/)'

# Read allowlist: path<TAB>baseline_count
ALLOW_PATHS=()
ALLOW_BASELINES=()
allow_count=0
if [ -f "$ALLOWLIST" ]; then
    while IFS=$'\t' read -r path baseline; do
        path="${path%$'\r'}"
        baseline="${baseline%$'\r'}"
        [ -z "${path:-}" ] && continue
        case "$path" in '#'*) continue ;; esac
        ALLOW_PATHS+=("$path")
        ALLOW_BASELINES+=("$baseline")
        allow_count=$((allow_count + 1))
    done < "$ALLOWLIST"
fi

allowlist_baseline() {
    local lookup="$1" i=0
    while [ "$i" -lt "${#ALLOW_PATHS[@]}" ]; do
        if [ "${ALLOW_PATHS[$i]}" = "$lookup" ]; then
            printf '%s\n' "${ALLOW_BASELINES[$i]}"
            return 0
        fi
        i=$((i + 1))
    done
    return 1
}

# Count long comment blocks (>=3 consecutive exempt-filtered // lines) in a file.
count_violations() {
    local file="$1"
    awk '
    BEGIN { run=0; total=0 }
    {
        s = $0
        # strip leading whitespace
        gsub(/^[[:space:]]+/, "", s)
        # exempt lines
        if (s ~ /^\/\/!/ || s ~ /^\/\/\// || s ~ /^#\[/ ||
            s ~ /^\/\/ eslint-disable/ || s ~ /^\/\/ @ts-/ ||
            s ~ /^\/\/ NOSONAR/ || s ~ /^\/\/ prettier-ignore/) {
            # treat as non-comment for run counting
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
    ' "$file"
}

new_violations=()
regressions=()

while IFS= read -r file; do
    rel="${file#$ROOT/}"
    rel="${rel#./}"
    count="$(count_violations "$file")"
    [ "$count" -eq 0 ] && continue

    if baseline="$(allowlist_baseline "$rel")"; then
        if [ "$count" -gt "$baseline" ]; then
            regressions+=("${rel}|${count}|${baseline}")
        fi
    else
        new_violations+=("${rel}|${count}")
    fi
done < <(
    find "$ROOT" -type f \( \
        -name '*.rs' -o \
        -name '*.ts' -o \
        -name '*.tsx' \
    \) | grep -Ev "$EXCLUDE_PATHS"
)

exit_code=0

if [ ${#new_violations[@]} -gt 0 ]; then
    echo
    echo "❌ Files with inline comment blocks >2 lines (not in allowlist):"
    for entry in "${new_violations[@]}"; do
        IFS='|' read -r p c <<< "$entry"
        printf '  %3d block(s)  %s\n' "$c" "$p"
    done
    [ "$MODE" = fail ] && exit_code=1
fi

if [ ${#regressions[@]} -gt 0 ]; then
    echo
    echo "❌ Allowlisted files that GREW past their baseline (must shrink, not grow):"
    for entry in "${regressions[@]}"; do
        IFS='|' read -r p c b <<< "$entry"
        printf '  %3d block(s)  %s  (baseline %d, +%d)\n' "$c" "$p" "$b" "$((c-b))"
    done
    [ "$MODE" = fail ] && exit_code=1
fi

if [ ${#new_violations[@]} -eq 0 ] && [ ${#regressions[@]} -eq 0 ]; then
    echo "✅ comment-length discipline: ${allow_count} allowlisted, no new violations, no regressions"
fi

exit $exit_code
