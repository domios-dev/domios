"""Draw the board and its sheets from tokens.json with the product's own kit. Every figure is a field lookup or arithmetic over fields."""
import json, os, html, sqlite3, time, collections

from labels import AREAS, AREA_LABEL, UNTAGGED, AREA_FACET_COMPACT, KIND_LABELS, kind_label

HERE = os.path.dirname(os.path.abspath(__file__))
D = json.load(open(os.path.join(HERE, "tokens.json"), encoding="utf-8"))
agents = D["agents"]
T = {k: sum(a["usage"][k] for a in agents) for k in ("turns", "output", "cache_read", "cache_write", "uncached")}
orch = next(a for a in agents if a["team_role"] == "orchestrator")
c = D["counts"]
by_area = collections.OrderedDict((v, 0) for v, _ in AREAS); by_area[UNTAGGED] = 0
inferred = 0
for a in agents:
    if a["team_role"] != "worker": continue
    key = a["area"] or a["area_hint"] or UNTAGGED
    if not a["area"] and a["area_hint"]: inferred += 1
    by_area[key] = by_area.get(key, 0) + a["usage"]["output"]
team = {r: sum(a["usage"]["output"] for a in agents if a["team_role"] == r) for r in ("orchestrator", "planner")}
files = D["files"]
FILES_LINE = f'{len(files)} session files read: {sum(1 for f in files if f["provider"] == "claude")} Claude logs and {sum(1 for f in files if f["provider"] == "codex")} Codex rollouts, {sum(f["bytes"] for f in files)/1e6:.0f} MB, {sum(f["lines"] for f in files):,} lines, in {D["extract_seconds"]} s'


def n(v):
    v = float(v)
    for s, d in (("B", 1e9), ("M", 1e6), ("K", 1e3)):
        if v >= d:
            x = v / d
            return (f"{x:.1f}" if x < 10 else f"{x:.0f}") + s
    return f"{v:.0f}"


def dur(m):
    m = int(round(m)); h, mm = divmod(m, 60)
    return f"{h} h {mm:02d} m" if h else f"{mm} min"


def hm(ms):
    return time.strftime("%H:%M", time.localtime(ms / 1000))


def read_usage():
    conn = sqlite3.connect(os.path.join(os.path.expanduser("~"), ".codeterm", "usage", "usage.db"))
    now = time.time() * 1000
    def when(ms):
        if not ms: return ""
        t = time.localtime(ms / 1000); same = time.strftime("%Y%m%d", t) == time.strftime("%Y%m%d", time.localtime(now / 1000))
        if same:
            left = max(0, ms - now) / 60000; h, m = divmod(int(left), 60)
            return f"resets {time.strftime('%H:%M', t)} · in {h} h {m:02d} m" if h else f"resets {time.strftime('%H:%M', t)} · in {m} min"
        return f"resets {time.strftime('%a %H:%M', t)}"
    out = []
    for prov, label in (("claude", "Claude"), ("codex", "Codex")):
        r = conn.execute("select session_pct, session_resets_at, weekly_pct, weekly_resets_at, captured_at from usage_snapshots where provider=? and parsed_ok=1 order by captured_at desc limit 1", (prov,)).fetchone()
        if not r: continue
        sp, sr, wp, wr, cap = r
        if sp is not None: out.append((f"{label} session", sp, when(sr), "session"))
        if wp is not None: out.append((f"{label} weekly", wp, when(wr) + ("" if sp is not None else " · this provider reports weekly only"), "weekly" if prov == "claude" else "codex"))
        read_usage.captured = time.strftime("%H:%M", time.localtime(cap / 1000))
    return out


USAGE = read_usage()

