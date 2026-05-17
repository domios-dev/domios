#!/usr/bin/env bash
# Verify the cleanup pass produced the expected state changes + audit trail.
set -euo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"

DB="$HOME/.codeterm-dev/mem/knowledge.db"
FAIL=0
fail() { echo "FAIL: $*" >&2; FAIL=1; }

# 1. No more seed-related duplicate relation groups (group_by from,rel,to).
seed_dupes=$(dc --json mem graph find-duplicate-relations \
        --group-by from_id,relation_type,to_id --limit 100 \
    | jq '[.groups[] | select((.key.from_id // "" | startswith("seed_")) or
                              (.key.to_id   // "" | startswith("seed_")))] | length')
if [ "$seed_dupes" != "0" ]; then
    fail "expected 0 seed-related dup-relation groups, got $seed_dupes"
fi

# 2. works_on merge happened — final confidence is 0.9 (max of 0.7, 0.9).
conf=$(sqlite3 "$DB" \
    "SELECT confidence FROM relations
     WHERE from_id='seed_person_owner' AND relation_type='works_on'")
if [ "$conf" != "0.9" ]; then
    fail "expected works_on confidence 0.9, got '$conf'"
fi

# 3. Exactly one works_on row remains.
works_count=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM relations
     WHERE from_id='seed_person_owner' AND relation_type='works_on'")
if [ "$works_count" != "1" ]; then
    fail "expected 1 works_on row, got $works_count"
fi

# 4. Exactly one knows row remains.
knows_count=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM relations
     WHERE from_id='seed_person_owner' AND relation_type='knows'")
if [ "$knows_count" != "1" ]; then
    fail "expected 1 knows row, got $knows_count"
fi

# 5. Aliasing happened: ≤1 seed_person_owner* entity remains.
remaining=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM entities WHERE id LIKE 'seed_person_owner%'")
if [ "$remaining" -gt "1" ]; then
    fail "expected ≤1 seed_person_owner* entity, got $remaining"
fi

# 6. Alias row was created in entity_aliases.
alias_count=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM entity_aliases WHERE alias_id LIKE 'seed_%' OR canonical_id LIKE 'seed_%'")
if [ "$alias_count" -lt "1" ]; then
    fail "expected ≥1 alias row referencing seeds, got $alias_count"
fi

# 7. Canonical entity got augmented with city + role.
props=$(sqlite3 "$DB" \
    "SELECT properties_json FROM entities WHERE id = 'seed_person_owner'")
if ! echo "$props" | jq -e '.city == "Riga"' >/dev/null 2>&1; then
    fail "expected city=Riga on seed_person_owner, got: $props"
fi
if ! echo "$props" | jq -e '.role == "Senior iOS dev"' >/dev/null 2>&1; then
    fail "expected role='Senior iOS dev' on seed_person_owner, got: $props"
fi

# 8. Audit log: dreamer-eval actor wrote ≥2 entries (delete + merge minimum;
#    augment-entity also writes; alias does not currently audit, that's OK).
audits=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM graph_purge_log WHERE actor='dreamer-eval'")
if [ "$audits" -lt "2" ]; then
    fail "expected ≥2 graph_purge_log entries from dreamer-eval, got $audits"
fi

if [ "$FAIL" -eq "0" ]; then
    echo "PASS — all assertions hold"
    exit 0
fi

echo
echo "--- post-state dump (debug) ---"
echo "relations (seed-related):"
sqlite3 "$DB" -header -column \
    "SELECT id, from_id, relation_type, to_id, confidence, properties_json
     FROM relations
     WHERE from_id LIKE 'seed_%' OR to_id LIKE 'seed_%';"
echo
echo "entities (seed_):"
sqlite3 "$DB" -header -column \
    "SELECT id, properties_json FROM entities WHERE id LIKE 'seed_%';"
echo
echo "entity_aliases (seed_):"
sqlite3 "$DB" -header -column \
    "SELECT * FROM entity_aliases WHERE alias_id LIKE 'seed_%' OR canonical_id LIKE 'seed_%';"
echo
echo "audit log (dreamer-eval):"
sqlite3 "$DB" -header -column \
    "SELECT id, purged_at, count, actor FROM graph_purge_log WHERE actor='dreamer-eval';"
exit 1
