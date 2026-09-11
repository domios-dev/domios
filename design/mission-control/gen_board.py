import json, os, time, html

SC = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.dirname(os.path.abspath(__file__))
D = json.load(open(os.path.join(SC, "timeline.json"), encoding="utf-8"))
now, approved = D["now"], D["approved"]
rows = [r for r in D["rows"] if any(a.get("assigned") for a in r["attempts"])]
origin = min(r["created"] for r in D["rows"] if any(a.get("assigned") for a in r["attempts"]))
span = now - origin

def hm(ts): return time.strftime("%H:%M", time.localtime(ts / 1000))
def dur(m):
    m = int(round(m)); h, mm = divmod(m, 60)
    return f"{h} h {mm:02d} m" if h else f"{mm} min"

# aggregates
work = sum((a["reported"] - a["assigned"]) for r in rows for a in r["attempts"] if a.get("reported") and a.get("assigned")) / 60000
wait = sum(((a["closed"] or now) - a["reported"]) for r in rows for a in r["attempts"] if a.get("reported")) / 60000
wait_open = sum((now - a["reported"]) for r in rows for a in r["attempts"] if a.get("reported") and not a.get("closed")) / 60000
wait_closed = wait - wait_open
pickup = sum((r["attempts"][0]["assigned"] - r["created"]) for r in rows if r["attempts"] and r["attempts"][0].get("assigned")) / 60000
FLOOR = 120 * 60000