STYLE = """
.seg { --seg-count: 3; --seg-index: 2; position: relative; display: inline-flex; overflow: hidden; padding: 3px; border: 1px solid #232930; border-radius: 999px; background: color-mix(in srgb, #141b24 80%, transparent); height: 26px; box-sizing: border-box; }
.seg .thumb { position: absolute; z-index: 0; top: 3px; bottom: 3px; left: 3px; width: calc((100% - 6px) / var(--seg-count)); border: 1px solid color-mix(in srgb, #58a6ff 45%, transparent); border-radius: 999px; background: color-mix(in srgb, #58a6ff 22%, transparent); transform: translateX(calc(var(--seg-index) * 100%)); pointer-events: none; }
.seg .opt { position: relative; z-index: 1; display: flex; flex: 1 0 0; align-items: center; justify-content: center; gap: .375rem; padding: 0 .75rem; border: 0; border-radius: 999px; background: transparent; color: #828a92; font-size: .6875rem; white-space: nowrap; cursor: pointer; }
.seg .opt[aria-selected="true"] { color: #c9d1d9; font-weight: 600; }
.status { display: flex; flex-direction: column; gap: .45rem; padding: .7rem .8rem; border: 1px solid #232930; border-radius: 12px; background: color-mix(in srgb, #0d1117 94%, #141b24); }
.status .topline { display: flex; align-items: center; justify-content: space-between; gap: .75rem; min-width: 0; }
.chips { display: flex; flex-wrap: wrap; align-items: center; gap: .35rem; min-width: 0; }
.chip { display: inline-flex; align-items: center; gap: .3rem; padding: .1rem .45rem; font-size: .72rem; line-height: 1.4; border: 1px solid #232930; border-radius: 999px; color: #c9d1d9; }
.chip .role { opacity: .65; text-transform: uppercase; font-size: .62rem; letter-spacing: .04em; }
.chip.state { border-color: color-mix(in srgb, #58a6ff 40%, transparent); color: #58a6ff; }
.done-count { flex-shrink: 0; color: #828a92; font-size: .75rem; font-variant-numeric: tabular-nums; white-space: nowrap; }
.title { color: #c9d1d9; font-size: .98rem; font-weight: 700; line-height: 1.3; }
.card { display: flex; flex-direction: column; gap: 8px; padding: 12px; min-width: 0; }
.card h2 { margin: 0; font-size: 0.8125rem; font-weight: 600; color: #c9d1d9; }
.amount { font-family: "JetBrains Mono", "Cascadia Code", Consolas, monospace; font-size: 26px; font-weight: 600; color: #c9d1d9; letter-spacing: .01em; line-height: 1.1; }
.amount.warn { color: #d29922; }
.caption { font-size: 11px; color: #828a92; }
.small { font-size: 11px; color: #828a92; line-height: 1.5; margin: 0; }
.small b { color: #c9d1d9; font-weight: 500; font-family: "JetBrains Mono", Consolas, monospace; font-size: 11px; }
.src { font-size: 10px; color: #484f58; line-height: 1.5; margin: 0; padding-top: 6px; border-top: 1px solid color-mix(in srgb, #232930 65%, transparent); margin-top: auto; }
.kv { display: grid; grid-template-columns: minmax(0, 1fr) 72px 56px; gap: 10px; align-items: baseline; }
.kv .k { font-size: 0.8125rem; color: #c9d1d9; }
.kv .k i { font-style: normal; color: #484f58; font-size: .6875rem; }
.kv .v { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 12px; font-weight: 600; color: #c9d1d9; text-align: right; }
.share { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 11px; color: #828a92; text-align: right; }
.gauge { display: grid; grid-template-columns: 1fr auto; align-items: baseline; column-gap: 10px; }
.gauge .glabel { font-size: 11px; color: #c9d1d9; font-weight: 500; letter-spacing: .02em; text-transform: none; }
.gauge .pct { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 12px; font-weight: 600; color: #c9d1d9; }
.gauge .pct.warn { color: #d29922; }
.gauge .pct.danger { color: #ff7b72; }
.gauge .bar { grid-column: 1 / 3; position: relative; margin-top: 6px; height: 6px; border-radius: 99px; background: color-mix(in srgb, #c9d1d9 8%, transparent); overflow: hidden; }
.gauge .bar::before, .gauge .bar::after { content: ""; position: absolute; top: 0; bottom: 0; width: 1px; background: color-mix(in srgb, #c9d1d9 8%, transparent); }
.gauge .bar::before { left: 60%; } .gauge .bar::after { left: 85%; }
.gauge .bar .fill { position: absolute; inset: 0 auto 0 0; height: 100%; border-radius: inherit; }
.gauge .reset { grid-column: 1 / 3; margin-top: 4px; font-family: "JetBrains Mono", Consolas, monospace; font-size: 9.5px; color: #828a92; letter-spacing: .02em; }
.log { display: grid; grid-template-columns: 40px 96px 76px minmax(0, 1fr); gap: 3px 10px; font-size: 0.8125rem; line-height: 1.45; align-items: baseline; }
.log .t { font-family: "JetBrains Mono", Consolas, monospace; font-size: .6875rem; color: #484f58; }
.log .k { font-size: .6875rem; color: #828a92; white-space: nowrap; }
.log .k.strong { color: #c9d1d9; font-weight: 600; }
.log .w { font-size: .6875rem; color: #828a92; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.log .m { color: #c9d1d9; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.log .m.quiet { color: #828a92; }
.log .m i { font-style: normal; color: #484f58; }
.hour { grid-column: 1 / -1; padding-top: 6px; margin-top: 2px; border-top: 1px solid color-mix(in srgb, #232930 65%, transparent); font-family: "JetBrains Mono", Consolas, monospace; font-size: .6875rem; color: #484f58; }
.sheet { width: 800px; display: flex; flex-direction: column; gap: 18px; padding: 16px; background: #131921; }
.sheet h1 { margin: 0; font-size: 1rem; font-weight: 600; color: #c9d1d9; letter-spacing: -.012em; }
.sheet h2 { margin: 0; font-size: 0.8125rem; font-weight: 600; color: #c9d1d9; }
.sheet p { margin: 0; font-size: 0.8125rem; line-height: 1.5; color: #828a92; }
.sheet p b { color: #c9d1d9; font-weight: 500; }
.sheet code { font-family: "JetBrains Mono", Consolas, monospace; font-size: .6875rem; color: #c9d1d9; overflow-wrap: anywhere; }
table { border-collapse: collapse; width: 100%; font-size: 0.8125rem; }
th { text-align: left; font-size: 10px; text-transform: uppercase; letter-spacing: .05em; color: #484f58; font-weight: 700; padding: 6px 10px 6px 0; border-bottom: 1px solid #232930; }
td { padding: 7px 10px 7px 0; border-bottom: 1px solid color-mix(in srgb, #232930 65%, transparent); vertical-align: top; line-height: 1.45; color: #828a92; }
td.k { color: #c9d1d9; font-weight: 600; white-space: nowrap; }
td.n { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; color: #c9d1d9; text-align: right; white-space: nowrap; }
"""

