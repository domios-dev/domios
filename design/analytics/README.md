# Global Analytics

Design artboards for the 1.10.20 Analytics view: a machine-wide dashboard
opened from a titlebar button beside Plugins, delivered as a default-enabled
core plugin. The frames are the owner-approval artifact for task_motf5q; the
host capability (task_rli26s) and the core plugin (task_w1du4w) build from
the Contract frame.

| Artboard | What it settles |
| --- | --- |
| `Main.dc.html` | Desktop hierarchy: scope, range, filters, totals, chart, breakdown, tabs created, allowance |
| `Phone.dc.html` | The same view at 390px |
| `PhoneFilters.dc.html` | Filters as a bottom sheet at 390px |
| `States.dc.html` | Recording off, ledger unavailable, nothing recorded, real zero, coverage gaps, null buckets, loading |
| `Titlebar.dc.html` | Placement, states, pinned-plugin pill, 390px width budget, behaviour |
| `Contract.dc.html` | Plugin boundary, query and response shape, the meaning of every figure |

Seed the canvas with the design skill's helper from this directory:

```
node <skill>/seed-canvas.mjs --template <skill>/payload.template.html \
  --out ../global-analytics.html --title "Global Analytics" \
  --artboard Main.dc.html --artboard Phone.dc.html --artboard PhoneFilters.dc.html \
  --artboard States.dc.html --artboard Contract.dc.html --artboard Titlebar.dc.html \
  --canvas canvas.json
```

The seeded bundle is gitignored; the `.dc.html` files are the sources. Edit
`_kit.css` once and run `python paste_kit.py` to paste it between every
artboard's `/*KIT*/` markers. Values are lifted from `src/styles/tokens.css`,
the github-dark output of `themeToCSS()`, `globals.css`,
`missionControlBoard.css` and `src/components/shared/design.ts`.

## Where the figures come from

Tokens, turns, providers, models and plans were read read-only from the dev
KPI ledger `~/.codeterm-dev/kpi_ledger.db`: 4,435 `token_usage` rows between
Sep 11 17:50 and Sep 14, bucketed by UTC day. This machine has no production
ledger. Allowance is the latest parsed `usage_snapshots` row per provider in
`~/.codeterm/usage/usage.db`. Workspace and tabs created are not recorded
anywhere yet, so they appear only in their not-recorded state. Bracketed amber
text marks the placeholders (plan title, team name, future tab counts).
