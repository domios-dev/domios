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
# Scope:
#   No path arguments      Sweep the whole tree (local audits, the deferred sweep).
#   --changed LIST         Read newline-separated paths from LIST.
#   PATH...                Scope to the named paths.
#   Scoped runs judge only the change under test; a path that is missing, deleted,
#   pruned, or not .rs/.ts/.tsx is dropped rather than failing the run.
#
# Usage:
#   scripts/check-comment-length.sh
#   scripts/check-comment-length.sh --fail
#   scripts/check-comment-length.sh --fail --changed changed-files.txt
#   scripts/check-comment-length.sh --fail src/foo.ts src-tauri/src/bar.rs

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ALLOWLIST="$ROOT/scripts/comment-length-allowlist.txt"

usage() {
    echo "usage: $0 [--warn|--fail] [--changed LISTFILE] [--] [PATH...]" >&2
    exit 2
}

MODE=warn
SCOPED=0
SCOPE_SRC="$(mktemp "${TMPDIR:-/tmp}/comment-length-scope.XXXXXX")"
SCOPE_LIST="$(mktemp "${TMPDIR:-/tmp}/comment-length-list.XXXXXX")"
trap 'rm -f "$SCOPE_SRC" "$SCOPE_LIST"' EXIT

add_scope_path() {
    SCOPED=1
    printf '%s\n' "$1" >> "$SCOPE_SRC"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --warn|warn) MODE=warn ;;
        --fail|fail) MODE=fail ;;
        --changed)
            [ "$#" -ge 2 ] || usage
            [ -f "$2" ] || { echo "changed-file list not found: $2" >&2; exit 2; }
            SCOPED=1
            cat "$2" >> "$SCOPE_SRC"
            shift
            ;;
        --)
            shift
            while [ "$#" -gt 0 ]; do add_scope_path "$1"; shift; done
            break
            ;;
        -*) usage ;;
        *) add_scope_path "$1" ;;
    esac
    shift
done

PRUNED_DIRS='target node_modules dist build gen references vendor pty-fork .git .claude refs'

swept_candidates() {
    find "$ROOT" \( -type d \( \
        -name target -o -name node_modules -o -name dist -o -name build -o \
        -name gen -o -name references -o -name vendor -o -name pty-fork -o \
        -name .git -o -name .claude -o -path "$ROOT/refs" \
    \) -prune \) -o \( -type f \( \
        -name '*.rs' -o \
        -name '*.ts' -o \
        -name '*.tsx' \
    \) -print0 \)
}

# A scoped run judges the change under test, so a path outside the check's own
# universe is dropped, never a failure: deletes, renames-away and non-source
# paths all reach this list from an ordinary diff.
scoped_candidates() {
    local path abs dir pruned seen=""
    while IFS= read -r path; do
        path="${path%$'\r'}"
        [ -n "$path" ] || continue
        case "$path" in
            *.rs|*.ts|*.tsx) ;;
            *) continue ;;
        esac
        pruned=0
        for dir in $PRUNED_DIRS; do
            case "/$path/" in */"$dir"/*) pruned=1 ;; esac
        done
        [ "$pruned" = 0 ] || continue
        case "$path" in
            /*) abs="$path" ;;
            [A-Za-z]:/*|[A-Za-z]:\\*) abs="$path" ;;
            *) abs="$ROOT/$path" ;;
        esac
        [ -f "$abs" ] || continue
        case "$seen" in *"|$abs|"*) continue ;; esac
        seen="$seen|$abs|"
        printf '%s\0' "$abs"
    done < "$SCOPE_SRC"
}

count_comment_blocks() {
    xargs -0 awk '
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
}

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
    if [ "$SCOPED" = 1 ]; then
        scoped_candidates > "$SCOPE_LIST"
    else
        swept_candidates > "$SCOPE_LIST"
    fi
    # xargs with no input still runs the command once, and an awk with no file
    # operand reads stdin and hangs. An empty change set exits before it.
    [ -s "$SCOPE_LIST" ] || exit 0
    count_comment_blocks < "$SCOPE_LIST"
)

# A gate that passed by looking at nothing is the failure this scoping exists to
# avoid, so a scoped run reports how many files it actually read.
scoped_count=0
if [ "$SCOPED" = 1 ] && [ -s "$SCOPE_LIST" ]; then
    while IFS= read -r -d '' _entry; do
        scoped_count=$((scoped_count + 1))
    done < "$SCOPE_LIST"
fi

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
    if [ "$SCOPED" = 1 ]; then
        echo "✅ comment-length discipline: ${scoped_count} changed file(s) checked, ${allow_count} allowlisted, no new violations, no regressions"
    else
        echo "✅ comment-length discipline: ${allow_count} allowlisted, no new violations, no regressions"
    fi
fi

exit $exit_code