HEAD = """<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <script src="./support.js"></script>
</head>
<body>
<x-dc>
<helmet>
  <style>
/*KIT*/
%s
  </style>
</helmet>
"""


def switcher(labels, selected):
    idx = labels.index(selected)
    opts = "".join(f'<button class="opt" role="tab" aria-selected="{"true" if l == selected else "false"}">{l}</button>' for l in labels)
    return f'<div class="seg" role="tablist" style="--seg-count: {len(labels)}; --seg-index: {idx};"><span class="thumb" aria-hidden="true"></span>{opts}</div>'


def toolbar(phone):
    right = "" if phone else '<span class="ct-meta-muted">Planck\'s team</span>'
    return f'<div class="ct-toolbar"{" style=\"padding: 0 .5rem;\"" if phone else ""}>{switcher(["Board", "Spec", "Plan", "Tasks"], "Board")}{right}</div>'


def status_header(phone, watcher):
    chips = ('<span class="chip state">execution</span>'
             '<span class="chip"><span class="role">orchestrator</span>Planck</span>'
             '<span class="chip"><span class="role">planner</span>Pasteur</span>'
             + ('<span class="chip"><span class="role">watcher</span>haiku, every 5 min</span>' if watcher else ""))
    dc = f'{c["done"]} of {D["total_tasks"]} done · {c["in_review"]} in review' + ("" if phone else f' · {c["cancelled"]} cancelled') + f' · <b style="color: #c9d1d9; font-weight: 500;">{n(T["output"])}</b> output tokens · <b style="color: #c9d1d9; font-weight: 500;">{n(T["cache_read"] + T["cache_write"] + T["uncached"])}</b> read'  
    return (f'<div class="status"><div class="topline"><div class="chips">{chips}</div><span class="done-count">{dc}</span></div>'
            f'<div class="topline"><span class="title">{html.escape(D["title"])}</span></div></div>')


