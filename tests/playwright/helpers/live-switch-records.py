import json
import pathlib
import sqlite3
import sys

provider, session, file_path = sys.argv[1:4]
if provider == "codex":
    roots = sorted(pathlib.Path.home().joinpath(".codex").glob("state_*.sqlite"))
    matches = []
    for db in roots:
        with sqlite3.connect(f"file:{db.as_posix()}?mode=ro", uri=True) as conn:
            matches.extend(conn.execute("SELECT rollout_path FROM threads WHERE id = ?", (session,)).fetchall())
    paths = {row[0] for row in matches if row[0]}
    if len(paths) != 1:
        raise RuntimeError(f"exact session {session} has {len(paths)} rollout paths")
    file_path = paths.pop()
if provider == "opencode":
    db = pathlib.Path.home() / ".local/share/opencode/opencode.db"
    with sqlite3.connect(f"file:{db.as_posix()}?mode=ro", uri=True) as conn:
        records = [{"id": row[0], **json.loads(row[1])} for row in conn.execute(
            "SELECT id, data FROM message WHERE session_id = ? ORDER BY time_created, id", (session,))]
        for record in records:
            record["parts"] = [json.loads(row[0]) for row in conn.execute(
                "SELECT data FROM part WHERE session_id = ? AND message_id = ? ORDER BY time_created, id", (session, record["id"]))]
    print(json.dumps({"path": str(db), "session": session, "records": records}))
else:
    records = []
    for line in pathlib.Path(file_path).read_text(encoding="utf-8").splitlines():
        try:
            records.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    print(json.dumps({"path": file_path, "session": session, "records": records}))