def strip(width, label_w, phone=False):
    rh = 20 if phone else 24
    bar = 6 if phone else 8
    scale = width / span
    x = lambda t: max(0, min(width, (t - origin) * scale))
    out = []
    for r in rows:
        name = r["name"]
        who = (r["attempts"][0].get("who") if r["attempts"] else None) or ""
        done = r["status"] in ("done", "cancelled")
        ncol = "#484f58" if done else "#828a92"
        maxc = 16 if phone else 30
        short = name if len(name) <= maxc else name[:maxc - 1].rstrip() + "…"
        segs = []
        if r["attempts"] and r["attempts"][0].get("assigned"):
            x0, x1 = x(r["created"]), x(r["attempts"][0]["assigned"])
            segs.append(f'<div title="waiting to be picked up {dur((r["attempts"][0]["assigned"]-r["created"])/60000)}" style="position: absolute; left: {x0:.1f}px; width: {max(2, x1-x0-2):.1f}px; top: {(rh-bar)//2}px; height: {bar}px; border-radius: 1px; background: #232930;"></div>')
        for a in r["attempts"]:
            if a.get("assigned") and a.get("reported"):
                x0, x1 = x(a["assigned"]), x(a["reported"])
                segs.append(f'<div title="{html.escape(who)} working {dur((a["reported"]-a["assigned"])/60000)}" style="position: absolute; left: {x0:.1f}px; width: {max(2, x1-x0-2):.1f}px; top: {(rh-bar)//2}px; height: {bar}px; border-radius: 1px; background: #58a6ff;"></div>')
                if a.get("reported"):
                    end = a["closed"] or now
                    x2, x3 = x(a["reported"]), x(end)
                    over = (not a.get("closed")) and (end - a["reported"] > FLOOR)
                    color = "#d29922" if over else "rgba(72,79,88,0.55)"
                    segs.append(f'<div title="waiting for a verdict {dur((end-a["reported"])/60000)}{"" if a.get("closed") else ", still running"}" style="position: absolute; left: {x2+1:.1f}px; width: {max(2, x3-x2-1):.1f}px; top: {(rh-bar)//2}px; height: {bar}px; border-radius: 1px; background: {color};"></div>')
                    if not a.get("closed"):
                        segs.append(f'<div title="still running" style="position: absolute; left: {x3-1:.1f}px; width: 2px; top: {(rh-bar)//2 - 3}px; height: {bar+6}px; background: #c9d1d9;"></div>')
            elif a.get("assigned"):
                x0 = x(a["assigned"])
                segs.append(f'<div title="{html.escape(who)} working {dur((now-a["assigned"])/60000)}, no report yet" style="position: absolute; left: {x0:.1f}px; width: {max(2, width-x0):.1f}px; top: {(rh-bar)//2}px; height: {bar}px; border-radius: 1px; background: #58a6ff;"></div>')
                segs.append(f'<div title="still running" style="position: absolute; left: {width-1:.1f}px; width: 2px; top: {(rh-bar)//2 - 3}px; height: {bar+6}px; background: #c9d1d9;"></div>')
        for h in r["holds"]:
            segs.append(f'<div title="held {hm(h)}" style="position: absolute; left: {x(h)-1:.1f}px; width: 2px; top: {(rh-bar)//2 - 4}px; height: {bar+8}px; background: #d29922;"></div>')
        clusters = []
        for rp in sorted(r["reports"]):
            if clusters and x(rp) - clusters[-1][0] < 6: clusters[-1][1] += 1
            else: clusters.append([x(rp), 1])
        for cx, n in clusters:
            segs.append(f'<div title="{n} report{"s" if n > 1 else ""}" style="position: absolute; left: {cx:.1f}px; width: 1px; top: {(rh-bar)//2 + bar + 2}px; height: 4px; background: #828a92;"></div>')
            if n > 1:
                segs.append(f'<span style="position: absolute; left: {cx+3:.1f}px; top: {(rh-bar)//2 + bar - 1}px; font-family: JetBrains Mono, Consolas, monospace; font-size: .6875rem; line-height: 1; color: #828a92;">{n}</span>')
        whotxt = f' <span style="color: #484f58;">{html.escape(who)}</span>' if who and not phone else ""
        out.append(
            f'<div style="display: grid; grid-template-columns: {label_w}px minmax(0, 1fr); gap: 8px; align-items: center; height: {rh}px;">'
            f'<span style="font-size: .6875rem; color: {ncol}; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">{html.escape(short)}{whotxt}</span>'
            f'<div style="position: relative; height: {rh}px; min-width: 0;">{"".join(segs)}</div></div>')
    # axis
    ticks = []
    t0 = origin
    first = (int(t0 / 3600000) + 1) * 3600000
    t = first
    while t < now:
        if (not phone) or (int(t / 3600000) % 2 == 0):
            ticks.append(f'<span style="position: absolute; left: {x(t):.1f}px; transform: translateX(-50%); font-family: JetBrains Mono, Consolas, monospace; font-size: .6875rem; color: #484f58;">{hm(t)}</span>')
        t += 3600000
    appr = f'<span style="position: absolute; left: {x(approved):.1f}px; top: -1px; width: 1px; height: 5px; background: #484f58;"></span><span style="position: absolute; left: {x(approved)+4:.1f}px; font-size: .6875rem; color: #484f58;">approved</span>'
    axis = (f'<div style="display: grid; grid-template-columns: {label_w}px minmax(0, 1fr); gap: 8px; align-items: start; height: 18px;">'
            f'<span style="font-size: .6875rem; color: #484f58;">{"" if phone else "from " + hm(origin)}</span>'
            f'<div style="position: relative; height: 18px; border-top: 1px solid #232930;">{"" if phone else appr}{"".join(ticks)}'
            f'<span style="position: absolute; right: 0; transform: translateX(0); font-family: JetBrains Mono, Consolas, monospace; font-size: .6875rem; color: #c9d1d9;">now {hm(now)}</span></div></div>')
    nowline = f'<div style="position: absolute; right: 0; top: 0; bottom: 18px; width: 1px; background: rgba(201,209,217,0.35); animation: ct-now-breathe 2.6s cubic-bezier(.42,0,.58,1) infinite;"></div>'
    return (f'<div style="position: relative; display: flex; flex-direction: column; gap: 0;">'
            f'<div style="position: absolute; left: {label_w+8}px; right: 0; top: 0; bottom: 0; pointer-events: none;">{nowline}</div>'
            f'{"".join(out)}{axis}</div>')

