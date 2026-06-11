#!/usr/bin/env bash
# Run every TS unit test under src/ and packages/.
#
# These are self-contained tsx scripts (each owns its assert harness and exits
# non-zero on failure). Plugin-side tests have their own runner
# (scripts/plugin-tests.sh, exposed as `pnpm test:plugins`) and are NOT included
# here. Invoked as `pnpm test:unit` and folded into `./build.sh test:all`.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

tests=()
while IFS= read -r f; do
    tests+=("$f")
done < <(
    find src packages -type f \( -name '*.test.ts' -o -name '*.test.tsx' \) \
        | grep -Ev '/node_modules/|/dist/' \
        | sort
)

if [ "${#tests[@]}" -eq 0 ]; then
    echo "no TS unit tests found under src/ or packages/"
    exit 0
fi

fails=0
for t in "${tests[@]}"; do
    echo
    echo "════ $t ════"
    if ! npx tsx "$t"; then
        fails=$((fails + 1))
        echo "  ✗ FAILED: $t"
    fi
done

echo
if [ "$fails" -eq 0 ]; then
    echo "✓ all ${#tests[@]} TS unit test file(s) green"
    exit 0
else
    echo "✗ $fails of ${#tests[@]} TS unit test file(s) failed"
    exit 1
fi
