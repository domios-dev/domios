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

`tokens.py` extracts everything the board shows into `tokens.json`: each
agent's token usage from its own session log, the Claude jsonl files under
`~/.claude/projects` and the Codex rollouts under `~/.codex/sessions`, which
are the files the KPI ledger imports when it is on; the plan's counts,
attempts, held reviews (the stored `review held` label, never a note's text),
decision gates, and every milestone, stride, planning entry and gate as the
event log; and the files it read with how long it took. `labels.py` is the
labelling scheme: role families evaluated in order with a visible Other, and
the kind labels for the log. `gen_board.py` draws `Main`, `Watcher` and
`Phone` from that file and writes the `Labels` and `Cost` sheets; runway is
read at draw time from `~/.codeterm/usage/usage.db`, the latest parsed
snapshot per provider with that provider's own reset. Never take runway from
`codeterm usage latest`, which prints one provider only.

Re-run `tokens.py` then `gen_board.py`, paste `_kit.css` into each `/*KIT*/`
marker, render the frames in headless Chrome to size the artboards, and
re-seed.