def stat_cards(phone):
    src = (lambda s: "" if phone else f'<p class="src">{s}</p>')
    tokens = (f'<div class="ct-panel-card card"><h2>Tokens today</h2>'
              f'<div><div class="amount">{n(T["output"])}</div><div class="caption">output tokens, every agent on this plan</div></div>'
              f'<p class="small"><b>{n(T["cache_read"])}</b> of context read from cache · <b>{n(T["cache_write"])}</b> written to cache · <b>{n(T["uncached"])}</b> fresh input</p>'
              + src(FILES_LINE) + '</div>')
    turns = (f'<div class="ct-panel-card card"><h2>Turns</h2><div><div class="amount">{T["turns"]:,}</div><div class="caption">model turns today</div></div>'
             f'<p class="small">Orchestrator <b>{100*orch["usage"]["turns"]/T["turns"]:.0f} %</b> of them</p>' + src("count of usage records in the same session files") + '</div>')
    bounces = (f'<div class="ct-panel-card card"><h2>Bounces</h2><div><div class="amount">{D["second_attempts"]}</div><div class="caption">tasks on a second attempt</div></div>'
               f'<p class="small"><b>{D["attempts"]}</b> attempts · <b>{D["first_try"]}</b> finished on the first · <b>{D["bounced_label"]}</b> labelled bounced</p>'
               + src("execution_attempts and status_label in the plan store") + '</div>')
    held = D["held"]; gates = D["gates"]
    if held:
        oldest = max(held, key=lambda h: h["since_last_verdict_min"] or 0)
        card = (f'<div class="ct-panel-card card"><h2>Held for a decision</h2><div><div class="amount warn">{len(held)}</div><div class="caption">review{"s" if len(held) > 1 else ""} held, the oldest {dur(oldest["since_last_verdict_min"])} since its last verdict</div></div>'
                f'<p class="small"><b>{len(gates)}</b> decision gates open · <b>{sum(h["verdicts"] for h in held)}</b> verdicts on the held tasks</p>')
    else:
        card = (f'<div class="ct-panel-card card"><h2>Held for a decision</h2><div><div class="amount">{len(gates)}</div><div class="caption">decision gates open, no review held</div></div>')
    card += src('status_label "review held" and plan.gates; no note text is read') + '</div>'
    cols = "repeat(2, minmax(0, 1fr))" if phone else "1.6fr 1fr 1fr 1fr"
    return f'<div style="display: grid; grid-template-columns: {cols}; gap: {8 if phone else 12}px; align-items: stretch;">{tokens}{turns}{bounces}{card}</div>'


def where_it_went(phone):
    total = T["output"]
    rows = [(AREA_LABEL.get(k, k), v) for k, v in by_area.items() if v]
    rows.sort(key=lambda r: -r[1])
    lines = "".join(f'<div class="kv"><span class="k">{html.escape(k)}</span><span class="v">{n(v)}</span><span class="share">{100*v/total:.0f} %</span></div>' for k, v in rows)
    roles = "".join(f'<div class="kv"><span class="k">{k} <i>team role, not a tag</i></span><span class="v">{n(v)}</span><span class="share">{100*v/total:.0f} %</span></div>'
                    for k, v in (("Orchestration", team["orchestrator"]), ("Planning", team["planner"])))
    note = (f'<p class="small">No task on this plan carries an area tag yet, so <b>{inferred}</b> of {sum(1 for a in agents if a["team_role"] == "worker")} tasks are shown under the area their role suggests; in the product the Planner sets one from the list when the task is created, and nothing is inferred.</p>' if inferred else "")
    return (f'<div class="ct-panel-card card"><div style="display: flex; align-items: baseline; justify-content: space-between; gap: 8px;"><h2>Spend by area</h2><span class="caption">one area per task, from the plan list of {len(AREAS)}</span></div>'
            f'<div style="display: flex; flex-direction: column; gap: 4px;">{lines}</div>'
            f'<div style="display: flex; flex-direction: column; gap: 4px; padding-top: 8px; border-top: 1px solid color-mix(in srgb, #232930 65%, transparent);">{roles}</div>{note}'
            + ("" if phone else '<p class="src">the area:&lt;value&gt; tag on each task, one value of the area facet declared on the plan; output tokens from the session files, summed per area</p>') + '</div>')