def split_bar(width):
    tot = work + wait + pickup
    parts = [("waiting to be picked up", pickup, "#232930"), ("working", work, "#58a6ff"), ("waiting for a verdict, elapsed", wait_closed, "rgba(72,79,88,0.55)"), ("waiting for a verdict, still running", wait_open, "rgba(72,79,88,0.55)")]
    cells = "".join(f'<div title="{n} {dur(v)}" style="width: {max(2, v/tot*width-2):.1f}px; height: 8px; border-radius: 1px; background: {c};"></div>' for n, v, c in parts)
    cells += '<div title="still running" style="width: 2px; height: 14px; margin-left: -2px; background: #c9d1d9;"></div>'
    return (f'<div style="display: flex; flex-direction: column; gap: 6px;">'
            f'<div class="fig"><span class="figure">{dur(tot)}</span><span class="unit">of task time today, summed over tasks; up to four agents ran at once, so this is not the clock</span></div>'
            f'<div style="display: flex; gap: 2px; align-items: center;">{cells}</div>'
            f'<p class="q" style="margin: 0;"><b>{dur(pickup)}</b> waiting to be picked up · <b>{dur(work)}</b> working · <b>{dur(wait)}</b> waiting for a verdict, of which <b>{dur(wait_open)}</b> is a clock still running on two open reviews</p></div>')

