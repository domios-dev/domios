#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(git -C "$SCRIPT_DIR/../.." rev-parse --show-toplevel)"
LOCAL_ROOT="$HOME/Developer/codeterm-worktrees/mem-eval-local"
ITEMS_PATH="$LOCAL_ROOT/snapshot/items.jsonl"
GRAPH_PATH="$LOCAL_ROOT/snapshot/graph.json"
MAP_PATH="$LOCAL_ROOT/id-map.json"
INSTANCE="${1:-memrerank}"

if [[ ! -f "$ITEMS_PATH" || ! -f "$GRAPH_PATH" ]]; then
  echo "missing private snapshot; run evals/mem-rerank-live/export.py first" >&2
  exit 1
fi

python3 - "$REPO_ROOT/evals/_lib/lib.sh" "$LOCAL_ROOT" "$ITEMS_PATH" "$GRAPH_PATH" "$MAP_PATH" "$INSTANCE" <<'PY'
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

lib_path, local_root, items_path, graph_path, map_path, instance = sys.argv[1:]
local_root = Path(local_root)
map_path = Path(map_path)
local_root.mkdir(parents=True, exist_ok=True, mode=0o700)
os.chmod(local_root, 0o700)
supported = {
    "skill", "fact", "note", "conversation", "script", "peer",
    "preference", "daily", "learning", "rule", "summary",
}
shell = 'source "$1"; shift; dc "$@"'


def dc(*args):
    env = os.environ.copy()
    env["CODETERM_INSTANCE"] = instance
    result = subprocess.run(
        ["bash", "-c", shell, "mem-rerank-live", lib_path, *map(str, args)],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(
            f"dc {' '.join(map(str, args[:3]))} failed ({result.returncode}): "
            f"{result.stderr.strip() or result.stdout.strip()}"
        )
    return result.stdout.strip()


def response_id(raw):
    value = json.loads(raw)
    candidates = [value]
    while candidates:
        current = candidates.pop()
        if isinstance(current, dict):
            if isinstance(current.get("id"), str):
                return current["id"]
            candidates.extend(current.values())
        elif isinstance(current, list):
            candidates.extend(current)
    raise RuntimeError("mem save response did not contain a new id")


def persist_map(mapping, imported, graph_counts=None):
    payload = {
        "instance": instance,
        "items": mapping,
        "imported_count": imported,
        "graph": graph_counts or {},
    }
    map_path.parent.mkdir(parents=True, exist_ok=True)
    temporary = map_path.with_suffix(".json.tmp")
    temporary.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, map_path)
    os.chmod(map_path, 0o600)


mapping = {}
imported = 0
with tempfile.TemporaryDirectory(prefix="mem-rerank-import-", dir=local_root) as temp_dir:
    for line_number, line in enumerate(Path(items_path).read_text(encoding="utf-8").splitlines(), start=1):
        if not line.strip():
            continue
        record = json.loads(line)
        original_kind = str(record["kind"])
        import_kind = original_kind if original_kind in supported else "note"
        tags = list(record.get("tags") or [])
        if import_kind != original_kind:
            tags.append(f"orig-kind-{original_kind}")
        body_path = Path(temp_dir) / f"body-{line_number}.txt"
        body_path.write_text(str(record.get("body") or ""), encoding="utf-8")
        args = [
            "mem", "save", "--json", "--kind", import_kind,
            "--title", str(record.get("title") or "untitled"),
            "--body-file", str(body_path),
            "--importance", str(float(record.get("importance", 3.0))),
            "--skip-extract", "--replace",
        ]
        if tags:
            args.extend(["--tags", ",".join(map(str, tags))])
        if record.get("area"):
            args.extend(["--area", str(record["area"])])
        new_id = response_id(dc(*args))
        if new_id in mapping.values() and mapping.get(record["id"]) != new_id:
            raise RuntimeError(f"multiple source ids map to dev id at snapshot line {line_number}")
        mapping[record["id"]] = new_id
        imported += 1
        persist_map(mapping, imported)

graph = json.loads(Path(graph_path).read_text(encoding="utf-8"))
entities = graph.get("entities", [])
for entity in entities:
    props = entity.get("properties", {})
    args = [
        "mem", "graph", "create", "--json", "--id", entity["id"],
        "--entity-type", entity.get("entity_type", "Unknown"),
        "--props", json.dumps(props, ensure_ascii=False, separators=(",", ":")),
        "--scope", entity.get("scope", "global"),
    ]
    dc(*args)

item_ids = set(mapping)
entity_ids = {entity["id"] for entity in entities}
known_ids = item_ids | entity_ids
id_map = {**mapping, **{entity_id: entity_id for entity_id in entity_ids}}
relations = graph.get("relations", [])
skipped_relations = 0
existing = set()
for node_id in known_ids:
    raw = dc("mem", "graph", "related", "--json", "--include-invalidated", "--id", id_map[node_id])
    value = json.loads(raw)
    for relation in value.get("relations", []):
        existing.add((
            relation.get("from_id"), relation.get("relation_type"), relation.get("to_id"),
            relation.get("valid_from"), relation.get("valid_to"), relation.get("scope"),
            relation.get("confidence"), relation.get("reason"),
        ))
for relation in relations:
    old_from = relation.get("from_id")
    old_to = relation.get("to_id")
    if old_from not in id_map or old_to not in id_map:
        skipped_relations += 1
        continue
    source = id_map[old_from]
    target = id_map[old_to]
    signature = (
        source, relation.get("relation_type"), target,
        relation.get("valid_from"), relation.get("valid_to"), relation.get("scope"),
        relation.get("confidence"), relation.get("reason"),
    )
    if signature in existing:
        continue
    args = [
        "mem", "graph", "relate", "--json", "--from", source,
        "--rel", relation["relation_type"], "--to", target,
        "--valid-from", relation.get("valid_from") or "1970-01-01T00:00:00Z",
        "--confidence", str(float(relation.get("confidence", 1.0))),
        "--scope", relation.get("scope", "global"),
    ]
    if relation.get("valid_to"):
        args.extend(["--valid-to", relation["valid_to"]])
    if relation.get("reason"):
        args.extend(["--reason", str(relation["reason"])])
    dc(*args)
    existing.add(signature)

persist_map(
    mapping,
    imported,
    {
        "entities_imported": len(entities),
        "relations_in_snapshot": len(relations),
        "relations_skipped_missing_endpoint": skipped_relations,
    },
)
print(json.dumps({
    "imported_items": imported,
    "mapped_items": len(mapping),
    "entities_imported": len(entities),
    "relations_in_snapshot": len(relations),
    "relations_skipped_missing_endpoint": skipped_relations,
    "mapping_path": str(map_path),
}))
PY
