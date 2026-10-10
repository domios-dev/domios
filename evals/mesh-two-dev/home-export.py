import argparse
import hashlib
import json
import pathlib
import sqlite3
import urllib.parse

TARGETS = json.loads(pathlib.Path(__file__).with_name("targets.json").read_text())

MAX_BYTES = 16 * 1024 * 1024
MAX_RECORDS = 10000


def validate_binding(binding):
    expected = {name: target["api_port"] for name, target in TARGETS.items()}
    if binding.get("home") not in expected or binding.get("port") != expected[binding["home"]]:
        raise ValueError("Only named QA DEV home records are allowed")
    if not binding.get("tab_id") or not binding.get("session_id"):
        raise ValueError("Exact tab/session binding required")
    tab = binding.get("tab_snapshot", {})
    if tab.get("id") != binding["tab_id"] or tab.get("agent_session_id") != binding["session_id"] or tab.get("agent_type") != binding.get("provider"):
        raise ValueError("Home tab snapshot does not own this provider session")
    meta = binding.get("metadata", {})
    if binding.get("format") == "grok":
        cwd = tab.get("cwd")
        if binding.get("provider") != "grok" or not cwd:
            raise ValueError("Grok provider and bound tab cwd required")
        if meta and (meta.get("session_id") != binding["session_id"] or meta.get("provider") != "grok"):
            raise ValueError("Provider metadata does not match the bound Grok tab")
        encoded_cwd = urllib.parse.quote(cwd, safe="~!*'()")
        expected_source = pathlib.Path.home() / ".grok" / "sessions" / encoded_cwd / binding["session_id"] / "updates.jsonl"
        source = pathlib.Path(binding.get("source_path") or meta.get("file_path") or expected_source)
        if not source.is_absolute() or not source.is_file():
            raise ValueError("Existing absolute provider-owned source required")
        if len(source.parents) < 4 or source.parents[3] != pathlib.Path.home() / ".grok" or source.parents[2].name != "sessions":
            raise ValueError("Grok source is outside its home sessions store")
        if source.name != "updates.jsonl" or source.parent.name != binding["session_id"] or urllib.parse.unquote(source.parents[1].name, errors="strict") != cwd:
            raise ValueError("Grok source does not belong to this tab cwd/session")
    else:
        if meta.get("session_id") != binding["session_id"] or meta.get("provider") != binding.get("provider"):
            raise ValueError("Provider metadata does not match the bound tab")
        source = pathlib.Path(meta.get("file_path", ""))
        if not source.is_absolute() or not source.is_file():
            raise ValueError("Existing absolute provider-owned source required")
    return source


def read_jsonl(path, session_id, kind, schema=None):
    with path.open("rb") as file:
        raw = file.read(MAX_BYTES + 1)
    if len(raw) > MAX_BYTES:
        raise ValueError("Record exceeds export cap; no silent tail truncation")
    records = [json.loads(line) for line in raw.decode("utf-8").splitlines() if line.strip()]
    if kind == "codex":
        headers = [r["payload"].get("id") for r in records if r.get("type") == "session_meta"]
        if not headers or any(value != session_id for value in headers):
            raise ValueError("Codex session_meta belongs to another session or is missing")
    elif kind == "claude":
        bound = [r.get("sessionId") for r in records if r.get("sessionId")]
        if not bound or any(value != session_id for value in bound):
            raise ValueError("Claude record belongs to another session or is missing")
    elif kind == "grok":
        if not records or any(r.get("params", {}).get("sessionId") != session_id for r in records):
            raise ValueError("Grok record belongs to another session or is missing")
    elif kind == "session-jsonl":
        if not schema or not schema.get("sessionPath"):
            raise ValueError("Unrecognized provider requires explicit session schema")
        values = []
        for record in records:
            value = record
            for key in schema["sessionPath"]:
                value = value.get(key) if isinstance(value, dict) else None
            if value is not None:
                values.append(value)
        if not values or any(value != session_id for value in values):
            raise ValueError("Provider record session does not match")
    else:
        raise ValueError("Unknown JSONL format")
    return raw, records


def read_opencode(path, session_id):
    uri = path.resolve().as_uri() + "?mode=ro"
    with sqlite3.connect(uri, uri=True, timeout=1) as db:
        db.row_factory = sqlite3.Row
        db.execute("PRAGMA query_only=ON")
        db.execute("BEGIN")
        session = db.execute("SELECT * FROM session WHERE id = ?", (session_id,)).fetchall()
        if len(session) != 1:
            raise ValueError("OpenCode session is absent")
        total = 0
        count = 0
        for table in ("message", "part"):
            count += db.execute(f"SELECT COUNT(*) FROM {table} WHERE session_id = ?", (session_id,)).fetchone()[0]
            total += db.execute(f"SELECT COALESCE(SUM(length(CAST(data AS BLOB))), 0) FROM {table} WHERE session_id = ?", (session_id,)).fetchone()[0]
        if total > MAX_BYTES or count > MAX_RECORDS:
            raise ValueError("Session exceeds export cap")
        messages = db.execute("SELECT * FROM message WHERE session_id = ? ORDER BY time_created, id", (session_id,)).fetchall()
        parts = db.execute("SELECT p.* FROM part p JOIN message m ON m.id = p.message_id WHERE p.session_id = ? AND m.session_id = ? ORDER BY p.time_created, p.id", (session_id, session_id)).fetchall()
        part_count = db.execute("SELECT COUNT(*) FROM part WHERE session_id = ?", (session_id,)).fetchone()[0]
        if len(parts) != part_count:
            raise ValueError("Part linked to foreign or missing message")
        rows = [{"table": "session", "row": dict(session[0])}]
        for table, records in (("message", messages), ("part", parts)):
            for record in records:
                row = dict(record)
                payload = json.loads(row["data"])
                if payload.get("sessionID", session_id) != session_id:
                    raise ValueError("Embedded provider session does not match")
                rows.append({"table": table, "row": row})
        return rows


def export(binding):
    source = validate_binding(binding)
    session_id = binding["session_id"]
    if binding.get("format") == "opencode-sqlite":
        if binding["provider"] != "opencode":
            raise ValueError("SQLite format does not match provider")
        records = read_opencode(source, session_id)
        raw = b"".join((json.dumps(row, ensure_ascii=False) + "\n").encode() for row in records)
    else:
        raw, records = read_jsonl(source, session_id, binding.get("format"), binding.get("schema"))
        if binding["format"] in ("claude", "codex", "grok") and binding["format"] != binding["provider"]:
            raise ValueError("Record format does not match provider")
    provenance = {"home": binding["home"], "port": binding["port"], "tab_id": binding["tab_id"], "session_id": session_id,
                  "provider": binding["provider"], "cwd": binding["tab_snapshot"].get("cwd"), "original_path": str(source), "format": binding["format"],
                  "sha256": hashlib.sha256(raw).hexdigest(), "records": len(records), "read_only": True}
    return raw, provenance


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binding")
    parser.add_argument("output")
    args = parser.parse_args()
    raw, provenance = export(json.loads(pathlib.Path(args.binding).read_text()))
    output = pathlib.Path(args.output)
    with output.open("xb") as file:
        file.write(raw)
    with pathlib.Path(str(output) + ".provenance.json").open("x") as file:
        json.dump(provenance, file, indent=2)


if __name__ == "__main__":
    main()