STYLE = """
.kpi { display: flex; flex-direction: column; gap: 6px; padding: 12px; min-width: 0; }
.kpi .fig { display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap; }
.kpi .fig .unit { font-size: 0.8125rem; color: #828a92; }
.q { font-size: 0.8125rem; line-height: 1.45; color: #828a92; margin: 0; }
.q b { color: #c9d1d9; font-weight: 500; }
.kpi .read { font-size: 0.8125rem; line-height: 1.45; color: #c9d1d9; margin: 0; padding-top: 6px; border-top: 1px solid color-mix(in srgb, #232930 65%, transparent); }
.cond { font-size: 0.8125rem; line-height: 1.5; color: #828a92; margin: 0; }
.cond b { color: #c9d1d9; font-weight: 500; }
.log { display: grid; grid-template-columns: 44px 68px minmax(0, 1fr); gap: 5px 10px; font-size: 0.8125rem; line-height: 1.45; }
.log .t { font-family: "JetBrains Mono", Consolas, monospace; font-size: .6875rem; color: #484f58; padding-top: 2px; }
.log .to { color: #828a92; }
.log .m { color: #c9d1d9; }
@keyframes ct-now-breathe { 0%, 100% { opacity: .35; } 50% { opacity: .9; } }
@media (prefers-reduced-motion: reduce) { * { animation: none !important; } }
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

def toolbar(phone=False):
    pad = ' style="padding: 0 .5rem;"' if phone else ""
    right = "" if phone else '<span class="ct-meta-muted">Planck\'s team</span>'
    return (f'<div class="ct-toolbar"{pad}><div style="display: flex; align-items: stretch; gap: 0;">'
            '<button class="ct-panel-tab ct-panel-tab--active">Board</button><button class="ct-panel-tab">Spec</button>'
            f'<button class="ct-panel-tab">Plan</button><button class="ct-panel-tab">Tasks</button></div>{right}</div>')

def header(watcher=False, phone=False):
    meta = "Planck · Pasteur planning · 3 agents live" + (" · a watcher on haiku" if watcher else "")
    right = "" if phone else f'<span class="ct-meta-muted" style="white-space: nowrap;">{meta}</span>'
    line = ('<p class="ct-meta" style="margin: 0;"><span style="color: #c9d1d9">Two verdicts and one task, then done.</span> '
            + ("" if phone else 'Last activity 21:43: Planck accepted <span style="color: #c9d1d9">Design the team view as mission control</span>. ')
            + 'Eight finished lanes not cleaned.' + ("" if phone else ' <button class="door" style="display: inline;">Plan</button>') + '</p>')
    return (f'<div style="display: flex; flex-direction: column; gap: 6px; padding: 4px 2px 0;">'
            f'<div style="display: flex; align-items: flex-start; justify-content: space-between; gap: 12px;"><span class="plan-title">CodeTerm 1.10.17 — land the fix wave and triage the issue backlog</span>{right}</div>'
            '<div style="display: flex; flex-wrap: wrap; align-items: center; gap: 6px;"><span class="ct-small-pill ct-chip-accent">execution</span>'
            '<span class="ct-plan-status-breakdown"><span style="color: #58a6ff">0 todo</span><span class="sep">&nbsp;·&nbsp;</span><span style="color: #d29922">2 review</span><span class="sep">&nbsp;·&nbsp;</span><span style="color: #ff7b72">0 blocked</span><span class="sep">&nbsp;·&nbsp;</span><span style="color: #7ee787">9 done</span></span>'
            f'<span class="ct-meta-muted">of 13, one cancelled</span></div>{line}</div>')

def cards(phone=False, with_read=True):
    read = '<p class="read">Ordinary for you, and what the plan waits on.</p>' if with_read else ""
    c = [
        ('<div class="fig"><span class="figure amber">3 h 34 m</span><span class="unit">oldest verdict outstanding</span></div>'
         '<p class="q"><b>2</b> of 5 review slots · credit <b>−2</b>, three closures from the wall</p>'
         + ("" if phone else '<p class="q">Tonight\'s closed verdicts took <b>0 min to 1 h 52 m</b></p>')),
        ('<div class="fig"><span class="figure">2 of 3</span><span class="unit">agents idle, waiting on a verdict</span></div>'
         '<p class="q"><b>1</b> working · <b>0</b> stuck on a question · <b>1</b> tab gone today</p>'),
        ('<div class="fig"><span class="figure amber">3 h 34 m</span><span class="unit">stopped on your approval</span></div>'
         '<p class="q">Asked twice; the current ask is 53 minutes old: <b>pending the user\'s direct approval, not on quality</b></p>'
         '<p class="q">Half of your holds clear in 20 min to 4 h</p>' + read),
        ('<div class="fig"><span class="figure">9</span><span class="unit">finished today, each on the first attempt</span></div>'
         '<p class="q">Workers held them <b>5 min to 1 h 50 m</b></p>'
         + ("" if phone else '<p class="q">Verdicts then took <b>0 min to 1 h 52 m</b> · two open now, 29 min and 3 h 34 m</p>')),
        ('<div class="fig"><span class="figure">16 of 32</span><span class="unit">reports tonight were on one task</span></div>'
         '<p class="q"><b>7</b> reported no change; the task did not move</p>'),
    ]
    cols = "repeat(2, minmax(0, 1fr))" if phone else "repeat(3, minmax(0, 1fr))"
    inner = "".join(f'<div class="ct-panel-card kpi">{x}</div>' for x in c)
    return f'<div style="display: grid; grid-template-columns: {cols}; gap: {8 if phone else 12}px; align-items: stretch;">{inner}</div>'

def cond(watcher=False, phone=False):
    extra = ", so the watcher cannot read them either" if watcher else ""
    door = '<button class="door" style="display: inline;">Settings</button>'
    return (f'<p class="cond">Weekly <b>25 %</b>, projected <b>50 %</b> by the reset at high confidence; session <b>12 %</b>, resets 23:10. '
            f'Chasing and compactions are not being recorded: the KPI ledger is off{extra}. {door}</p>')

def day_card(width, label_w, phone=False, watcher=False):
    marks = ""
    if watcher and not phone:
        # the watcher's own messages, placed in time above the strip
        scale = width / span
        pts = [("18:24", "to you", 18*60+24), ("21:05", "to Planck", 21*60+5), ("21:35", "to Planck", 21*60+35)]
        base = time.localtime(now / 1000); day0 = time.mktime((base.tm_year, base.tm_mon, base.tm_mday, 0, 0, 0, 0, 0, -1)) * 1000
        ms = "".join(f'<span style="position: absolute; left: {max(0,(day0 + m*60000 - approved)*scale):.1f}px; transform: translateX(-50%); font-size: .6875rem; color: #c9d1d9; white-space: nowrap;">{t} {who}</span>' for t, who, m in pts)
        marks = (f'<div style="display: grid; grid-template-columns: {label_w}px minmax(0, 1fr); gap: 8px; height: 18px;"><span class="q" style="font-size: .6875rem;">the watcher wrote</span>'
                 f'<div style="position: relative; height: 18px;">{ms}</div></div>')
    cap = ("Today, by task: dark before pickup, blue while a worker held it, grey while it waited for a verdict. A bright edge at the end of a bar is a clock still running; amber is that clock past the two-hour floor. A tick for each report, a taller one for each hold; a numeral where reports fell too close to draw apart."
           if not phone else "Blue: a worker held it. Grey: waiting for a verdict. A bright end: still running. Amber: past the floor. Ticks: reports.")
    return (f'<div class="ct-panel-card kpi" style="gap: 10px;">'
            f'{split_bar(width + label_w + 8)}'
            f'<div style="padding-top: 10px; border-top: 1px solid color-mix(in srgb, #232930 65%, transparent);"><p class="q">{cap}</p></div>{marks}{strip(width, label_w, phone)}</div>')

def log_card():
    return ('<div class="ct-panel-card kpi" style="gap: 8px;">'
            '<div style="display: flex; align-items: baseline; justify-content: space-between; gap: 8px;"><span class="q" style="color: #c9d1d9;">What the watcher wrote today</span>'
            '<span class="ct-meta-muted">wakes every five minutes · last woke 3 min ago and said nothing new</span></div>'
            '<div class="log">'
            '<span class="t">18:24</span><span class="to">to you</span><span class="m">Stop a successful first-run spawn reporting as failed is held pending your direct approval; Planck cannot clear it alone. Nothing again until the hold clears.</span>'
            '<span class="t">21:05</span><span class="to">to Planck</span><span class="m">Review at 4 of 5 slots and credit one closure from the wall: close a review before dispatching. Give Spec and Plan their own pages has waited 2 h and the tab that filed it is gone; that verdict is yours.</span>'
            '<span class="t">21:35</span><span class="to">to Planck</span><span class="m">Review back to 2 of 5; the warning is withdrawn. One hold remains on the user.</span>'
            '<span class="t">—</span><span class="to">to nobody</span><span class="m">Seven reports that moved nothing: the loop ended before the watcher woke. Two verdicts from done: a reading, not an alert. Eight lanes uncleaned: below the threshold worth saying.</span>'
            '</div></div>')

def page(width, phone=False, watcher=False):
    label_w = 110 if phone else 236
    strip_w = width - 24 - 24 - label_w - 8
    body = [header(watcher, phone), cards(phone), day_card(strip_w, label_w, phone, watcher), cond(watcher, phone)]
    if watcher: body.append(log_card())
    return (HEAD % (STYLE + (".ct-panel-tab { padding: .75rem .75rem; font-size: 0.875rem; }" if phone else ""))
            + f'<div style="width: {width}px; display: flex; flex-direction: column; background: #131921;">{toolbar(phone)}'
            + '<div style="display: flex; flex-direction: column; gap: 14px; padding: 12px 12px 16px;">' + "".join(body) + '</div></div>\n</x-dc>\n</body>\n</html>\n')

for name, w, ph, wa in (("Main", 960, False, False), ("Watcher", 960, False, True), ("Phone", 390, True, False)):
    open(os.path.join(OUT, name + ".src"), "w", encoding="utf-8").write(page(w, ph, wa))
    print(name, "written")
print("minutes: work %.0f wait %.0f (open %.0f) pickup %.0f; origin %s" % (work, wait, wait_open, pickup, hm(origin)))
