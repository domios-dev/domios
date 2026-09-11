"""Sum each agent's token usage from its own session log into tokens.json.

Claude sessions are the jsonl files under ~/.claude/projects; Codex sessions
are the rollout files under ~/.codex/sessions. These are the files the KPI
ledger imports when it is on, so the figures match what the ledger would show.
"""
import glob, json, os, sqlite3, time

HERE = os.path.dirname(os.path.abspath(__file__))
HOME = os.path.expanduser("~")
PLAN = "pl_ad3tcu"

conn = sqlite3.connect(os.path.join(HOME, ".codeterm", "plans.db"))
plan = json.loads(conn.execute("select document_json from plans where id=?", (PLAN,)).fetchone()[0])
now = time.time() * 1000

claude_logs = {os.path.basename(f)[:-6]: f for f in glob.glob(os.path.join(HOME, ".claude", "projects", "*", "*.jsonl"))}
codex_logs = glob.glob(os.path.join(HOME, ".codex", "sessions", "**", "*.jsonl"), recursive=True)


def claude_usage(path):
    u = {"turns": 0, "output": 0, "cache_read": 0, "cache_write": 0, "uncached": 0}
    for line in open(path, encoding="utf-8", errors="ignore"):
        try:
            e = json.loads(line)
        except Exception:
            continue
        m = e.get("message") or {}
        us = m.get("usage") if isinstance(m, dict) else None
        if e.get("type") == "assistant" and us:
            u["turns"] += 1
            u["output"] += us.get("output_tokens", 0)
            u["cache_read"] += us.get("cache_read_input_tokens", 0)
            u["cache_write"] += us.get("cache_creation_input_tokens", 0)
            u["uncached"] += us.get("input_tokens", 0)
    return u


def codex_usage(session_id):
    files = [f for f in codex_logs if session_id in os.path.basename(f)]
    u = {"turns": 0, "output": 0, "cache_read": 0, "cache_write": 0, "uncached": 0}
    if not files:
        return u
    last = None
    for line in open(files[0], encoding="utf-8", errors="ignore"):
        try:
            e = json.loads(line)
        except Exception:
            continue
        p = e.get("payload") or {}
        if e.get("type") == "event_msg" and p.get("type") == "token_count" and p.get("info"):
            u["turns"] += 1
            last = p["info"].get("total_token_usage")
    if last:
        u["output"] = last.get("output_tokens", 0)
        u["cache_read"] = last.get("cached_input_tokens", 0)
        u["cache_write"] = last.get("cache_write_input_tokens", 0)
        u["uncached"] = last.get("input_tokens", 0) - last.get("cached_input_tokens", 0)
    return u


def status_of(task):
    if task is None:
        return "live"
    return task["status"].replace("_", " ")


agents = []
for t in plan["tasks"]:
    for a in t.get("execution_attempts", []):
        agents.append({"name": a.get("executor_title"), "role": t.get("executor_role") or "worker", "model": (a.get("model") or "").split(":")[-1],
                       "provider": a.get("provider"), "session": a.get("session_id"), "task": t["id"], "status": status_of(t),
                       "label": t.get("status_label"), "attempts": len(t.get("execution_attempts", []))})
agents.append({"name": plan["owner"]["title"], "role": "orchestrator", "model": "opus", "provider": "claude", "session": plan["owner"]["session_id"], "task": None, "status": "live", "label": None, "attempts": 0})
agents.append({"name": plan["planner"]["title"], "role": "planner", "model": "gpt-5.6", "provider": "codex", "session": plan["planner"]["session_id"], "task": None, "status": "live", "label": None, "attempts": 0})

for ag in agents:
    ag["usage"] = claude_usage(claude_logs[ag["session"]]) if ag["session"] in claude_logs else codex_usage(ag["session"])

counts = {s: sum(1 for t in plan["tasks"] if t["status"] == s) for s in ("pending", "in_progress", "in_review", "done", "cancelled", "blocked")}
holds = []
for t in plan["tasks"]:
    if t["status"] == "in_review":
        hs = sorted(m["ts"] for m in (t.get("milestones") or []) if m.get("kind") == "review" and (m.get("text") or "").startswith("Hold"))
        if hs:
            rep = t["execution_attempts"][-1].get("reported_at")
            holds.append({"task": t["id"], "name": t["name"], "waiting_min": (now - rep) / 60000 if rep else None, "holds": len(hs), "latest_min": (now - hs[-1]) / 60000})
second = sum(1 for t in plan["tasks"] if len(t.get("execution_attempts", [])) >= 2)
first_try = sum(1 for t in plan["tasks"] if t["status"] == "done" and len(t.get("execution_attempts", [])) == 1)

out = {"read_at": time.strftime("%H:%M"), "now": now, "title": plan["title"], "counts": counts, "total_tasks": len(plan["tasks"]),
       "attempts": sum(len(t.get("execution_attempts", [])) for t in plan["tasks"]), "second_attempts": second, "first_try": first_try,
       "holds": holds, "agents": agents}
json.dump(out, open(os.path.join(HERE, "tokens.json"), "w", encoding="utf-8"), indent=1)
print("wrote tokens.json:", len(agents), "agents, read at", out["read_at"])
