---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_63c585953fef4dbdb9e1d906077f8b18
handle: ghost-ellipsis-tabs-session-restore-renders-tabs-for-dead-panes
title: 'Ghost ellipsis tabs: session restore renders tabs for dead panes'
status: done
---

## What

BUG (round-29 — refiled standalone; previously mis-filed as side evidence on group-v2, which is why it survived two rounds): tabs titled "…" (and stale neighbors like dead test panes) appear in the sidebar. EVIDENCE: the ghost tabs' panes do not exist in ANY daemon's /api/panes (verified against both prod and dev during a live occurrence) — these are SESSION-RESTORED tabs whose panes never rematerialized. The "…" is the title fallback for a restored tab with no live pane.

## Two converging sources

1. **The empty picker tab persists into session snapshots** — the round-18 virtual-tab requirement not yet enforced: a picker tab captured in the snapshot restores after daemon restart as a contentless tab rendering "…".
2. **Restore integrity in general**: any restored tab whose pane cannot rematerialize (pane killed while daemon was down, test panes cleaned server-side, provider session gone) currently renders as a live-looking ghost tab instead of being dropped.

## Fix

- Snapshot writer: tabs flagged as picker/virtual (group-v2's virtual-tab work) are EXCLUDED from session snapshots — coordinate with task_e0ddee08, this is its criterion made concrete.
- Restore path: after restore + rematerialization pass, any tab whose pane failed to materialize is REMOVED from the live tab strip (optionally surfaced in Recently closed with its last known title) — never rendered as an active "…" tab. Log one INFO line per dropped ghost.
- Title fallback hygiene: if a tab ever renders without a resolvable title, show a debug-attributable placeholder in dev builds (e.g. "unresolved:<tab_id_prefix>") instead of a silent "…" — ghosts must be identifiable, not decorative.

## Executable test plan (Playwright + restart harness — the part that was missing before)

1. RESTORE-GHOST REPRO: on dev, spawn an agent pane, kill its pane server-side via API (pane close) while keeping the tab in the snapshot (or craft a snapshot fixture with a tab whose pane id is bogus); restart the daemon; reconnect UI. PASS: no tab with title "…" or the bogus id renders; Recently closed contains the entry; log shows the dropped-ghost INFO.
2. PICKER PERSISTENCE: open New session picker (do not confirm); restart daemon; reconnect. PASS: no picker tab restored, no ghost.
3. HAPPY RESTORE: normal agent tabs restore with correct titles (regression guard).
4. Grep/unit: snapshot serializer skips virtual-flagged tabs (unit with a fixture snapshot).
