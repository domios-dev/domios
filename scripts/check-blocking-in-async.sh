#!/usr/bin/env bash
# Flags obvious direct blocking calls inside async daemon command/service code.
#
# Scope is intentionally conservative: async fns under src-tauri/src/commands/
# and src-tauri/src/service/. Blocking work there must move behind
# spawn_blocking, tokio::time, tokio::process, or an existing async helper.
# Existing offenders are locked in scripts/blocking-in-async-allowlist.txt so
# the hook stays green while still blocking new regressions.
set -euo pipefail

REPO="$(git rev-parse --show-toplevel)"
SRC="${REPO}/src-tauri/src"
ALLOWLIST="${REPO}/scripts/blocking-in-async-allowlist.txt"

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

while IFS= read -r file; do
    rel="${file#$REPO/}"
    if grep -Eq 'use[[:space:]]+std::process::Command\b' "$file"; then
        has_std_command_import=1
    else
        has_std_command_import=0
    fi

    awk -v rel="$rel" -v has_std_command_import="$has_std_command_import" '
        function count_char(text, char, n, i) {
            n = 0
            for (i = 1; i <= length(text); i++) {
                if (substr(text, i, 1) == char) {
                    n++
                }
            }
            return n
        }

        function emit(pattern) {
            print rel ":" NR ":" pattern
        }

        function is_blocking_line(line) {
            if (line ~ /std::process::Command::new[[:space:]]*\(/) {
                emit("std::process::Command::new")
            } else if (has_std_command_import == 1 && line ~ /(^|[^[:alnum:]_:])Command::new[[:space:]]*\(/) {
                emit("Command::new")
            } else if (line ~ /std::thread::sleep[[:space:]]*\(/) {
                emit("std::thread::sleep")
            } else if (line ~ /reqwest::blocking::/) {
                emit("reqwest::blocking")
            }
        }

        {
            line = $0

            if (!in_fn && line ~ /(^|[[:space:]])async[[:space:]]+fn[[:space:]]+[[:alnum:]_]+/) {
                in_fn = 1
                brace_depth = 0
                saw_open = 0
                ignore_depth = -1
            }

            if (in_fn && ignore_depth < 0 && line ~ /(tokio::task::|tauri::async_runtime::)?spawn_blocking[[:space:]]*\(/) {
                pending_ignore = 1
            }

            if (in_fn && ignore_depth < 0 && !pending_ignore) {
                is_blocking_line(line)
            }

            if (in_fn) {
                opens = count_char(line, "{")
                closes = count_char(line, "}")
                old_depth = brace_depth
                brace_depth += opens - closes
                if (opens > 0) {
                    saw_open = 1
                }
                if (pending_ignore && opens > 0) {
                    ignore_depth = old_depth + opens - closes
                    if (ignore_depth < 1) {
                        ignore_depth = 1
                    }
                    pending_ignore = 0
                }
                if (ignore_depth >= 0 && brace_depth < ignore_depth) {
                    ignore_depth = -1
                }
                if (saw_open && brace_depth <= 0) {
                    in_fn = 0
                    pending_ignore = 0
                    ignore_depth = -1
                }
            }
        }
    ' "$file" >> "$TMP"
done < <(
    find "${SRC}/commands" "${SRC}/service" -type f -name '*.rs' \
        ! -name 'tests.rs' \
        ! -name '*_tests.rs' \
        ! -path '*/tests/*'
)

if [[ -f "$ALLOWLIST" ]]; then
    BAD=$(grep -Fvx -f <(grep -Ev '^[[:space:]]*(#|$)' "$ALLOWLIST") "$TMP" || true)
else
    BAD=$(cat "$TMP")
fi

if [[ -n "$BAD" ]]; then
    echo "❌ blocking calls in async command/service code — use spawn_blocking, tokio::time, tokio::process, or an async helper:"
    echo "$BAD"
    exit 1
fi

count=$(wc -l < "$TMP" | tr -d ' ')
echo "✅ check-blocking-in-async: ${count} baselined finding(s), no new async blocking calls"
