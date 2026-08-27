#!/usr/bin/env bash
# macOS ships bash 3.2, where "${arr[@]}" on an EMPTY array is an unbound
# variable under `set -u`. bash 4.4+ accepts it, so the bug is invisible on
# Linux and Windows and only fires on a mac — which for release scripts means
# it fires once, during a release, after the tag is already pushed.
#
# Flags a bare "${NAME[@]}" expansion when the same script both runs under
# `set -u` and initialises NAME as an empty array. The safe form is
# ${NAME[@]+"${NAME[@]}"}.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FAIL=0

while IFS= read -r script; do
  grep -qE '^set -[a-z]*u|^set -o nounset' "$script" || continue

  # Arrays this script initialises empty; those are the ones that can trip.
  empties=$(grep -oE '^[[:space:]]*([A-Za-z_][A-Za-z0-9_]*)=\(\)' "$script" \
            | grep -oE '[A-Za-z_][A-Za-z0-9_]*' | sort -u)
  [ -n "$empties" ] || continue

  while IFS= read -r name; do
    [ -n "$name" ] || continue
    # Two things make an expansion safe, and both are common: the +alternate
    # guard on the line itself, or a `${#name[@]}` emptiness test somewhere in
    # the script, which is how every guarded loop here is written. Only an
    # expansion with neither is reported — a check that fires on correct code
    # teaches people to silence it.
    grep -qE "\\\$\{#$name\[[@*]\]\}" "$script" && continue
    hits=$(grep -nE "\"\\\$\{$name\[[@*]\]\}\"" "$script" \
           | grep -vE "\\\$\{$name\[[@*]\]\+" || true)
    [ -n "$hits" ] || continue
    while IFS= read -r hit; do
      echo "$script:${hit%%:*}: \"\${$name[@]}\" is unbound on bash 3.2 when $name is empty"
      echo "    fix: \${$name[@]+\"\${$name[@]}\"}"
      FAIL=1
    done <<< "$hits"
  done <<< "$empties"
done < <(find "$ROOT/scripts" "$ROOT/.codeterm" "$ROOT/evals" -name '*.sh' -type f 2>/dev/null)

if [ "$FAIL" -ne 0 ]; then
  echo
  echo "Empty-array expansions above are safe on the runner that ran this check and"
  echo "fatal on macOS. Use the \${arr[@]+\"\${arr[@]}\"} form."
  exit 1
fi

echo "✅ no bash 3.2 empty-array expansions"
