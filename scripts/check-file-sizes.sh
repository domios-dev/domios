#!/usr/bin/env bash
# Source-file size discipline check.
#
# Walks the tree and flags files >1200 lines among .rs / .ts / .tsx / .py / .swift.
# There is NO allowlist: a file over the limit is a bug to fix by splitting, not
# to baseline away. Every code file stays under the limit, no exceptions.
#
# Modes:
#   --warn  (default)  Print violations, exit 0.
#   --fail             Exit 1 if any file exceeds the limit.
#
# Usage:
#   scripts/check-file-sizes.sh
#   scripts/check-file-sizes.sh --fail

set -euo pipefail

LIMIT=1200
MOD_RS_LIMIT=800                                                 # mod.rs target — facade + light glue, heavy logic moves out
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODE="${1:-warn}"
case "$MODE" in
    --warn|warn) MODE=warn ;;
    --fail|fail) MODE=fail ;;
    *) echo "usage: $0 [--warn|--fail]" >&2; exit 2 ;;
esac

# Include-list: only these extensions count as "source files" for the discipline.
# Data, configs, lockfiles, generated bindings are explicitly out of scope.
# The filter uses POSIX-portable `-name` alternation in the find call below so
# BSD find (macOS) and GNU find (Linux/CI) both work — `-regextype` is GNU-only
# and silently scans zero files on macOS.

# Path-based exclusions for build artifacts, vendored sources, generated code.
EXCLUDE_PATHS='(/target/|/node_modules/|/dist/|/build/|/gen/|/references/|/vendor/|/pty-fork/|/\.git/|/\.claude/)'

violations=()           # files exceeding LIMIT
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

    if [ "$lines" -gt "$LIMIT" ]; then
        violations+=("${rel}|${lines}")
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

if [ ${#violations[@]} -gt 0 ]; then
    echo
    echo "❌ files exceeding $LIMIT lines — split them, do not baseline:"
    for entry in "${violations[@]}"; do
        IFS='|' read -r p l <<< "$entry"
        printf '  %5d  %s\n' "$l" "$p"
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

if [ ${#violations[@]} -eq 0 ]; then
    echo "✅ file-size discipline: no files over $LIMIT lines"
fi

exit $exit_code
