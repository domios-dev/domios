#!/usr/bin/env python3
"""
Phase 0 cleanup of knowledge.db.

What it does:
  1. Merges 4 near-duplicate worker_pool facts into one canonical fact.
  2. Strips the "Verified by Andrey" hallucination from any body.
  3. Renormalizes area for content about codeterm internals to area='codeterm'.
  4. Logs every change to stdout.

Safe to re-run — each operation checks state before writing.
Backup is assumed to already exist at knowledge.db.backup-pre-cleanup.
"""
import argparse
import json
import sqlite3
import sys
from datetime import datetime, timezone

CANONICAL_ID = "fact_worker_pool_per_provider_semaphore_cap"
CANONICAL_TITLE = "Worker pool per-provider semaphore cap (N=3)"
CANONICAL_BODY = (
    "The mem worker pool uses a per-provider tokio Semaphore that caps "
    "concurrent extraction workers at N=3 per provider (configurable via "
    "memory.workers_per_provider). Jobs beyond the cap wait in FIFO order "
    "until a permit is released. All three completion paths release the "
    "permit: happy path, failure path, and the 30-minute watchdog. "
    "Implemented in src-tauri/src/mem/worker_pool.rs (commit 853f904)."
)
DUPE_IDS = [
    "fact_worker_pool_per_provider_semaphore",
    "fact_worker_pool_semaphore_caps_at_n_3",
    "fact_per_provider_semaphore_cap",
]

# Notes that are about the codeterm worker pool — normalize area.
CODETERM_AREA_NOTE_IDS = [
    "note_pool_stress_1",
    "note_pool_stress_2",
    "note_pool_stress_3",
    "note_pool_stress_4",
    "note_pool_stress_5",
    "note_pool_stress_6",
    "note_pool_stress_7",
    "note_pool_stress_8",
]

HALLUCINATION_PATTERNS = [
    "Verified by Andrey on",
    "Verification requested by Andrey",
    "verified by Andrey",
]


def log(msg: str) -> None:
    print(f"[cleanup] {msg}")


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def merge_worker_pool_facts(conn: sqlite3.Connection) -> dict:
    changes = {"merged": [], "updated_canonical": False}
    c = conn.cursor()

    c.execute(
        "SELECT id FROM mem_items WHERE id = ?",
        (CANONICAL_ID,),
    )
    if c.fetchone() is None:
        # Promote one of the dupes to canonical if the canonical is gone.
        c.execute(
            "SELECT id FROM mem_items WHERE id IN ({})".format(
                ",".join("?" * len(DUPE_IDS))
            ),
            DUPE_IDS,
        )
        existing = [r[0] for r in c.fetchall()]
        if not existing:
            log("no worker_pool facts to merge — already clean")
            return changes
        promote_from = existing[0]
        log(f"canonical id absent; renaming {promote_from} -> {CANONICAL_ID}")
        c.execute(
            "UPDATE mem_items SET id = ? WHERE id = ?",
            (CANONICAL_ID, promote_from),
        )
        c.execute(
            "UPDATE mem_fts SET mem_id = ? WHERE mem_id = ?",
            (CANONICAL_ID, promote_from),
        )

    # Rewrite canonical body + title + area + importance.
    now = now_iso()
    c.execute(
        """
        UPDATE mem_items
           SET title       = ?,
               body_inline = ?,
               summary     = ?,
               area        = 'codeterm',
               importance  = 4.0,
               updated_at  = ?
         WHERE id = ?
        """,
        (CANONICAL_TITLE, CANONICAL_BODY, CANONICAL_BODY[:200], now, CANONICAL_ID),
    )
    c.execute(
        """
        UPDATE mem_fts
           SET title = ?, summary = ?, body = ?
         WHERE mem_id = ?
        """,
        (CANONICAL_TITLE, CANONICAL_BODY[:200], CANONICAL_BODY, CANONICAL_ID),
    )
    changes["updated_canonical"] = True
    log(f"canonical updated: {CANONICAL_ID}")

    # Delete duplicates from mem_items and mem_fts.
    for dupe_id in DUPE_IDS:
        c.execute("SELECT id FROM mem_items WHERE id = ?", (dupe_id,))
        if c.fetchone() is None:
            continue
        c.execute("DELETE FROM mem_items WHERE id = ?", (dupe_id,))
        c.execute("DELETE FROM mem_fts WHERE mem_id = ?", (dupe_id,))
        changes["merged"].append(dupe_id)
        log(f"deleted duplicate: {dupe_id}")

    return changes


