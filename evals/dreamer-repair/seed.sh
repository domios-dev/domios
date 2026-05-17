#!/usr/bin/env bash
# Seed intentional graph pollution for the dreamer-repair smoke test.
# Idempotent: deletes any prior seed rows before re-inserting.
set -euo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"

DB="$HOME/.codeterm-dev/mem/knowledge.db"
[ -f "$DB" ] || { echo "ERROR: $DB not found — is dev installed?" >&2; exit 1; }

# Clean up any prior seed rows so re-runs are deterministic.
sqlite3 "$DB" "
    DELETE FROM relations WHERE from_id LIKE 'seed_%' OR to_id LIKE 'seed_%';
    DELETE FROM entities WHERE id LIKE 'seed_%';
    DELETE FROM entity_aliases WHERE alias_id LIKE 'seed_%' OR canonical_id LIKE 'seed_%';
    DELETE FROM graph_purge_log WHERE actor = 'dreamer-eval';
"

NOW="2026-01-01T00:00:00Z"

# Two near-duplicate Person entities (differ only by trailing space + case),
# plus one extra Person referenced by the relations.
sqlite3 "$DB" "
    INSERT INTO entities (id, entity_type, scope, properties_json, created_at, updated_at)
    VALUES
        ('seed_person_owner',  'Person', 'global', '{\"name\":\"SeedAndrey\"}',   '$NOW', '$NOW'),
        ('seed_person_ownery', 'Person', 'global', '{\"name\":\"seedandrey \"}',  '$NOW', '$NOW'),
        ('seed_person_anna',    'Person', 'global', '{\"name\":\"SeedAnna\"}',     '$NOW', '$NOW'),
        ('seed_proj_test',      'Project','global', '{\"name\":\"SeedTestProj\"}', '$NOW', '$NOW');
"

# Two relation duplicate groups:
#   - 3x (andrey, knows, anna) at distinct valid_from — identical otherwise.
#     UNIQUE(from,rel,to,valid_from) lets these coexist; the Dreamer's job is
#     to spot them with group_by=(from,rel,to) and DELETE all but one.
#   - 2x (andrey, works_on, test) with different confidence + properties.
#     Dreamer should MERGE these (keep max confidence + union properties).
sqlite3 "$DB" "
    INSERT INTO relations
        (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
    VALUES
        ('seed_person_owner','knows','seed_person_anna','global','2026-01-01T00:00:00Z',NULL,1.0,'{}','2026-01-01T00:00:00Z'),
        ('seed_person_owner','knows','seed_person_anna','global','2026-01-02T00:00:00Z',NULL,1.0,'{}','2026-01-02T00:00:00Z'),
        ('seed_person_owner','knows','seed_person_anna','global','2026-01-03T00:00:00Z',NULL,1.0,'{}','2026-01-03T00:00:00Z'),
        ('seed_person_owner','works_on','seed_proj_test','global','2026-01-01T00:00:00Z',NULL,0.7,'{\"src\":\"x\"}','2026-01-01T00:00:00Z'),
        ('seed_person_owner','works_on','seed_proj_test','global','2026-01-02T00:00:00Z',NULL,0.9,'{\"src\":\"y\"}','2026-01-02T00:00:00Z');
"

echo "seeded relations:"
sqlite3 "$DB" "SELECT id, from_id, relation_type, to_id, confidence
               FROM relations
               WHERE from_id LIKE 'seed_%' OR to_id LIKE 'seed_%'
               ORDER BY id;"
echo
echo "seeded entities:"
sqlite3 "$DB" "SELECT id, entity_type, json_extract(properties_json, '\$.name') AS name
               FROM entities
               WHERE id LIKE 'seed_%'
               ORDER BY id;"