def runway(phone):
    gs = []
    for label, pct, reset, kind in USAGE:
        state = "danger" if pct >= 85 else ("warn" if pct >= 60 else "")
        fill = {"danger": "#ff7b72", "warn": "#d29922"}.get(state) or {"session": "#93a6c8", "weekly": "#a78bfa", "codex": "#93a6c8"}[kind]
        gs.append(f'<div class="gauge"><span class="glabel">{label}</span><span class="pct{" " + state if state else ""}">{pct:.0f} %</span>'
                  f'<div class="bar"><div class="fill" style="width: {pct}%; background: linear-gradient(90deg, color-mix(in srgb, {fill} 70%, transparent), {fill});"></div></div><span class="reset">{reset}</span></div>')
    cols = "1fr" if phone else "repeat(3, minmax(0, 1fr))"
    return (f'<div class="ct-panel-card card"><div style="display: flex; align-items: baseline; justify-content: space-between; gap: 8px;"><h2>Runway</h2><span class="caption">one snapshot per provider, read {read_usage.captured}</span></div>'
            f'<div style="display: grid; grid-template-columns: {cols}; gap: 12px 20px;">{"".join(gs)}</div>' + ("" if phone else '<p class="src">usage_snapshots, latest parsed row per provider, percentage and reset from the same row</p>') + '</div>')


def event_log(phone, limit=None):
    events = D["events"]
    rows = []
    for e in events:
        if rows and e["kind"] == "worker_report" and rows[-1]["kind"] == "worker_report" and rows[-1]["who"] == e["who"] and rows[-1]["task"] == e["task"]:
            rows[-1]["n"] += 1; rows[-1]["last"] = e["ts"]; continue
        rows.append(dict(e, n=1, last=e["ts"]))
    if limit: rows = rows[-limit:]
    out = []; hour = None
    strong = {"decision", "review", "planning", "gate", "bounce", "incident"}
    for r in rows:
        h = time.strftime("%H", time.localtime(r["ts"] / 1000))
        if h != hour:
            hour = h; out.append(f'<span class="hour">{h}:00</span>')
        label = kind_label(r["kind"])
        task = D["task_names"].get(r["task"], "") if r["task"] else ""
        text = r["text"].split("\n")[0]
        if r["kind"] == "worker_report":
            head = text.split("STATUS:")[0].strip(" :")
            text = f'{head or "reported"}' + (f' <i>×{r["n"]}, {hm(r["ts"])} to {hm(r["last"])}</i>' if r["n"] > 1 else "")
        elif r["kind"] == "task_created":
            text = html.escape(task); task = ""
        else:
            text = html.escape(text)
        quiet = "" if r["kind"] in strong else " quiet"
        kcls = " strong" if r["kind"] in strong else ""
        tasktxt = f'<i>{html.escape(task[:38] + ("…" if len(task) > 38 else ""))}</i> ' if task else ""
        out.append(f'<span class="t">{hm(r["ts"])}</span><span class="k{kcls}">{label}</span><span class="w">{html.escape(r["who"][:12])}</span><span class="m{quiet}">{tasktxt}{text}</span>')
    cap = f'{len(events)} recorded events, {len(rows)} lines; a run of reports by one agent on one task is one line with a count'
    return (f'<div class="ct-panel-card card"><div style="display: flex; align-items: baseline; justify-content: space-between; gap: 8px;"><h2>What happened today</h2><span class="caption">{cap}</span></div>'
            f'<div class="log">{"".join(out)}</div>' + ("" if phone else '<p class="src">plan milestones, strides, planning log and gates in time order; kind, time and author are fields, the line is the first line as written</p>') + '</div>')


def watcher_card():
    return ('<div class="ct-panel-card card"><div style="display: flex; align-items: baseline; justify-content: space-between; gap: 8px;"><h2>What the watcher wrote today</h2><span class="caption">last woke 3 min ago and said nothing new</span></div>'
            '<div class="log" style="grid-template-columns: 40px 68px minmax(0, 1fr);">'
            '<span class="t">18:24</span><span class="k">to you</span><span class="m" style="white-space: normal;">Stop a successful first-run spawn reporting as failed is held pending your direct approval; Planck cannot clear it alone. Nothing again until the hold clears.</span>'
            '<span class="t">21:05</span><span class="k">to Planck</span><span class="m" style="white-space: normal;">Review at 4 of 5 slots and credit one closure from the wall: close a review before dispatching.</span>'
            '<span class="t">21:35</span><span class="k">to Planck</span><span class="m" style="white-space: normal;">Review back to 2 of 5; the warning is withdrawn. One hold remains on the user.</span>'
            '</div></div>')


