#!/usr/bin/env bash
# check-no-raw-graph-sql.sh — Track 3 of v1.1.23 guardrail.
#
# After v1.1.23, all graph-table mutations (entities, relations,
# entity_aliases) MUST route through the codeterm-mem-store crate.
# Two consumer files USED to issue raw SQL — they no longer should:
#
#   cli/src/cmd/worker.rs
#   src-tauri/src/mem/dreaming.rs
#
# This script greps both for raw graph-table SQL patterns and exits
# non-zero if any leak in. Documentation guard — wire into CI when a
# CI surface exists. Patterns that trigger:
#
#   conn.execute(...UPDATE relations
#   conn.execute(...DELETE FROM relations
#   conn.execute(...INSERT INTO relations
#   conn.execute(...DELETE FROM entities
#   conn.execute(...INSERT INTO entities
#   conn.execute(...UPDATE entities
#   conn.execute(...INSERT INTO entity_aliases (or REPLACE / OR IGNORE)
#   conn.execute(...DELETE FROM entity_aliases
#   conn.prepare(...FROM relations
#   conn.prepare(...FROM entities
#
# mem_items / mem_fts / mem_vectors SQL is allowed inline — those are
# separate concerns owned by the librarian / file_sync, not the graph.

set -u

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
FILES=(
    "$ROOT/cli/src/cmd/worker.rs"
    "$ROOT/src-tauri/src/mem/dreaming.rs"
)

# Each pattern is a POSIX-extended regex passed to `grep -E`.
PATTERNS=(
    'conn\.execute\([^)]*UPDATE[[:space:]]+relations'
    'conn\.execute\([^)]*DELETE[[:space:]]+FROM[[:space:]]+relations'
    'conn\.execute\([^)]*INSERT[[:space:]]+INTO[[:space:]]+relations'
    'conn\.execute\([^)]*DELETE[[:space:]]+FROM[[:space:]]+entities'
    'conn\.execute\([^)]*INSERT[[:space:]]+INTO[[:space:]]+entities'
    'conn\.execute\([^)]*UPDATE[[:space:]]+entities'
    'conn\.execute\([^)]*INSERT[[:space:]]+(OR[[:space:]]+(IGNORE|REPLACE)[[:space:]]+)?INTO[[:space:]]+entity_aliases'
    'conn\.execute\([^)]*DELETE[[:space:]]+FROM[[:space:]]+entity_aliases'
    'conn\.prepare\([^)]*FROM[[:space:]]+relations'
    'conn\.prepare\([^)]*FROM[[:space:]]+entities'
)

leaks=0
for f in "${FILES[@]}"; do
    if [ ! -f "$f" ]; then
        continue
    fi
    for p in "${PATTERNS[@]}"; do
        # -n line numbers, -E extended regex; suppress "no match" exit code with || true.
        hits=$(grep -nE "$p" "$f" || true)
        if [ -n "$hits" ]; then
            echo "RAW GRAPH SQL LEAK in $f matching /$p/:"
            echo "$hits"
            leaks=$((leaks + 1))
        fi
    done
done

if [ "$leaks" -gt 0 ]; then
    echo ""
    echo "FAIL: found $leaks raw-graph-SQL pattern leak(s)."
    echo "Route the SQL through codeterm-mem-store::graph (see Track 3 of v1.1.23)."
    exit 1
fi

echo "OK: no raw graph SQL in worker.rs or dreaming.rs."
exit 0
