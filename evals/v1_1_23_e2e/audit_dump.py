import sqlite3, sys, json
db = sys.argv[1]
con = sqlite3.connect(db)
out = {}
for table in ("graph_purge_log", "lesson_audit"):
    try:
        rows = con.execute(f"SELECT * FROM {table} ORDER BY rowid DESC LIMIT 20").fetchall()
        cols = [d[0] for d in con.execute(f"SELECT * FROM {table} LIMIT 1").description] if rows else []
        out[table] = {"count": len(rows), "cols": cols, "rows": [list(r) for r in rows]}
    except Exception as e:
        out[table] = {"error": str(e)}
print(json.dumps(out, default=str))
