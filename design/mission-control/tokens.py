"""Extract everything the board shows into tokens.json, with its sources named.

Tokens and turns: each agent's own session log, matched by the attempt record's
session id. Claude logs are the jsonl files under ~/.claude/projects, Codex
rollouts are under ~/.codex/sessions. These are the files the KPI ledger
imports when it is on. Everything else is a field lookup in the plan store or
the usage store. No text is parsed for meaning anywhere in this file.
"""
import glob, json, os, sqlite3, time

from labels import role_label

HERE = os.path.dirname(os.path.abspath(__file__))
HOME = os.path.expanduser("~")
PLAN = "pl_ad3tcu"
T0 = time.perf_counter()

conn = sqlite3.connect(os.path.join(HOME, ".codeterm", "plans.db"))
plan = json.loads(conn.execute("select document_json from plans where id=?", (PLAN,)).fetchone()[0])
now = time.time() * 1000

claude_logs = {os.path.basename(f)[:-6]: f for f in glob.glob(os.path.join(HOME, ".claude", "projects", "*", "*.jsonl"))}
codex_logs = glob.glob(os.path.join(HOME, ".codex", "sessions", "**", "*.jsonl"), recursive=True)
files_read = []


def claude_usage(path):
    u = {"turns": 0, "output": 0, "cache_read": 0, "cache_write": 0, "uncached": 0}
    lines = 0
    for line in open(path, encoding="utf-8", errors="ignore"):
        lines += 1
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
    files_read.append({"provider": "claude", "path": path, "bytes": os.path.getsize(path), "lines": lines})
    return u


def codex_usage(session_id):
    files = [f for f in codex_logs if session_id in os.path.basename(f)]
    u = {"turns": 0, "output": 0, "cache_read": 0, "cache_write": 0, "uncached": 0}
    if not files:
        return u
    last = None
    lines = 0
    for line in open(files[0], encoding="utf-8", errors="ignore"):
        lines += 1
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
    files_read.append({"provider": "codex", "path": files[0], "bytes": os.path.getsize(files[0]), "lines": lines})
    return u


agents = []
for t in plan["tasks"]:
    for a in t.get("execution_attempts", []):
        label, unmapped = role_label("worker", t.get("executor_role"))
        agents.append({"name": a.get("executor_title"), "team_role": "worker", "executor_role": t.get("executor_role"), "label": label, "unmapped": unmapped,
                       "model": (a.get("model") or "").split(":")[-1], "provider": a.get("provider"), "session": a.get("session_id"), "task": t["id"],
                       "status": t["status"], "status_label": t.get("status_label"), "attempts": len(t.get("execution_attempts", []))})
for team_role, ref, model, prov in (("orchestrator", plan["owner"], "opus", "claude"), ("planner", plan["planner"], "gpt-5.6", "codex")):
    label, _ = role_label(team_role, None)
    agents.append({"name": ref["title"], "team_role": team_role, "executor_role": None, "label": label, "unmapped": None, "model": model, "provider": prov,
                   "session": ref["session_id"], "task": None, "status": "live", "status_label": None, "attempts": 0})
for ag in agents:
    ag["usage"] = claude_usage(claude_logs[ag["session"]]) if ag["session"] in claude_logs else codex_usage(ag["session"])

# Fields, not prose: a held review is status_label == "review held", written by review_gate.rs; its age is the newest review milestone's ts.
held = []
for t in plan["tasks"]:
    if t.get("status_label") == "review held":
        verdicts = [m["ts"] for m in (t.get("milestones") or []) if m.get("kind") == "review"]
        held.append({"task": t["id"], "name": t["name"], "since_last_verdict_min": (now - max(verdicts)) / 60000 if verdicts else None, "verdicts": len(verdicts)})
gates = [{"question": g.get("question"), "status": g.get("status"), "asked_at": g.get("asked_at"), "blocking": len(g.get("blocking_task_ids") or [])}
         for g in (plan.get("gates") or []) if str(g.get("status")).lower() in ("open", "escalated")]
counts = {s: sum(1 for t in plan["tasks"] if t["status"] == s) for s in ("pending", "in_progress", "in_review", "done", "cancelled", "blocked")}
second = sum(1 for t in plan["tasks"] if len(t.get("execution_attempts", [])) >= 2)
bounced_label = sum(1 for t in plan["tasks"] if t.get("status_label") == "review bounced")
first_try = sum(1 for t in plan["tasks"] if t["status"] == "done" and len(t.get("execution_attempts", [])) == 1)

# The event log: every milestone and stride the store holds, in time order, with the kind field and the text as written.
# Authors are named from the plan record itself: owner and planner titles, and each attempt's executor title by its tab id.
tab_names = {plan["owner"].get("tab_id"): plan["owner"].get("title"), plan["planner"].get("tab_id"): plan["planner"].get("title")}
for t in plan["tasks"]:
    for a in t.get("execution_attempts", []):
        if a.get("executor_tab_id") and a.get("executor_title"):
            tab_names[a["executor_tab_id"]] = a["executor_title"]


def who(m):
    f = m.get("from") or {}
    tab = f.get("tab_id")
    return tab_names.get(tab) or f.get("title") or tab or ""

names = {t["id"]: t["name"] for t in plan["tasks"]}
events = []
for e in plan.get("planning_log", []):
    events.append({"ts": e.get("ts") or e.get("at"), "kind": "planning", "who": str((e.get("by") or {}).get("kind") or ""), "task": None, "text": f'{e.get("from")} to {e.get("to")}'})
for t in plan["tasks"]:
    events.append({"ts": t["created_at"], "kind": "task_created", "who": "", "task": t["id"], "text": t["name"]})
    for m in t.get("milestones") or []:
        events.append({"ts": m["ts"], "kind": m.get("kind"), "who": who(m), "task": t["id"], "text": (m.get("text") or "").strip()})
for m in plan.get("strides") or []:
    events.append({"ts": m["ts"], "kind": m.get("kind"), "who": who(m), "task": None, "text": (m.get("text") or "").strip()})
for g in plan.get("gates") or []:
    events.append({"ts": g.get("asked_at"), "kind": "gate", "who": tab_names.get(g.get("asked_by_tab_id")) or g.get("asked_by_tab_id") or "", "task": None, "text": g.get("question") or ""})
events.sort(key=lambda e: e["ts"] or 0)

elapsed = time.perf_counter() - T0
out = {"read_at": time.strftime("%H:%M"), "now": now, "title": plan["title"], "counts": counts, "total_tasks": len(plan["tasks"]),
       "attempts": sum(len(t.get("execution_attempts", [])) for t in plan["tasks"]), "second_attempts": second, "bounced_label": bounced_label, "first_try": first_try,
       "held": held, "gates": gates, "agents": agents, "task_names": names, "events": events,
       "files": files_read, "extract_seconds": round(elapsed, 2)}
json.dump(out, open(os.path.join(HERE, "tokens.json"), "w", encoding="utf-8"), indent=1)
print(f"wrote tokens.json: {len(agents)} agents, {len(files_read)} session files, {sum(f['bytes'] for f in files_read)/1e6:.1f} MB, {len(events)} events, {elapsed:.2f} s, read at {out['read_at']}")