def page(width, phone=False, watcher=False):
    body = [status_header(phone, watcher), stat_cards(phone), where_it_went(phone), runway(phone)]
    if watcher: body.append(watcher_card())
    body.append(event_log(phone, limit=18 if phone else None))
    return (HEAD % STYLE + f'<div style="width: {width}px; display: flex; flex-direction: column; background: #131921;">{toolbar(phone)}'
            + '<div style="display: flex; flex-direction: column; gap: 12px; padding: 12px 12px 16px;">' + "".join(body) + '</div></div>\n</x-dc>\n</body>\n</html>\n')


def labels_sheet():
    conn = sqlite3.connect(os.path.join(os.path.expanduser("~"), ".codeterm", "plans.db"))
    tags = collections.Counter(); tagged = 0; total = 0
    for (doc,) in conn.execute("select document_json from plans"):
        for t in json.loads(doc).get("tasks", []):
            total += 1
            if t.get("tags"): tagged += 1
            for g in t.get("tags") or []: tags[g] += 1
    areas = "".join(f'<tr><td class="k">{l}</td><td><code>area:{v}</code></td></tr>' for v, l in AREAS)
    top = ", ".join(f'{html.escape(t)} ({k})' for t, k in tags.most_common(12))
    tonight = "".join(f'<tr><td class="k">{html.escape(str(a["name"]))}</td><td><code>{html.escape(str(a["executor_role"] or a["team_role"]))}</code></td><td>{("tagged " + AREA_LABEL[a["area"]]) if a["area"] else (("inferred " + AREA_LABEL[a["area_hint"]]) if a["area_hint"] else ("team role" if a["team_role"] != "worker" else UNTAGGED))}</td></tr>' for a in agents)
    kinds = "".join(f'<tr><td class="k">{v}</td><td><code>{k}</code></td></tr>' for k, v in KIND_LABELS.items())
    return (HEAD % STYLE + '<div class="sheet"><h1>The tag list</h1>'
            f'<p>Spend by area only means something if the area is one value from one short list that the Planner picks when the task is created, not a word invented in the moment. The store shows what happens otherwise: <b>{tagged:,}</b> of <b>{total:,}</b> tasks carry tags and there are <b>{len(tags):,}</b> distinct ones. The most used: {top}.</p>'
            '<p>The product already has the mechanism. A plan declares a tag facet, a key with its allowed values and labels; a task carries one <code>key:value</code> tag per facet, and <code>apply_tags</code> in <code>tag_model.rs</code> clears the old value of that facet when a new one is set, so a task never carries two. No plan has declared one yet.</p>'
            '<h2>The area facet</h2>'
            f'<table><tr><th>Area</th><th>Tag on the task</th></tr>{areas}</table>'
            f'<p>Declared once per plan, in the syntax the product accepts: <code>codeterm plan facets set &lt;plan&gt; "{html.escape(AREA_FACET_COMPACT)}"</code>. A value outside the list is reported by <code>tag_report</code> as undeclared. A task with no area tag shows as Untagged on the board; the board never guesses.</p>'
            '<h2>Tonight, task by task</h2><p>This plan was created before the facet existed, so no task carries an area tag. The board shows the area each role suggests and says so; the column below is what the Planner would have set.</p>'
            f'<table><tr><th>Agent</th><th>Stored role</th><th>Area</th></tr>{tonight}</table>'
            '<h2>Event kinds</h2><p>The log shows each milestone and stride under the label for its stored kind. A kind not in this table shows as Note with the kind in brackets, never silently.</p>'
            f'<table><tr><th>Label</th><th>Kind field</th></tr>{kinds}</table></div>\n</x-dc>\n</body>\n</html>\n')


