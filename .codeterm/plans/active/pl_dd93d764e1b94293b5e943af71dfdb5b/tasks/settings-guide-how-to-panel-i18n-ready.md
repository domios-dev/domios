---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_f8f202bbfa974325ac030715dbf5ebd0
handle: settings-guide-how-to-panel-i18n-ready
title: 'Settings: Guide / How-to panel (i18n-ready)'
status: done
---

## What

CodeTerm has zero in-app explanation of its concepts. Add a **Guide** ("How to") entry to the Settings menu (`src/components/Titlebar/SettingsMenu.tsx`, panel host `src/components/Settings/SettingsPanel.tsx`) that opens an illustrated, sectioned guide. English first; structure prepared for localization from day one.

## Architecture (i18n-ready)

- Content lives as structured data, NOT hardcoded JSX: `src/help/content/en.ts` exporting typed sections (`{ id, title, blocks: (paragraph | list | figure | tip | shortcut-table)[] }`). A future `ru.ts` etc. drops in by locale key; the renderer never contains copy.
- Renderer: `HelpPanel` with a left rail of section anchors (mirroring the Settings visual language from the existing menu — icon + title + subtitle rows) and a scrollable content pane; deep-linkable section ids (e.g. settings opens directly to "Plans").
- Figures: 3-4 lightweight inline SVG diagrams drawn in CodeTerm palette (CSS vars, no raster screenshots — they localize and re-theme for free and never go stale in dark/light).

## Sections + draft copy (worker refines wording, keeps structure)

1. **What is CodeTerm** — "CodeTerm is a terminal where AI agents live and work. It multiplexes regular shells and AI agent sessions in one window, lets agents coordinate with each other, remember what they learn, and reach across your machines." Figure: none, hero-style opener.
2. **Workspaces, tabs & panes** — workspaces group related work per project; each tab holds a pane: a real terminal (pty) or an agent chat. Splits, Chat/Terminal toggle. Tip: Cmd/Ctrl+K command palette.
3. **Agents** — spawn agents (Claude, Codex, and other providers) into panes; the always-on General Agent ("main") is the machine's brain. Roles: an Orchestrator drives multi-step work by spawning Workers; the sidebar tree shows who is working for whom, live. Figure A: hierarchy diagram (General Agent / Orchestrator -> Workers).
4. **Plans** — durable orchestration state: a Goal (north star), a Spec (what/why), a Plan (how, in waves), and Tasks with acceptance criteria. Views: Board (columns by status), List (stream), Canvas (spatial). Task lifecycle: pending -> in progress -> in review (and QA) -> done; blocked/parked for stuck work. Working points record what happened; loops re-run a task until its stop condition. Figure B: pipeline diagram Goal->Spec->Plan->Tasks; Figure C: status flow.
5. **Memory** — agents save durable facts, learnings, and skills to mem; hybrid keyword+semantic search; the Memory panel shows items and background workers. Ask any agent to "remember" something.
6. **Mesh** — pair machines into a private mesh: see peers, send messages/tasks to their agents, share plans. Enable per-machine in the Mesh tab.
7. **Integrations** — bridges (e.g. Telegram) route external messages to your agents and back. Configured in Settings -> Integrations.
8. **Tips & shortcuts** — command palette, keybindings table (pull actual bindings from config rather than hardcoding), where logs live, how updates work.

## Placement

New row in the Settings dropdown between Logs and Check for updates: icon book/question, title "Guide", subtitle "What CodeTerm is and how to work with it". Opens HelpPanel (same surface class as other settings panels).

## Out of scope

Localized translations themselves (structure only), interactive tutorials/tours, web docs export.
