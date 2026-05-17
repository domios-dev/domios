#!/usr/bin/env bash
# Deterministic stand-in for the Dreamer's A.4 self-repair pass.
# Runs the CLI commands a Dreamer WOULD run — no LLM in this loop.
set -euo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"

DB="$HOME/.codeterm-dev/mem/knowledge.db"

echo "[A.4a] Looking for duplicate relations (group by from,rel,to)..."
groups_json=$(dc --json mem graph find-duplicate-relations \
    --group-by from_id,relation_type,to_id --limit 100)
group_count=$(echo "$groups_json" | jq '.groups | length')
echo "       found $group_count group(s)"

# --- Group 1: (andrey, knows, anna) — three IDENTICAL rows.
# Strategy: keep the lowest rowid, delete the rest.
knows_rowids=$(echo "$groups_json" \
    | jq -r '.groups[] | select(.key.relation_type == "knows") | .rowids | join(",")')
if [ -n "$knows_rowids" ]; then
    keep_id=$(echo "$knows_rowids" | cut -d',' -f1)
    drop_ids=$(echo "$knows_rowids" | cut -d',' -f2-)
    if [ -n "$drop_ids" ]; then
        echo "[A.4a] knows group rowids=$knows_rowids  → delete $drop_ids (keep $keep_id)"
        dc --json mem graph delete-relations --rowids "$drop_ids" --actor "dreamer-eval" >/dev/null
    fi
fi

# --- Group 2: (andrey, works_on, test) — different confidence/properties.
# Strategy: pick the row with highest confidence as keep, merge the others
# into it (folds properties, takes max confidence).
works_rowids=$(echo "$groups_json" \
    | jq -r '.groups[] | select(.key.relation_type == "works_on") | .rowids | join(",")')
if [ -n "$works_rowids" ]; then
    # Look up confidence for each rowid in one shot and pick the max.
    IFS=',' read -ra rowid_arr <<< "$works_rowids"
    in_clause=$(IFS=','; echo "${rowid_arr[*]}")
    best_id=$(sqlite3 "$DB" \
        "SELECT id FROM relations WHERE id IN ($in_clause) ORDER BY confidence DESC, id ASC LIMIT 1")
    merge_ids=$(printf '%s\n' "${rowid_arr[@]}" | grep -v "^${best_id}$" | paste -sd',' -)
    if [ -n "$merge_ids" ]; then
        echo "[A.4a] works_on group rowids=$works_rowids  → keep $best_id, merge $merge_ids"
        dc --json mem graph merge-relations \
            --keep "$best_id" --merge "$merge_ids" --actor "dreamer-eval" >/dev/null
    fi
fi

echo
echo "[A.4b] Looking for duplicate entities (exact-name)..."
ent_json=$(dc --json mem graph find-duplicate-entities --strategy exact-name --limit 50)
ent_group_count=$(echo "$ent_json" | jq '.groups | length')
echo "       found $ent_group_count group(s)"

# Alias each pair. Heuristic: shorter id is canonical (deterministic).
# `add_alias` rewires relations, merges properties, and deletes the alias entity.
echo "$ent_json" \
    | jq -r '.groups[] | select(.member_ids | length == 2) | "\(.member_ids[0]) \(.member_ids[1])"' \
    | while read -r id_a id_b; do
        [ -z "$id_a" ] && continue
        if [ "${#id_a}" -le "${#id_b}" ]; then
            canonical="$id_a"; alias="$id_b"
        else
            canonical="$id_b"; alias="$id_a"
        fi
        echo "[A.4b] alias $alias → $canonical"
        dc --json mem graph alias --alias "$alias" --canonical "$canonical" >/dev/null
    done

echo
echo "[A.4c] Augmenting seed_person_owner with city + role..."
dc --json mem graph augment-entity \
    --id "seed_person_owner" \
    --patch '{"city":"Riga","role":"Senior iOS dev"}' \
    --actor "dreamer-eval" >/dev/null
echo "       done"

echo
echo "[done] Cleanup pass complete."
