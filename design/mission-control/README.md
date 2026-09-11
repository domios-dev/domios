# Mission control as a reading of the team

Design artboards for the team Board: what a background watcher over the plan
state and the KPI ledger could conclude, and the same conclusions read straight
off compact cards without one. Two versions on one canvas; they differ only in
the told-whom lines.

Seed the canvas with the design skill's helper from this directory:

```
node <skill>/seed-canvas.mjs --template <skill>/payload.template.html \
  --out ../mission-control-insights.html --title "Mission Control Insights" \
  --artboard Main.dc.html --artboard Watcher.dc.html --artboard Message.dc.html \
  --artboard Phone.dc.html --artboard States.dc.html --artboard Kpis.dc.html \
  --artboard Conclusions.dc.html --canvas canvas.json
```

The seeded bundle is gitignored; the `.dc.html` files are the sources. Each
inlines `_kit.css`, which carries the values lifted from `src/styles/tokens.css`
and `globals.css` under the github-dark default; edit the kit once and paste it
into every artboard's style block.

Every figure on the board frames was read from plan `pl_ad3tcu`, the tab
snapshot and `codeterm usage` on the night of drawing. The KPI ledger was off
on that machine, so the effort card shows its not-recording state and no ledger
figure on the canvas is measured.

## The board is generated, not drawn

`tokens.py` sums every agent's token usage from its own session log, the
Claude jsonl files under `~/.claude/projects` and the Codex rollouts under
`~/.codex/sessions`, which are the files the KPI ledger imports when it is on,
and writes them with the plan's counts and holds into `tokens.json`.
`gen_board.py` draws `Main`, `Watcher` and `Phone` from that file, so every
figure on the board is read, not typed. The runway gauges are the one
exception: their percentages are copied from `codeterm usage latest` and the
Codex rollout's rate-limit record at the time of the read, and the generator
names them at its top.

Re-run both, then paste `_kit.css` into the style block (the generator leaves a
`/*KIT*/` marker), to redraw the board for another night.
