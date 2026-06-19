#!/usr/bin/env bash
# Source-file size discipline check.
#
# Walks the tree and flags files >1000 lines among .rs / .ts / .tsx / .py / .swift.
# Existing oversized files are listed in scripts/file-size-allowlist.txt and
# treated as graceful-deprecation: must shrink, not grow. New files always
# fail unless they're under the limit.
#
# Modes:
#   --warn  (default)  Print violations, exit 0. Used while we drain the
#                      backlog so CI doesn't break on legacy files.
#   --fail             Exit 1 on:
#                        * any file NOT in allowlist that exceeds the limit
#                        * any file IN allowlist that grew past its baseline
#                          (the allowlist locks each file's max line count;
#                          additions push the count up only via removals).
#
# Usage:
#   scripts/check-file-sizes.sh
#   scripts/check-file-sizes.sh --fail

set -euo pipefail

LIMIT=1000
MOD_RS_LIMIT=800                                                 # mod.rs target — facade + light glue, heavy logic moves out
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ALLOWLIST="$ROOT/scripts/file-size-allowlist.txt"
MODE="${1:-warn}"
case "$MODE" in
    --warn|warn) MODE=warn ;;
    --fail|fail) MODE=fail ;;
    *) echo "usage: $0 [--warn|--fail]" >&2; exit 2 ;;
esac

# Include-list: only these extensions count as "source files" for the discipline.
# Data, configs, lockfiles, generated bindings are explicitly out of scope.
# Listed here for documentation; the actual filter uses POSIX-portable
# `-name` alternation in the find call below so BSD find (macOS) and GNU
# find (Linux/CI) both work — `-regextype` is GNU-only and silently scans
# zero files on macOS.

# Path-based exclusions for build artifacts, vendored sources, generated code.
EXCLUDE_PATHS='(/target/|/node_modules/|/dist/|/build/|/gen/|/references/|/vendor/|/pty-fork/|/\.git/)'

# Read the allowlist into parallel arrays: path -> baseline line count.
# Lines starting with # are comments; blank lines ignored. Strip trailing \r
# so CRLF-checked-out allowlists on Windows still parse to integers
# (otherwise `[ "$lines" -gt "$baseline" ]` aborts with
#  `integer expression expected` because baseline carries a \r).
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
    lookup="$1"
    i=0
    while [ "$i" -lt "${#ALLOW_PATHS[@]}" ]; do
        if [ "${ALLOW_PATHS[$i]}" = "$lookup" ]; then
            printf '%s\n' "${ALLOW_BASELINES[$i]}"
            return 0
        fi
        i=$((i + 1))
    done
    return 1
}

new_violations=()       # files NOT in allowlist that exceed LIMIT
regressions=()          # files IN allowlist that grew past their baseline
mod_rs_violations=()    # mod.rs files over MOD_RS_LIMIT (advisory, always warn)

while IFS= read -r file; do
    rel="${file#$ROOT/}"
    rel="${rel#./}"
    lines="$(wc -l < "$file" | tr -d ' ')"

    # mod.rs has a tighter target — flag separately, doesn't fail in either mode
    case "$rel" in *'/mod.rs'|'mod.rs')
        if [ "$lines" -gt "$MOD_RS_LIMIT" ]; then
            mod_rs_violations+=("${rel}|${lines}")
        fi
    esac

    if [ "$lines" -le "$LIMIT" ]; then
        # Allowlist entries that are now under LIMIT should be removed from the
        # allowlist — flag for cleanup (warning only, never fails).
        if allowlist_baseline "$rel" >/dev/null; then
            echo "info: $rel ($lines lines) is now under $LIMIT — remove from allowlist"
        fi
        continue
    fi

    if baseline="$(allowlist_baseline "$rel")"; then
        if [ "$lines" -gt "$baseline" ]; then
            regressions+=("${rel}|${lines}|${baseline}")
        fi
    else
        new_violations+=("${rel}|${lines}")
    fi
done < <(
    find "$ROOT" -type f \( \
        -name '*.rs' -o \
        -name '*.ts' -o \
        -name '*.tsx' -o \
        -name '*.py' -o \
        -name '*.swift' \
    \) | grep -Ev "$EXCLUDE_PATHS"
)

# Reporting
exit_code=0

if [ ${#new_violations[@]} -gt 0 ]; then
    echo
    echo "❌ NEW or non-allowlisted files exceeding $LIMIT lines:"
    for entry in "${new_violations[@]}"; do
        IFS='|' read -r p l <<< "$entry"
        printf '  %5d  %s\n' "$l" "$p"
    done
    [ "$MODE" = fail ] && exit_code=1
fi

if [ ${#regressions[@]} -gt 0 ]; then
    echo
    echo "❌ Allowlisted files that GREW past their baseline (must shrink, not grow):"
    for entry in "${regressions[@]}"; do
        IFS='|' read -r p l b <<< "$entry"
        printf '  %5d  %s  (baseline %d, +%d)\n' "$l" "$p" "$b" "$((l-b))"
    done
    [ "$MODE" = fail ] && exit_code=1
fi

if [ ${#mod_rs_violations[@]} -gt 0 ]; then
    echo
    echo "ℹ️  mod.rs files over $MOD_RS_LIMIT lines (advisory — facade only, move logic out):"
    for entry in "${mod_rs_violations[@]}"; do
        IFS='|' read -r p l <<< "$entry"
        printf '  %5d  %s\n' "$l" "$p"
    done
fi

if [ ${#new_violations[@]} -eq 0 ] && [ ${#regressions[@]} -eq 0 ]; then
    echo "✅ file-size discipline: ${allow_count} allowlisted, no new violations, no regressions"
fi

exit $exit_code
