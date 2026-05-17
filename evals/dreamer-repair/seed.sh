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
    DELETE FROM graph_purge_log WHERE actor IN ('dreamer-eval','add_alias');
"

NOW="2026-01-01T00:00:00Z"

# Two near-duplicate Person entities (differ only by trailing space + case),
# plus one extra Person referenced by the relations.
# seed_orphan_stub: a degree-0 entity to exercise delete-entity (no canonical to alias into).
sqlite3 "$DB" "
    INSERT INTO entities (id, entity_type, scope, properties_json, created_at, updated_at)
    VALUES
        ('seed_person_zzbeta',     'Person', 'global', '{\"name\":\"Zzbeta_Eval_Person\"}',  '$NOW', '$NOW'),
        ('seed_person_zzbetay',    'Person', 'global', '{\"name\":\"zzbeta_eval_person \"}', '$NOW', '$NOW'),
        ('seed_person_zzomega',    'Person', 'global', '{\"name\":\"Zzomega_Eval_Person\"}', '$NOW', '$NOW'),
        ('seed_proj_zzeval',       'Project','global', '{\"name\":\"Zzeval_Test_Proj\"}',    '$NOW', '$NOW'),
        ('seed_orphan_zzstub',     'Project','global', '{\"name\":\"Zzorphan_Stub\"}',       '$NOW', '$NOW');
"

# Plant an orphan vector by inserting a row referencing a non-existent mem_id.
# The repair-orphan-vectors primitive should surface and clean it.
# (Skipped if mem_vectors virtual table isn't accessible from raw sqlite3 — the
# eval will still cover the path via the REST endpoint.)
sqlite3 "$DB" "INSERT OR IGNORE INTO mem_vectors (mem_id, embedding) VALUES ('seed_ghost_vector', X'00000000');" 2>/dev/null || true

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
        ('seed_person_zzbeta','knows','seed_person_zzomega','global','2026-01-01T00:00:00Z',NULL,1.0,'{}','2026-01-01T00:00:00Z'),
        ('seed_person_zzbeta','knows','seed_person_zzomega','global','2026-01-02T00:00:00Z',NULL,1.0,'{}','2026-01-02T00:00:00Z'),
        ('seed_person_zzbeta','knows','seed_person_zzomega','global','2026-01-03T00:00:00Z',NULL,1.0,'{}','2026-01-03T00:00:00Z'),
        ('seed_person_zzbeta','works_on','seed_proj_zzeval','global','2026-01-01T00:00:00Z',NULL,0.7,'{\"src\":\"x\"}','2026-01-01T00:00:00Z'),
        ('seed_person_zzbeta','works_on','seed_proj_zzeval','global','2026-01-02T00:00:00Z',NULL,0.9,'{\"src\":\"y\"}','2026-01-02T00:00:00Z');
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
