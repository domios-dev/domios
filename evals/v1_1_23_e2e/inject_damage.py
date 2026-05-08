#!/usr/bin/env python3
"""
v1.1.23 E2E damage injection.

Picks 3-5 active relations and 1-2 entities to deliberately damage in the dev
mem DB, then writes a manifest at evals/v1_1_23_e2e/damage-manifest.md so
every step of the E2E run knows what should heal.

THIS IS THE ONLY ALLOWED RAW-SQL BYPASS in v1.1.23 — explicitly outside the
store layer because we are *creating* the bad state on purpose.

Targets dev DB only: ~/.codeterm-dev/mem/knowledge.db.
Refuses to run against ~/.codeterm/ (prod) — fail-loud.
"""

import argparse
import os
import random
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path


PAST_VALID_TO = "2025-01-01T00:00:00.000000+00:00"


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def assert_dev_db(db_path: Path) -> None:
    s = str(db_path).replace("\\", "/")
    if "/.codeterm-dev/" not in s and "\\.codeterm-dev\\" not in str(db_path):
        sys.stderr.write(
            f"REFUSING: db_path does not look like dev DB: {db_path}\n"
            "Damage injection only allowed against ~/.codeterm-dev/mem/knowledge.db\n"
        )
        sys.exit(2)


def pick_active_relations(conn: sqlite3.Connection, want: int = 4) -> list[tuple]:
    cur = conn.execute(
        "SELECT from_id, relation_type, to_id, valid_from, confidence "
        "FROM relations WHERE valid_to IS NULL"
    )
    rows = cur.fetchall()
    if len(rows) < want:
        return rows
    rng = random.Random(20260508)
    return rng.sample(rows, want)


def pick_doomed_entities(conn: sqlite3.Connection, want: int = 2) -> list[tuple]:
    cur = conn.execute(
        """
        SELECT e.id, e.entity_type
          FROM entities e
         WHERE EXISTS (SELECT 1 FROM relations r
                        WHERE (r.from_id = e.id OR r.to_id = e.id)
                          AND r.valid_to IS NULL)
           AND e.id NOT IN (
               SELECT id FROM entities
                WHERE id LIKE 'topic\\_%' ESCAPE '\\'
                   OR id LIKE 'role\\_%' ESCAPE '\\'
                   OR id LIKE 'project\\_%' ESCAPE '\\'
                   OR id IN ('user_andrey','andrey','partner_anya','machine_blackstar')
           )
         ORDER BY random()
         LIMIT ?
        """,
        (want,),
    )
    rows = cur.fetchall()
    return rows