def strip_hallucinations(conn: sqlite3.Connection) -> list:
    c = conn.cursor()
    stripped = []
    for pat in HALLUCINATION_PATTERNS:
        c.execute(
            "SELECT id, body_inline FROM mem_items WHERE body_inline LIKE ?",
            (f"%{pat}%",),
        )
        for item_id, body in c.fetchall():
            if body is None:
                continue
            new_body = body
            for p in HALLUCINATION_PATTERNS:
                # Remove whole sentence ending with period.
                idx = new_body.find(p)
                while idx != -1:
                    end = new_body.find(".", idx)
                    if end == -1:
                        end = len(new_body) - 1
                    # Extend sentence start to previous period or beginning.
                    start = new_body.rfind(".", 0, idx)
                    start = 0 if start == -1 else start + 1
                    new_body = (new_body[:start] + new_body[end + 1:]).strip()
                    idx = new_body.find(p)
            if new_body != body:
                now = now_iso()
                c.execute(
                    "UPDATE mem_items SET body_inline = ?, updated_at = ? WHERE id = ?",
                    (new_body, now, item_id),
                )
                c.execute(
                    "UPDATE mem_fts SET body = ? WHERE mem_id = ?",
                    (new_body, item_id),
                )
                stripped.append(item_id)
                log(f"stripped hallucination from: {item_id}")
    return stripped


def normalize_note_areas(conn: sqlite3.Connection) -> list:
    c = conn.cursor()
    normalized = []
    for note_id in CODETERM_AREA_NOTE_IDS:
        c.execute(
            "SELECT area FROM mem_items WHERE id = ?",
            (note_id,),
        )
        row = c.fetchone()
        if row is None:
            continue
        if row[0] == "codeterm":
            continue
        now = now_iso()
        c.execute(
            "UPDATE mem_items SET area = 'codeterm', updated_at = ? WHERE id = ?",
            (now, note_id),
        )
        normalized.append((note_id, row[0]))
        log(f"normalized area: {note_id} {row[0]!r} -> 'codeterm'")
    return normalized


def report(conn: sqlite3.Connection) -> None:
    c = conn.cursor()
    c.execute("SELECT COUNT(*) FROM mem_items")
    total = c.fetchone()[0]
    c.execute("SELECT kind, COUNT(*) FROM mem_items GROUP BY kind ORDER BY kind")
    by_kind = c.fetchall()
    c.execute("SELECT area, COUNT(*) FROM mem_items GROUP BY area ORDER BY area")
    by_area = c.fetchall()
    log(f"final total: {total}")
    log(f"by kind:  {by_kind}")
    log(f"by area:  {by_area}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", required=True)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    conn = sqlite3.connect(args.db)
    conn.execute("BEGIN")
    try:
        merged = merge_worker_pool_facts(conn)
        stripped = strip_hallucinations(conn)
        normalized = normalize_note_areas(conn)
        if args.dry_run:
            conn.rollback()
            log("dry-run — rolled back")
        else:
            conn.commit()
            log("committed")
        report(conn)
        log(f"summary: merged={len(merged['merged'])} stripped={len(stripped)} normalized={len(normalized)}")
    except Exception as exc:
        conn.rollback()
        log(f"ERROR: {exc}")
        return 1
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
