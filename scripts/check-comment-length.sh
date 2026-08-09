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

new_violations=()
regressions=()

while IFS=$'\t' read -r file count; do
    rel="${file#$ROOT/}"
    rel="${rel#./}"

    if baseline="$(allowlist_baseline "$rel")"; then
        if [ "$count" -gt "$baseline" ]; then
            regressions+=("${rel}|${count}|${baseline}")
        fi
    else
        new_violations+=("${rel}|${count}")
    fi
done < <(
    find "$ROOT" \( -type d \( \
        -name target -o -name node_modules -o -name dist -o -name build -o \
        -name gen -o -name references -o -name vendor -o -name pty-fork -o \
        -name .git -o -name .claude -o -path "$ROOT/refs" \
    \) -prune \) -o \( -type f \( \
        -name '*.rs' -o \
        -name '*.ts' -o \
        -name '*.tsx' \
    \) -print0 \) | xargs -0 awk '
        function finish_file() {
            if (!have_file) return
            if (run >= 3) total++
            if (total > 0) printf "%s\t%d\n", current_file, total
        }
        FNR == 1 {
            finish_file()
            current_file = FILENAME
            have_file = 1
            run = 0
            total = 0
        }
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
            if (s ~ /^\/\//) run++
            else {
                if (run >= 3) total++
                run = 0
            }
        }
        END { finish_file() }
    '
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
