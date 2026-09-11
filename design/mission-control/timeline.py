import sqlite3, json, os, time
c = sqlite3.connect(os.path.expanduser("~/.codeterm/plans.db"))
p = json.loads(c.execute("select document_json from plans where id='pl_ad3tcu'").fetchone()[0]); now=time.time()*1000
e = p["planning_log"][-1]; approved = e.get("ts") or e.get("at")
rows=[]
for t in sorted(p["tasks"], key=lambda t: t["created_at"]):
    r = {"id": t["id"], "name": t["name"], "status": t["status"], "label": t.get("status_label"), "created": t["created_at"], "attempts": [], "holds": [], "reports": [], "verdicts": []}
    for a in t.get("execution_attempts", []):
        r["attempts"].append({"who": a.get("executor_title"), "assigned": a.get("assigned_at"), "reported": a.get("reported_at"), "closed": a.get("closed_at")})
    for m in t.get("milestones") or []:
        if m.get("kind")=="review":
            (r["holds"] if (m.get("text") or "").startswith("Hold") else r["verdicts"]).append(m["ts"])
        if m.get("kind")=="worker_report": r["reports"].append(m["ts"])
    rows.append(r)
work = sum((a["reported"]-a["assigned"]) for r in rows for a in r["attempts"] if a.get("reported") and a.get("assigned"))/60000
wait_closed = sum((a["closed"]-a["reported"]) for r in rows for a in r["attempts"] if a.get("closed") and a.get("reported"))/60000
wait_open = sum((now-a["reported"]) for r in rows for a in r["attempts"] if a.get("reported") and not a.get("closed"))/60000
pending = sum(((r["attempts"][0]["assigned"] if r["attempts"] and r["attempts"][0].get("assigned") else now) - r["created"]) for r in rows)/60000
json.dump({"now": now, "approved": approved, "rows": rows, "sum": {"work": work, "wait_closed": wait_closed, "wait_open": wait_open, "pending": pending}}, open(os.path.join(os.environ.get("SC", "."), "timeline.json"), "w"))
print("approved", time.strftime("%H:%M", time.localtime(approved/1000)), "now", time.strftime("%H:%M", time.localtime(now/1000)), "span min %.0f" % ((now-approved)/60000))
print("minutes: working %.0f · verdict wait closed %.0f · open %.0f · before pickup %.0f" % (work, wait_closed, wait_open, pending))
for r in rows:
    a = r["attempts"][0] if r["attempts"] else {}
    f = lambda ts: time.strftime("%H:%M", time.localtime(ts/1000)) if ts else "--:--"
    print(f'{r["id"]} {r["status"]:<11} {str(a.get("who")):<8} created {f(r["created"])} assigned {f(a.get("assigned"))} reported {f(a.get("reported"))} closed {f(a.get("closed"))} holds {len(r["holds"])} verdicts {len(r["verdicts"])} reports {len(r["reports"])} | {r["name"][:40]}')
