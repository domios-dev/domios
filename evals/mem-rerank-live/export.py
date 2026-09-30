#!/usr/bin/env python3
"""Export the caller's production memory through the CodeTerm CLI."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


LOCAL_ROOT = Path.home() / "Developer" / "codeterm-worktrees" / "mem-eval-local"
SNAPSHOT_DIR = LOCAL_ROOT / "snapshot"
ITEMS_PATH = SNAPSHOT_DIR / "items.jsonl"
GRAPH_PATH = SNAPSHOT_DIR / "graph.json"
MAX_NODES = 100_000


def run_cli(args: list[str], env: dict[str, str]) -> str:
    result = subprocess.run(
        ["codeterm", *args],
        check=False,
        capture_output=True,
        text=True,
        env=env,
    )
    if result.returncode:
        raise RuntimeError(
            f"codeterm {' '.join(args[:3])} failed ({result.returncode}): "
            f"{result.stderr.strip() or result.stdout.strip()}"
        )
    return result.stdout


def parse_json_or_jsonl(text: str) -> Any:
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return [json.loads(line) for line in text.splitlines() if line.strip()]


def parent_prod_env() -> dict[str, str]:
    env = os.environ.copy()
    env.pop("CODETERM_INSTANCE", None)
    configured_port = env.get("CODETERM_API_PORT")
    if configured_port and configured_port != "7685":
        raise RuntimeError(
            f"parent CODETERM_API_PORT must be 7685 for prod export; got {configured_port}"
        )
    identity = json.loads(run_cli(["identify", "--json"], env))
    connection = identity.get("connection", {})
    if connection.get("port") != 7685 or connection.get("instance"):
        raise RuntimeError(
            "prod export requires the parent CodeTerm connection on port 7685 "
            "with CODETERM_INSTANCE unset"
        )
    return env


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        "w", encoding="utf-8", dir=path.parent, delete=False
    ) as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write("\n")
        temporary = Path(stream.name)
    os.replace(temporary, path)


def inventory_nodes(env: dict[str, str], stats: dict[str, Any]) -> list[dict[str, Any]]:
    orphan_result = json.loads(
        run_cli(["mem", "graph", "orphans", "--json", "--limit", str(MAX_NODES)], env)
    )
    island_result = parse_json_or_jsonl(
        run_cli(
            [
                "mem",
                "graph",
                "islands",
                "--json",
                "--max-size",
                str(MAX_NODES),
                "--limit",
                str(MAX_NODES),
            ],
            env,
        )
    )
    if isinstance(island_result, list):
        if len(island_result) == 1 and isinstance(island_result[0], dict):
            island_result = island_result[0]
        else:
            island_result = {"islands": island_result}
    rows = list(orphan_result.get("orphans", []))
    rows.extend(
        member
        for component in island_result.get("islands", [])
        for member in component.get("members", [])
    )
    ids = [row.get("id") for row in rows]
    if not all(isinstance(item_id, str) and item_id for item_id in ids):
        raise RuntimeError("graph inventory returned a node without an id")
    if len(ids) != len(set(ids)):
        raise RuntimeError("graph inventory returned duplicate node ids")
    expected = int(stats["total_items"]) + int(stats["total_entities"])
    if len(rows) != expected:
        raise RuntimeError(
            f"graph inventory returned {len(rows)} nodes; stats reports {expected}"
        )
    item_count = sum(not str(row.get("kind", "")).startswith("entity:") for row in rows)
    entity_count = len(rows) - item_count
    if item_count != int(stats["total_items"]) or entity_count != int(
        stats["total_entities"]
    ):
        raise RuntimeError(
            f"inventory mismatch: {item_count} items/{entity_count} entities; "
            f"stats reports {stats['total_items']}/{stats['total_entities']}"
        )
    return rows


def extract_object(value: Any, key: str) -> dict[str, Any]:
    if isinstance(value, dict):
        candidate = value.get(key)
        if isinstance(candidate, dict):
            return candidate
        return value
    raise RuntimeError(f"expected JSON object from mem {key}")


def export_items(rows: list[dict[str, Any]], env: dict[str, str]) -> tuple[list[dict[str, Any]], list[str]]:
    all_item_rows = [
        row
        for row in rows
        if not str(row.get("kind", "")).startswith("entity:")
    ]
    archived_ids = [row["id"] for row in all_item_rows if row.get("kind") == "archived"]
    item_rows = [row for row in all_item_rows if row.get("kind") != "archived"]
    records: list[dict[str, Any]] = []
    for index, inventory_row in enumerate(item_rows, start=1):
        item_id = inventory_row["id"]
        raw = json.loads(run_cli(["mem", "get", item_id, "--json"], env))
        item = extract_object(raw, "item")
        if item.get("id") != item_id:
            raise RuntimeError(f"mem get returned a mismatched id at item {index}")
        if item.get("kind") == "archived":
            continue
        body = item.get("body")
        if not isinstance(body, str):
            raise RuntimeError(f"mem get returned no body for item {index}")
        records.append(
            {
                "id": item_id,
                "kind": item.get("kind"),
                "title": item.get("title", ""),
                "body": body,
                "tags": item.get("tags", []),
                "importance": item.get("importance", 3.0),
                "area": item.get("area"),
            }
        )
    return records, archived_ids


def export_graph(rows: list[dict[str, Any]], env: dict[str, str]) -> dict[str, Any]:
    entity_rows = [
        row for row in rows if str(row.get("kind", "")).startswith("entity:")
    ]
    entities: list[dict[str, Any]] = []
    for index, row in enumerate(entity_rows, start=1):
        entity_id = row["id"]
        raw = json.loads(run_cli(["mem", "graph", "get", entity_id, "--json"], env))
        entity = extract_object(raw, "entity")
        if entity.get("id") != entity_id:
            raise RuntimeError(f"graph get returned a mismatched id at entity {index}")
        entities.append(entity)

    relation_by_key: dict[tuple[Any, ...], dict[str, Any]] = {}
    for row in rows:
        node_id = row["id"]
        raw = json.loads(
            run_cli(
                [
                    "mem",
                    "graph",
                    "related",
                    "--id",
                    node_id,
                    "--json",
                    "--include-invalidated",
                ],
                env,
            )
        )
        for relation in raw.get("relations", []):
            key = (
                relation.get("from_id"),
                relation.get("relation_type"),
                relation.get("to_id"),
                relation.get("valid_from"),
                relation.get("valid_to"),
                relation.get("scope"),
                relation.get("confidence"),
                relation.get("reason"),
            )
            relation_by_key[key] = relation

    return {
        "entities": entities,
        "relations": list(relation_by_key.values()),
        "archived_item_ids": sorted(archived_ids),
        "node_inventory_count": len(rows),
    }


def write_jsonl(path: Path, records: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        "w", encoding="utf-8", dir=path.parent, delete=False
    ) as stream:
        for record in records:
            stream.write(json.dumps(record, ensure_ascii=False) + "\n")
        temporary = Path(stream.name)
    os.replace(temporary, path)


def main() -> int:
    repo_root = Path(__file__).resolve().parents[2]
    if any(path.resolve().is_relative_to(repo_root) for path in (ITEMS_PATH, GRAPH_PATH)):
        raise RuntimeError("personal snapshots must stay outside the repository")
    LOCAL_ROOT.mkdir(parents=True, exist_ok=True, mode=0o700)
    SNAPSHOT_DIR.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(LOCAL_ROOT, 0o700)
    os.chmod(SNAPSHOT_DIR, 0o700)
    env = parent_prod_env()
    print("READ-ONLY PROD EXPORT via parent CodeTerm CLI at port 7685", flush=True)
    stats = json.loads(run_cli(["mem", "stats", "--json"], env))
    rows = inventory_nodes(env, stats)
    records, archived_ids = export_items(rows, env)
    expected_nonarchived = sum(
        count for kind, count in stats.get("by_kind", {}).items() if kind != "archived"
    )
    if len(records) != expected_nonarchived:
        raise RuntimeError(
            f"exported {len(records)} items; stats has {expected_nonarchived} non-archived kinds"
        )
    graph = export_graph(rows, env)
    write_jsonl(ITEMS_PATH, records)
    write_json(GRAPH_PATH, graph)
    print(
        json.dumps(
            {
                "items": len(records),
                "archived_skipped": len(archived_ids),
                "entities": len(graph["entities"]),
                "relations": len(graph["relations"]),
                "items_path": str(ITEMS_PATH),
                "graph_path": str(GRAPH_PATH),
            }
        )
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"export failed: {error}", file=sys.stderr)
        raise SystemExit(1)