def main() -> int:
    home = Path.home()
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--db",
        default=str(home / ".codeterm-dev" / "mem" / "knowledge.db"),
        help="Path to dev mem DB",
    )
    parser.add_argument(
        "--manifest",
        default="evals/v1_1_23_e2e/damage-manifest.md",
        help="Where to write the damage manifest",
    )
    parser.add_argument("--rels", type=int, default=4, help="How many relations to invalidate (3-5)")
    parser.add_argument("--ents", type=int, default=2, help="How many entities to delete (1-2)")
    args = parser.parse_args()

    db_path = Path(args.db)
    manifest_path = Path(args.manifest)
    assert_dev_db(db_path)

    if not db_path.exists():
        sys.stderr.write(f"DB not found: {db_path}\n")
        return 2

    conn = sqlite3.connect(str(db_path))
    conn.execute("PRAGMA busy_timeout = 5000")

    rels = pick_active_relations(conn, want=args.rels)
    ents = pick_doomed_entities(conn, want=args.ents)

    if not rels:
        sys.stderr.write("No active relations found — nothing to damage\n")
        return 3

    # Also pick a couple of relations to invalidate AND orphan (so purge has work).
    # These must have endpoints we're not going to keep alive — pick relations
    # touching the doomed entities, force-invalidate them, and let the entity
    # delete leave the relation orphaned with valid_to set.
    orphan_rels = []
    if ents:
        ent_ids = [e[0] for e in ents]
        placeholders = ",".join("?" * len(ent_ids))
        cur = conn.execute(
            f"SELECT from_id, relation_type, to_id FROM relations "
            f"WHERE valid_to IS NULL AND (from_id IN ({placeholders}) OR to_id IN ({placeholders})) "
            f"LIMIT 3",
            ent_ids + ent_ids,
        )
        orphan_rels = cur.fetchall()

    # Apply damage in a single transaction.
    conn.execute("BEGIN IMMEDIATE")
    try:
        for from_id, rt, to_id, _vf, _c in rels:
            conn.execute(
                "UPDATE relations SET valid_to = ? "
                "WHERE from_id = ? AND relation_type = ? AND to_id = ? "
                "AND valid_to IS NULL",
                (PAST_VALID_TO, from_id, rt, to_id),
            )
        for from_id, rt, to_id in orphan_rels:
            conn.execute(
                "UPDATE relations SET valid_to = ? "
                "WHERE from_id = ? AND relation_type = ? AND to_id = ? "
                "AND valid_to IS NULL",
                (PAST_VALID_TO, from_id, rt, to_id),
            )
        for ent_id, _et in ents:
            conn.execute("DELETE FROM entities WHERE id = ?", (ent_id,))
        conn.commit()
    except Exception as e:
        conn.rollback()
        sys.stderr.write(f"Damage transaction failed, rolled back: {e}\n")
        return 4

    ts = utc_now_iso()
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    with manifest_path.open("w", encoding="utf-8") as f:
        f.write("# v1.1.23 E2E damage manifest\n\n")
        f.write(f"- Injected at: {ts}\n")
        f.write(f"- Target DB: {db_path}\n")
        f.write(f"- forced valid_to: `{PAST_VALID_TO}`\n\n")
        f.write("## Forced-invalidated relations\n\n")
        f.write("| from_id | relation_type | to_id |\n")
        f.write("|---|---|---|\n")
        for from_id, rt, to_id, _vf, _c in rels:
            f.write(f"| `{from_id}` | `{rt}` | `{to_id}` |\n")
        f.write("\n## Forced-invalidated relations touching doomed entities (orphan-purge case)\n\n")
        f.write("| from_id | relation_type | to_id |\n")
        f.write("|---|---|---|\n")
        for from_id, rt, to_id in orphan_rels:
            f.write(f"| `{from_id}` | `{rt}` | `{to_id}` |\n")
        f.write("\n## Deleted entities (broken-endpoint case)\n\n")
        f.write("| id | entity_type |\n")
        f.write("|---|---|\n")
        for ent_id, et in ents:
            f.write(f"| `{ent_id}` | `{et}` |\n")
        f.write("\n## Expected healing per Track\n\n")
        f.write("- **Track 1 (graph reads):** UI / `mem graph related` must NOT show the invalidated edges by default; `--include-invalidated` should reveal them.\n")
        f.write("- **Track 2 (graph mutations):** `mem graph revive-broken` should restore the relations whose endpoints both still exist; `mem graph purge-invalidated` should hard-delete the rest with audit.\n")
        f.write("- **Track 3 (Dreamer normalize):** Dreamer normalize phase should heal/skip relations referencing the deleted entities (cascade-death class).\n")
        f.write("- **Track 4 (Dreamer recluster):** must run end-to-end without crashing on the damaged graph.\n")
        f.write("- **Track 8/9 (evolution schema):** `dreaming-light` worker must produce a JSON report including `lessons_promoted` / `lessons_already_high_confidence`.\n")
        f.write("- **Audit:** every destructive op should show up in `graph_purge_log` / `lesson_audit`.\n")

    print(f"OK damage injected: {len(rels)} relations invalidated, {len(orphan_rels)} orphan-bound relations invalidated, {len(ents)} entities deleted")
    print(f"Manifest: {manifest_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
