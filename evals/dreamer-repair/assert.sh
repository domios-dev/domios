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
     WHERE from_id='seed_person_zzbeta' AND relation_type='works_on'")
if [ "$conf" != "0.9" ]; then
    fail "expected works_on confidence 0.9, got '$conf'"
fi

# 3. Exactly one works_on row remains.
works_count=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM relations
     WHERE from_id='seed_person_zzbeta' AND relation_type='works_on'")
if [ "$works_count" != "1" ]; then
    fail "expected 1 works_on row, got $works_count"
fi

# 4. Exactly one knows row remains.
knows_count=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM relations
     WHERE from_id='seed_person_zzbeta' AND relation_type='knows'")
if [ "$knows_count" != "1" ]; then
    fail "expected 1 knows row, got $knows_count"
fi

# 5. Aliasing happened: ≤1 seed_person_zzbeta* entity remains.
remaining=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM entities WHERE id LIKE 'seed_person_zzbeta%'")
if [ "$remaining" -gt "1" ]; then
    fail "expected ≤1 seed_person_zzbeta* entity, got $remaining"
fi

# 6. Alias row was created in entity_aliases.
alias_count=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM entity_aliases WHERE alias_id LIKE 'seed_%' OR canonical_id LIKE 'seed_%'")
if [ "$alias_count" -lt "1" ]; then
    fail "expected ≥1 alias row referencing seeds, got $alias_count"
fi

# 7. Canonical entity got augmented with city + role.
props=$(sqlite3 "$DB" \
    "SELECT properties_json FROM entities WHERE id = 'seed_person_zzbeta'")
if ! echo "$props" | jq -e '.city == "Riga"' >/dev/null 2>&1; then
    fail "expected city=Riga on seed_person_zzbeta, got: $props"
fi
if ! echo "$props" | jq -e '.role == "Senior iOS dev"' >/dev/null 2>&1; then
    fail "expected role='Senior iOS dev' on seed_person_zzbeta, got: $props"
fi

# 8. Audit log: dreamer-eval actor wrote ≥4 entries (delete-relations, merge-relations,
#    augment-entity, delete-entity, repair-orphan-vectors, repair-orphan-blobs as applicable).
audits=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM graph_purge_log WHERE actor='dreamer-eval'")
if [ "$audits" -lt "4" ]; then
    fail "expected ≥4 graph_purge_log entries from dreamer-eval, got $audits"
fi

# 9. delete-entity executed — seed_orphan_zzstub is gone.
orphan_remaining=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM entities WHERE id='seed_orphan_zzstub'")
if [ "$orphan_remaining" != "0" ]; then
    fail "expected seed_orphan_zzstub deleted, still present ($orphan_remaining)"
fi

# 10. add_alias audit row landed (#22). actor='add_alias' on the rows the aliasing step produced.
alias_audits=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM graph_purge_log
     WHERE actor='add_alias'
     AND details_json LIKE '%seed_%'")
if [ "$alias_audits" -lt "1" ]; then
    fail "expected ≥1 add_alias audit row for seed_ aliases, got $alias_audits"
fi

# 11. repair-orphan-vectors ran — the seeded ghost vector is gone (or stayed if
#     the sqlite3 INSERT into vec0 wasn't possible; either way no failure).
ghost_remaining=$(sqlite3 "$DB" \
    "SELECT COUNT(*) FROM mem_vectors WHERE mem_id='seed_ghost_vector'" 2>/dev/null || echo "0")
if [ "$ghost_remaining" != "0" ]; then
    fail "expected seed_ghost_vector removed by repair-orphan-vectors, got $ghost_remaining"
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
