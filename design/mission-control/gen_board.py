"""Draw Main, Watcher and Phone from tokens.json with the product's own kit."""
import json, os, html

HERE = os.path.dirname(os.path.abspath(__file__))
D = json.load(open(os.path.join(HERE, "tokens.json"), encoding="utf-8"))

# Read from `codeterm usage latest` and the Codex rollout's rate_limits at the time tokens.json was written.
USAGE = [("Claude session", 24, "resets 23:10 · in 58m", "session"), ("Claude weekly", 27, "resets Sat 17:00", "weekly"), ("Codex weekly", 80, "resets Tue 05:50", "codex")]

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
.share { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 11px; color: #828a92; text-align: right; }
.row .name { font-size: 0.8125rem; color: #c9d1d9; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.row .who { font-size: .6875rem; color: #484f58; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.track { position: relative; height: 6px; border-radius: 99px; background: color-mix(in srgb, #c9d1d9 8%, transparent); overflow: hidden; }
.fill { height: 100%; border-radius: inherit; background: linear-gradient(90deg, color-mix(in srgb, #58a6ff 70%, transparent), #58a6ff); }
.val { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 12px; font-weight: 600; color: #c9d1d9; text-align: right; }
.turns { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 11px; color: #828a92; text-align: right; white-space: nowrap; }
.st { font-size: .6875rem; text-align: right; white-space: nowrap; }
.gauge { display: grid; grid-template-columns: 1fr auto; align-items: baseline; column-gap: 10px; }
.gauge .label { font-size: 11px; color: #c9d1d9; font-weight: 500; letter-spacing: .02em; }
.gauge .pct { font-family: "JetBrains Mono", Consolas, monospace; font-variant-numeric: tabular-nums; font-size: 12px; font-weight: 600; color: #c9d1d9; }
.gauge .pct.warn { color: #d29922; }
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
    return f'<div class="ct-toolbar"{" style=\"padding: 0 .5rem;\"" if phone else ""}>{switcher(["Board", "Tasks", "Spec", "Plan"], "Board")}{right}</div>'


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


def by_agent(phone):
    rows = sorted(agents, key=lambda a: -a["usage"]["output"])
    shown = rows[:6] if phone else rows
    out = []
    for a in shown:
        u = a["usage"]; chip, col = status_chip(a)
        share = 100 * u["output"] / T["output"]
        who = f'{a["group"]} · {a["model"]}'
        if phone:
            out.append(f'<div class="row" style="grid-template-columns: 72px minmax(0, 1fr) 56px 44px; height: 24px;"><span class="name">{html.escape(a["name"])}</span><span class="who">{who}</span><span class="val">{n(u["output"])}</span><span class="share">{share:.0f} %</span></div>')
        else:
            out.append(f'<div class="row"><span class="name">{html.escape(a["name"])}</span><span class="who">{who}</span><span class="val">{n(u["output"])}</span><span class="share">{share:.0f} %</span><span class="turns">{u["turns"]:,} turns</span><span class="st" style="color: {col};">{chip}</span></div>')
    more = f'<p class="small">and {len(rows) - len(shown)} more, {n(sum(a["usage"]["output"] for a in rows[len(shown):]))} between them</p>' if len(shown) < len(rows) else ""
    cols = "" if phone else '<div class="row" style="height: 18px;"><span class="caption">agent</span><span class="caption">role · model</span><span class="caption" style="text-align: right;">output</span><span class="caption" style="text-align: right;">share</span><span class="caption" style="text-align: right;">turns</span><span class="caption" style="text-align: right;">status</span></div>'
    head = ('<div style="display: flex; align-items: center; justify-content: space-between; gap: 8px;"><h2>By agent</h2>'
            + ("" if phone else switcher(["Output", "Context read", "Turns"], "Output")) + '</div>')
    return f'<div class="ct-panel-card card">{head}{cols}<div style="display: flex; flex-direction: column; gap: 2px;">{"".join(out)}</div>{more}</div>'


def runway(phone):
    gs = []
    for label, pct, reset, kind in USAGE:
        warn = pct >= 60
        fill = "#d29922" if warn else {"session": "#93a6c8", "weekly": "#a78bfa", "codex": "#93a6c8"}[kind]
        gs.append(f'<div class="gauge"><span class="label">{label}</span><span class="pct{" warn" if warn else ""}">{pct} %</span>'
                  f'<div class="bar"><div class="fill" style="width: {pct}%; background: linear-gradient(90deg, color-mix(in srgb, {fill} 70%, transparent), {fill});"></div></div><span class="reset">{reset}</span></div>')
    cols = "1fr" if phone else "repeat(3, minmax(0, 1fr))"
    return f'<div class="ct-panel-card card"><h2>Runway</h2><div style="display: grid; grid-template-columns: {cols}; gap: 12px 20px;">{"".join(gs)}</div></div>'


def watcher_card():
    return ('<div class="ct-panel-card card"><div style="display: flex; align-items: baseline; justify-content: space-between; gap: 8px;"><h2>What the watcher wrote today</h2><span class="caption">last woke 3 min ago and said nothing new</span></div>'
            '<div class="log">'
            '<span class="t">18:24</span><span class="to">to you</span><span class="m">Stop a successful first-run spawn reporting as failed is held pending your direct approval; Planck cannot clear it alone. Nothing again until the hold clears.</span>'
            '<span class="t">21:05</span><span class="to">to Planck</span><span class="m">Review at 4 of 5 slots and credit one closure from the wall: close a review before dispatching.</span>'
            '<span class="t">21:35</span><span class="to">to Planck</span><span class="m">Review back to 2 of 5; the warning is withdrawn. One hold remains on the user.</span>'
            '</div></div>')


def page(width, phone=False, watcher=False):
    body = [status_header(phone, watcher), stat_cards(phone), by_agent(phone), runway(phone)]
    if watcher:
        body.append(watcher_card())
    return (HEAD % STYLE + f'<div style="width: {width}px; display: flex; flex-direction: column; background: #131921;">{toolbar(phone)}'
            + '<div style="display: flex; flex-direction: column; gap: 12px; padding: 12px 12px 16px;">' + "".join(body) + '</div></div>\n</x-dc>\n</body>\n</html>\n')


for name, w, ph, wa in (("Main", 960, False, False), ("Watcher", 960, False, True), ("Phone", 390, True, False)):
    open(os.path.join(HERE, name + ".src"), "w", encoding="utf-8").write(page(w, ph, wa))
    print(name, "written")
print("tokens: output", n(T["output"]), "cache_read", n(T["cache_read"]), "turns", T["turns"], "orchestrator share %.0f%%" % (100 * orch["usage"]["turns"] / T["turns"]))
