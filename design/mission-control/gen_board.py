"""Draw Main, Watcher and Phone from tokens.json with the product's own kit."""
import json, os, html

HERE = os.path.dirname(os.path.abspath(__file__))
D = json.load(open(os.path.join(HERE, "tokens.json"), encoding="utf-8"))

# Runway is read from ~/.codeterm/usage/usage.db: the latest parsed snapshot per provider, each carrying its own reset.
import sqlite3, time
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

agents = D["agents"]
T = {k: sum(a["usage"][k] for a in agents) for k in ("turns", "output", "cache_read", "cache_write", "uncached")}
by_role = {}
for a in agents:
    r = "design" if a["role"].startswith("frontend-designer") else ("fix" if a["role"] not in ("orchestrator", "planner") else a["role"])
    a["group"] = r
    by_role.setdefault(r, 0)
    by_role[r] += a["usage"]["output"]
orch = next(a for a in agents if a["group"] == "orchestrator")
c = D["counts"]
hold = D["holds"][0] if D["holds"] else None


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


def status_chip(a):
    s = a["status"]
    if a["label"] in ("tab-gone", "pane-gone"):
        return ("gone", "#ff7b72")
    if a["group"] in ("orchestrator", "planner"):
        return ("working" if a["group"] == "orchestrator" else "idle", "#828a92")
    return ({"in review": "in review", "done": "done", "in progress": "working"}.get(s, s), {"in review": "#d29922", "done": "#7ee787", "in progress": "#58a6ff"}.get(s, "#828a92"))


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
.row { display: grid; grid-template-columns: 96px minmax(0, 1fr) 64px 56px 80px 74px; gap: 10px; align-items: center; height: 26px; }
.kv { display: grid; grid-template-columns: minmax(0, 1fr) 72px 56px; gap: 10px; align-items: baseline; max-width: 520px; }
.kv .k { font-size: 0.8125rem; color: #c9d1d9; }
.kv .v { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 12px; font-weight: 600; color: #c9d1d9; text-align: right; }
.share { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 11px; color: #828a92; text-align: right; }
.row .name { font-size: 0.8125rem; color: #c9d1d9; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.row .who { font-size: .6875rem; color: #484f58; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.track { position: relative; height: 6px; border-radius: 99px; background: color-mix(in srgb, #c9d1d9 8%, transparent); overflow: hidden; }
.fill { height: 100%; border-radius: inherit; background: linear-gradient(90deg, color-mix(in srgb, #58a6ff 70%, transparent), #58a6ff); }
.val { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 12px; font-weight: 600; color: #c9d1d9; text-align: right; }
.turns { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 11px; color: #828a92; text-align: right; white-space: nowrap; }
.st { font-size: .6875rem; text-align: right; white-space: nowrap; }
.gauge { display: grid; grid-template-columns: 1fr auto; align-items: baseline; column-gap: 10px; }
.gauge .glabel { font-size: 11px; color: #c9d1d9; font-weight: 500; letter-spacing: .02em; text-transform: none; }
.gauge .pct { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 12px; font-weight: 600; color: #c9d1d9; }
.gauge .pct.warn { color: #d29922; }
.gauge .pct.danger { color: #ff7b72; }
.gauge .bar { grid-column: 1 / 3; position: relative; margin-top: 6px; height: 6px; border-radius: 99px; background: color-mix(in srgb, #c9d1d9 8%, transparent); overflow: hidden; }
.gauge .bar::before, .gauge .bar::after { content: ""; position: absolute; top: 0; bottom: 0; width: 1px; background: color-mix(in srgb, #c9d1d9 8%, transparent); }
.gauge .bar::before { left: 60%; } .gauge .bar::after { left: 85%; }
.gauge .bar .fill { position: absolute; inset: 0 auto 0 0; }
.gauge .reset { grid-column: 1 / 3; margin-top: 4px; font-family: "JetBrains Mono", Consolas, monospace; font-size: 9.5px; color: #828a92; letter-spacing: .02em; }
.log { display: grid; grid-template-columns: 44px 68px minmax(0, 1fr); gap: 5px 10px; font-size: 0.8125rem; line-height: 1.45; }
.log .t { font-family: "JetBrains Mono", Consolas, monospace; font-size: .6875rem; color: #484f58; padding-top: 2px; }
.log .to { color: #828a92; } .log .m { color: #c9d1d9; }
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
    dc = f'{c["done"]} of {D["total_tasks"]} done · {c["in_review"]} in review' + ("" if phone else f' · {c["cancelled"]} cancelled')
    return (f'<div class="status"><div class="topline"><div class="chips">{chips}</div><span class="done-count">{dc}</span></div>'
            f'<div class="topline"><span class="title">{html.escape(D["title"])}</span></div></div>')


def stat_cards(phone):
    tokens = (f'<div class="ct-panel-card card"><h2>Tokens today</h2>'
              f'<div><div class="amount">{n(T["output"])}</div><div class="caption">output tokens, all agents on this plan</div></div>'
              f'<p class="small"><b>{n(T["cache_read"])}</b> of context read back from cache · <b>{n(T["cache_write"])}</b> written to cache · <b>{n(T["uncached"])}</b> fresh input</p>'
              + ("" if phone else f'<p class="small">Design work wrote <b>{100*by_role["design"]/T["output"]:.0f} %</b> of the output; the orchestrator read <b>{100*orch["usage"]["cache_read"]/T["cache_read"]:.0f} %</b> of all context.</p>')
              + '</div>')
    turns = (f'<div class="ct-panel-card card"><h2>Turns</h2><div><div class="amount">{T["turns"]:,}</div><div class="caption">model turns today</div></div>'
             f'<p class="small">Planck took <b>{100*orch["usage"]["turns"]/T["turns"]:.0f} %</b> of them</p></div>')
    bounces = (f'<div class="ct-panel-card card"><h2>Bounces</h2><div><div class="amount">{D["second_attempts"]}</div><div class="caption">tasks sent back for a second attempt</div></div>'
               f'<p class="small"><b>{D["attempts"]}</b> attempts · <b>{D["first_try"]}</b> finished on the first</p></div>')
    if hold:
        wait = (f'<div class="ct-panel-card card"><h2>Waiting on you</h2><div><div class="amount warn">{dur(hold["waiting_min"])}</div><div class="caption">one verdict held for your approval</div></div>'
                f'<p class="small">Held <b>{hold["holds"]}</b> times, the latest {dur(hold["latest_min"])} ago</p></div>')
    else:
        wait = '<div class="ct-panel-card card"><h2>Waiting on you</h2><div><div class="amount">0</div><div class="caption">nothing needs your answer</div></div></div>'
    cols = "repeat(2, minmax(0, 1fr))" if phone else "1.6fr 1fr 1fr 1fr"
    return f'<div style="display: grid; grid-template-columns: {cols}; gap: {8 if phone else 12}px; align-items: stretch;">{tokens}{turns}{bounces}{wait}</div>'


def where_it_went(phone):
    total = T["output"]
    kinds = [("design tasks", by_role.get("design", 0)), ("fix tasks", by_role.get("fix", 0)), ("orchestration", by_role.get("orchestrator", 0)), ("planning", by_role.get("planner", 0))]
    kinds.sort(key=lambda k: -k[1])
    lines = "".join(f'<div class="kv"><span class="k">{k}</span><span class="v">{n(v)}</span><span class="share">{100*v/total:.0f} %</span></div>' for k, v in kinds)
    overhead = (f'<p class="small">The Orchestrator alone took <b>{100*orch["usage"]["turns"]/T["turns"]:.0f} %</b> of all turns and re-read <b>{100*orch["usage"]["cache_read"]/T["cache_read"]:.0f} %</b> of all context: that is the cost of coordination, not of work.</p>')
    waste = f'<p class="small">Spent on attempts that were bounced: <b>{n(sum(a["usage"]["output"] for a in agents if a["attempts"] >= 2))}</b> output tokens.</p>'
    return f'<div class="ct-panel-card card"><h2>Where the output went</h2><div style="display: flex; flex-direction: column; gap: 4px;">{lines}</div>{overhead}{waste}</div>'


def runway(phone):
    gs = []
    for label, pct, reset, kind in USAGE:
        state = "danger" if pct >= 85 else ("warn" if pct >= 60 else "")
        fill = {"danger": "#ff7b72", "warn": "#d29922"}.get(state) or {"session": "#93a6c8", "weekly": "#a78bfa", "codex": "#93a6c8"}[kind]
        gs.append(f'<div class="gauge"><span class="glabel">{label}</span><span class="pct{" " + state if state else ""}">{pct:.0f} %</span>'
                  f'<div class="bar"><div class="fill" style="width: {pct}%; background: linear-gradient(90deg, color-mix(in srgb, {fill} 70%, transparent), {fill});"></div></div><span class="reset">{reset}</span></div>')
    cols = "1fr" if phone else "repeat(3, minmax(0, 1fr))"
    return f'<div class="ct-panel-card card"><div style="display: flex; align-items: baseline; justify-content: space-between; gap: 8px;"><h2>Runway</h2><span class="caption">one snapshot per provider, read {read_usage.captured}</span></div><div style="display: grid; grid-template-columns: {cols}; gap: 12px 20px;">{"".join(gs)}</div></div>'


def watcher_card():
    return ('<div class="ct-panel-card card"><div style="display: flex; align-items: baseline; justify-content: space-between; gap: 8px;"><h2>What the watcher wrote today</h2><span class="caption">last woke 3 min ago and said nothing new</span></div>'
            '<div class="log">'
            '<span class="t">18:24</span><span class="to">to you</span><span class="m">Stop a successful first-run spawn reporting as failed is held pending your direct approval; Planck cannot clear it alone. Nothing again until the hold clears.</span>'
            '<span class="t">21:05</span><span class="to">to Planck</span><span class="m">Review at 4 of 5 slots and credit one closure from the wall: close a review before dispatching.</span>'
            '<span class="t">21:35</span><span class="to">to Planck</span><span class="m">Review back to 2 of 5; the warning is withdrawn. One hold remains on the user.</span>'
            '</div></div>')


def page(width, phone=False, watcher=False):
    body = [status_header(phone, watcher), stat_cards(phone), where_it_went(phone), runway(phone)]
    if watcher:
        body.append(watcher_card())
    return (HEAD % STYLE + f'<div style="width: {width}px; display: flex; flex-direction: column; background: #131921;">{toolbar(phone)}'
            + '<div style="display: flex; flex-direction: column; gap: 12px; padding: 12px 12px 16px;">' + "".join(body) + '</div></div>\n</x-dc>\n</body>\n</html>\n')


for name, w, ph, wa in (("Main", 960, False, False), ("Watcher", 960, False, True), ("Phone", 390, True, False)):
    open(os.path.join(HERE, name + ".src"), "w", encoding="utf-8").write(page(w, ph, wa))
    print(name, "written")
print("tokens: output", n(T["output"]), "cache_read", n(T["cache_read"]), "turns", T["turns"], "orchestrator share %.0f%%" % (100 * orch["usage"]["turns"] / T["turns"]))