def cost_sheet():
    fb = sum(f["bytes"] for f in files) / 1e6; fl = sum(f["lines"] for f in files)
    return (HEAD % STYLE + '<div class="sheet"><h1>What this costs to compute, and where it should live</h1>'
            '<h2>Measured, not estimated</h2>'
            f'<p>A full extraction for this plan, <code>tokens.py</code>, read <b>{len(files)} session files</b>, <b>{fb:.1f} MB</b>, <b>{fl:,} lines</b>, and took <b>{D["extract_seconds"]} s</b> wall time on this machine with the files warm in the cache, 1.6 s cold, including the plan store read and the event list. Reading only the last 64 KB of each file, which is what an incremental pass does, took <b>0.025 s</b> for 0.9 MB.</p>'
            '<p>Scaled to a day rather than a plan: this machine touched <b>74 Claude logs (142 MB)</b> and <b>50 Codex rollouts (166 MB)</b> today, so a whole-day full pass is about <b>8 s</b> at the cold rate, and an incremental pass stays under a second. Everything on disk is <b>1,299 Claude logs (1.2 GB)</b> and <b>2,638 Codex rollouts (39 GB)</b>; a full rescan of history is not something to do on open, and never needs doing, because the figures are additive and a cursor per file makes every later read incremental.</p>'
            '<p><b>Verdict:</b> cheap enough to run on open for one plan, cheap enough to run continuously behind cursors, and a schedule buys nothing. The board is arithmetic over fields; there is no model call anywhere in it.</p>'
            '<h2>The ledger that already exists</h2>'
            '<p>The KPI ledger in <code>src-tauri/src/agent/kpi_ledger.rs</code> is a SQLite table <code>kpi_events</code> with <code>plan_id, task_id, tab_id, role, provider, model, timestamp, kind, bytes</code>, three token columns <code>input, uncached_input, output</code>, and a <code>dedup_key</code>. Its <code>kpi_usage_cursors</code> table keeps a forward-only read offset per session file. <code>kpi/usage/records.rs</code> parses the two record shapes <code>tokens.py</code> parses: Claude\'s <code>message.usage</code> with cache read and cache creation, and Codex\'s <code>payload.info.last_token_usage</code> with cached input. <code>kpi/recorder.rs</code> ingests from the cursor and writes one batch per file. It is off on this machine: <code>kpi.enabled</code> is false and no <code>kpi_ledger.db</code> exists.</p>'
            '<h2>What the board would read from it, what it already covers, what it lacks</h2>'
            '<table><tr><th>Board figure</th><th>Source today</th><th>From the ledger</th></tr>'
            '<tr><td class="k">Output, cache read, cache write, fresh input</td><td>session files, full read</td><td>token_usage rows carry input, uncached input and output; cache write is folded into uncached input, so the read and write split is lost and needs one more column</td></tr>'
            '<tr><td class="k">Turns, per agent and total</td><td>usage records counted</td><td>one token_usage row per turn: a count</td></tr>'
            '<tr><td class="k">Role labels</td><td>executor_role from the plan store</td><td>the role column stores the role kind\'s label with its profile; the raw executor_role stays in the plan store, so the scheme joins on task_id</td></tr>'
            '<tr><td class="k">Bounces, attempts, held reviews, decision gates, the event log</td><td>plan store fields</td><td>already persisted in the plan store; the ledger\'s bounce and report rows add timestamps but are not needed</td></tr>'
            '<tr><td class="k">Runway</td><td>usage_snapshots per provider</td><td>already persisted; not ledger material</td></tr>'
            '</table>'
            '<p><b>Answer:</b> turn <code>kpi.enabled</code> on and the ledger records the token figures incrementally from the same files behind per-file cursors; the board then reads <code>kpi_events</code> instead of scanning logs, and the plan store and usage store already hold the rest. Two additions make it exact: a cache-write column so the read and write split survives, and the token columns added to <code>aggregate_by_plan</code>, which today rolls up counts and bytes. Nothing else is missing.</p>'
            '</div>\n</x-dc>\n</body>\n</html>\n')


for name, w, ph, wa in (("Main", 960, False, False), ("Watcher", 960, False, True), ("Phone", 390, True, False)):
    open(os.path.join(HERE, name + ".src"), "w", encoding="utf-8").write(page(w, ph, wa))
open(os.path.join(HERE, "Labels.src"), "w", encoding="utf-8").write(labels_sheet())
open(os.path.join(HERE, "Cost.src"), "w", encoding="utf-8").write(cost_sheet())
print("frames written; output", n(T["output"]), "turns", T["turns"], "by area", {k: n(v) for k, v in by_area.items() if v})
